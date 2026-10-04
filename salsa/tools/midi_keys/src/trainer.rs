//! Application-independent adaptive training engine. No audio/terminal calls.
mod courses;
mod curriculum;
mod exercise;
mod learner;
mod music;
mod performance;
mod practice;
mod spelling;
pub use spelling::NoteSpelling;

use curriculum::{curriculum, Skill};
use exercise::Exercise;
use keyboard::Event;
use learner::{Attempt, MidiEvidence, Profile, Store};
use std::collections::{BTreeSet, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn default_profile_path() -> Result<PathBuf, String> {
    Store::default_path()
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Waiting,
    Listening,
    Answering,
    Feedback,
    Rest,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromptRole {
    Note,
    Interval,
    Above,
    Below,
}

/// Semantic emphasis in the visible prompt; contains no grading information.
pub struct PromptHighlight {
    pub range: std::ops::Range<usize>,
    pub role: PromptRole,
}

pub struct ExerciseLogEntry<'a> {
    pub number: u64,
    pub title: &'a str,
    pub prompt: &'a str,
    pub entered: String,
    pub correct: bool,
    pub assisted: bool,
}

/// Read-only presentation data. Deliberately excludes the grading rubric,
/// expected notes, skill identity, and mutable learner evidence.
pub struct TrainingView<'a> {
    /// Newest first, including results saved in previous sessions.
    pub recent_exercises: Vec<ExerciseLogEntry<'a>>,
    pub title: &'a str,
    pub prompt: &'a str,
    pub prompt_highlights: &'a [PromptHighlight],
    pub reason: &'a str,
    pub feedback: &'a str,
    pub played: &'a [usize],
    pub spelling: Option<&'a NoteSpelling>,
    pub phase: Phase,
    pub completed: u64,
    pub mastered: usize,
    pub total: usize,
    pub timed_phrase: bool,
}

pub struct Session {
    graph: Vec<Skill>,
    profile: Profile,
    exercise: Option<Exercise>,
    phase: Phase,
    reason: String,
    played: Vec<usize>,
    feedback: String,
    assisted: bool,
    store: Store,
    down: BTreeSet<(usize, usize)>,
    playback: VecDeque<(Instant, Event)>,
    listening_until: Option<Instant>,
    submit_at: Option<Instant>,
    onsets: Vec<Instant>,
    evidence: Vec<MidiEvidence>,
    input_started: Option<Instant>,
    metronome_at: Option<Instant>,
    advance_at: Option<Instant>,
    answer_started_at: Option<Instant>,
    finish_at: Option<Instant>,
    visible: bool,
    paused_feedback_delay: Option<Duration>,
}

