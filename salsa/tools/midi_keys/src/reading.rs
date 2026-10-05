//! Unheard notation exercises. A single event model drives engraving, grading,
//! and post-attempt playback; hand labels describe parts, not detected hands.
use crate::curriculum::{add, Skill, Task, CHORDS};
use crate::exercise::{Answer, Exercise, Random};
use crate::learner::MidiEvidence;
use crate::performance::{Frame, Performance};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Hand {
    Right,
    Left,
}
impl Hand {
    pub fn id(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Right => "Right hand / treble",
            Self::Left => "Left hand / bass",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voicing {
    Triad,
    RootSeventh,
    ThirdSeventh,
}
impl Voicing {
    pub fn id(self) -> &'static str {
        match self {
            Self::Triad => "triad",
            Self::RootSeventh => "root7",
            Self::ThirdSeventh => "third7",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Triad => "root-position triads",
            Self::RootSeventh => "root–7th shells",
            Self::ThirdSeventh => "3rd–7th shells",
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum ReadingTask {
    Staff {
        hand: Hand,
        level: u8,
    },
    Together {
        level: u8,
    },
    Symbols {
        root: usize,
        quality: usize,
        voicing: Voicing,
    },
    Lead {
        level: u8,
    },
}
impl ReadingTask {
    pub fn stage(self) -> u8 {
        match self {
            Self::Staff { level: 0..=3, .. } => 1,
            _ => 2,
        }
    }
    pub fn lead(self) -> bool {
        matches!(self, Self::Symbols { .. } | Self::Lead { .. })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct WrittenEvent {
    /// Feedback-only marking; never used to grade the response.
    pub incorrect: bool,
    pub hand: Hand,
    /// Eighth-note ticks. Empty choices represent a written rest.
    pub tick: u32,
    pub duration: u32,
    pub tie_tick: Option<u32>,
    /// Alternatives are explicit legal pitches, never unbounded transpositions.
    pub choices: Vec<Vec<usize>>,
}
impl WrittenEvent {
    pub fn notes(&self) -> &[usize] {
        self.choices.first().map(Vec::as_slice).unwrap_or(&[])
    }
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct ChordSymbol {
    pub tick: u32,
    pub text: String,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct WrittenScore {
    /// Presentation-only position of the currently sounding bar.
    pub active_tick: Option<u32>,
    /// Bars kept together on one page; simple accompaniment uses a full chart.
    pub bars_per_page: u32,
    pub written_spelling: Option<crate::NoteSpelling>,
    pub caption: String,
    pub events: Vec<WrittenEvent>,
    pub chords: Vec<ChordSymbol>,
    pub hands: Vec<Hand>,
    pub key_fifths: i8,
    pub ticks: u32,
    pub bpm: Option<u32>,
    pub lead: bool,
    pub symbols_only: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadingResult {
    pub right: Option<bool>,
    pub left: Option<bool>,
    pub symbols: Option<bool>,
    pub timing: Option<bool>,
    pub holds: Option<bool>,
    pub coordination: Option<bool>,
    pub details: Vec<String>,
}
impl ReadingResult {
    pub fn components(&self) -> Vec<(&'static str, bool)> {
        let mut result = vec![];
        for (label, value) in [
            ("timing", self.timing),
            ("holds", self.holds),
            ("right", self.right),
            ("left", self.left),
            ("symbols", self.symbols),
            ("coordination", self.coordination),
        ] {
            if let Some(value) = value {
                result.push((label, value));
            }
        }
        result
    }
}
fn staff_id(hand: Hand, level: u8) -> String {
    format!("reading.{}.{}", hand.id(), level)
}
fn symbol_id(root: usize, quality: usize, voicing: Voicing) -> String {
    format!("reading.symbol.{root}.{quality}.{}", voicing.id())
}
const LEVELS: [&str; 11] = [
    "single notes · five-note position",
    "single notes · wider register",
    "four-note steps · untimed",
    "four-note skips · untimed",
    "quarter-note melodies · 60 BPM",
    "half notes and rests · 60 BPM",
    "eighth notes and rests · 60 BPM",
    "G major · one sharp",
    "F major · one flat",
    "familiar triads · 60 BPM",
    "ties across a barline · 60 BPM",
];
pub fn extend(graph: &mut Vec<Skill>) {
    for hand in [Hand::Right, Hand::Left] {
        for level in 0..11 {
            let mut deps = if level == 0 {
                vec![]
            } else {
                vec![staff_id(hand, level - 1)]
            };
            if level == 10 {
                deps = vec![staff_id(hand, 6), "rhythm.3.60".into()];
            }
            if level == 4 {
                deps.push("rhythm.0.60".into());
            }
            if level == 9 {
                deps.extend([0, 5, 7].map(|r| format!("chord.build.{r}.0")));
            }
            add(
                graph,
                staff_id(hand, level),
                format!("Read · {} · {}", hand.label(), LEVELS[level as usize]),
                Task::Reading(ReadingTask::Staff { hand, level }),
                &deps,
            );
        }
    }
    for (level, label) in [
        "alternating hands · untimed",
        "simultaneous notes · untimed",
        "melody over held bass",
        "independent rhythms",
        "melody with chord accompaniment",
    ]
    .iter()
    .enumerate()
    {
        let mut deps = vec![
            staff_id(Hand::Right, if level < 2 { 3 } else { 6 }),
            staff_id(Hand::Left, if level < 2 { 3 } else { 6 }),
        ];
        if level > 0 {
            deps.push(format!("reading.together.{}", level - 1));
        }
        if level == 4 {
            deps.push(staff_id(Hand::Left, 9));
        }
        add(
            graph,
            format!("reading.together.{level}"),
            format!("Read · hands together · {label}"),
            Task::Reading(ReadingTask::Together { level: level as u8 }),
            &deps,
        );
    }
    for (root, quality) in [(0, 0), (5, 0), (7, 0), (0, 5), (5, 5), (7, 4)] {
        let voicings: &[Voicing] = if quality == 0 {
            &[Voicing::Triad]
        } else {
            &[Voicing::RootSeventh, Voicing::ThirdSeventh]
        };
        for &voicing in voicings {
            let mut deps = vec![
                format!("chord.build.{root}.{quality}"),
                staff_id(Hand::Left, 3),
            ];
            if voicing == Voicing::ThirdSeventh {
                deps.push(symbol_id(root, quality, Voicing::RootSeventh));
            }
            add(
                graph,
                symbol_id(root, quality, voicing),
                format!(
                    "Read chord symbol · {} · {}",
                    chord_name(root, quality),
                    voicing.label()
                ),
                Task::Reading(ReadingTask::Symbols {
                    root,
                    quality,
                    voicing,
                }),
                &deps,
            );
        }
    }
    for (level, label) in [
        "melody over roots",
        "melody over familiar triads",
        "triad changes by bar",
        "melody over root–7th shells",
        "shell changes by bar",
        "rhythmic 3rd–7th shells",
    ]
    .iter()
    .enumerate()
    {
        let voicing = if level < 3 {
            Voicing::Triad
        } else if level < 5 {
            Voicing::RootSeventh
        } else {
            Voicing::ThirdSeventh
        };
        let mut deps = vec![format!(
            "reading.together.{}",
            if level == 0 { 2 } else { 4 }
        )];
        deps.extend([0, 5, 7].map(|root| {
            symbol_id(
                root,
                if level < 3 {
                    0
                } else if root == 7 {
                    4
                } else {
                    5
                },
                voicing,
            )
        }));
        if level > 0 {
            deps.push(format!("reading.lead.{}", level - 1));
        }
        add(
            graph,
            format!("reading.lead.{level}"),
            format!("Lead sheet · {label}"),
            Task::Reading(ReadingTask::Lead { level: level as u8 }),
            &deps,
        );
    }
    // A lead sheet must not introduce an unfamiliar voicing at the same time
    // as notation/coordination. Require established symbol/voicing execution.
    for skill in graph
        .iter_mut()
        .filter(|s| matches!(s.task, Task::Reading(ReadingTask::Lead { .. })))
    {
        for requirement in skill
            .requires
            .iter_mut()
            .filter(|r| r.skill.starts_with("reading.symbol."))
        {
            requirement.score = 9.0;
            requirement.attempts = 12;
        }
    }
}
fn chord_name(root: usize, quality: usize) -> String {
    format!(
        "{}{}",
        crate::curriculum::ROOTS[root],
        match quality {
            4 => "7",
            5 => "maj7",
            _ => "",
        }
    )
}
fn voice_choices(root: usize, quality: usize, voicing: Voicing) -> Vec<Vec<usize>> {
    let intervals = match voicing {
        Voicing::Triad => CHORDS[quality].to_vec(),
        Voicing::RootSeventh => vec![0, CHORDS[quality][3]],
        Voicing::ThirdSeventh => vec![CHORDS[quality][1], CHORDS[quality][3]],
    };
    [36, 24, 48]
        .into_iter()
        .filter_map(|base| {
            let notes: Vec<_> = intervals.iter().map(|n| base + root + n).collect();
            notes.iter().all(|n| *n < 60).then_some(notes)
        })
        .collect()
}
fn event(hand: Hand, tick: u32, duration: u32, notes: Vec<usize>) -> WrittenEvent {
    WrittenEvent {
        incorrect: false,
        hand,
        tick,
        duration,
        tie_tick: None,
        choices: if notes.is_empty() {
            vec![]
        } else {
            vec![notes]
        },
    }
}

pub fn generate(task: ReadingTask, seed: u64, ex: &mut Exercise) {
    let mut rng = Random(seed.max(1));
    let mut score = WrittenScore {
        active_tick: None,
        bars_per_page: 2,
        written_spelling: None,
        caption: "Read the written pitches, including octaves.".into(),
        events: vec![],
        chords: vec![],
        hands: vec![],
        key_fifths: 0,
        ticks: 8,
        bpm: None,
        lead: task.lead(),
        symbols_only: matches!(task, ReadingTask::Symbols { .. }),
    };
    let instruction;
    match task {
        ReadingTask::Staff { hand, level } => {
            score.hands.push(hand);
            score.bpm = (level >= 4).then_some(60);
            score.ticks = if level < 2 {
                2
            } else if level < 4 {
                8
            } else {
                16
            };
            score.key_fifths = match level {
                7 => 1,
                8 => -1,
                _ => 0,
            };
            let root = match level {
                7 => 7,
                8 => 5,
                _ => 0,
            };
            let base = if hand == Hand::Right {
                60
            } else if level >= 7 {
                36
            } else {
                48
            };
            let scale = [0, 2, 4, 5, 7, 9, 11, 12];
            if level < 2 {
                let note = if level == 0 {
                    base + scale[rng.take(5)]
                } else {
                    let notes = if hand == Hand::Right {
                        vec![60, 62, 64, 65, 67, 69, 71, 72, 74, 76, 77, 79]
                    } else {
                        vec![36, 38, 40, 41, 43, 45, 47, 48, 50, 52, 53, 55, 57, 59]
                    };
                    notes[rng.take(notes.len())]
                };
                score.events.push(event(hand, 0, 2, vec![note]));
            } else if level == 9 {
                for (i, root) in [0, 5, 7, 0].into_iter().enumerate() {
                    let chord: Vec<_> = CHORDS[0].iter().map(|n| base + root + n).collect();
                    score.events.push(event(hand, i as u32 * 4, 4, chord));
                }
            } else {
                let rhythm: Vec<(u32, u32, bool)> = match level {
                    5 => vec![
                        (0, 4, false),
                        (4, 2, false),
                        (6, 2, true),
                        (8, 4, false),
                        (12, 4, false),
                    ],
                    10 => vec![
                        (0, 2, false),
                        (2, 2, false),
                        (4, 2, false),
                        (6, 4, false),
                        (10, 2, false),
                        (12, 4, false),
                    ],
                    6 => vec![
                        (0, 2, false),
                        (2, 1, false),
                        (3, 1, false),
                        (4, 2, true),
                        (6, 2, false),
                        (8, 4, false),
                        (12, 1, false),
                        (13, 1, false),
                        (14, 2, false),
                    ],
                    _ => (0..score.ticks / 2).map(|i| (i * 2, 2, false)).collect(),
                };
                let mut degree = rng.take(5);
                for (tick, duration, rest) in rhythm {
                    score.events.push(event(
                        hand,
                        tick,
                        duration,
                        if rest {
                            vec![]
                        } else {
                            vec![base + root + scale[degree]]
                        },
                    ));
                    let step = if level == 3 { 2 } else { 1 };
                    if level == 10 && tick == 6 {
                        score.events.last_mut().unwrap().tie_tick = Some(8);
                    }
                    degree = if rng.take(2) == 0 {
                        degree.saturating_sub(step)
                    } else {
                        (degree + step).min(if hand == Hand::Left && base == 48 {
                            6
                        } else {
                            7
                        })
                    };
                }
            }
            instruction = format!(
                "{}: read the staff and play the written pitches, including octaves.",
                hand.label()
            );
        }
        ReadingTask::Together { level } => {
            score.hands = vec![Hand::Right, Hand::Left];
            score.bpm = (level >= 2).then_some(60);
            score.ticks = 16;
            let melody = [60, 62, 64, 67, 65, 64, 62, 60];
            for (i, &note) in melody.iter().enumerate() {
                let note = if rng.take(3) == 0 {
                    melody[(i + 1) % melody.len()]
                } else {
                    note
                };
                if level == 0 {
                    let hand = if i % 2 == 0 { Hand::Right } else { Hand::Left };
                    score.events.push(event(
                        hand,
                        i as u32 * 2,
                        2,
                        vec![if hand == Hand::Right { note } else { note - 24 }],
                    ));
                    score.events.push(event(
                        if hand == Hand::Right {
                            Hand::Left
                        } else {
                            Hand::Right
                        },
                        i as u32 * 2,
                        2,
                        vec![],
                    ));
                } else {
                    score
                        .events
                        .push(event(Hand::Right, i as u32 * 2, 2, vec![note]));
                    if level == 1 {
                        score
                            .events
                            .push(event(Hand::Left, i as u32 * 2, 2, vec![note - 24]));
                    }
                }
            }
            if level >= 2 {
                let duration = if level == 2 { 8 } else { 4 };
                for tick in (0..16).step_by(duration as usize) {
                    let root = if tick < 8 { 0 } else { 7 };
                    let notes = if level == 4 {
                        CHORDS[0].iter().map(|n| 36 + root + n).collect()
                    } else {
                        vec![48 + if tick < 8 { 0 } else { 7 }]
                    };
                    score.events.push(event(Hand::Left, tick, duration, notes));
                }
            }
            instruction="Read both staves. Play the right-hand and left-hand parts together; keep long notes held while the other part moves.".into();
        }
        ReadingTask::Symbols {
            root,
            quality,
            voicing,
        } => {
            score.hands = vec![Hand::Left];
            score.chords.push(ChordSymbol {
                tick: 0,
                text: chord_name(root, quality),
            });
            score.events.push(WrittenEvent {
                incorrect: false,
                hand: Hand::Left,
                tick: 0,
                duration: 8,
                tie_tick: None,
                choices: voice_choices(root, quality, voicing),
            });
            instruction=format!("Read the chord symbol. Left hand: play {} together below middle C; either available octave is accepted.",voicing.label());
        }
        ReadingTask::Lead { level } => {
            score.hands = vec![Hand::Right, Hand::Left];
            score.ticks = 16;
            score.bpm = Some(60);
            let progression = if level == 0 || level == 1 || level == 3 {
                [0, 0]
            } else if rng.take(2) == 0 {
                [0, 7]
            } else {
                [5, 0]
            };
            for (bar, root) in progression.into_iter().enumerate() {
                let tick = bar as u32 * 8;
                let quality = if level < 3 {
                    0
                } else if root == 7 {
                    4
                } else {
                    5
                };
                score.chords.push(ChordSymbol {
                    tick,
                    text: chord_name(root, quality),
                });
                for i in 0..4 {
                    let pitches = [0, 2, 4, 5, 7, 9, 11];
                    score.events.push(event(
                        Hand::Right,
                        tick + i * 2,
                        2,
                        vec![60 + pitches[rng.take(7)]],
                    ));
                }
                let voicing = if level < 3 {
                    Voicing::Triad
                } else if level < 5 {
                    Voicing::RootSeventh
                } else {
                    Voicing::ThirdSeventh
                };
                let choices = if level == 0 {
                    vec![vec![36 + root], vec![48 + root]]
                } else {
                    voice_choices(root, quality, voicing)
                };
                let duration = if level == 5 { 4 } else { 8 };
                for offset in (0..8).step_by(duration as usize) {
                    score.events.push(WrittenEvent {
                        incorrect: false,
                        hand: Hand::Left,
                        tick: tick + offset,
                        duration,
                        tie_tick: None,
                        choices: choices.clone(),
                    });
                }
            }
            instruction=format!("Read the melody with your right hand. Left hand: {} below middle C, {}. Use the chord symbols; accompaniment notes are not printed.",if level==0 {"roots"}else if level<3 {"root-position triads"}else if level<5 {"root–7th shells"}else{"3rd–7th shells"},if level==5 {"on beats 1 and 3"}else{"once per bar, held for the whole bar"});
        }
    }
    score.events.sort_by_key(|e| e.tick);
    ex.title = if score.lead {
        "LEAD-SHEET READING"
    } else {
        "SIGHT READING"
    }
    .into();
    ex.prompt = format!(
        "{instruction} {}",
        if score.bpm.is_some() {
            "60 BPM. Listen to the click, then start on any beat when ready."
        } else {
            "Untimed. Completes after the last expected note."
        }
    );
    ex.explanation =
        "Compare your answer with the written score. e plays an example; r retries the exercise."
            .into();
    ex.variant = format!("reading:{:?}:{:?}", score.events, score.chords);
    ex.bpm = score.bpm;
    ex.metronome = score.bpm.is_some();
    ex.answer = Answer::Performance(score.performance(None));
    ex.spelling = score.spelling();
    ex.reading = Some(score);
}
impl WrittenScore {
    pub fn page_ticks(&self) -> u32 {
        self.bars_per_page.max(1) * 8
    }
    pub fn page_count(&self) -> usize {
        self.ticks.div_ceil(self.page_ticks()).max(1) as usize
    }
    pub fn page(&self, page: usize) -> Self {
        let start = page as u32 * self.page_ticks();
        if start >= self.ticks {
            let mut blank = self.clone();
            blank.events.clear();
            blank.chords.clear();
            blank.ticks = self.page_ticks();
            return blank;
        }
        let end = (start + self.page_ticks()).min(self.ticks);
        let mut result = self.clone();
        result.ticks = end - start;
        result.active_tick = self
            .active_tick
            .filter(|t| *t >= start && *t < end)
            .map(|t| t - start);
        result.events = self
            .events
            .iter()
            .filter(|e| e.tick < end && e.tick + e.duration > start)
            .map(|e| {
                let mut e = e.clone();
                let stop = (e.tick + e.duration).min(end);
                e.tick = e.tick.max(start) - start;
                e.duration = stop - start - e.tick;
                e.tie_tick = e
                    .tie_tick
                    .filter(|t| *t > start && *t < end)
                    .map(|t| t - start);
                e
            })
            .collect();
        result.chords = self
            .chords
            .iter()
            .filter(|c| c.tick >= start && c.tick < end)
            .map(|c| ChordSymbol {
                tick: c.tick - start,
                text: c.text.clone(),
            })
            .collect();
        result
    }
    pub fn spelling(&self) -> crate::NoteSpelling {
        if let Some(spelling) = &self.written_spelling {
            return spelling.clone();
        }
        crate::NoteSpelling::for_scale(
            if self.key_fifths > 0 {
                7
            } else if self.key_fifths < 0 {
                5
            } else {
                0
            },
            crate::music::Scale::Major,
        )
    }
    pub fn milliseconds(&self, ticks: u32) -> u64 {
        u64::from(ticks) * 30000 / u64::from(self.bpm.unwrap_or(60))
    }
    pub fn performance(&self, hand: Option<Hand>) -> Performance {
        let mut frames: Vec<Frame> = vec![];
        for e in self
            .events
            .iter()
            .filter(|e| !e.notes().is_empty() && hand.is_none_or(|h| e.hand == h))
        {
            let at = self.milliseconds(e.tick);
            if let Some(frame) = frames.last_mut().filter(|f| f.at_ms == at) {
                frame.choices = frame
                    .choices
                    .iter()
                    .flat_map(|a| {
                        e.choices.iter().map(move |b| {
                            let mut c = a.clone();
                            c.extend(b);
                            c
                        })
                    })
                    .collect();
            } else {
                frames.push(Frame {
                    at_ms: at,
                    hold_ms: None,
                    choices: e.choices.clone(),
                });
            }
        }
        Performance {
            frames,
            accompaniment: false,
            exact_register: true,
            any_pitch: false,
            timed: self.bpm.is_some(),
            fixed_start: false,
            tolerance_ms: 180,
            smooth: false,
            preserve_contour: false,
        }
    }
    pub fn end_ms(&self) -> u64 {
        self.milliseconds(self.ticks) + 650
    }
    /// Reading assesses written duration, not the playback's articulation gap.
    /// Unmarked notes allow detached playing: up to half a beat of silence,
    /// capped at half the note value. Long bass notes and ties still require
    /// sustained holds. A quarter beat of overlap is allowed at the release.
    pub fn hold_window_ms(&self, duration: u32) -> (u64, u64) {
        let nominal = self.milliseconds(duration);
        let shortest = self.milliseconds(duration.min(2));
        (nominal.saturating_sub(shortest / 2), nominal + shortest / 4)
    }

    pub fn hold_matches(&self, duration: u32, held: Option<u64>) -> bool {
        let (min, max) = self.hold_window_ms(duration);
        held.is_some_and(|held| (min..=max).contains(&held))
    }

    pub fn holds_correct(&self, evidence: &[MidiEvidence]) -> bool {
        if self.bpm.is_none() {
            return true;
        }
        for hand in &self.hands {
            let attacks: Vec<_> = evidence
                .iter()
                .enumerate()
                .filter(|(_, e)| e.velocity > 0 && ((*hand == Hand::Right) == (e.note >= 60)))
                .collect();
            let mut offset = 0;
            for e in self
                .events
                .iter()
                .filter(|e| e.hand == *hand && !e.notes().is_empty())
            {
                let count = e.notes().len();
                let Some(group) = attacks.get(offset..offset + count) else {
                    return false;
                };
                for &(index, _) in group {
                    if !self.hold_matches(e.duration, crate::evidence::held_ms(evidence, index)) {
                        return false;
                    }
                }
                offset += count;
            }
            if offset != attacks.len() {
                return false;
            }
        }
        true
    }
    pub fn evaluate(
        &self,
        played: &[usize],
        onsets: &[Instant],
        evidence: &[MidiEvidence],
    ) -> ReadingResult {
        let part = |hand| {
            let notes: Vec<_> = played
                .iter()
                .copied()
                .filter(|n| (hand == Hand::Right) == (*n >= 60))
                .collect();
            self.performance(Some(hand)).pitch_correct(&notes)
        };
        let right = self.hands.contains(&Hand::Right).then(|| part(Hand::Right));
        let left = self.hands.contains(&Hand::Left).then(|| part(Hand::Left));
        let timing = self.performance(None).timing_correct(onsets, None);
        let holds = self.holds_correct(evidence);
        let mut details = vec![];
        for hand in &self.hands {
            let actual = played
                .iter()
                .copied()
                .filter(|n| (*hand == Hand::Right) == (*n >= 60))
                .map(|n| self.spelling().note_name(n))
                .collect::<Vec<_>>()
                .join(" ");
            let expected = self
                .events
                .iter()
                .filter(|e| e.hand == *hand && !e.notes().is_empty())
                .map(|e| {
                    let choices = e
                        .choices
                        .iter()
                        .map(|c| {
                            c.iter()
                                .copied()
                                .map(|n| self.spelling().note_name(n))
                                .collect::<Vec<_>>()
                                .join("+")
                        })
                        .collect::<Vec<_>>();
                    choices.join(" or ")
                })
                .collect::<Vec<_>>()
                .join(" → ");
            details.push(format!("{} expected: {expected}", hand.id()));
            details.push(format!(
                "{} played: {}",
                hand.id(),
                if actual.is_empty() { "—" } else { &actual }
            ));
        }
        if self.bpm.is_some() {
            let first = evidence
                .iter()
                .find(|e| e.velocity > 0)
                .map_or(0, |e| e.offset_ms);
            for hand in &self.hands {
                let attacks: Vec<_> = evidence
                    .iter()
                    .enumerate()
                    .filter(|(_, e)| e.velocity > 0 && ((*hand == Hand::Right) == (e.note >= 60)))
                    .collect();
                let expected: Vec<_> = self
                    .events
                    .iter()
                    .filter(|e| e.hand == *hand)
                    .flat_map(|e| e.notes().iter().map(move |_| e))
                    .collect();
                let expected_times: Vec<_> = expected
                    .iter()
                    .map(|e| self.milliseconds(e.tick) as i64)
                    .collect();
                let actual_times: Vec<_> = attacks
                    .iter()
                    .map(|(_, e)| e.offset_ms as i64 - first as i64)
                    .collect();
                for (expected_index, actual_index) in
                    crate::rhythm::align_attacks(&expected_times, &actual_times, self.bpm.unwrap())
                {
                    let Some(expected_index) = expected_index else {
                        let on = attacks[actual_index.unwrap()].1;
                        details.push(format!(
                            "{} extra {} at +{}ms",
                            hand.id(),
                            self.spelling().note_name(on.note),
                            on.offset_ms.saturating_sub(first)
                        ));
                        continue;
                    };
                    let event = expected[expected_index];
                    let beat = 1.0 + f64::from(event.tick) / 2.;
                    let Some(actual_index) = actual_index else {
                        details.push(format!("{} beat {beat:.1}: missing note", hand.id()));
                        continue;
                    };
                    let (index, _) = attacks[actual_index];
                    let delta = actual_times[actual_index] - expected_times[expected_index];
                    let next = crate::evidence::next_key_event(evidence, index);
                    let hold = crate::evidence::held_ms(evidence, index);
                    let expected = self.milliseconds(event.duration);
                    let (min, max) = self.hold_window_ms(event.duration);
                    if delta.unsigned_abs() > 180 || !self.hold_matches(event.duration, hold) {
                        let held = hold.map(|h| format!("{h}ms")).unwrap_or_else(|| {
                            if next.is_some() {
                                "retriggered before release".into()
                            } else {
                                "no release".into()
                            }
                        });
                        details.push(format!(
                            "{} beat {beat:.1}: {}ms {}; held {held} / {expected}ms (allowed {min}–{max}ms; attack ±180ms)",
                            hand.id(),
                            delta.unsigned_abs(),
                            if delta < 0 { "early" } else { "late" }
                        ));
                    }
                }
            }
        }
        ReadingResult {
            right,
            left,
            symbols: self.lead.then_some(left.unwrap_or(false)),
            timing: self.bpm.map(|_| timing),
            holds: self.bpm.map(|_| holds),
            coordination: (self.hands.len() == 2).then_some(timing && holds),
            details,
        }
    }
    pub fn playback(&self) -> Vec<(u64, keyboard::Event)> {
        let mut events = vec![];
        for e in &self.events {
            for &note in e.notes() {
                let at = self.milliseconds(e.tick);
                events.push((
                    at,
                    keyboard::Event::Note {
                        channel: 15,
                        note,
                        velocity: 85,
                    },
                ));
                events.push((
                    at + self.milliseconds(e.duration).saturating_sub(60),
                    keyboard::Event::Note {
                        channel: 15,
                        note,
                        velocity: 0,
                    },
                ));
            }
        }
        events.sort_by_key(|(at, event)| {
            (
                *at,
                matches!(event, keyboard::Event::Note { velocity: 1.., .. }),
            )
        });
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Phase, Session};
    use keyboard::Event;
    use std::time::Duration;
    fn perform(
        score: &WrittenScore,
        start: Instant,
    ) -> (Vec<usize>, Vec<Instant>, Vec<MidiEvidence>) {
        let mut evidence = vec![];
        for e in &score.events {
            for &note in e.notes() {
                evidence.push(MidiEvidence {
                    offset_ms: score.milliseconds(e.tick),
                    channel: 0,
                    note,
                    velocity: 90,
                });
                evidence.push(MidiEvidence {
                    offset_ms: score.milliseconds(e.tick + e.duration) - 60,
                    channel: 0,
                    note,
                    velocity: 0,
                });
            }
        }
        evidence.sort_by_key(|e| (e.offset_ms, e.velocity > 0));
        let notes = evidence
            .iter()
            .filter(|e| e.velocity > 0)
            .map(|e| e.note)
            .collect();
        let onsets = evidence
            .iter()
            .filter(|e| e.velocity > 0)
            .map(|e| start + Duration::from_millis(e.offset_ms))
            .collect();
        (notes, onsets, evidence)
    }
    #[test]
    fn timed_reading_clicks_before_input_and_does_not_restart_on_first_note() {
        let mut session = Session::preview("reading.together.2").unwrap();
        let now = Instant::now();
        session.start(now);
        let clicks = |events: Vec<Event>| {
            events
                .into_iter()
                .filter(|e| matches!(e, Event::MetronomeClick))
                .count()
        };
        assert_eq!(clicks(session.tick(now).unwrap()), 1);
        assert_eq!(
            clicks(session.tick(now + Duration::from_secs(5)).unwrap()),
            1
        );
        assert!(session.played.is_empty());
        assert_eq!(session.phase, Phase::Answering);
        assert!(
            session.finish_at.is_none(),
            "waiting for the beat must not start the exercise timer"
        );
        session.input(
            Event::Note {
                channel: 0,
                note: 48,
                velocity: 80,
            },
            now + Duration::from_millis(5250),
        );
        assert_eq!(
            clicks(session.tick(now + Duration::from_millis(5250)).unwrap()),
            0
        );
        assert_eq!(
            clicks(session.tick(now + Duration::from_secs(6)).unwrap()),
            1
        );
        session.clear_answer();
        assert_eq!(
            clicks(session.tick(now + Duration::from_secs(7)).unwrap()),
            1
        );
        session.show_browser();
        assert_eq!(
            clicks(session.tick(now + Duration::from_secs(8)).unwrap()),
            0
        );
    }
    #[test]
    fn unmarked_quarters_accept_detached_playing_and_show_neutral_rests() {
        let session = Session::preview("reading.together.2").unwrap();
        let ex = session.exercise.as_ref().unwrap();
        let score = ex.reading.as_ref().unwrap();
        let (notes, onsets, mut evidence) = perform(score, Instant::now());
        for off in evidence
            .iter_mut()
            .filter(|e| e.velocity == 0 && e.note >= 60)
        {
            off.offset_ms -= 340; // 600ms holds, leaving 400ms between quarters.
        }
        evidence.sort_by_key(|e| e.offset_ms);
        assert!(score
            .evaluate(&notes, &onsets, &evidence)
            .components()
            .iter()
            .all(|(_, ok)| *ok));
        // Exercise the annotation logic without its all-correct shortcut.
        let response = crate::score_support::response(ex, &notes, &evidence, 0, false);
        assert!(response.events.iter().any(|e| e.notes().is_empty()));
        assert!(response.events.iter().all(|e| !e.incorrect));
    }
    #[test]
    fn natural_legato_capture_passes_with_consistent_feedback_and_highlights() {
        let session = Session::preview("reading.together.2").unwrap();
        let ex = session.exercise.as_ref().unwrap();
        let score = ex.reading.as_ref().unwrap();
        let evidence: Vec<_> = [
            (0, 48, 51),
            (11, 62, 74),
            (1006, 64, 69),
            (1020, 62, 0),
            (2015, 67, 69),
            (2025, 64, 0),
            (2971, 65, 73),
            (2989, 67, 0),
            (3857, 65, 0),
            (3944, 65, 71),
            (3972, 55, 46),
            (4155, 48, 0),
            (4914, 65, 0),
            (4929, 62, 62),
            (5828, 62, 0),
            (5961, 62, 64),
            (6862, 60, 64),
            (6899, 62, 0),
            (7953, 60, 0),
            (7979, 55, 0),
        ]
        .into_iter()
        .map(|(offset_ms, note, velocity)| MidiEvidence {
            offset_ms,
            note,
            velocity,
            channel: 0,
        })
        .collect();
        let start = Instant::now();
        let notes: Vec<_> = evidence
            .iter()
            .filter(|e| e.velocity > 0)
            .map(|e| e.note)
            .collect();
        let onsets: Vec<_> = evidence
            .iter()
            .filter(|e| e.velocity > 0)
            .map(|e| start + Duration::from_millis(e.offset_ms))
            .collect();
        assert!(ex.correct(&notes));
        assert!(ex.holds_correct(&evidence));
        let result = score.evaluate(&notes, &onsets, &evidence);
        assert!(result.components().iter().all(|(_, ok)| *ok));
        assert_eq!(result.details.len(), 4);
        let response = crate::score_support::response(ex, &notes, &evidence, 0, false);
        assert!(response.events.iter().all(|e| !e.incorrect));
    }

    #[test]
    fn reading_hold_windows_scale_with_tempo_and_still_reject_wrong_lengths() {
        let session = Session::preview("reading.together.2").unwrap();
        let mut score = session.exercise.as_ref().unwrap().reading.clone().unwrap();
        for bpm in [60, 120] {
            score.bpm = Some(bpm);
            for duration in [1, 2, 4, 8] {
                let (min, max) = score.hold_window_ms(duration);
                assert!(score.hold_matches(duration, Some(min)));
                assert!(score.hold_matches(duration, Some(max)));
                assert!(!score.hold_matches(duration, Some(min - 1)));
                assert!(!score.hold_matches(duration, Some(max + 1)));
                assert!(!score.hold_matches(duration, None));
                assert!(!score.hold_matches(duration, Some(score.milliseconds(duration) / 3)));
            }
        }
        score.bpm = Some(60);
        assert_eq!(score.hold_window_ms(8), (3500, 4250));
        assert_eq!(score.hold_window_ms(2), (500, 1250));
        assert_eq!(score.hold_window_ms(1), (250, 625));
    }
    #[test]
    fn extra_bass_attack_does_not_shift_later_feedback() {
        let session = Session::preview("reading.together.2").unwrap();
        let mut score = session.exercise.as_ref().unwrap().reading.clone().unwrap();
        score.events = vec![
            event(Hand::Left, 0, 8, vec![48]),
            event(Hand::Left, 8, 8, vec![55]),
        ];
        score.hands = vec![Hand::Left];
        let evidence: Vec<_> = [
            (0, 48, 52),
            (114, 48, 15),
            (3959, 55, 46),
            (4269, 48, 0),
            (4282, 48, 0),
            (7927, 55, 0),
        ]
        .into_iter()
        .map(|(offset_ms, note, velocity)| MidiEvidence {
            offset_ms,
            note,
            velocity,
            channel: 0,
        })
        .collect();
        let start = Instant::now();
        let onsets: Vec<_> = evidence
            .iter()
            .filter(|e| e.velocity > 0)
            .map(|e| start + Duration::from_millis(e.offset_ms))
            .collect();
        let result = score.evaluate(&[48, 48, 55], &onsets, &evidence);
        assert_eq!(result.left, Some(false));
        assert!(result
            .details
            .iter()
            .any(|s| s == "left extra C3 at +114ms"));
        assert!(result
            .details
            .iter()
            .any(|s| s.contains("retriggered before release")));
        assert!(
            !result.details.iter().any(|s| s.contains("beat 5.0")),
            "{:?}",
            result.details
        );
        let evidence: Vec<_> = evidence.into_iter().filter(|e| e.note == 55).collect();
        // Preserve the global exercise anchor with the right hand.
        let mut evidence = evidence;
        evidence.insert(
            0,
            MidiEvidence {
                offset_ms: 0,
                note: 62,
                velocity: 80,
                channel: 0,
            },
        );
        let result = score.evaluate(&[55], &onsets, &evidence);
        assert!(result
            .details
            .iter()
            .any(|s| s == "left beat 1.0: missing note"));
        assert!(!result.details.iter().any(|s| s.contains("beat 5.0")));
    }
    #[test]
    fn every_reading_score_is_unheard_gradeable_and_stays_in_its_part_register() {
        for skill in crate::curriculum::curriculum()
            .iter()
            .filter(|s| matches!(s.task, Task::Reading(_)))
        {
            for seed in 1..12 {
                let ex = Exercise::generate(skill, seed);
                assert!(ex.playback.is_empty());
                let score = ex.reading.as_ref().unwrap();
                for event in &score.events {
                    for choice in &event.choices {
                        assert!(
                            choice
                                .iter()
                                .all(|n| *n < 128 && ((event.hand == Hand::Right) == (*n >= 60))),
                            "{} {:?}",
                            skill.id,
                            event
                        );
                    }
                }
                let (notes, onsets, evidence) = perform(score, Instant::now());
                assert!(ex.correct(&notes), "{}", skill.id);
                assert!(ex.performance_timing_correct(&onsets, None), "{}", skill.id);
                assert!(ex.holds_correct(&evidence), "{}", skill.id);
                assert!(
                    score
                        .evaluate(&notes, &onsets, &evidence)
                        .components()
                        .iter()
                        .all(|(_, ok)| *ok),
                    "{}",
                    skill.id
                );
                let wrong: Vec<_> = notes.iter().map(|n| n + 12).collect();
                if !score.symbols_only {
                    assert!(
                        !ex.correct(&wrong),
                        "{} accepts wrong written octave",
                        skill.id
                    );
                }
            }
        }
    }
    #[test]
    fn held_bass_and_alternative_voicings_are_independent_of_melody_errors() {
        let session = Session::preview("reading.lead.2").unwrap();
        let score = session.exercise.as_ref().unwrap().reading.as_ref().unwrap();
        let mut alternative = score.clone();
        for event in alternative
            .events
            .iter_mut()
            .filter(|e| e.hand == Hand::Left)
        {
            let last = event.choices.last().unwrap().clone();
            event.choices = vec![last];
        }
        let (mut notes, onsets, mut evidence) = perform(&alternative, Instant::now());
        assert!(score.performance(None).pitch_correct(&notes));
        assert!(score.holds_correct(&evidence));
        let wrong = notes.iter_mut().find(|n| **n >= 60).unwrap();
        *wrong += 1;
        let result = score.evaluate(&notes, &onsets, &evidence);
        assert_eq!(result.right, Some(false));
        assert_eq!(result.left, Some(true));
        assert_eq!(result.symbols, Some(true));
        let bass_off = evidence
            .iter_mut()
            .find(|e| e.velocity == 0 && e.note < 60)
            .unwrap();
        bass_off.offset_ms = 200;
        assert!(!score.holds_correct(&evidence));
    }
    #[test]
    fn direct_reading_autosubmits_without_target_audio_and_feedback_playback_is_ungraded() {
        let mut session = Session::preview("reading.right.0").unwrap();
        let now = Instant::now();
        session.start(now);
        assert!(session
            .tick(now)
            .unwrap()
            .iter()
            .all(|e| !matches!(e, Event::Note { velocity: 1.., .. })));
        let note = session.exercise.as_ref().unwrap().expected_evidence()[0];
        session.input(
            Event::Note {
                channel: 0,
                note,
                velocity: 90,
            },
            now,
        );
        session.tick(now).unwrap();
        assert!(session.phase == Phase::Feedback);
        assert_eq!(session.feedback, "Correct!");
        assert!(session.play_reading_solution(now));
        assert!(session
            .tick(now)
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::Note { velocity: 1.., .. })));
        assert_eq!(session.profile.completed, 0);
        session.show_browser();
        assert!(session
            .tick(now + Duration::from_secs(30))
            .unwrap()
            .is_empty());
    }
    #[test]
    fn component_evidence_survives_reload_without_penalizing_correct_chord_prerequisites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profile.json");
        let mut session = Session::open(path.clone()).unwrap();
        let skill = session
            .graph
            .iter()
            .find(|s| s.id == "reading.lead.1")
            .unwrap()
            .clone();
        session.exercise = Some(Exercise::generate(&skill, 42));
        let score = session.exercise.as_ref().unwrap().reading.as_ref().unwrap();
        let now = Instant::now();
        let (_, _, mut evidence) = perform(score, now);
        // Change the first right-hand note and its release together.
        let original = evidence
            .iter()
            .find(|e| e.velocity > 0 && e.note >= 60)
            .unwrap()
            .note;
        for e in evidence.iter_mut().filter(|e| e.note == original) {
            e.note += 1;
        }
        session.start(now);
        for e in evidence {
            let at = now + Duration::from_millis(e.offset_ms);
            session.tick(at).unwrap();
            session.input(
                Event::Note {
                    channel: e.channel,
                    note: e.note,
                    velocity: e.velocity,
                },
                at,
            );
        }
        session.tick(now + Duration::from_secs(9)).unwrap();
        assert!(session.phase == Phase::Feedback);
        assert_eq!(
            session.profile.reading_components["reading.lead.1:right"].correct,
            0
        );
        assert_eq!(
            session.profile.reading_components["reading.lead.1:left"].correct,
            1
        );
        drop(session);
        let loaded = Session::open(path).unwrap();
        assert_eq!(
            loaded.profile.reading_components["reading.lead.1:symbols"].correct,
            1
        );
        assert!(loaded
            .profile
            .recent_attempts
            .last()
            .unwrap()
            .reading
            .is_some());
    }
    #[test]
    fn combined_and_lead_courses_require_both_hands_and_known_voicings() {
        let graph = crate::curriculum::curriculum();
        let get = |id: &str| graph.iter().find(|s| s.id == id).unwrap();
        assert!(get("reading.together.0")
            .requires
            .iter()
            .any(|r| r.skill == "reading.right.3"));
        assert!(get("reading.together.0")
            .requires
            .iter()
            .any(|r| r.skill == "reading.left.3"));
        for level in 0..6 {
            let lead = get(&format!("reading.lead.{level}"));
            assert_eq!(
                lead.requires
                    .iter()
                    .filter(|r| r.skill.starts_with("reading.symbol."))
                    .count(),
                3
            );
        }
    }
}

#[cfg(test)]
mod flow_tests {
    use super::*;
    use crate::{Phase, Session};
    use keyboard::Event;
    use std::time::Duration;
    #[test]
    fn retry_is_practice_and_ties_are_one_attack_with_a_long_hold() {
        let mut session = Session::preview("reading.right.0").unwrap();
        let now = Instant::now();
        session.start(now);
        session.replay(now);
        let note = session.exercise.as_ref().unwrap().expected_evidence()[0];
        session.input(
            Event::Note {
                channel: 0,
                note,
                velocity: 90,
            },
            now,
        );
        session.tick(now).unwrap();
        assert!(session.assisted);
        assert!(session.feedback.contains("no mastery credit"));
        let mut tied = Session::preview("reading.right.10").unwrap();
        tied.start(now);
        let score = tied.exercise.as_ref().unwrap().reading.as_ref().unwrap();
        let tie = score.events.iter().find(|e| e.tie_tick.is_some()).unwrap();
        assert_eq!(tie.tick, 6);
        assert_eq!(tie.duration, 4);
        assert_eq!(tie.tie_tick, Some(8));
        let notes = score.events.iter().flat_map(|e| e.notes()).count();
        assert_eq!(score.performance(None).note_count(), notes);
        assert_eq!(
            score
                .playback()
                .iter()
                .filter(|(_, e)| matches!(e, Event::Note { velocity: 1.., .. }))
                .count(),
            notes
        );
    }
    #[test]
    fn reading_without_images_never_enters_adaptive_practice_and_paused_input_does_not_score() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::open(dir.path().join("profile.json")).unwrap();
        session.exercise = Some(Exercise::generate(
            session
                .graph
                .iter()
                .find(|s| s.id == "reading.right.0")
                .unwrap(),
            1,
        ));
        session.set_reading_supported(false);
        assert!(session
            .exercise
            .as_ref()
            .is_none_or(|e| e.reading.is_none()));
        let mut preview = Session::preview("reading.left.0").unwrap();
        let now = Instant::now();
        preview.set_visible(false, now);
        preview.start(now);
        preview.input(
            Event::Note {
                channel: 0,
                note: 48,
                velocity: 90,
            },
            now,
        );
        assert!(preview
            .tick(now + Duration::from_secs(30))
            .unwrap()
            .is_empty());
        assert!(preview.phase == Phase::Waiting);
        assert!(preview.played.is_empty());
    }
    #[test]
    fn new_reading_branches_enter_at_a_reasonable_point_without_gating_ear_training() {
        let graph = crate::curriculum::curriculum();
        let mut profile = crate::learner::Profile::default();
        let targets = [
            "reading.right.0",
            "reading.left.0",
            "reading.together.0",
            "reading.lead.0",
        ];
        let mut first = std::collections::BTreeMap::new();
        for turn in 1..=10000 {
            let skill = profile.next(&graph, 1000).unwrap().0;
            if targets.contains(&skill.id.as_str()) {
                first.entry(skill.id.clone()).or_insert(turn);
            }
            let attempt = crate::tests::result(&skill.id, true, turn as u64);
            profile.record(skill, attempt);
            if first.len() == targets.len() {
                break;
            }
        }
        eprintln!("Reading curriculum (perfect-answer simulation): {first:?}");
        for id in targets {
            assert!(first.contains_key(id), "{id} never introduced");
        }
        assert!(first["reading.right.0"] <= 3000 && first["reading.left.0"] <= 3000);
        assert!(first["reading.together.0"] > first["reading.right.0"]);
        assert!(first["reading.lead.0"] > first["reading.together.0"]);
        for skill in graph.iter().filter(|s| !matches!(s.task, Task::Reading(_))) {
            assert!(skill
                .requires
                .iter()
                .all(|r| !r.skill.starts_with("reading.")));
        }
    }
}
