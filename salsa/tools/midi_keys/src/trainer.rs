//! Application-independent adaptive training engine. No audio/terminal calls.
mod browser;
mod courses;
mod curriculum;
mod debug_capture;
mod evidence;
mod exercise;
mod insights;
mod learner;
mod music;
mod performance;
mod practice;
mod reading;
mod recognition;
mod rhythm;
mod score_support;
mod tempo;
pub use reading::{ChordSymbol, Hand, ReadingResult, WrittenEvent, WrittenScore};
pub use rhythm::{RhythmReport, RhythmRow, RhythmScore};
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
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
    ChordQuality,
    Above,
    Below,
}

/// Semantic emphasis in the visible prompt; contains no grading information.
pub struct PromptHighlight {
    pub range: std::ops::Range<usize>,
    pub role: PromptRole,
}

pub struct ExerciseLogEntry<'a> {
    pub performance_score: Option<u8>,
    pub number: u64,
    pub title: &'a str,
    pub prompt: &'a str,
    pub entered: String,
    pub correct: bool,
    pub assisted: bool,
}

/// Searchable catalog metadata, available only in the explicit preview mode.
pub struct BrowserView<'a> {
    pub categories: &'static [&'static str],
    pub category: usize,
    pub query: &'a str,
    pub items: Vec<(&'a str, &'a str)>,
    pub selected: usize,
}

/// Stable screen family selected before an answer, independent of feedback notation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExerciseLayout {
    Theory,
    Score,
    Rhythm,
}

/// Exercise presentation without unrevealed answers or mutable learner evidence.
pub struct TrainingView<'a> {
    pub layout: ExerciseLayout,
    pub feedback_correct: Option<bool>,
    pub feedback_grade: Option<u8>,
    pub skill_id: &'a str,
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
    pub browser: Option<BrowserView<'a>>,
    pub preview: bool,
    pub insights: Vec<String>,
    pub insights_open: bool,
    pub insight_offset: usize,
    pub display_score: Option<WrittenScore>,
    pub played_score: Option<WrittenScore>,
    pub score_page: usize,
    pub current_bar: Option<usize>,
    pub score_pages: usize,
    pub reading_score: Option<&'a WrittenScore>,
    pub reading_result: Option<&'a ReadingResult>,
    pub rhythm_score: Option<RhythmScore>,
    pub rhythm_report: Option<&'a RhythmReport>,
    pub comparison_paused: bool,
    pub comparison_offset: usize,
    pub timed_phrase: bool,
    pub awaiting_start: bool,
    pub exploring: bool,
}

pub struct Session {
    feedback_correct: Option<bool>,
    debug: Option<debug_capture::Capture>,
    feedback_score: Option<WrittenScore>,
    played_score: Option<WrittenScore>,
    score_page: usize,
    current_bar: Option<usize>,
    score_follow: bool,
    reading_supported: bool,
    reading_result: Option<ReadingResult>,
    rhythm_report: Option<RhythmReport>,
    comparison_paused: bool,
    comparison_offset: usize,
    browser: Option<browser::Browser>,
    insights_open: bool,
    insight_offset: usize,
    graph: Vec<Skill>,
    profile: Profile,
    exercise: Option<Exercise>,
    phase: Phase,
    reason: String,
    played: Vec<usize>,
    feedback: String,
    performance_score: Option<u8>,
    assisted: bool,
    store: Option<Store>,
    down: BTreeSet<(usize, usize)>,
    playback: VecDeque<(Instant, Event)>,
    listening_until: Option<Instant>,
    exploring: bool,
    submit_at: Option<Instant>,
    onsets: Vec<Instant>,
    evidence: Vec<MidiEvidence>,
    input_started: Option<Instant>,
    metronome: Option<tempo::BeatClock>,
    advance_at: Option<Instant>,
    answer_started_at: Option<Instant>,
    finish_at: Option<Instant>,
    visible: bool,
    paused_feedback_delay: Option<Duration>,
}

