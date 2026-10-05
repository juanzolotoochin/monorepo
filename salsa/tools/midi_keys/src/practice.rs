//! Generators for tonal, rhythmic, and multi-event exercises.
use crate::curriculum::{Direction, ROOTS};
use crate::exercise::{Answer, Exercise};
use crate::music::Scale;
use crate::performance::{Frame, Performance};
use crate::NoteSpelling;
use keyboard::Event;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Motif {
    Free,
    Repeated,
    Steps,
    Leaps,
    Contour,
    Transpose,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rhythm {
    Pulse,
    Eighths,
    Rests,
    Ties,
    Syncopation,
    SilentPulse,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harmony {
    Function(usize),
    Progression(usize),
    Cadence(bool),
    Bass(bool),
    Shell(bool),
    VoiceLeading,
    Harmonize(usize),
    Accompany { bars: usize, thirds: bool },
    Coordination(bool),
}
#[derive(Clone, Debug)]
pub enum Practice {
    Tonic {
        root: usize,
        scale: Scale,
    },
    Degree {
        root: usize,
        scale: Scale,
        degree: usize,
    },
    Movement {
        root: usize,
        scale: Scale,
        direction: Direction,
        triad: bool,
    },
    Resolve {
        root: usize,
        scale: Scale,
        degree: usize,
    },
    Scale {
        root: usize,
        scale: Scale,
        direction: Direction,
        hear: bool,
        bpm: Option<u32>,
    },
    Melody {
        root: usize,
        scale: Scale,
        length: usize,
        motif: Motif,
        rhythmic: bool,
    },
    Rhythm {
        kind: Rhythm,
        bpm: u32,
    },
    Meter {
        beats: usize,
        bpm: u32,
    },
    Harmony {
        root: usize,
        scale: Scale,
        kind: Harmony,
    },
}
impl Practice {
    pub fn spelling(&self) -> NoteSpelling {
        match *self {
            Self::Degree { root, scale, .. }
            | Self::Movement { root, scale, .. }
            | Self::Resolve { root, scale, .. }
            | Self::Scale {
                root,
                scale,
                hear: false,
                ..
            }
            | Self::Melody { root, scale, .. }
            | Self::Harmony { root, scale, .. } => NoteSpelling::for_scale(root, scale),
            _ => NoteSpelling::default(),
        }
    }
}
pub(crate) fn accompaniment_degrees(bars: usize) -> &'static [usize] {
    if bars == 12 {
        &[0, 0, 0, 0, 3, 3, 0, 0, 4, 3, 0, 4]
    } else {
        &[0, 3, 4, 0]
    }
}

pub(crate) fn accompaniment_chord(
    root: usize,
    scale: Scale,
    degree: usize,
    thirds: bool,
) -> String {
    let base = scale.steps()[degree];
    let interval = |n: usize| scale.steps()[n % 7] + 12 * (n / 7) - base;
    let third = interval(degree + 2);
    let fifth = interval(degree + 4);
    let seventh = interval(degree + 6);
    let name = NoteSpelling::for_scale(root, scale).note_name(60 + root + base);
    let root_name = name.trim_end_matches(|c: char| c.is_ascii_digit());
    let quality = if thirds {
        if fifth == 6 {
            "dim"
        } else if third == 3 {
            "m"
        } else {
            ""
        }
    } else if fifth == 6 {
        "m7b5"
    } else if third == 3 {
        "m7"
    } else if seventh == 11 {
        "maj7"
    } else {
        "7"
    };
    format!("{root_name}{quality}")
}

fn note(events: &mut Vec<(u64, Event)>, pitch: usize, at: u64, duration: u64, velocity: u8) {
    events.push((
        at,
        Event::Note {
            channel: 15,
            note: pitch,
            velocity,
        },
    ));
    events.push((
        at + duration,
        Event::Note {
            channel: 15,
            note: pitch,
            velocity: 0,
        },
    ));
}
fn chord(events: &mut Vec<(u64, Event)>, pitches: &[usize], at: u64, duration: u64) {
    for &pitch in pitches {
        note(events, pitch, at, duration, 80);
    }
}
fn reference(ex: &mut Exercise, root: usize, scale: Scale) -> u64 {
    let mut notes: Vec<_> = scale.steps().iter().map(|n| 60 + root + n).collect();
    notes.push(72 + root);
    for (i, &pitch) in notes.iter().enumerate() {
        note(&mut ex.playback, pitch, i as u64 * 300, 240, 70);
    }
    notes.len() as u64 * 300 + 600
}
fn frame(notes: Vec<usize>, at_ms: u64, hold_ms: Option<u64>) -> Frame {
    Frame {
        choices: vec![notes],
        at_ms,
        hold_ms,
    }
}
fn performance(frames: Vec<Frame>, timed: bool) -> Performance {
    Performance {
        frames,
        accompaniment: false,
        timed,
        exact_register: false,
        any_pitch: false,
        fixed_start: false,
        tolerance_ms: 160,
        smooth: false,
        preserve_contour: false,
    }
}
fn render_performance(ex: &mut Exercise, score: &Performance, start: u64) {
    for (i, f) in score.frames.iter().enumerate() {
        let short = score
            .frames
            .get(i + 1)
            .map(|next| ((next.at_ms - f.at_ms) * 3 / 4).clamp(60, 350))
            .unwrap_or(350);
        chord(
            &mut ex.playback,
            &f.choices[0],
            start + f.at_ms,
            f.hold_ms.unwrap_or(short),
        );
    }
}
fn describe(notes: &[usize], spelling: &NoteSpelling) -> String {
    notes
        .iter()
        .map(|&n| spelling.note_name(n))
        .collect::<Vec<_>>()
        .join(" → ")
}

pub fn generate(task: &Practice, seed: u64, ex: &mut Exercise) {
    let mut rng = crate::exercise::Random(seed.max(1));
    ex.spelling = task.spelling();
    ex.metronome = false;
    match *task {
        Practice::Tonic { root, scale } => {
            ex.title = "FIND THE TONIC".into();
            ex.prompt =
                "Hear four chords. The first and last are the home chord. Hum their bass note, then find it on your keyboard. Play that home note (tonic), in any octave.".into();
            for (i, degree) in [0, 3, 4, 0].into_iter().enumerate() {
                chord(
                    &mut ex.playback,
                    &scale.triad(48 + root, degree),
                    i as u64 * 800,
                    650,
                );
            }
            ex.answer_policy = crate::exercise::AnswerPolicy::ExploreThenAnswer;
            ex.answer = Answer::PitchClasses(vec![root]);
            ex.explanation = format!("The home note is {} ({}).", ROOTS[root], scale.id());
        }
        Practice::Degree {
            root,
            scale,
            degree,
        } => {
            ex.title = "ONE NOTE IN A SCALE".into();
            ex.prompt = format!(
                "Hear {} {}, then one note. Play that note in any octave.",
                ROOTS[root],
                scale.id()
            );
            let start = reference(ex, root, scale);
            let target = 48 + 12 * rng.take(3) + root + scale.steps()[degree];
            note(&mut ex.playback, target, start, 650, 85);
            ex.answer = Answer::PitchClasses(vec![target]);
            ex.explanation = format!(
                "Scale tone {}: {}.",
                degree + 1,
                ex.spelling.note_name(target)
            );
        }
        Practice::Movement {
            root,
            scale,
            direction,
            triad,
        } => {
            let steps = scale.steps();
            let mut degrees: Vec<usize> = if triad {
                vec![
                    0,
                    steps.iter().position(|n| *n == 3 || *n == 4).unwrap_or(1),
                    steps.iter().position(|n| *n == 7).unwrap_or(2),
                ]
            } else {
                (0..steps.len()).collect()
            };
            degrees.sort_unstable();
            degrees.dedup();
            let first = rng.take(degrees.len());
            let mut second = rng.take(degrees.len() - 1);
            if second >= first {
                second += 1;
            }
            let mut notes = vec![
                60 + root + steps[degrees[first]],
                60 + root + steps[degrees[second]],
            ];
            notes.sort_unstable();
            if direction == Direction::Down {
                notes.reverse();
            }
            let start = reference(ex, root, scale);
            ex.title = "TWO NOTES IN A SCALE".into();
            ex.prompt = format!("Hear {} {}, then two notes. Replay those notes in any starting octave, preserving their direction.", ROOTS[root], scale.id());
            for (i, &pitch) in notes.iter().enumerate() {
                note(&mut ex.playback, pitch, start + i as u64 * 650, 450, 85);
            }
            ex.explanation = describe(&notes, &ex.spelling);
            ex.answer = Answer::OctaveSequence(notes);
        }
        Practice::Resolve {
            root,
            scale,
            degree,
        } => {
            ex.title = "TENSION AND RESOLUTION".into();
            ex.prompt = format!("Hear {} {}, then a tense note. Play that note, then resolve it to any tone of the tonic triad. Two notes, in order.", ROOTS[root], scale.id());
            let start = reference(ex, root, scale);
            let target = 60 + root + scale.steps()[degree];
            note(&mut ex.playback, target, start, 600, 85);
            let stable = scale.triad(60 + root, 0);
            ex.answer = Answer::Performance(performance(
                vec![
                    frame(vec![target], 0, None),
                    Frame {
                        choices: stable.iter().map(|n| vec![*n]).collect(),
                        at_ms: 600,
                        hold_ms: None,
                    },
                ],
                false,
            ));
            ex.explanation = format!(
                "{} resolves to a tonic-chord tone: {}.",
                ex.spelling.note_name(target),
                describe(&stable, &ex.spelling)
            );
        }
        Practice::Scale {
            root,
            scale,
            direction,
            hear,
            bpm,
        } => {
            let mut notes: Vec<_> = scale.steps().iter().map(|n| 60 + root + n).collect();
            notes.push(72 + root);
            if direction == Direction::Down {
                notes.reverse();
            }
            if direction == Direction::Both {
                let tail: Vec<_> = notes[..notes.len() - 1].iter().rev().copied().collect();
                notes.extend(tail);
            }
            ex.title = if hear {
                "SCALE FAMILY RECOGNITION"
            } else {
                "SCALE CONSTRUCTION"
            }
            .into();
            ex.prompt = if hear {
                format!("Hear a scale. Play the same scale pattern ascending from any starting note ({} notes, including the octave).", notes.len())
            } else {
                format!(
                    "Play {} {} scale {}. Any starting octave; one octave, one note at a time.",
                    ROOTS[root],
                    scale.id(),
                    match direction {
                        Direction::Down => "descending",
                        Direction::Both => "up then down (one top note)",
                        _ => "ascending",
                    }
                )
            };
            ex.answer = if hear {
                Answer::TransposedSequence(notes.clone())
            } else {
                Answer::OctaveSequence(notes.clone())
            };
            if hear {
                for (i, &n) in notes.iter().enumerate() {
                    note(&mut ex.playback, n, i as u64 * 400, 320, 80);
                }
            }
            ex.explanation = format!(
                "{}: {}",
                scale.id(),
                describe(&notes, &NoteSpelling::for_scale(root, scale))
            );
            if let Some(bpm) = bpm {
                ex.prompt.push_str(&format!(
                    " {bpm} BPM; four count-in beats, then one note per beat."
                ));
                let start = ex.add_count_in(4, bpm);
                ex.answer_after_ms = Some(start);
                ex.bpm = Some(bpm);
                ex.metronome = true;
            }
        }
        Practice::Melody {
            root,
            scale,
            length,
            motif,
            rhythmic,
        } => {
            let steps = scale.steps();
            let triad_degrees = [
                0,
                steps.iter().position(|n| *n == 3 || *n == 4).unwrap_or(1),
                steps.iter().position(|n| *n == 7).unwrap_or(2),
            ];
            let mut degrees = if length == 2 {
                let fifth = triad_degrees[2];
                if rng.take(2) == 0 {
                    vec![0, fifth]
                } else {
                    vec![fifth, 0]
                }
            } else {
                vec![if length == 3 {
                    triad_degrees[rng.take(3)]
                } else {
                    rng.take(steps.len())
                }]
            };
            for i in degrees.len()..length {
                let prev = degrees[i - 1];
                degrees.push(match motif {
                    Motif::Repeated => {
                        if i % 2 == 1 {
                            prev
                        } else {
                            (prev + 1) % steps.len()
                        }
                    }
                    Motif::Steps => {
                        if prev + 1 < steps.len() {
                            prev + 1
                        } else {
                            prev - 1
                        }
                    }
                    Motif::Leaps => (prev + 2) % steps.len(),
                    Motif::Contour => {
                        if i < length / 2 {
                            (prev + 1).min(steps.len() - 1)
                        } else {
                            prev.saturating_sub(1)
                        }
                    }
                    _ if length == 3 => triad_degrees[rng.take(3)],
                    _ => rng.take(steps.len()),
                });
            }
            let notes: Vec<_> = degrees.iter().map(|&d| 60 + root + steps[d]).collect();
            let start = reference(ex, root, scale);
            ex.title = if rhythmic {
                "MELODY / RHYTHM"
            } else if motif == Motif::Transpose {
                "MELODIC TRANSPOSITION"
            } else {
                "MELODIC DICTATION"
            }
            .into();
            ex.prompt = format!("Hear {} {} and a {length}-note phrase. Replay it in any starting octave, preserving its pitches and contour.", ROOTS[root],scale.id());
            let mut score = performance(
                notes
                    .iter()
                    .enumerate()
                    .map(|(i, n)| frame(vec![*n], i as u64 * 600, None))
                    .collect(),
                rhythmic,
            );
            if rhythmic {
                let mut at = 0;
                for (i, f) in score.frames.iter_mut().enumerate() {
                    f.at_ms = at;
                    let duration = if i % 3 == 1 { 300 } else { 600 };
                    f.hold_ms = Some(duration - 80);
                    at += duration;
                }
                score.tolerance_ms = 100;
                score.preserve_contour = true;
                ex.bpm = Some(100);
                ex.prompt.push_str(
                    " Match rhythm and note lengths too; start when ready after listening.",
                );
            }
            render_performance(ex, &score, start);
            if motif == Motif::Transpose {
                let target = (root + 5) % 12;
                let shift = target as i32 - root as i32;
                let shifted: Vec<_> = notes.iter().map(|n| (*n as i32 + shift) as usize).collect();
                ex.prompt = format!("Hear a {length}-note phrase in {} {}. Transpose it to {} {}, starting on {}. Any octave.",ROOTS[root],scale.id(),ROOTS[target],scale.id(),NoteSpelling::for_scale(target,scale).note_name(shifted[0]));
                ex.spelling = NoteSpelling::for_scale(target, scale);
                ex.answer = Answer::OctaveSequence(shifted.clone());
                ex.explanation = describe(&shifted, &ex.spelling);
            } else {
                ex.explanation = describe(&notes, &ex.spelling);
                if rhythmic {
                    score.exact_register = false;
                    ex.answer = Answer::Performance(score);
                } else {
                    ex.answer = Answer::OctaveSequence(notes);
                }
            }
        }
        Practice::Rhythm { kind, bpm } => {
            let beat = 60000 / u64::from(bpm);
            let (units, lengths): (&[u64], &[u64]) = match kind {
                Rhythm::Pulse | Rhythm::SilentPulse => {
                    (&[0, 2, 4, 6, 8, 10, 12, 14], &[1, 1, 1, 1, 1, 1, 1, 1])
                }
                Rhythm::Eighths => (&[0, 1, 2, 3, 4, 5, 6, 7], &[1, 1, 1, 1, 1, 1, 1, 1]),
                Rhythm::Rests => (&[0, 2, 6, 8, 12, 14], &[1, 1, 1, 1, 1, 1]),
                Rhythm::Ties => (&[0, 3, 4, 8, 11, 12], &[3, 1, 4, 3, 1, 2]),
                Rhythm::Syncopation => (&[0, 3, 5, 8, 11, 13], &[1, 1, 2, 1, 1, 2]),
            };
            let mut score = performance(
                units
                    .iter()
                    .zip(lengths)
                    .map(|(u, l)| {
                        frame(
                            vec![72],
                            u * beat / 2,
                            if matches!(kind, Rhythm::Pulse | Rhythm::Eighths | Rhythm::SilentPulse)
                            {
                                None
                            } else {
                                Some((l * beat / 2).saturating_sub(50))
                            },
                        )
                    })
                    .collect(),
                true,
            );
            score.any_pitch = true;
            score.tolerance_ms = (beat / 6).max(60);
            ex.title = "RHYTHM".into();
            ex.bpm = Some(bpm);
            ex.prompt = format!("Listen, then echo {} taps using any MIDI notes at {bpm} BPM. Match the spacing and held lengths; start when ready.", units.len());
            if kind == Rhythm::Pulse {
                ex.prompt = format!("Play 8 quarter notes using any MIDI notes at {bpm} BPM with the drum beat. Join on any beat; hold each note until just before the next beat.");
                ex.metronome = true;
                ex.pulse_backing = true;
                for frame in &mut score.frames {
                    frame.hold_ms = Some(beat.saturating_sub(60));
                }
            } else if kind == Rhythm::SilentPulse {
                ex.prompt = format!("Keep the pulse at {bpm} BPM: four count-in beats, then eight short taps using any MIDI notes. The click drops out after your first four taps.");
                let start = ex.add_count_in(4, bpm);
                for i in 0..4 {
                    note(&mut ex.playback, 96, start + i * beat, 60, 45);
                }
                ex.answer_after_ms = Some(start);
                score.fixed_start = true;
            } else {
                let start = ex.add_count_in(4, bpm);
                render_performance(ex, &score, start);
            }
            ex.explanation = if kind == Rhythm::Pulse {
                format!("Eight evenly spaced quarter notes at {bpm} BPM, aligned with the drums and held for almost one beat each.")
            } else {
                format!("{kind:?} at {bpm} BPM. Match the heard attacks, rests, and releases.")
            };
            ex.answer = Answer::Performance(score);
        }
        Practice::Meter { beats, bpm } => {
            let beat = 60000 / u64::from(bpm);
            ex.title = "METER / DOWNBEATS".into();
            ex.prompt = format!("Hear three bars of accented pulse ({bpm} BPM). Tap only the next four downbeats using any MIDI notes, beginning immediately after the third bar.");
            for i in 0..beats * 3 {
                note(
                    &mut ex.playback,
                    96,
                    i as u64 * beat,
                    60,
                    if i % beats == 0 { 95 } else { 40 },
                );
            }
            let start = beats as u64 * 3 * beat;
            let mut score = performance(
                (0..4)
                    .map(|i| frame(vec![72], i * beats as u64 * beat, None))
                    .collect(),
                true,
            );
            score.any_pitch = true;
            score.fixed_start = true;
            score.tolerance_ms = beat / 5;
            ex.answer_after_ms = Some(start);
            ex.bpm = Some(bpm);
            ex.answer = Answer::Performance(score);
            ex.explanation = format!(
                "The meter groups {beats} beats. Tap every {beats} beats on the strong beat."
            );
        }
        Practice::Harmony { root, scale, kind } => harmony(ex, root, scale, kind, &mut rng),
    }
    ex.variant = format!("{}:{seed}", task_id(task));
    ex.playback.sort_by_key(|(at, event)| {
        (
            *at,
            matches!(
                event,
                Event::Note {
                    velocity: 1..=127,
                    ..
                }
            ),
        )
    });
}
fn task_id(task: &Practice) -> String {
    format!("{task:?}")
}

fn progression_degrees(rng: &mut crate::exercise::Random) -> [usize; 4] {
    // All phrases use the four functions taught before progression practice.
    // Vary the order so replay cannot be solved by memorizing one sequence.
    [[0, 5, 3, 4], [0, 3, 4, 0], [0, 4, 5, 3], [0, 5, 4, 0]][rng.take(4)]
}

fn harmony(
    ex: &mut Exercise,
    root: usize,
    scale: Scale,
    kind: Harmony,
    rng: &mut crate::exercise::Random,
) {
    let tonic = 48 + root;
    let triad = |degree| scale.triad(tonic, degree);
    let context = |ex: &mut Exercise| {
        for (i, d) in [0, 3, 4, 0].into_iter().enumerate() {
            chord(&mut ex.playback, &triad(d), i as u64 * 600, 450);
        }
        3000
    };
    ex.title = "HARMONY IN A KEY".into();
    ex.prompt = format!("In {} {}: ", ROOTS[root], scale.id());
    match kind {
        Harmony::Function(degree) => {
            let start = context(ex);
            let notes = triad(degree);
            chord(&mut ex.playback, &notes, start, 850);
            ex.prompt.push_str("hear the key established, then a chord. Reproduce that chord in this key, any octave or inversion. Play three notes together.");
            ex.explanation = format!(
                "Scale-degree {} triad: {}",
                degree + 1,
                describe(&notes, &ex.spelling)
            );
            ex.answer = Answer::Performance(performance(vec![frame(notes, 0, None)], false));
        }
        Harmony::Progression(_) | Harmony::Cadence(_) => {
            let degrees = match kind {
                Harmony::Cadence(true) => vec![4, 0],
                Harmony::Cadence(false) => vec![0, 4],
                _ if length_of(kind) == 2 => vec![0, [3, 4, 5][rng.take(3)]],
                _ => progression_degrees(rng).to_vec(),
            };
            let start = context(ex);
            let score = performance(
                degrees
                    .iter()
                    .enumerate()
                    .map(|(i, &d)| frame(triad(d), i as u64 * 1100, None))
                    .collect(),
                false,
            );
            render_performance(ex, &score, start);
            ex.prompt.push_str("hear the key established, then a chord phrase. Replay the final phrase: three notes together per chord, with a brief release between chords. Any octave or inversion.");
            ex.explanation = format!(
                "Chord degrees: {}{}",
                degrees
                    .iter()
                    .map(|d| (d + 1).to_string())
                    .collect::<Vec<_>>()
                    .join(" → "),
                if matches!(kind, Harmony::Cadence(true)) {
                    ". An arrival on the tonic."
                } else if matches!(kind, Harmony::Cadence(false)) {
                    ". A half cadence, ending on the dominant."
                } else {
                    ""
                }
            );
            ex.answer = Answer::Performance(score);
        }
        Harmony::Bass(roots) => {
            let degrees = progression_degrees(rng);
            let mut notes = vec![];
            let start = context(ex);
            for (i, d) in degrees.into_iter().enumerate() {
                let mut pitches = triad(d);
                let target = pitches[0];
                if roots {
                    pitches[0] += 12;
                    pitches.sort_unstable();
                    chord(&mut ex.playback, &pitches, start + i as u64 * 900, 650);
                } else {
                    note(&mut ex.playback, target, start + i as u64 * 900, 650, 80);
                }
                notes.push(target);
            }
            ex.prompt.push_str(if roots {"hear four inverted chords, then play their four ROOT notes in order, in any octave."} else {"hear a four-note bass line, then replay it in any starting octave."});
            ex.explanation = describe(&notes, &ex.spelling);
            ex.answer = if roots {
                Answer::PitchClasses(notes)
            } else {
                Answer::OctaveSequence(notes)
            };
        }
        Harmony::Shell(thirds) => {
            let steps = scale.steps();
            let notes = if thirds {
                vec![tonic + steps[2], tonic + steps[4]]
            } else {
                vec![tonic, tonic + steps[6]]
            };
            ex.prompt.push_str(if thirds {
                "play the 3–5 two-note voicing of the tonic chord, together. Any octave."
            } else {
                "play the 1–7 two-note shell of the tonic seventh chord, together. Any octave."
            });
            ex.explanation = describe(&notes, &ex.spelling);
            ex.answer = Answer::Performance(performance(vec![frame(notes, 0, None)], false));
        }
        Harmony::VoiceLeading => {
            let mut score = performance(
                [0, 3, 4, 0]
                    .iter()
                    .enumerate()
                    .map(|(i, &d)| frame(triad(d), i as u64 * 1000, None))
                    .collect(),
                false,
            );
            score.smooth = true;
            for i in 1..score.frames.len() {
                let previous = score.frames[i - 1].choices[0].clone();
                let base = score.frames[i].choices[0].clone();
                let mut candidates = vec![];
                for inversion in 0..3 {
                    for shift in [-12, 0, 12] {
                        let mut notes: Vec<_> = base
                            .iter()
                            .enumerate()
                            .map(|(j, n)| {
                                (*n as i32 + shift + if j < inversion { 12 } else { 0 }) as usize
                            })
                            .collect();
                        notes.sort_unstable();
                        let cost: usize = notes
                            .iter()
                            .zip(&previous)
                            .map(|(a, b)| a.abs_diff(*b))
                            .sum();
                        candidates.push((cost, notes));
                    }
                }
                candidates.sort_by_key(|(cost, _)| *cost);
                score.frames[i].choices = vec![candidates.remove(0).1];
            }
            ex.prompt.push_str("play triads on degrees 1 → 4 → 5 → 1. Three notes together per chord, release between chords. Choose inversions so each sorted voice moves at most five semitones.");
            ex.explanation =
                "Keep common tones; move each voice by no more than a fourth between chords."
                    .into();
            ex.answer = Answer::Performance(score);
        }
        Harmony::Harmonize(length) => {
            let melody: Vec<_> = [0, 3, 4, 0]
                .into_iter()
                .take(length)
                .map(|d| 72 + root + scale.steps()[d])
                .collect();
            let start = context(ex);
            for (i, &n) in melody.iter().enumerate() {
                note(&mut ex.playback, n, start + i as u64 * 800, 600, 85);
            }
            let frames = melody
                .iter()
                .enumerate()
                .map(|(i, n)| Frame {
                    choices: (0..7)
                        .map(triad)
                        .filter(|chord| chord.iter().any(|c| c % 12 == n % 12))
                        .collect(),
                    at_ms: i as u64 * 1000,
                    hold_ms: None,
                })
                .collect();
            ex.prompt.push_str(&format!("hear {length} melody note(s). Play one diatonic triad containing each note, in order. Three notes together per chord; any inversion. Several answers are valid."));
            ex.explanation="Each melody note must belong to the corresponding triad, built entirely from this scale.".into();
            ex.answer = Answer::Performance(performance(frames, false));
        }
        Harmony::Accompany { bars, thirds } => {
            ex.title = "ACCOMPANIMENT".into();
            let degrees = accompaniment_degrees(bars);
            let start = ex.add_count_in(4, 60);
            let frames: Vec<_> = degrees
                .iter()
                .enumerate()
                .map(|(i, &d)| {
                    let seventh: Vec<_> = [d, d + 2, d + 4, d + 6]
                        .into_iter()
                        .map(|n| tonic + scale.steps()[n % 7] + 12 * (n / 7))
                        .collect();
                    let notes = if thirds {
                        vec![seventh[1], seventh[2]]
                    } else {
                        vec![seventh[0], seventh[3]]
                    };
                    for j in 0..4 {
                        note(
                            &mut ex.playback,
                            triad(d)[j % 3] + 24,
                            start + i as u64 * 4000 + j as u64 * 1000,
                            700,
                            70,
                        );
                    }
                    frame(notes, i as u64 * 4000, Some(4000))
                })
                .collect();
            let mut score = performance(frames, true);
            score.fixed_start = true;
            score.tolerance_ms = 250;
            score.accompaniment = true;
            ex.prompt.push_str(&format!("{bars} bars · 60 BPM · four count-in beats. Follow the chord names above the score. Play each chord's {} together on beat 1; sustain for three beats or longer (±250 ms); a gap before the next chord is fine. Repeat on every bar, even when the chord stays the same. Any octave.", if thirds { "third and fifth" } else { "root and seventh" }));
            ex.explanation = "Follow the chord names above the score. Play the written two-note voicing on beat 1 of every bar, including repeated chords; sustain for at least three beats.".into();
            ex.answer_after_ms = Some(start);
            ex.bpm = Some(60);
            ex.answer = Answer::Performance(score);
        }
        Harmony::Coordination(changing) => {
            ex.title = "BASS + MELODY COORDINATION".into();
            let start = ex.add_count_in(4, 60);
            let bars = if changing { 4 } else { 1 };
            let mut frames = vec![];
            for bar in 0..bars {
                let degree = if changing { [0, 3, 4, 0][bar] } else { 0 };
                let bass = 36 + root + scale.steps()[degree];
                let at = bar as u64 * 4000;
                frames.push(frame(vec![bass], at, Some(4000)));
                for (i, &offset) in [0, 2, 4, 2].iter().enumerate() {
                    let n = degree + offset;
                    let melody = 60 + root + scale.steps()[n % 7] + 12 * (n / 7);
                    frames.push(frame(vec![melody], at + 500 + i as u64 * 1000, Some(400)));
                }
            }
            let mut score = performance(frames, true);
            score.fixed_start = true;
            score.exact_register = true;
            score.tolerance_ms = 180;
            ex.prompt=format!("{bars} bar(s), 4/4, 60 BPM. Read the score, then press Enter for the spoken count-in. LEFT HAND: hold the bass for the whole bar. RIGHT HAND: rest on 1, 2, 3, 4; play the written eighth notes on each &. Count: 1 & 2 & 3 & 4 &.");
            ex.explanation="Keep the bass held while playing the upper part. MIDI checks overlapping notes and timing, not which physical hand you use.".into();
            ex.answer_after_ms = Some(start);
            ex.bpm = Some(60);
            ex.answer = Answer::Performance(score);
        }
    }
}
fn length_of(kind: Harmony) -> usize {
    if let Harmony::Progression(n) = kind {
        n
    } else {
        2
    }
}
