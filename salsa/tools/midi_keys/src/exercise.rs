use crate::curriculum::{Direction, Skill, Task, CHORDS, INTERVALS, QUALITIES, ROOTS};
use crate::{NoteSpelling, PromptHighlight, PromptRole};
use keyboard::Event;
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub enum Answer {
    OctaveSequence(Vec<usize>),
    TransposedSequence(Vec<usize>),
    Performance(crate::performance::Performance),
    PitchClasses(Vec<usize>),
    Interval(usize),
    AnchoredInterval {
        root: usize,
        semitones: usize,
        direction: Direction,
    },
    Quality(usize),
    RootChord {
        root: usize,
        quality: usize,
    },
    Inversion {
        root: Option<usize>,
        quality: usize,
        inversion: usize,
    },
}

pub struct Exercise {
    pub spelling: NoteSpelling,
    pub skill_id: String,
    pub title: String,
    pub prompt: String,
    pub prompt_highlights: Vec<PromptHighlight>,
    pub explanation: String,
    pub variant: String,
    pub answer: Answer,
    pub playback: Vec<(u64, Event)>,
    pub bpm: Option<u32>,
    pub answer_after_ms: Option<u64>,
    pub metronome: bool,
}

pub(crate) struct Random(pub(crate) u64);
impl Random {
    pub(crate) fn take(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 as usize % n
    }
}
fn notes_text(notes: &[usize], spelling: &NoteSpelling) -> String {
    notes
        .iter()
        .map(|&n| spelling.note_name(n))
        .collect::<Vec<_>>()
        .join(" → ")
}
fn sequence(notes: &[usize], start: u64) -> Vec<(u64, Event)> {
    notes
        .iter()
        .enumerate()
        .flat_map(|(i, &note)| {
            [
                (
                    start + i as u64 * 550,
                    Event::Note {
                        channel: 15,
                        note,
                        velocity: 90,
                    },
                ),
                (
                    start + i as u64 * 550 + 400,
                    Event::Note {
                        channel: 15,
                        note,
                        velocity: 0,
                    },
                ),
            ]
        })
        .collect()
}
fn together(notes: &[usize]) -> Vec<(u64, Event)> {
    [0, 1000]
        .into_iter()
        .flat_map(|time| {
            notes.iter().map(move |&note| {
                (
                    time,
                    Event::Note {
                        channel: 15,
                        note,
                        velocity: if time == 0 { 90 } else { 0 },
                    },
                )
            })
        })
        .collect()
}