impl Session {
    pub fn set_reading_supported(&mut self, supported: bool) {
        self.reading_supported = supported;
        if !supported
            && self.browser.is_none()
            && self.exercise.as_ref().is_some_and(|e| e.reading.is_some())
        {
            self.prepare();
        }
    }
    pub fn browsing(&self) -> bool {
        self.browser.as_ref().is_some_and(|b| b.open)
    }
    pub fn previewing(&self) -> bool {
        self.browser.is_some()
    }
    pub fn browser_edit(&mut self, key: u8) {
        if let Some(browser) = &mut self.browser {
            browser.edit(key, &self.graph);
        }
    }
    pub fn browser_category(&mut self, forward: bool) {
        if let Some(browser) = &mut self.browser {
            browser.change_category(forward, &self.graph);
        }
    }
    pub fn browser_navigate(&mut self, forward: bool) {
        if let Some(browser) = &mut self.browser {
            browser.navigate(forward);
        }
    }
    pub fn browser_select(&mut self) {
        if self.browser.as_mut().is_some_and(|b| b.select()) {
            self.prepare();
        }
    }
    pub fn show_browser(&mut self) {
        if let Some(browser) = &mut self.browser {
            browser.open = true;
            self.prepare();
        }
    }
    pub fn new_preview(&mut self) {
        if self.browser.is_some() {
            self.prepare();
        }
    }