impl Session {
    pub fn phase(&self) -> Phase {
        self.phase
    }
    pub fn view(&self) -> TrainingView<'_> {
        TrainingView {
            recent_exercises: self
                .profile
                .recent_attempts
                .iter()
                .rev()
                .take(20)
                .map(|attempt| ExerciseLogEntry {
                    number: attempt.number,
                    title: if attempt.title.is_empty() {
                        self.graph
                            .iter()
                            .find(|s| s.id == attempt.skill)
                            .map(|s| s.title.as_str())
                            .unwrap_or(&attempt.skill)
                    } else {
                        &attempt.title
                    },
                    prompt: if attempt.prompt.is_empty() {
                        "Prompt unavailable (older result)"
                    } else {
                        &attempt.prompt
                    },
                    entered: if attempt.answer.is_empty() {
                        "No notes".into()
                    } else if attempt.spelled_answer.is_empty() {
                        attempt
                            .answer
                            .iter()
                            .map(|&n| keyboard::note_name(n))
                            .collect::<Vec<_>>()
                            .join(" → ")
                    } else {
                        attempt.spelled_answer.join(" → ")
                    },
                    correct: attempt.correct,
                    assisted: attempt.assisted,
                })
                .collect(),
            title: self
                .exercise
                .as_ref()
                .map(|e| e.title.as_str())
                .unwrap_or("SESSION COMPLETE"),
            prompt: self
                .exercise
                .as_ref()
                .map(|e| e.prompt.as_str())
                .unwrap_or(""),
            reason: &self.reason,
            prompt_highlights: self
                .exercise
                .as_ref()
                .map(|e| e.prompt_highlights.as_slice())
                .unwrap_or(&[]),
            feedback: &self.feedback,
            played: &self.played,
            spelling: self.exercise.as_ref().map(|e| &e.spelling),
            phase: self.phase,
            completed: self.profile.completed,
            mastered: self.mastered_count(),
            total: self.graph.len(),
            timed_phrase: self.exercise.as_ref().is_some_and(
                |e| matches!(&e.answer, exercise::Answer::Performance(score) if score.timed),
            ),
        }
    }
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let (store, profile) = Store::open(path)?;
        let graph = curriculum();
        curriculum::validate(&graph)?;
        let mut session = Self {
            graph,
            profile,
            exercise: None,
            phase: Phase::Waiting,
            reason: String::new(),
            played: vec![],
            feedback: String::new(),
            assisted: false,
            store,
            down: BTreeSet::new(),
            playback: VecDeque::new(),
            listening_until: None,
            submit_at: None,
            onsets: vec![],
            evidence: vec![],
            input_started: None,
            metronome_at: None,
            advance_at: None,
            answer_started_at: None,
            finish_at: None,
            visible: true,
            paused_feedback_delay: None,
        };
        session.prepare();
        Ok(session)
    }
    fn prepare(&mut self) {
        self.clear_answer();
        self.answer_started_at = None;
        self.finish_at = None;
        self.feedback.clear();
        self.assisted = false;
        self.metronome_at = None;
        self.advance_at = None;
        let Some((skill, reason)) = self.profile.next(&self.graph, now_seconds()) else {
            self.exercise = None;
            self.phase = Phase::Rest;
            return;
        };
        let seed = (self.profile.completed + 1).wrapping_mul(0x9e3779b97f4a7c15) ^ now_seconds();
        self.exercise = Some(Exercise::generate(skill, seed));
        self.reason = reason.into();
        self.phase = Phase::Waiting;
    }
    pub fn ready(&mut self, now: Instant) {
        if !self.visible || self.phase != Phase::Waiting {
            return;
        }
        self.replay(now);
    }
    pub fn replay(&mut self, now: Instant) {
        if !self.visible {
            return;
        }
        if !matches!(
            self.phase,
            Phase::Waiting | Phase::Listening | Phase::Answering
        ) {
            return;
        }
        self.clear_answer();
        if let Some(exercise) = &self.exercise {
            self.playback = exercise
                .playback
                .iter()
                .map(|&(ms, event)| (now + Duration::from_millis(ms), event))
                .collect();
            if let Some((last, _)) = self.playback.back() {
                self.listening_until = Some(
                    exercise
                        .answer_after_ms
                        .map(|ms| now + Duration::from_millis(ms))
                        .unwrap_or(*last + Duration::from_millis(350)),
                );
                self.metronome_at = exercise
                    .bpm
                    .filter(|_| exercise.metronome)
                    .map(|_| self.listening_until.unwrap());
                self.phase = Phase::Listening;
            } else {
                self.phase = Phase::Answering;
            }
            self.answer_started_at = Some(self.listening_until.unwrap_or(now));
            self.finish_at = exercise
                .fixed_duration_ms()
                .map(|ms| self.answer_started_at.unwrap() + Duration::from_millis(ms));
            // Stop sounding notes before restarting the new playback timeline.
            self.playback.push_front((now, Event::ClearChannel(15)));
        }
    }
    pub fn clear_answer(&mut self) {
        self.played.clear();
        self.down.clear();
        self.submit_at = None;
        self.onsets.clear();
        self.evidence.clear();
        self.input_started = None;
        if self
            .exercise
            .as_ref()
            .is_some_and(|e| e.relative_duration_ms().is_some())
        {
            self.finish_at = None;
        }
    }
    pub fn panic(&mut self) {
        self.clear_answer();
        self.playback.clear();
        self.listening_until = None;
        self.metronome_at = None;
        self.finish_at = None;
        self.answer_started_at = None;
        if self.phase == Phase::Listening {
            self.phase = Phase::Answering;
            self.feedback = "Playback stopped. Press r to listen again.".into();
        }
    }
    pub fn input(&mut self, event: Event, now: Instant) {
        if !self.visible {
            return;
        }
        if self.phase == Phase::Listening && self.listening_until.is_some_and(|at| at <= now) {
            self.phase = Phase::Answering;
            self.listening_until = None;
        }
        if self.phase != Phase::Answering {
            return;
        }
        match event {
            Event::Note {
                channel,
                note,
                velocity,
            } if channel < 16 && note < 128 => {
                let start = *self.input_started.get_or_insert(now);
                if self.evidence.len() < 4096 {
                    self.evidence.push(MidiEvidence {
                        offset_ms: now.saturating_duration_since(start).as_millis() as u64,
                        channel,
                        note,
                        velocity,
                    });
                }
                if velocity > 0 {
                    if self.played.is_empty() {
                        if let Some(ms) = self
                            .exercise
                            .as_ref()
                            .and_then(|e| e.relative_duration_ms())
                        {
                            self.finish_at = Some(now + Duration::from_millis(ms));
                        }
                    }
                    self.down.insert((channel, note));
                    if self.played.len() < 128 {
                        self.played.push(note);
                        self.onsets.push(now);
                    }
                    self.submit_at = None;
                } else {
                    self.down.remove(&(channel, note));
                    if self.down.is_empty()
                        && self
                            .exercise
                            .as_ref()
                            .is_some_and(|e| self.played.len() >= e.minimum_notes())
                    {
                        self.submit_at =
                            Some(self.finish_at.unwrap_or(now + Duration::from_millis(450)));
                    }
                }
            }
            Event::ClearChannel(_) | Event::Reset => self.clear_answer(),
            _ => {}
        }
    }
    pub fn tick(&mut self, now: Instant) -> Result<Vec<Event>, String> {
        if !self.visible {
            return Ok(vec![]);
        }
        let mut events = Vec::new();
        while self.playback.front().is_some_and(|(at, _)| *at <= now) {
            events.push(self.playback.pop_front().unwrap().1);
        }
        if let Some(at) = self.metronome_at {
            if at <= now && matches!(self.phase, Phase::Listening | Phase::Answering) {
                events.push(Event::Note {
                    channel: 15,
                    note: 96,
                    velocity: 45,
                });
                self.playback.push_back((
                    now + Duration::from_millis(60),
                    Event::Note {
                        channel: 15,
                        note: 96,
                        velocity: 0,
                    },
                ));
                let bpm = self.exercise.as_ref().unwrap().bpm.unwrap();
                let beat = Duration::from_millis(60000 / u64::from(bpm));
                let mut next = at + beat;
                while next <= now {
                    next += beat;
                }
                self.metronome_at = Some(next);
            }
        }
        if self.phase == Phase::Listening && self.listening_until.is_some_and(|at| at <= now) {
            self.phase = Phase::Answering;
            self.listening_until = None;
        }
        if self.phase == Phase::Answering
            && (self.submit_at.is_some_and(|at| at <= now)
                || self.finish_at.is_some_and(|at| at <= now))
        {
            self.finish(self.played.is_empty(), now)?;
        }
        if self.phase == Phase::Feedback && self.advance_at.is_some_and(|at| at <= now) {
            events.push(Event::Reset);
            self.advance();
        }
        Ok(events)
    }
    pub fn hint(&mut self) {
        if self.visible && self.phase == Phase::Answering {
            self.assisted = true;
            self.feedback = self
                .exercise
                .as_ref()
                .map(|e| {
                    format!(
                        "Hint: {}  (practice only; no mastery credit)",
                        e.explanation
                    )
                })
                .unwrap_or_default();
        }
    }
    pub fn submit(&mut self, give_up: bool) -> Result<(), String> {
        self.finish(give_up, Instant::now())
    }
    fn finish(&mut self, give_up: bool, now: Instant) -> Result<(), String> {
        if !self.visible || self.phase != Phase::Answering || (!give_up && self.played.is_empty()) {
            return Ok(());
        }
        let exercise = self.exercise.as_ref().unwrap();
        let pitch_correct = !give_up && exercise.correct(&self.played);
        let timing_correct =
            exercise.performance_timing_correct(&self.onsets, self.answer_started_at);
        let duration_correct = exercise.holds_correct(&self.evidence);
        let correct = pitch_correct && timing_correct && duration_correct;
        let skill = self
            .graph
            .iter()
            .find(|s| s.id == exercise.skill_id)
            .unwrap();
        let mut next = self.profile.clone();
        next.record(
            skill,
            Attempt {
                title: exercise.title.clone(),
                prompt: exercise.prompt.clone(),
                spelled_answer: self
                    .played
                    .iter()
                    .map(|&n| exercise.spelling.note_name(n))
                    .collect(),
                number: 0,
                skill: skill.id.clone(),
                variant: exercise.variant.clone(),
                correct,
                assisted: self.assisted,
                at: now_seconds(),
                answer: self.played.clone(),
                expected: exercise.expected_evidence(),
                evidence: self.evidence.clone(),
                bpm: exercise.bpm,
                pitch_correct,
                timing_correct,
                duration_correct,
            },
        );
        self.store.save(&next)?;
        self.profile = next;
        self.feedback = if correct {
            if self.assisted {
                "Good practice. Hint used; no mastery credit.".into()
            } else {
                "Correct!".into()
            }
        } else if pitch_correct && !timing_correct {
            "Right notes; the rhythm or chord attacks need work.".into()
        } else if pitch_correct && !duration_correct {
            "Right notes; check how long you hold them and where you release.".into()
        } else {
            format!("Not yet.  {}", exercise.explanation)
        };
        self.phase = Phase::Feedback;
        self.advance_at =
            Some(now + Duration::from_secs(if correct && !self.assisted { 2 } else { 4 }));
        self.submit_at = None;
        self.finish_at = None;
        self.metronome_at = None;
        self.playback.clear();
        self.playback.push_back((now, Event::ClearChannel(15)));
        Ok(())
    }
    pub fn advance(&mut self) {
        if self.visible && self.phase == Phase::Feedback {
            self.prepare();
        }
    }
    pub fn mastered_count(&self) -> usize {
        self.graph
            .iter()
            .filter(|s| self.profile.skills.get(&s.id).is_some_and(|s| s.mastered()))
            .count()
    }
    pub fn path(&self) -> &std::path::Path {
        &self.store.path
    }

    /// Hidden training cannot emit audio or score background MIDI. Restart an
    /// interrupted prompt when visible again; preserve already-saved feedback.
    pub fn set_visible(&mut self, visible: bool, now: Instant) -> Option<Event> {
        if self.visible == visible {
            return None;
        }
        self.visible = visible;
        if visible {
            if let Some(delay) = self.paused_feedback_delay.take() {
                self.advance_at = Some(now + delay);
            }
            return None;
        }
        self.paused_feedback_delay = self
            .advance_at
            .take()
            .map(|at| at.saturating_duration_since(now));
        self.playback.clear();
        self.listening_until = None;
        self.metronome_at = None;
        self.answer_started_at = None;
        self.finish_at = None;
        self.submit_at = None;
        if !matches!(self.phase, Phase::Feedback | Phase::Rest) {
            self.clear_answer();
            self.phase = Phase::Waiting;
        }
        Some(Event::ClearChannel(15))
    }
}

#[cfg(test)]
mod curriculum_analysis;
#[cfg(test)]
mod practice_tests;
#[cfg(test)]
mod tests;