impl Exercise {
    pub fn generate(skill: &Skill, seed: u64) -> Self {
        let mut rng = Random(seed.max(1));
        let register = 36 + rng.take(5) * 12;
        let tonic = 48 + rng.take(25);
        let spelling = NoteSpelling::for_task(&skill.task, tonic);
        let notes_text = |notes: &[usize]| notes_text(notes, &spelling);
        let mut exercise = Self {
            spelling: NoteSpelling::default(),
            skill_id: skill.id.clone(),
            title: String::new(),
            prompt: String::new(),
            prompt_highlights: vec![],
            explanation: String::new(),
            variant: String::new(),
            answer: Answer::OctaveSequence(vec![]),
            playback: vec![],
            bpm: None,
            answer_after_ms: None,
            metronome: true,
        };
        match skill.task {
            Task::Practice(ref task) => {
                crate::practice::generate(task, seed, &mut exercise);
                return exercise;
            }
            Task::BuildInterval {
                semitones,
                direction,
            } => {
                let target = if direction == Direction::Down {
                    tonic - semitones
                } else {
                    tonic + semitones
                };
                let notes = vec![tonic, target];
                exercise.title = "INTERVAL CONSTRUCTION".into();
                exercise.prompt.push_str("Play ");
                exercise.emphasize(ROOTS[tonic % 12], PromptRole::Note);
                exercise.prompt.push_str(", then a ");
                exercise.emphasize(INTERVALS[semitones - 1], PromptRole::Interval);
                exercise.prompt.push(' ');
                if direction == Direction::Down {
                    exercise.emphasize("below", PromptRole::Below);
                } else {
                    exercise.emphasize("above", PromptRole::Above);
                }
                exercise.prompt.push_str(" it. Any starting octave.");
                exercise.explanation = format!(
                    "Example: {}. Any starting octave is accepted.",
                    notes_text(&notes)
                );
                exercise.variant = format!("{}", tonic % 12);
                exercise.answer = Answer::AnchoredInterval {
                    root: tonic % 12,
                    semitones,
                    direction,
                };
            }
            Task::HearInterval {
                semitones,
                direction,
            } => {
                let mut notes = vec![tonic, tonic + semitones];
                if direction == Direction::Down {
                    notes.reverse();
                }
                exercise.title = "INTERVAL RECOGNITION".into();
                exercise.prompt = "Listen, then play any two notes the same distance apart. Any starting note; either direction.".into();
                exercise.explanation =
                    format!("A {} ({} semitones).", INTERVALS[semitones - 1], semitones);
                exercise.variant = format!("{tonic}");
                exercise.answer = Answer::Interval(semitones);
                exercise.playback = if direction == Direction::Together {
                    together(&notes)
                } else {
                    sequence(&notes, 0)
                };
            }
            Task::BuildChord { root, quality } | Task::HearChord { root, quality } => {
                let notes: Vec<_> = CHORDS[quality]
                    .iter()
                    .map(|n| register + root + n)
                    .collect();
                let hearing = matches!(skill.task, Task::HearChord { .. });
                exercise.title = if hearing {
                    "CHORD RECOGNITION"
                } else {
                    "CHORD CONSTRUCTION"
                }
                .into();
                exercise.prompt = if hearing {
                    format!("Listen, then play a {}-note chord of the same quality. Any key or inversion; play together or roll the chord.",notes.len())
                } else {
                    format!("Play a {} {} chord ({} distinct notes). Any octave or inversion; play together or roll it.", ROOTS[root], QUALITIES[quality],notes.len())
                };
                exercise.explanation = if hearing {
                    format!(
                        "A {} chord. Any root or inversion is accepted.",
                        QUALITIES[quality]
                    )
                } else {
                    format!(
                        "{} {}: {}",
                        ROOTS[root],
                        QUALITIES[quality],
                        notes_text(&notes)
                    )
                };
                exercise.variant = format!("{register}");
                exercise.answer = if hearing {
                    Answer::Quality(quality)
                } else {
                    Answer::RootChord { root, quality }
                };
                if hearing {
                    exercise.playback = together(&notes);
                }
            }
            Task::Scale {
                root,
                minor,
                direction,
            } => {
                let degrees = if minor {
                    [0, 2, 3, 5, 7, 8, 10, 12]
                } else {
                    [0, 2, 4, 5, 7, 9, 11, 12]
                };
                let mut notes: Vec<_> = degrees.iter().map(|n| register + root + n).collect();
                match direction {
                    Direction::Down => notes.reverse(),
                    Direction::Both => {
                        notes.extend(degrees[..7].iter().rev().map(|n| register + root + n))
                    }
                    _ => {}
                }
                exercise.title = "SCALES".into();
                exercise.prompt = format!(
                    "Play {} {} {}. Start on {} in any octave. One note at a time.",
                    ROOTS[root],
                    if minor { "natural minor" } else { "major" },
                    match direction {
                        Direction::Down => "descending",
                        Direction::Both => "up, then down (one top note)",
                        _ => "ascending",
                    },
                    ROOTS[notes[0] % 12]
                );
                exercise.explanation = notes_text(&notes);
                exercise.variant = format!("{register}");
                exercise.answer = Answer::OctaveSequence(notes);
            }
            Task::ContextInterval {
                root,
                first,
                second,
            } => {
                let degrees = [0, 2, 4, 5, 7, 9, 11, 12];
                let scale: Vec<_> = degrees.iter().map(|n| register + root + n).collect();
                let notes = vec![scale[first], scale[second]];
                exercise.title = "INTERVALS IN A KEY".into();
                exercise.prompt = format!("Hear a {} major scale, then two notes. Play those two notes in {} major, in order. Any octave.", ROOTS[root], ROOTS[root]);
                exercise.explanation = format!(
                    "Degrees {} and {}: {}",
                    first + 1,
                    second + 1,
                    notes_text(&notes)
                );
                exercise.variant = format!("{register}");
                exercise.playback = sequence(&scale, 0);
                exercise.playback.extend(sequence(&notes, 5200));
                exercise.answer = Answer::PitchClasses(notes);
            }
            Task::Inversion {
                root,
                quality,
                inversion,
                hearing,
            } => {
                let bass = CHORDS[quality][inversion] % 12;
                let mut notes: Vec<_> = CHORDS[quality]
                    .iter()
                    .map(|offset| register + root + bass + (offset + 12 - bass) % 12)
                    .collect();
                notes.sort_unstable();
                let position = if inversion == 0 {
                    "root position".into()
                } else {
                    format!("inversion {inversion}")
                };
                exercise.title = if hearing {
                    "INVERSION RECOGNITION"
                } else {
                    "CHORD INVERSIONS"
                }
                .into();
                exercise.prompt = if hearing {
                    "Hear the root note, then a chord. Play the same chord quality and inversion in any key. The bass note matters.".into()
                } else {
                    format!(
                        "Play {} {} in {position}. Any octave. The bass note matters.",
                        ROOTS[root], QUALITIES[quality]
                    )
                };
                exercise.explanation = format!(
                    "{} · {position}. Example: {}",
                    QUALITIES[quality],
                    notes_text(&notes)
                );
                exercise.variant = format!("{register}");
                exercise.answer = Answer::Inversion {
                    root: if hearing { None } else { Some(root) },
                    quality,
                    inversion,
                };
                if hearing {
                    exercise.playback = sequence(&[register + root], 0);
                    exercise
                        .playback
                        .extend(together(&notes).into_iter().map(|(t, e)| (t + 1000, e)));
                }
            }
            Task::TimedScale {
                root,
                minor,
                direction,
                bpm,
            } => {
                let mut base = skill.clone();
                base.task = Task::Scale {
                    root,
                    minor,
                    direction,
                };
                exercise = Self::generate(&base, seed);
                exercise.title = "SCALES / TEMPO".into();
                exercise.prompt.push_str(&format!(
                    " {bpm} BPM: four count-in beats, then one note per beat."
                ));
                exercise.bpm = Some(bpm);
                let beat = 60000 / u64::from(bpm);
                exercise.answer_after_ms = Some(4 * beat);
                exercise.playback = (0..4)
                    .flat_map(|i| {
                        [
                            (
                                i * beat,
                                Event::Note {
                                    channel: 15,
                                    note: 96,
                                    velocity: if i == 0 { 70 } else { 45 },
                                },
                            ),
                            (
                                i * beat + 60,
                                Event::Note {
                                    channel: 15,
                                    note: 96,
                                    velocity: 0,
                                },
                            ),
                        ]
                    })
                    .collect();
            }
        }
        exercise.spelling = spelling;
        exercise
    }
    fn emphasize(&mut self, text: &str, role: PromptRole) {
        let start = self.prompt.len();
        self.prompt.push_str(text);
        self.prompt_highlights.push(PromptHighlight {
            range: start..self.prompt.len(),
            role,
        });
    }