    pub fn pause_comparison(&mut self) {
        if self.phase == Phase::Feedback {
            self.advance_at = None;
            self.comparison_paused = true;
        }
    }
    pub fn play_reading_solution(&mut self, now: Instant) -> bool {
        if self.phase != Phase::Feedback || !self.visible {
            return false;
        }
        let Some(score) = self.exercise.as_ref().and_then(|e| e.reading.as_ref()) else {
            return false;
        };
        self.playback = score
            .playback()
            .into_iter()
            .map(|(ms, event)| (now + Duration::from_millis(ms), event))
            .collect();
        self.playback.push_front((now, Event::ClearChannel(15)));
        self.pause_comparison();
        true
    }
    pub fn cycle_insights(&mut self, forward: bool) {
        if let Some(result) = &self.reading_result {
            let count = result
                .details
                .iter()
                .map(|s| s.len() / 40 + 1)
                .sum::<usize>()
                + 4;
            self.comparison_offset = if forward {
                (self.comparison_offset + 1).min(count.saturating_sub(1))
            } else {
                self.comparison_offset.saturating_sub(1)
            };
            self.pause_comparison();
            return;
        }
        if self.rhythm_report.is_some() {
            self.pause_comparison();
            let count = self.rhythm_report.as_ref().unwrap().rows.len();
            self.comparison_offset = if forward {
                (self.comparison_offset + 1).min(count.saturating_sub(1))
            } else {
                self.comparison_offset.saturating_sub(1)
            };
            return;
        }

        self.browse_insights(forward);
    }
    pub fn browse_insights(&mut self, forward: bool) {
        let count = self.view().insights.len().max(1);
        self.insight_offset =
            (self.insight_offset % count + if forward { 1 } else { count - 1 }) % count;
    }
    pub fn toggle_insights(&mut self) {
        self.insights_open = !self.insights_open;
    }
    pub fn phase(&self) -> Phase {
        self.phase
    }
    fn shown_score(&self) -> Option<&WrittenScore> {
        let ex = self.exercise.as_ref()?;
        // Keep the dedicated percussion/timing comparison for rhythm exercises.
        if ex.rhythm_score().is_some() {
            return None;
        }
        if self.phase == Phase::Feedback {
            self.feedback_score.as_ref()
        } else {
            ex.prompt_score.as_ref()
        }
    }
    pub fn navigate_score(&mut self, forward: bool) -> bool {
        let Some(score) = self.shown_score() else {
            return false;
        };
        let pages = score
            .page_count()
            .max(self.played_score.as_ref().map_or(1, |s| s.page_count()));
        self.score_page = if forward {
            (self.score_page + 1).min(pages - 1)
        } else {
            self.score_page.saturating_sub(1)
        };
        self.score_follow = false;
        self.pause_comparison();
        true
    }
    pub fn view(&self) -> TrainingView<'_> {
        TrainingView {
            layout: self.exercise.as_ref().map_or(ExerciseLayout::Theory, |e| {
                if e.rhythm_score().is_some() {
                    ExerciseLayout::Rhythm
                } else if e.prompt_score.is_some() {
                    ExerciseLayout::Score
                } else {
                    ExerciseLayout::Theory
                }
            }),
            feedback_correct: self.feedback_correct,
            feedback_grade: self.performance_score,
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
                    performance_score: attempt.performance_score,
                    correct: attempt.successful(),
                    assisted: attempt.assisted,
                })
                .collect(),
            skill_id: self.exercise.as_ref().map_or("", |e| e.skill_id.as_str()),
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
            browser: self
                .browser
                .as_ref()
                .filter(|b| b.open)
                .map(|b| BrowserView {
                    categories: browser::CATEGORIES,
                    category: b.category,
                    query: &b.query,
                    selected: b.selected,
                    items: b
                        .matches
                        .iter()
                        .map(|i| (self.graph[*i].title.as_str(), self.graph[*i].id.as_str()))
                        .collect(),
                }),
            preview: self.browser.is_some(),
            insights: if self.browser.is_some() {
                vec![
                    "Preview mode: progress is never recorded.".into(),
                    "b: exercise list · v: another variant · r: replay".into(),
                ]
            } else {
                self.profile.insights_at(
                    &self.graph,
                    now_seconds(),
                    self.exercise.as_ref().map(|e| e.skill_id.as_str()),
                )
            },
            insights_open: self.insights_open,
            insight_offset: self.insight_offset,
            exploring: self.exploring,
            display_score: self.shown_score().map(|s| {
                let mut score = s.clone();
                score.active_tick = self
                    .current_bar
                    .filter(|_| self.phase == Phase::Answering)
                    .map(|b| b as u32 * 8);
                score.page(self.score_page)
            }),
            played_score: self
                .shown_score()
                .and_then(|_| self.played_score.as_ref())
                .map(|s| s.page(self.score_page)),
            score_page: self.score_page,
            current_bar: self.current_bar,
            score_pages: self
                .shown_score()
                .map_or(1, |s| s.page_count())
                .max(self.played_score.as_ref().map_or(1, |s| s.page_count())),
            reading_score: self.exercise.as_ref().and_then(|e| e.reading.as_ref()),
            reading_result: self.reading_result.as_ref(),
            rhythm_score: self.exercise.as_ref().and_then(|e| e.rhythm_score()),
            rhythm_report: self.rhythm_report.as_ref(),
            comparison_paused: self.comparison_paused,
            comparison_offset: self.comparison_offset,
            timed_phrase: self.exercise.as_ref().is_some_and(|e| e.bpm.is_some()),
            awaiting_start: self.phase == Phase::Waiting
                && self
                    .exercise
                    .as_ref()
                    .is_some_and(|e| e.requires_explicit_start()),
        }
    }
    pub fn open(path: PathBuf) -> Result<Self, String> {
        let (store, profile) = Store::open(path)?;
        Self::new(Some(store), profile, false)
    }
    pub fn browse() -> Result<Self, String> {
        Self::new(None, Profile::default(), true)
    }
    /// Open an exact curriculum ID without loading or writing learner state.
    pub fn preview(id: &str) -> Result<Self, String> {
        let mut session = Self::browse()?;
        session
            .browser
            .as_mut()
            .unwrap()
            .select_id(id, &session.graph)?;
        session.prepare();
        Ok(session)
    }
    fn new(store: Option<Store>, profile: Profile, preview: bool) -> Result<Self, String> {
        let graph = curriculum();
        curriculum::validate(&graph)?;
        let mut session = Self {
            debug: None,
            feedback_score: None,
            played_score: None,
            score_page: 0,
            current_bar: None,
            score_follow: true,
            reading_supported: true,
            reading_result: None,
            rhythm_report: None,
            feedback_correct: None,
            comparison_paused: false,
            comparison_offset: 0,
            browser: preview.then(|| browser::Browser::new(&graph)),
            insights_open: true,
            insight_offset: 0,
            graph,
            profile,
            exercise: None,
            phase: Phase::Waiting,
            reason: String::new(),
            played: vec![],
            feedback: String::new(),
            performance_score: None,
            assisted: false,
            store,
            down: BTreeSet::new(),
            playback: VecDeque::new(),
            listening_until: None,
            exploring: false,
            submit_at: None,
            onsets: vec![],
            evidence: vec![],
            input_started: None,
            metronome: None,
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
        if let Some(debug) = &mut self.debug {
            debug.reset_result();
        }
        self.feedback_correct = None;
        self.feedback_score = None;
        self.played_score = None;
        self.score_page = 0;
        self.current_bar = None;
        self.score_follow = true;
        self.reading_result = None;
        self.rhythm_report = None;
        self.comparison_paused = false;
        self.comparison_offset = 0;
        self.clear_answer();
        self.answer_started_at = None;
        self.finish_at = None;
        self.feedback.clear();
        self.performance_score = None;
        self.assisted = false;
        self.metronome = None;
        self.advance_at = None;
        self.playback.clear();
        self.listening_until = None;
        if let Some(browser) = &mut self.browser {
            if browser.open {
                self.exercise = None;
                self.phase = Phase::Rest;
            } else if let Some(index) = browser.chosen {
                if let Some(debug) = &mut self.debug {
                    debug.begin(Instant::now());
                }
                browser.variation += 1;
                self.exercise = Some(Exercise::generate(
                    &self.graph[index],
                    browser.variation.wrapping_mul(0x9e3779b97f4a7c15),
                ));
                self.reason = format!(
                    "Preview · {} · variation {} · no progress recorded",
                    self.graph[index].id, browser.variation
                );
                self.phase = Phase::Waiting;
            }
            return;
        }
        let eligible;
        let graph = if self.reading_supported {
            &self.graph
        } else {
            eligible = self
                .graph
                .iter()
                .filter(|s| !matches!(s.task, crate::curriculum::Task::Reading(_)))
                .cloned()
                .collect::<Vec<_>>();
            &eligible
        };
        let Some((skill, reason)) = self.profile.next(graph, now_seconds()) else {
            self.exercise = None;
            self.phase = Phase::Rest;
            return;
        };
        let seed = (self.profile.completed + 1).wrapping_mul(0x9e3779b97f4a7c15) ^ now_seconds();
        self.exercise = Some(Exercise::generate(skill, seed));
        self.reason = reason.into();
        self.phase = Phase::Waiting;
    }
    /// Called automatically when the MIDI device is ready and keys are released.
    pub fn ready(&mut self, now: Instant) {
        if self
            .exercise
            .as_ref()
            .is_some_and(|e| e.requires_explicit_start())
        {
            return;
        }
        self.start(now);
    }
    /// Explicit start from Enter, or automatic start for exercises without a count-in.
    pub fn start(&mut self, now: Instant) {
        if !self.visible || self.phase != Phase::Waiting {
            return;
        }
        self.replay(now);
    }
    pub fn replay(&mut self, now: Instant) {
        if !self.visible {
            return;
        }
        if self.phase == Phase::Feedback {
            // The answer has been revealed: retries are practice, not fresh mastery evidence.
            self.assisted = true;
            self.advance_at = None;
            self.paused_feedback_delay = None;
            self.phase = Phase::Waiting;
            self.feedback.clear();
            self.performance_score = None;
        }
        if !matches!(
            self.phase,
            Phase::Waiting | Phase::Listening | Phase::Answering
        ) {
            return;
        }
        if self.phase == Phase::Answering
            && self.exercise.as_ref().is_some_and(|e| e.reading.is_some())
        {
            self.assisted = true;
        }
        if let Some(debug) = &mut self.debug {
            debug.reset_result();
        }
        self.feedback_correct = None;
        self.feedback_score = None;
        self.played_score = None;
        self.score_page = 0;
        self.current_bar = None;
        self.score_follow = true;
        self.reading_result = None;
        self.rhythm_report = None;
        self.comparison_paused = false;
        self.comparison_offset = 0;
        self.clear_answer();
        self.listening_until = None;
        self.metronome = None;
        if let Some(exercise) = &self.exercise {
            self.exploring = exercise.answer_policy == exercise::AnswerPolicy::ExploreThenAnswer;
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
                self.metronome = exercise
                    .bpm
                    .filter(|_| exercise.metronome)
                    .map(|bpm| tempo::BeatClock::new(self.listening_until.unwrap(), bpm));
                self.phase = Phase::Listening;
            } else {
                self.phase = Phase::Answering;
                if exercise.pulse_backing || (exercise.metronome && exercise.bpm.is_some()) {
                    self.metronome = Some(tempo::BeatClock::new(now, exercise.bpm.unwrap()));
                }
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
        if self.phase == Phase::Answering
            && !self.played.is_empty()
            && self.exercise.as_ref().is_some_and(|e| e.reading.is_some())
        {
            self.assisted = true;
        }
        self.submit_at = None;
        self.played.clear();
        self.down.clear();
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
        self.metronome = None;
        self.finish_at = None;
        self.answer_started_at = None;
        if matches!(self.phase, Phase::Listening | Phase::Answering)
            && self
                .exercise
                .as_ref()
                .is_some_and(|e| e.requires_explicit_start())
        {
            self.phase = Phase::Waiting;
            self.feedback = "Stopped. Press Enter when ready to restart.".into();
        } else if self.phase == Phase::Listening {
            self.phase = Phase::Answering;
            self.feedback = "Playback stopped. Press r to listen again.".into();
        }
    }
    pub fn input(&mut self, event: Event, now: Instant) {
        self.debug_event("decoded_input", event, now);
        if !self.visible {
            return;
        }
        // The first downbeat has the same early-attack tolerance as later beats.
        // Keep the scheduled answer clock; accepting an early note must not move it.
        let early_attack = matches!(event, Event::Note { velocity: 1.., .. })
            && self.exercise.as_ref().is_some_and(|ex| {
                matches!(&ex.answer, exercise::Answer::Performance(p)
                    if p.fixed_start && self.listening_until.is_some_and(|at|
                        now < at && at.duration_since(now) <= Duration::from_millis(p.tolerance_ms)))
            });
        if self.phase == Phase::Listening
            && (early_attack || self.listening_until.is_some_and(|at| at <= now))
        {
            self.phase = Phase::Answering;
            self.listening_until = None;
        }
        if self.phase != Phase::Answering || self.exploring {
            return;
        }
        match event {
            Event::Note {
                channel,
                note,
                velocity,
            } if channel < 16 && note < 128 => {
                let fixed_notes = self.exercise.as_ref().and_then(|e| e.fixed_answer_notes());
                if velocity > 0 && fixed_notes.is_some_and(|n| self.played.len() >= n) {
                    return;
                }
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
                    self.submit_at = None;
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
                    if fixed_notes.is_some_and(|n| self.played.len() >= n) {
                        self.submit_at = Some(now);
                    }
                } else {
                    self.down.remove(&(channel, note));
                    if fixed_notes.is_none()
                        && self.down.is_empty()
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
        if self.score_follow && self.phase == Phase::Answering {
            if let Some(ex) = &self.exercise {
                if let Some(score) = &ex.prompt_score {
                    let fixed =
                        matches!(&ex.answer,exercise::Answer::Performance(p) if p.fixed_start);
                    if let (Some(start), Some(bpm)) = (
                        if fixed {
                            self.answer_started_at
                        } else {
                            self.onsets.first().copied()
                        },
                        ex.bpm,
                    ) {
                        let tick = now.saturating_duration_since(start).as_millis()
                            * u128::from(bpm)
                            / 30000;
                        self.current_bar = Some(
                            (tick as usize / 8)
                                .min(score.ticks.div_ceil(8).saturating_sub(1) as usize),
                        );
                        self.score_page = (tick as usize / score.page_ticks() as usize)
                            .min(score.page_count() - 1);
                    }
                }
            }
        }
        if !self.visible {
            return Ok(vec![]);
        }
        let mut events = Vec::new();
        while self.playback.front().is_some_and(|(at, _)| *at <= now) {
            events.push(self.playback.pop_front().unwrap().1);
        }
        if matches!(self.phase, Phase::Listening | Phase::Answering) {
            if let Some(beat) = self.metronome.as_mut().and_then(|clock| clock.poll(now)) {
                if self.exercise.as_ref().unwrap().pulse_backing {
                    events.push(Event::DrumBeat {
                        beat: (beat % 4) as u8,
                    });
                } else {
                    events.push(Event::MetronomeClick);
                }
            }
        }
        if self.phase == Phase::Listening && self.listening_until.is_some_and(|at| at <= now) {
            self.phase = Phase::Answering;
            self.listening_until = None;
        }
        if self.phase == Phase::Answering
            && (self.finish_at.is_some_and(|at| at <= now)
                || self.submit_at.is_some_and(|at| at <= now))
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
        if self.phase == Phase::Waiting && !give_up {
            self.start(Instant::now());
            return Ok(());
        }
        if self.visible && self.phase == Phase::Answering && self.exploring && !give_up {
            self.clear_answer();
            self.exploring = false;
            return Ok(());
        }
        self.finish(give_up, Instant::now())
    }
    fn finish(&mut self, give_up: bool, now: Instant) -> Result<(), String> {
        if !self.visible || self.phase != Phase::Answering || (!give_up && self.played.is_empty()) {
            return Ok(());
        }
        let exercise = self.exercise.as_ref().unwrap();
        let origin_ms = self
            .input_started
            .zip(self.answer_started_at)
            .map_or(0, |(input, start)| evidence::signed_ms(input, start));
        let accompaniment = match &exercise.answer {
            exercise::Answer::Performance(p) if p.accompaniment => {
                Some(p.accompaniment_report(&self.evidence, origin_ms))
            }
            _ => None,
        };
        self.rhythm_report =
            exercise.rhythm_report(&self.onsets, self.answer_started_at, &self.evidence);
        let rhythm = exercise.rhythm_score();
        let rhythm_result = self.rhythm_report.as_ref().zip(rhythm.as_ref());
        let pitch_correct = !give_up
            && if let Some((report, _)) = rhythm_result {
                report.count_correct()
            } else {
                accompaniment.as_ref().map_or_else(
                    || exercise.correct(&self.played),
                    |r| r.pitches == r.total && r.extras == 0,
                )
            };
        let timing_correct = if let Some((report, score)) = rhythm_result {
            report.timing_correct(score.tolerance_ms)
        } else {
            accompaniment.as_ref().map_or_else(
                || exercise.performance_timing_correct(&self.onsets, self.answer_started_at),
                |r| r.timing == r.total,
            )
        };
        let duration_correct = if let Some((report, score)) = rhythm_result {
            report.holds_correct(score.tolerance_ms)
        } else {
            accompaniment.as_ref().map_or_else(
                || exercise.holds_correct(&self.evidence),
                |r| r.holds == r.total,
            )
        };
        let correct = pitch_correct && timing_correct && duration_correct;
        self.performance_score = rhythm_result
            .map(|(report, score)| report.grade(score))
            .or_else(|| accompaniment.as_ref().map(|r| r.score()))
            .map(|grade| if give_up { 0 } else { grade });
        self.reading_result = exercise
            .reading
            .as_ref()
            .map(|score| score.evaluate(&self.played, &self.onsets, &self.evidence));
        if let Some(store) = &self.store {
            let skill = self
                .graph
                .iter()
                .find(|s| s.id == exercise.skill_id)
                .unwrap();
            let mut next = self.profile.clone();
            next.record(
                skill,
                Attempt {
                    performance_score: self.performance_score,
                    reading: self.reading_result.clone(),
                    recognition: None,
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
            store.save(&next)?;
            self.profile = next;
        }
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
        if let Some(report) = &accompaniment {
            self.feedback = format!(
                "{}/100 · {}{}",
                self.performance_score.unwrap(),
                report.feedback(),
                if self.assisted {
                    " · Hint used; no mastery credit."
                } else {
                    ""
                }
            );
        }
        if rhythm_result.is_some() {
            self.feedback = format!(
                "{}/100 · {}",
                self.performance_score.unwrap(),
                self.feedback
            );
        }
        // Small terminals should land on the first mistake, not a screen of
        // correct notes above it. The view clamps this when every row fits.
        self.comparison_offset = self
            .rhythm_report
            .as_ref()
            .and_then(|report| {
                let tolerance = exercise.rhythm_score()?.tolerance_ms;
                report.rows.iter().position(|row| !row.correct(tolerance))
            })
            .unwrap_or(0);
        if let Some((report, score)) = rhythm_result {
            self.feedback_score = Some(score.notation());
            self.played_score = Some(report.notation(score));
        } else {
            self.feedback_score = Some(score_support::feedback_expected(
                exercise,
                &self.played,
                correct,
            ));
            self.played_score = Some(score_support::response(
                exercise,
                &self.played,
                &self.evidence,
                if matches!(&exercise.answer, exercise::Answer::Performance(p) if p.fixed_start) {
                    origin_ms
                } else {
                    0
                },
                correct,
            ));
        }
        self.score_page = 0;
        self.current_bar = None;
        self.phase = Phase::Feedback;
        self.feedback_correct = Some(correct);
        // Preserve every imperfect attempt for deliberate review, across all families.
        self.comparison_paused = !correct || self.assisted || self.store.is_none();
        self.advance_at = (!self.comparison_paused).then_some(now + Duration::from_secs(2));
        self.submit_at = None;
        self.finish_at = None;
        self.metronome = None;
        self.playback.clear();
        self.playback.push_back((now, Event::ClearChannel(15)));
        self.debug_result(
            correct,
            pitch_correct,
            timing_correct,
            duration_correct,
            give_up,
        );
        self.flush_debug(true)?;
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
    pub fn path(&self) -> Option<&std::path::Path> {
        self.store.as_ref().map(|store| store.path.as_path())
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
        self.metronome = None;
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