    pub fn minimum_notes(&self) -> usize {
        match &self.answer {
            Answer::PitchClasses(n) | Answer::OctaveSequence(n) | Answer::TransposedSequence(n) => {
                n.len()
            }
            Answer::Performance(score) => score.note_count(),
            Answer::Interval(_) | Answer::AnchoredInterval { .. } => 2,
            Answer::Quality(q)
            | Answer::RootChord { quality: q, .. }
            | Answer::Inversion { quality: q, .. } => CHORDS[*q].len(),
        }
    }
    pub fn correct(&self, played: &[usize]) -> bool {
        let pcs: BTreeSet<_> = played.iter().map(|n| n % 12).collect();
        let chord = |root: usize, quality: usize| {
            pcs == CHORDS[quality].iter().map(|n| (n + root) % 12).collect()
        };
        match &self.answer {
            Answer::Performance(score) => score.pitch_correct(played),
            Answer::OctaveSequence(expected) | Answer::TransposedSequence(expected) => {
                played.len() == expected.len()
                    && !played.is_empty()
                    && (matches!(self.answer, Answer::TransposedSequence(_))
                        || played[0] % 12 == expected[0] % 12)
                    && played.iter().zip(expected).all(|(a, b)| {
                        *a as i32 - played[0] as i32 == *b as i32 - expected[0] as i32
                    })
            }
            Answer::PitchClasses(expected) => {
                played.len() == expected.len()
                    && played.iter().zip(expected).all(|(a, b)| a % 12 == b % 12)
            }
            Answer::Interval(interval) => {
                played.len() == 2 && played[0].abs_diff(played[1]) == *interval
            }
            Answer::AnchoredInterval {
                root,
                semitones,
                direction,
            } => {
                played.len() == 2
                    && played[0] % 12 == *root
                    && played[0].abs_diff(played[1]) == *semitones
                    && (played[1] < played[0]) == (*direction == Direction::Down)
            }
            Answer::Quality(quality) => (0..12).any(|root| chord(root, *quality)),
            Answer::RootChord { root, quality } => chord(*root, *quality),
            Answer::Inversion {
                root,
                quality,
                inversion,
            } => (0..12).any(|candidate| {
                root.is_none_or(|r| r == candidate)
                    && chord(candidate, *quality)
                    && played.iter().min().is_some_and(|bass| {
                        bass % 12 == (candidate + CHORDS[*quality][*inversion]) % 12
                    })
            }),
        }
    }
    pub fn expected_evidence(&self) -> Vec<usize> {
        match &self.answer {
            Answer::PitchClasses(n) | Answer::OctaveSequence(n) | Answer::TransposedSequence(n) => {
                n.clone()
            }
            Answer::Performance(score) => score.example(),
            Answer::Interval(n) => vec![*n],
            Answer::AnchoredInterval {
                root,
                semitones,
                direction,
            } => {
                let start = 60 + root;
                vec![
                    start,
                    if *direction == Direction::Down {
                        start - semitones
                    } else {
                        start + semitones
                    },
                ]
            }
            Answer::Quality(q) => CHORDS[*q].to_vec(),
            Answer::RootChord { root, quality } => {
                CHORDS[*quality].iter().map(|n| (n + root) % 12).collect()
            }
            Answer::Inversion {
                root,
                quality,
                inversion,
            } => vec![root.unwrap_or(128), *quality, *inversion],
        }
    }
    pub fn timing_correct(&self, onsets: &[std::time::Instant]) -> bool {
        let Some(bpm) = self.bpm else {
            return true;
        };
        if onsets.len() != self.minimum_notes() {
            return false;
        }
        let beat = 60.0 / f64::from(bpm);
        onsets.iter().enumerate().all(|(i, at)| {
            (at.saturating_duration_since(onsets[0]).as_secs_f64() - i as f64 * beat).abs()
                <= beat * 0.25
        })
    }
    pub fn performance_timing_correct(
        &self,
        onsets: &[std::time::Instant],
        start: Option<std::time::Instant>,
    ) -> bool {
        if let Answer::Performance(score) = &self.answer {
            score.timing_correct(onsets, start)
        } else {
            self.timing_correct(onsets)
        }
    }
    pub fn holds_correct(&self, evidence: &[crate::learner::MidiEvidence]) -> bool {
        if let Answer::Performance(score) = &self.answer {
            score.holds_correct(evidence)
        } else {
            true
        }
    }
    pub fn fixed_duration_ms(&self) -> Option<u64> {
        match &self.answer {
            Answer::Performance(score) if score.fixed_start => {
                Some(score.end_ms() + score.tolerance_ms + 450)
            }
            _ => None,
        }
    }
    pub fn relative_duration_ms(&self) -> Option<u64> {
        match &self.answer {
            Answer::Performance(score) if score.timed && !score.fixed_start => {
                Some(score.end_ms() + score.tolerance_ms + 450)
            }
            _ => None,
        }
    }
}
