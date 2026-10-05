//! Explicit notation policy for every exercise family. Listening targets and
//! construction answers are never printed before submission. Guided scales and
//! motor-coordination tasks intentionally provide the complete written pattern.
use crate::curriculum::{Direction, Skill, Task, CHORDS};
use crate::exercise::{Answer, Exercise};
use crate::learner::MidiEvidence;
use crate::music::Scale;
use crate::practice::{Harmony, Practice};
use crate::reading::{ChordSymbol, Hand, WrittenEvent, WrittenScore};
use std::collections::BTreeSet;

pub fn scale_task(task: &Task) -> bool {
    matches!(
        task,
        Task::Scale { .. }
            | Task::Practice(Practice::Scale {
                hear: false,
                bpm: None,
                ..
            })
    )
}
pub fn extend(graph: &mut Vec<Skill>) {
    let mut guided = vec![];
    for skill in graph.iter_mut().filter(|s| scale_task(&s.task)) {
        let mut guide = skill.clone();
        guide.id = format!("guided.{}", skill.id);
        guide.title = format!("Score-guided · {}", skill.title);
        guide.task = Task::GuidedScale(Box::new(skill.task.clone()));
        skill.requires.push(crate::curriculum::Requirement {
            skill: guide.id.clone(),
            score: 7.0,
            attempts: 6,
        });
        skill.title = format!("From memory · {}", skill.title);
        guided.push(guide);
    }
    graph.extend(guided);
}
pub fn before_answer(task: &Task) -> bool {
    match task {
        Task::GuidedScale(_) | Task::Reading(_) => true,
        Task::Practice(Practice::Harmony {
            kind: Harmony::Coordination(_) | Harmony::Accompany { .. },
            ..
        }) => true,
        Task::BuildInterval { .. }
        | Task::HearInterval { .. }
        | Task::BuildChord { .. }
        | Task::HearChord { .. }
        | Task::Inversion { .. }
        | Task::Scale { .. }
        | Task::TimedScale { .. }
        | Task::ContextInterval { .. }
        | Task::Practice(_) => false,
    }
}
fn event(notes: Vec<usize>, tick: u32, duration: u32) -> WrittenEvent {
    let hand = if notes.iter().all(|n| *n < 60) {
        Hand::Left
    } else {
        Hand::Right
    };
    WrittenEvent {
        incorrect: false,
        hand,
        tick,
        duration: duration.max(1),
        tie_tick: None,
        choices: vec![notes],
    }
}
fn template(ex: &Exercise) -> WrittenScore {
    WrittenScore {
        active_tick: None,
        bars_per_page: ex.expected_score.as_ref().map_or(2, |s| s.bars_per_page),
        events: vec![],
        chords: ex
            .expected_score
            .as_ref()
            .map_or_else(Vec::new, |s| s.chords.clone()),
        hands: vec![],
        key_fifths: 0,
        ticks: 8,
        bpm: ex.bpm,
        lead: false,
        symbols_only: false,
        written_spelling: Some(ex.spelling.clone()),
        caption: if ex.bpm.is_some() {
            "Timing is shown on an eighth-note grid; grading uses exact MIDI timing.".into()
        } else {
            "Pitch order / voicing example; spacing and note lengths are not graded.".into()
        },
    }
}
fn finish(score: &mut WrittenScore) {
    score.events.sort_by_key(|e| e.tick);
    score.ticks = score
        .events
        .iter()
        .map(|e| e.tick + e.duration)
        .max()
        .unwrap_or(8)
        .max(8)
        .div_ceil(8)
        * 8;
    score.hands = [Hand::Right, Hand::Left]
        .into_iter()
        .filter(|hand| score.events.iter().any(|e| e.hand == *hand))
        .collect();
    if score.hands.is_empty() {
        score.hands.push(Hand::Right);
    }
}
fn grouped(score: &mut WrittenScore, notes: &[usize], tick: u32, duration: u32) {
    for hand in [Hand::Right, Hand::Left] {
        let group: Vec<_> = notes
            .iter()
            .copied()
            .filter(|n| (*n >= 60) == (hand == Hand::Right))
            .collect();
        if !group.is_empty() {
            score.events.push(event(group, tick, duration));
        }
    }
}
fn chord_example(root: usize, quality: usize, inversion: usize) -> Vec<usize> {
    CHORDS[quality]
        .iter()
        .enumerate()
        .map(|(i, n)| 48 + root + n + if i < inversion { 12 } else { 0 })
        .collect()
}
pub fn expected(ex: &Exercise) -> WrittenScore {
    if let Some(reading) = &ex.reading {
        let mut score = reading.clone();
        score.lead = false;
        score.symbols_only = false;
        return score;
    }
    let mut score = template(ex);
    match &ex.answer {
        Answer::Performance(performance) => {
            for (i, frame) in performance.frames.iter().enumerate() {
                let tick = if performance.timed {
                    (frame.at_ms * u64::from(ex.bpm.unwrap_or(60)) / 30000) as u32
                } else {
                    i as u32 * 2
                };
                let duration = if performance.timed {
                    frame
                        .hold_ms
                        .map(|ms| {
                            (ms as f64 * f64::from(ex.bpm.unwrap_or(60)) / 30000.).round() as u32
                        })
                        .unwrap_or(2)
                } else {
                    2
                };
                grouped(&mut score, &frame.choices[0], tick, duration);
            }
        }
        Answer::OctaveSequence(notes)
        | Answer::TransposedSequence(notes)
        | Answer::PitchClasses(notes) => {
            for (i, &note) in notes.iter().enumerate() {
                score.events.push(event(
                    vec![if note < 12 { 60 + note } else { note }],
                    i as u32 * 2,
                    2,
                ));
            }
        }
        Answer::AnchoredInterval {
            root,
            semitones,
            direction,
        } => {
            let first = 60 + root;
            for (i, n) in [
                first,
                if *direction == Direction::Down {
                    first - semitones
                } else {
                    first + semitones
                },
            ]
            .into_iter()
            .enumerate()
            {
                score.events.push(event(vec![n], i as u32 * 2, 2));
            }
        }
        Answer::Interval(semitones) => {
            score.events = vec![event(vec![60], 0, 2), event(vec![60 + semitones], 2, 2)];
        }
        Answer::Quality(q) => grouped(&mut score, &chord_example(0, *q, 0), 0, 4),
        Answer::RootChord { root, quality } => {
            grouped(&mut score, &chord_example(*root, *quality, 0), 0, 4)
        }
        Answer::Inversion {
            root,
            quality,
            inversion,
        } => grouped(
            &mut score,
            &chord_example(root.unwrap_or(0), *quality, *inversion),
            0,
            4,
        ),
    }
    finish(&mut score);
    score
}
fn key(task: &Task) -> Option<(usize, Scale)> {
    match task {
        Task::GuidedScale(inner) => key(inner),
        Task::Scale { root, minor, .. } | Task::TimedScale { root, minor, .. } => {
            Some((*root, if *minor { Scale::Minor } else { Scale::Major }))
        }
        Task::Practice(
            Practice::Scale { root, scale, .. }
            | Practice::Melody { root, scale, .. }
            | Practice::Harmony { root, scale, .. },
        ) => Some((*root, *scale)),
        _ => None,
    }
}
fn key_fifths(root: usize, scale: Scale) -> i8 {
    if !matches!(scale, Scale::Major | Scale::Minor) {
        return 0;
    }
    let spelling = crate::NoteSpelling::for_scale(root, scale);
    let fifths: i8 = scale
        .steps()
        .iter()
        .map(|step| spelling.staff_position(60 + root + step).1)
        .sum();
    // Theoretical keys such as Db minor need more than seven accidentals.
    // Print their written accidentals explicitly instead of an enharmonic signature.
    if (-7..=7).contains(&fifths) {
        fifths
    } else {
        0
    }
}
/// Fill written rests so an offbeat melody cannot look like downbeat quarters.
fn fill_rests(score: &mut WrittenScore) {
    let mut rests = vec![];
    for &hand in &score.hands {
        let occupied: BTreeSet<_> = score
            .events
            .iter()
            .filter(|e| e.hand == hand)
            .flat_map(|e| e.tick..e.tick + e.duration)
            .collect();
        let mut tick = 0;
        while tick < score.ticks {
            if occupied.contains(&tick) {
                tick += 1;
                continue;
            }
            let mut end = tick + 1;
            while end < score.ticks && !occupied.contains(&end) && end / 8 == tick / 8 {
                end += 1;
            }
            while tick < end {
                let duration = [8, 4, 2, 1]
                    .into_iter()
                    .find(|d| tick % d == 0 && tick + d <= end)
                    .unwrap();
                rests.push(WrittenEvent {
                    incorrect: false,
                    hand,
                    tick,
                    duration,
                    tie_tick: None,
                    choices: vec![],
                });
                tick += duration;
            }
        }
    }
    score.events.extend(rests);
    score.events.sort_by_key(|e| e.tick);
}
pub fn decorate(task: &Task, ex: &mut Exercise) {
    let mut answer = expected(ex);
    if let Some((root, scale)) = key(task) {
        answer.key_fifths = key_fifths(root, scale);
    }
    if matches!(task, Task::GuidedScale(_)) {
        // Example notation uses a comfortable octave, while grading continues
        // accepting the original task's octave-independent scale contract.
        let lowest = answer
            .events
            .iter()
            .flat_map(|e| e.notes())
            .min()
            .copied()
            .unwrap_or(60);
        let shift = 60i32 - (lowest / 12 * 12) as i32;
        for e in &mut answer.events {
            for choice in &mut e.choices {
                for n in choice {
                    *n = (*n as i32 + shift) as usize;
                }
            }
            e.hand = Hand::Right;
        }
        answer.hands = vec![Hand::Right];
        ex.prompt.push_str(" Use the score first; a later exercise asks for this scale from memory. Any starting octave is accepted.");
        answer.caption = "Score-guided scale · any starting octave is accepted.".into();
    }
    if matches!(
        task,
        Task::Practice(Practice::Harmony {
            kind: Harmony::Coordination(_),
            ..
        })
    ) {
        answer.caption="LH: hold each bass note for its whole bar. RH: rest on each numbered beat; play on each &.".into();
        fill_rests(&mut answer);
    }
    if let Task::Practice(Practice::Harmony {
        root,
        scale,
        kind: Harmony::Accompany { bars, thirds },
    }) = task
    {
        // A single treble staff can include notes below middle C. Never move
        // close voicings into the bass merely to keep them on one staff.
        if let Answer::Performance(performance) = &ex.answer {
            answer.events = performance
                .frames
                .iter()
                .map(|frame| {
                    let mut notes = frame.choices[0].clone();
                    while notes.iter().min().is_some_and(|n| *n < 55) {
                        for note in &mut notes {
                            *note += 12;
                        }
                    }
                    let mut written = event(notes, (frame.at_ms / 500) as u32, 8);
                    written.hand = Hand::Right;
                    written
                })
                .collect();
            answer.hands = vec![Hand::Right];
        }
        answer.bars_per_page = *bars as u32;
        answer.chords = crate::practice::accompaniment_degrees(*bars)
            .iter()
            .enumerate()
            .map(|(i, &degree)| ChordSymbol {
                tick: i as u32 * 8,
                text: crate::practice::accompaniment_chord(*root, *scale, degree, *thirds),
            })
            .collect();
        answer.caption = "One chord per bar: play its written two-note voicing on beat 1 and sustain for at least three beats.".into();
        fill_rests(&mut answer);
    }
    ex.prompt_score = before_answer(task).then(|| {
        if let Some(reading) = &ex.reading {
            reading.clone()
        } else {
            answer.clone()
        }
    });
    ex.expected_score = Some(answer);
}

pub fn played(
    ex: &Exercise,
    notes: &[usize],
    evidence: &[MidiEvidence],
    origin_ms: i64,
) -> WrittenScore {
    let mut score = template(ex);
    score.key_fifths = ex.expected_score.as_ref().map_or(0, |s| s.key_fifths);
    if ex.bpm.is_some() {
        let first = if matches!(&ex.answer, Answer::Performance(p) if p.fixed_start) {
            0
        } else {
            evidence
                .iter()
                .find(|e| e.velocity > 0)
                .map_or(0, |e| e.offset_ms)
        };
        for (i, on) in evidence.iter().enumerate().filter(|(_, e)| e.velocity > 0) {
            let bpm = f64::from(ex.bpm.unwrap());
            let tick = ((on.offset_ms as i64 - first as i64 + origin_ms).max(0) as f64 * bpm
                / 30000.)
                .round() as u32;
            let duration = crate::evidence::held_ms(evidence, i)
                .map(|held| ((held as f64 + 60.) * bpm / 30000.).round() as u32)
                .unwrap_or(1)
                .max(1);
            score.events.push(event(vec![on.note], tick, duration));
        }
    } else {
        match &ex.answer {
            Answer::Performance(performance) => {
                let mut offset = 0;
                for (i, frame) in performance.frames.iter().enumerate() {
                    let end = (offset + frame.choices[0].len()).min(notes.len());
                    grouped(&mut score, &notes[offset..end], i as u32 * 2, 2);
                    offset = end;
                }
                for (i, &note) in notes[offset..].iter().enumerate() {
                    score.events.push(event(
                        vec![note],
                        (performance.frames.len() + i) as u32 * 2,
                        2,
                    ));
                }
            }
            Answer::Quality(_) | Answer::RootChord { .. } | Answer::Inversion { .. } => {
                grouped(&mut score, notes, 0, 4)
            }
            _ => {
                for (i, &note) in notes.iter().enumerate() {
                    score.events.push(event(vec![note], i as u32 * 2, 2));
                }
            }
        }
    }
    finish(&mut score);
    if ex.bpm.is_some() {
        if let Some(expected) = &ex.expected_score {
            score.ticks = score.ticks.max(expected.ticks);
            for &hand in &expected.hands {
                if (ex.reading.is_some()
                    || matches!(&ex.answer, Answer::Performance(p) if p.exact_register))
                    && !score.hands.contains(&hand)
                {
                    score.hands.push(hand);
                }
            }
        }
        fill_rests(&mut score);
    }
    score
}

/// Show accepted articulation as the written musical value, rather than a
/// literal transcription of small release gaps. Keep actual pitches and all
/// flagged errors; this is presentation only and never changes MIDI evidence.
fn accompaniment_notation(ex: &Exercise, mut score: WrittenScore) -> WrittenScore {
    if !matches!(&ex.answer, Answer::Performance(p) if p.accompaniment) {
        return score;
    }
    let Some(expected) = &ex.expected_score else {
        return score;
    };
    let erroneous_rests: Vec<_> = score
        .events
        .iter()
        .filter(|e| e.notes().is_empty() && e.incorrect)
        .map(|e| (e.tick, e.tick + e.duration))
        .collect();
    score.events.retain(|e| !e.notes().is_empty());
    for e in &mut score.events {
        // Accompaniment voicings belong to one staff, including notes below C4.
        e.hand = expected.hands[0];
        if !e.incorrect {
            if let Some(wanted) = expected
                .events
                .iter()
                .filter(|target| !target.notes().is_empty())
                .min_by_key(|target| target.tick.abs_diff(e.tick))
                .filter(|target| target.tick.abs_diff(e.tick) <= 1)
            {
                e.tick = wanted.tick;
                e.duration = wanted.duration;
                e.tie_tick = wanted.tie_tick;
            }
        }
    }
    let mut chords: Vec<WrittenEvent> = vec![];
    for e in score.events.drain(..) {
        if let Some(chord) = chords.iter_mut().find(|c| {
            c.tick == e.tick
                && c.duration == e.duration
                && c.hand == e.hand
                && c.incorrect == e.incorrect
                && c.tie_tick == e.tie_tick
        }) {
            chord.choices[0].extend_from_slice(e.notes());
            chord.choices[0].sort_unstable();
        } else {
            chords.push(e);
        }
    }
    score.events = chords;
    finish(&mut score);
    score.ticks = score.ticks.max(expected.ticks);
    fill_rests(&mut score);
    for rest in score.events.iter_mut().filter(|e| e.notes().is_empty()) {
        rest.incorrect = erroneous_rests
            .iter()
            .any(|&(start, end)| start < rest.tick + rest.duration && rest.tick < end);
    }
    score.caption =
        "Accepted timing and release gaps use the written rhythm; pitches are what you played."
            .into();
    score
}

/// Mark feedback using original MIDI timing, before the score quantizes it.
pub fn response(
    ex: &Exercise,
    notes: &[usize],
    evidence: &[MidiEvidence],
    origin_ms: i64,
    correct: bool,
) -> WrittenScore {
    let mut actual = played(ex, notes, evidence, origin_ms);
    if correct {
        return accompaniment_notation(ex, actual);
    }
    let expected = feedback_expected(ex, notes, false);
    let first = if matches!(&ex.answer, Answer::Performance(p) if p.fixed_start) {
        0
    } else {
        evidence
            .iter()
            .find(|e| e.velocity > 0)
            .map_or(0, |e| e.offset_ms)
    };
    let tolerance = match &ex.answer {
        Answer::Performance(p) => p.tolerance_ms,
        _ => 180,
    };
    let any_pitch = matches!(&ex.answer, Answer::Performance(p) if p.any_pitch);
    let pitch_correct = ex.correct(notes);
    let separate_hands =
        ex.reading.is_some() || matches!(&ex.answer, Answer::Performance(p) if p.exact_register);
    let parts: Vec<_> = if !separate_hands {
        vec![None]
    } else {
        actual.hands.iter().copied().map(Some).collect()
    };
    for hand in parts {
        let targets: Vec<_> = expected
            .events
            .iter()
            .filter(|e| hand.is_none_or(|h| e.hand == h) && !e.notes().is_empty())
            .flat_map(|e| {
                std::iter::repeat_n(e, if ex.bpm.is_some() { e.notes().len() } else { 1 })
            })
            .collect();
        let indices: Vec<_> = actual
            .events
            .iter()
            .enumerate()
            .filter(|(_, e)| hand.is_none_or(|h| e.hand == h) && !e.notes().is_empty())
            .map(|(i, _)| i)
            .collect();
        let attacks: Vec<_> = evidence
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                e.velocity > 0 && hand.is_none_or(|h| (h == Hand::Right) == (e.note >= 60))
            })
            .collect();
        let target_times: Vec<_> = targets
            .iter()
            .map(|e| expected.milliseconds(e.tick) as i64)
            .collect();
        let times: Vec<_> = if ex.bpm.is_some() {
            attacks
                .iter()
                .map(|(_, e)| e.offset_ms as i64 - first as i64 + origin_ms)
                .collect()
        } else {
            indices
                .iter()
                .map(|&i| actual.milliseconds(actual.events[i].tick) as i64)
                .collect()
        };
        let mut acceptable_releases = BTreeSet::new();
        for (target, input) in
            crate::rhythm::align_attacks(&target_times, &times, ex.bpm.unwrap_or(60))
        {
            let Some(input) = input else {
                continue;
            };
            let event = &mut actual.events[indices[input]];
            let Some(target) = target else {
                event.incorrect = true;
                continue;
            };
            let wanted = targets[target];
            event.incorrect = !pitch_correct
                && !wanted.choices.iter().any(|choice| {
                    event.notes().iter().all(|n| {
                        choice.iter().any(|wanted| {
                            if matches!(&ex.answer, Answer::Performance(p) if !p.exact_register) {
                                wanted % 12 == n % 12
                            } else {
                                wanted == n
                            }
                        })
                    })
                });
            if ex.bpm.is_some() {
                let (index, _) = attacks[input];
                let held = crate::evidence::held_ms(evidence, index);
                let target_hold = if let Answer::Performance(p) = &ex.answer {
                    p.frames
                        .iter()
                        .find(|f| f.at_ms.abs_diff(target_times[target] as u64) < 2)
                        .and_then(|f| f.hold_ms)
                } else {
                    None
                };
                let hold_ok = if ex.reading.is_some() {
                    expected.hold_matches(wanted.duration, held)
                } else if let Answer::Performance(p) = &ex.answer {
                    target_hold.is_none_or(|target| p.hold_matches(target, held))
                } else {
                    true
                };
                if hold_ok {
                    acceptable_releases.insert(wanted.tick);
                }
                event.incorrect |=
                    times[input].abs_diff(target_times[target]) > tolerance || !hold_ok;
            }
        }
        // Show detached releases as rests without calling them errors when
        // the hold is acceptable. Missing notes and short holds remain red.
        for rest in actual
            .events
            .iter_mut()
            .filter(|e| hand.is_none_or(|h| e.hand == h) && e.notes().is_empty())
        {
            rest.incorrect = !any_pitch
                && targets.iter().any(|e| {
                    !acceptable_releases.contains(&e.tick)
                        && e.tick < rest.tick + rest.duration
                        && rest.tick < e.tick + e.duration
                });
        }
    }
    accompaniment_notation(ex, actual)
}

pub fn feedback_expected(ex: &Exercise, notes: &[usize], correct: bool) -> WrittenScore {
    let mut score = ex.expected_score.clone().unwrap_or_else(|| expected(ex));
    // Match legal octave/transposition choices when comparing melodic answers.
    if let Some(&first) = notes.first() {
        if let Answer::OctaveSequence(expected) | Answer::TransposedSequence(expected) = &ex.answer
        {
            if let Some(&start) = expected.first() {
                let difference = first as i32 - start as i32;
                let shift = if matches!(ex.answer, Answer::TransposedSequence(_)) {
                    difference
                } else {
                    (first as i32 / 12 - start as i32 / 12) * 12
                };
                for e in &mut score.events {
                    for choice in &mut e.choices {
                        for n in choice {
                            *n = (*n as i32 + shift).clamp(0, 127) as usize;
                        }
                    }
                    e.hand = if e.notes().iter().all(|n| *n < 60) {
                        Hand::Left
                    } else {
                        Hand::Right
                    };
                }
                finish(&mut score);
            }
        }
    }
    if correct
        && ex.bpm.is_none()
        && ex.reading.is_none()
        && !matches!(
            ex.answer,
            Answer::OctaveSequence(_) | Answer::TransposedSequence(_) | Answer::PitchClasses(_)
        )
    {
        score = played(ex, notes, &[], 0);
        score.caption="Your answer is a valid realization; other permitted octaves or voicings may also work.".into();
    } else if matches!(
        ex.answer,
        Answer::Quality(_)
            | Answer::RootChord { .. }
            | Answer::Inversion { .. }
            | Answer::Interval(_)
    ) {
        score.caption =
            "Example solution: allowed transpositions / voicings may differ from this notation."
                .into();
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        curriculum::curriculum,
        learner::{Mastery, Profile},
    };

    #[test]
    fn accompaniment_has_a_complete_named_chart_and_whole_bar_voicings() {
        let graph = curriculum();
        for thirds in [false, true] {
            let id = format!("harmony.7.major.accompany.12.{thirds}");
            let ex = Exercise::generate(graph.iter().find(|s| s.id == id).unwrap(), 42);
            let score = ex.prompt_score.as_ref().unwrap();
            assert_eq!(score.bars_per_page, 12);
            assert_eq!(score.page_count(), 1);
            assert_eq!(score.page(0).events.len(), 12);
            assert_eq!(score.hands, [Hand::Right]);
            assert!(score
                .events
                .iter()
                .all(|e| e.duration == 8 && e.notes().len() == 2));
            let names: Vec<_> = score.chords.iter().map(|c| c.text.as_str()).collect();
            assert_eq!(
                names,
                if thirds {
                    vec!["G", "G", "G", "G", "C", "C", "G", "G", "D", "C", "G", "D"]
                } else {
                    vec![
                        "Gmaj7", "Gmaj7", "Gmaj7", "Gmaj7", "Cmaj7", "Cmaj7", "Gmaj7", "Gmaj7",
                        "D7", "Cmaj7", "Gmaj7", "D7",
                    ]
                }
            );
            let notes: Vec<_> = score
                .events
                .iter()
                .flat_map(|e| e.notes().iter().copied())
                .collect();
            assert!(ex.correct(&notes), "notation must be a valid answer");
            assert_eq!(
                played(&ex, &notes, &[], 0).chords,
                score.chords,
                "feedback retains the chord chart and its image geometry"
            );
            assert!(!ex.prompt.contains("chord degrees"));
            assert!(!ex.prompt.contains("3½"));
            let Answer::Performance(p) = &ex.answer else {
                unreachable!()
            };
            assert!(p.frames.iter().all(|f| f.hold_ms == Some(4000)));
        }
    }
    #[test]
    fn extra_bass_is_red_without_marking_the_later_correct_note() {
        let graph = curriculum();
        let mut ex = Exercise::generate(
            graph.iter().find(|s| s.id == "reading.together.2").unwrap(),
            42,
        );
        let score = ex.expected_score.as_mut().unwrap();
        score.events = vec![event(vec![48], 0, 8), event(vec![55], 8, 8)];
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
        let score = response(&ex, &[48, 48, 55], &evidence, 0, false);
        assert!(score
            .events
            .iter()
            .filter(|e| e.notes() == [48])
            .all(|e| e.incorrect));
        assert!(
            !score
                .events
                .iter()
                .find(|e| e.notes() == [55])
                .unwrap()
                .incorrect
        );
    }
    #[test]
    fn rhythm_errors_do_not_mark_permitted_pitches_as_wrong() {
        let graph = curriculum();
        let ex = Exercise::generate(graph.iter().find(|s| s.id == "rhythm.0.60").unwrap(), 42);
        let mut evidence = vec![];
        let mut notes = vec![];
        for i in 0..8 {
            let note = 48 + i * 2;
            notes.push(note);
            let offset_ms = i as u64 * 1000 + if i == 3 { 300 } else { 0 };
            evidence.push(MidiEvidence {
                offset_ms,
                note,
                velocity: 80,
                channel: 0,
            });
            evidence.push(MidiEvidence {
                offset_ms: offset_ms + 940,
                note,
                velocity: 0,
                channel: 0,
            });
        }
        evidence.sort_by_key(|e| e.offset_ms);
        let score = response(&ex, &notes, &evidence, 0, false);
        let errors: Vec<_> = score
            .events
            .iter()
            .filter(|e| !e.notes().is_empty() && e.incorrect)
            .collect();
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].notes(), [54]);
    }
    #[test]
    fn response_marks_raw_hold_errors_and_writes_rests_for_silence() {
        let graph = curriculum();
        let mut ex = Exercise::generate(
            graph.iter().find(|s| s.id == "reading.together.2").unwrap(),
            42,
        );
        let mut expected = ex.expected_score.clone().unwrap();
        expected.events = vec![
            event(vec![60], 0, 2),
            event(vec![62], 2, 2),
            event(vec![48], 0, 8),
        ];
        expected.hands = vec![Hand::Right, Hand::Left];
        expected.ticks = 8;
        ex.expected_score = Some(expected);
        let evidence: Vec<_> = [(0, 60, 80), (490, 60, 0), (1000, 62, 80), (1400, 62, 0)]
            .into_iter()
            .map(|(offset_ms, note, velocity)| MidiEvidence {
                offset_ms,
                note,
                velocity,
                channel: 0,
            })
            .collect();
        let score = response(&ex, &[60, 62], &evidence, 0, false);
        let c = score.events.iter().find(|e| e.notes() == [60]).unwrap();
        assert_eq!(c.duration, 1, "short hold is shown as an eighth note");
        assert!(
            c.incorrect,
            "raw 490ms hold is shorter than the 500ms reading minimum"
        );
        assert!(score
            .events
            .iter()
            .any(|e| e.hand == Hand::Right && e.tick == 3 && e.notes().is_empty() && e.incorrect));
        assert!(score.events.iter().any(|e| e.hand == Hand::Left
            && e.tick == 0
            && e.duration == 8
            && e.notes().is_empty()
            && e.incorrect));
        assert!(score
            .events
            .iter()
            .any(|e| e.hand == Hand::Right && e.tick == 4 && e.notes().is_empty() && !e.incorrect));
        let correct = response(&ex, &[60, 62], &evidence, 0, true);
        assert!(correct.events.iter().all(|e| !e.incorrect));
    }
    #[test]
    fn notation_policy_covers_every_skill_without_leaking_listening_answers() {
        let graph = curriculum();
        let mut guided = 0;
        for skill in &graph {
            let ex = Exercise::generate(skill, 42);
            assert_eq!(
                ex.prompt_score.is_some(),
                before_answer(&skill.task),
                "{}",
                skill.id
            );
            assert!(ex.expected_score.is_some(), "{}", skill.id);
            if let Task::GuidedScale(_) = &skill.task {
                guided += 1;
                let memory = graph
                    .iter()
                    .find(|s| s.id == skill.id.strip_prefix("guided.").unwrap())
                    .unwrap();
                assert!(memory.requires.iter().any(|r| r.skill == skill.id));
                assert!(Exercise::generate(memory, 42).prompt_score.is_none());
                let mut profile = Profile::default();
                for dependency in memory.requires.iter().filter(|r| r.skill != skill.id) {
                    profile.skills.insert(
                        dependency.skill.clone(),
                        Mastery {
                            score: 10.,
                            attempts: 12,
                            ..Mastery::default()
                        },
                    );
                }
                assert!(!profile.unlocked(memory));
                profile.skills.insert(
                    skill.id.clone(),
                    Mastery {
                        score: 7.,
                        attempts: 6,
                        ..Mastery::default()
                    },
                );
                assert!(profile.unlocked(memory));
                profile.skills.clear();
                // Existing practice is retained; adding a guide does not lock old history.
                profile.skills.insert(
                    memory.id.clone(),
                    Mastery {
                        attempts: 1,
                        ..Mastery::default()
                    },
                );
                assert!(profile.unlocked(memory));
            }
        }
        assert_eq!(guided, 432);
    }
    #[test]
    fn dictation_comparison_preserves_wrong_missing_and_extra_notes() {
        let graph = curriculum();
        let skill = graph
            .iter()
            .find(|s| {
                matches!(
                    s.task,
                    Task::Practice(Practice::Melody {
                        length: 3,
                        rhythmic: false,
                        ..
                    })
                )
            })
            .unwrap();
        let ex = Exercise::generate(skill, 42);
        let expected = ex.expected_evidence();
        let response = vec![expected[0], expected[1] + 1];
        let actual = played(&ex, &response, &[], 0);
        assert_eq!(
            actual
                .events
                .iter()
                .flat_map(|e| e.notes())
                .copied()
                .collect::<Vec<_>>(),
            response
        );
        assert_eq!(feedback_expected(&ex, &response, false).events.len(), 3);
        let mut extra = expected.clone();
        extra.push(72);
        assert_eq!(played(&ex, &extra, &[], 0).events.len(), 4);
        assert!(actual.page(10).events.is_empty());
    }
    #[test]
    fn coordination_score_explains_overlapping_parts_and_offbeats() {
        let graph = curriculum();
        let skill = graph
            .iter()
            .find(|s| {
                matches!(
                    s.task,
                    Task::Practice(Practice::Harmony {
                        kind: Harmony::Coordination(true),
                        ..
                    })
                )
            })
            .unwrap();
        let ex = Exercise::generate(skill, 42);
        let score = ex.prompt_score.unwrap();
        assert_eq!(score.page_count(), 2);
        for bar in 0..4 {
            assert!(score
                .events
                .iter()
                .any(|e| e.hand == Hand::Left && e.tick == bar * 8 && e.duration == 8));
            for beat in 0..4 {
                assert!(score.events.iter().any(|e| e.hand == Hand::Right
                    && e.tick == bar * 8 + beat * 2
                    && e.notes().is_empty()));
                assert!(score.events.iter().any(|e| e.hand == Hand::Right
                    && e.tick == bar * 8 + beat * 2 + 1
                    && !e.notes().is_empty()));
            }
        }
    }
    #[test]
    fn played_timing_retains_fixed_start_delay_and_long_holds() {
        let graph = curriculum();
        let skill = graph
            .iter()
            .find(|s| {
                matches!(
                    s.task,
                    Task::Practice(Practice::Harmony {
                        kind: Harmony::Coordination(false),
                        ..
                    })
                )
            })
            .unwrap();
        let ex = Exercise::generate(skill, 42);
        let evidence = vec![
            MidiEvidence {
                offset_ms: 0,
                channel: 0,
                note: 48,
                velocity: 90,
            },
            MidiEvidence {
                offset_ms: 5000,
                channel: 0,
                note: 48,
                velocity: 0,
            },
        ];
        let score = played(&ex, &[48], &evidence, 500);
        let note = score.events.iter().find(|e| e.notes() == [48]).unwrap();
        assert_eq!(note.tick, 1);
        assert_eq!(note.duration, 10);
    }
    #[test]
    fn signatures_follow_written_keys_including_seven_flats_and_theoretical_minor() {
        assert_eq!(key_fifths(8, Scale::Minor), -7);
        assert_eq!(key_fifths(1, Scale::Minor), 0);
        for root in 0..12 {
            for scale in [Scale::Major, Scale::Minor] {
                let fifths = key_fifths(root, scale);
                if fifths == 0 && root == 1 {
                    continue;
                }
                let order = if fifths > 0 {
                    [3, 0, 4, 1, 5, 2, 6]
                } else {
                    [6, 2, 5, 1, 4, 0, 3]
                };
                let spelling = crate::NoteSpelling::for_scale(root, scale);
                for step in scale.steps() {
                    let (position, accidental) = spelling.staff_position(60 + root + step);
                    let expected =
                        if order[..fifths.unsigned_abs() as usize].contains(&(position % 7)) {
                            fifths.signum()
                        } else {
                            0
                        };
                    assert_eq!(accidental, expected, "{root} {scale:?}");
                }
            }
        }
    }
    #[test]
    fn submitted_dictation_exposes_both_scores_only_in_feedback() {
        use crate::{Phase, Session};
        use keyboard::Event;
        use std::time::{Duration, Instant};
        let mut session = Session::preview("melody.0.3").unwrap();
        let now = Instant::now();
        session.start(now);
        assert!(session.view().display_score.is_none());
        let start = now + Duration::from_secs(30);
        session.tick(start).unwrap();
        if session.view().exploring {
            session.submit(false).unwrap();
        }
        assert!(session.view().display_score.is_none());
        for (i, note) in [60, 63, 67].into_iter().enumerate() {
            session.input(
                Event::Note {
                    channel: 0,
                    note,
                    velocity: 90,
                },
                start + Duration::from_millis(i as u64 * 500),
            );
        }
        session.tick(start + Duration::from_secs(2)).unwrap();
        let view = session.view();
        assert!(view.phase == Phase::Feedback);
        assert!(view.display_score.is_some());
        assert_eq!(
            view.played_score
                .unwrap()
                .events
                .iter()
                .flat_map(|e| e.notes())
                .copied()
                .collect::<Vec<_>>(),
            [60, 63, 67]
        );
    }
    #[test]
    fn wrong_first_pitch_does_not_move_the_example_to_another_octave() {
        let graph = curriculum();
        let skill = graph.iter().find(|s| s.id == "melody.0.5").unwrap();
        let mut ex = Exercise::generate(skill, 42);
        ex.answer = Answer::OctaveSequence(vec![69, 67, 60, 65, 64]);
        ex.expected_score = Some(expected(&ex));
        let score = feedback_expected(&ex, &[60, 63, 67], false);
        assert_eq!(score.events[0].notes(), [69]);
        assert_eq!(score.hands, [Hand::Right]);
    }
    #[test]
    fn accompaniment_correction_does_not_erase_later_bars_or_legal_octaves() {
        let graph = curriculum();
        let ex = Exercise::generate(
            graph
                .iter()
                .find(|s| s.id == "harmony.7.major.accompany.12.true")
                .unwrap(),
            42,
        );
        // Recorded regression: mostly correct low-register chords, an F-natural
        // in bar 9 followed by a correction, then a late/short bar 11.
        let evidence: Vec<_> = [
            (0, 47, 48),
            (0, 50, 40),
            (3415, 50, 0),
            (3440, 47, 0),
            (3928, 47, 43),
            (3991, 50, 42),
            (7478, 50, 0),
            (7488, 47, 0),
            (7889, 50, 69),
            (7900, 47, 60),
            (11535, 50, 0),
            (11547, 47, 0),
            (11932, 47, 49),
            (11995, 50, 42),
            (15052, 50, 0),
            (15106, 47, 0),
            (15883, 52, 59),
            (15883, 55, 45),
            (19429, 55, 0),
            (19442, 52, 0),
            (19931, 52, 56),
            (19931, 55, 46),
            (23425, 55, 0),
            (23425, 52, 0),
            (23925, 47, 44),
            (23983, 50, 50),
            (27448, 50, 0),
            (27448, 47, 0),
            (27919, 47, 57),
            (27923, 50, 55),
            (30973, 47, 0),
            (31046, 50, 0),
            (32005, 53, 67),
            (32010, 57, 51),
            (33853, 53, 0),
            (33870, 57, 0),
            (33974, 57, 35),
            (33980, 54, 43),
            (35333, 57, 0),
            (35368, 54, 0),
            (35858, 52, 50),
            (35867, 55, 39),
            (39388, 52, 0),
            (39494, 55, 0),
            (40483, 47, 54),
            (40524, 50, 54),
            (43351, 47, 0),
            (43416, 50, 0),
            (43891, 54, 67),
            (43912, 57, 42),
            (48016, 54, 0),
            (48055, 57, 0),
        ]
        .into_iter()
        .map(|(offset_ms, note, velocity)| MidiEvidence {
            offset_ms,
            note,
            velocity,
            channel: 0,
        })
        .collect();
        let Answer::Performance(p) = &ex.answer else {
            panic!()
        };
        let report = p.accompaniment_report(&evidence, 76);
        assert_eq!(
            (
                report.pitches,
                report.timing,
                report.holds,
                report.total,
                report.extras
            ),
            (11, 11, 11, 12, 1)
        );
        assert_eq!(report.issues, ["bar 9: notes/hold", "bar 11: timing"]);
        let notes: Vec<_> = evidence
            .iter()
            .filter(|e| e.velocity > 0)
            .map(|e| e.note)
            .collect();
        let response = response(&ex, &notes, &evidence, 76, false);
        let wrong: Vec<_> = response
            .events
            .iter()
            .filter(|e| e.incorrect && !e.notes().is_empty())
            .collect();
        assert_eq!(
            wrong.iter().map(|e| e.notes().len()).sum::<usize>(),
            6,
            "Only the bad chord, its correction, and the late/short chord are red"
        );
        assert!(
            response
                .events
                .iter()
                .filter(|e| e.tick < 64)
                .all(|e| !e.incorrect),
            "Normal releases and legal lower octaves stay neutral"
        );
        let score = ex.expected_score.as_ref().unwrap();
        assert_eq!(score.events[0].notes(), [59, 62]);
        assert!(score.events.iter().all(|e| e.hand == Hand::Right));
    }

    #[test]
    fn accompaniment_accepts_detached_holds_but_not_stabs_or_missing_releases() {
        let graph = curriculum();
        let ex = Exercise::generate(
            graph
                .iter()
                .find(|s| s.id == "harmony.7.major.accompany.12.true")
                .unwrap(),
            42,
        );
        let Answer::Performance(p) = &ex.answer else {
            panic!()
        };
        for duration in [2750, 2757, 2766, 3000, 3400, 4000, 4500] {
            assert!(p.hold_matches(4000, Some(duration)));
        }
        for duration in [500, 2749, 4501] {
            assert!(!p.hold_matches(4000, Some(duration)));
        }
        assert!(!p.hold_matches(4000, None));
        let mut other_exercise = p.clone();
        other_exercise.accompaniment = false;
        assert!(
            !other_exercise.hold_matches(4000, Some(3000)),
            "Three beats must not satisfy whole notes in other exercise types"
        );
        assert!(other_exercise.hold_matches(4000, Some(4000)));
        let mut evidence = vec![];
        for frame in &p.frames {
            for &note in &frame.choices[0] {
                evidence.push(MidiEvidence {
                    offset_ms: frame.at_ms,
                    note,
                    channel: 0,
                    velocity: 80,
                });
                evidence.push(MidiEvidence {
                    offset_ms: frame.at_ms + 3400,
                    note,
                    channel: 0,
                    velocity: 0,
                });
            }
        }
        evidence.sort_by_key(|e| e.offset_ms);
        let report = p.accompaniment_report(&evidence, 0);
        assert_eq!(
            (report.pitches, report.timing, report.holds, report.extras),
            (12, 12, 12, 0)
        );
    }
    #[test]
    fn accepted_accompaniment_has_the_same_staff_chords_and_rhythm_as_the_score() {
        let graph = curriculum();
        let ex = Exercise::generate(
            graph
                .iter()
                .find(|s| s.id == "harmony.7.major.accompany.12.true")
                .unwrap(),
            42,
        );
        // Successful captured attempt: human attacks and detached releases,
        // including B3/D4 straddling middle C. These are still whole-bar chords.
        let evidence: Vec<_> = [
            (0, 62, 60),
            (0, 59, 42),
            (3540, 59, 0),
            (3567, 62, 0),
            (3878, 62, 81),
            (3878, 59, 73),
            (7610, 59, 0),
            (7615, 62, 0),
            (7862, 62, 60),
            (7873, 59, 55),
            (11494, 59, 0),
            (11538, 62, 0),
            (11854, 62, 62),
            (11875, 59, 55),
            (15317, 59, 0),
            (15356, 62, 0),
            (15868, 67, 55),
            (15879, 64, 60),
            (19507, 67, 0),
            (19538, 64, 0),
            (19863, 67, 70),
            (19873, 64, 67),
            (23375, 67, 0),
            (23394, 64, 0),
            (23912, 62, 64),
            (23912, 59, 55),
            (27455, 59, 0),
            (27518, 62, 0),
            (27913, 62, 52),
            (27913, 59, 48),
            (31453, 62, 0),
            (31472, 59, 0),
            (31911, 66, 62),
            (31963, 69, 62),
            (35396, 69, 0),
            (35438, 66, 0),
            (35862, 64, 46),
            (35874, 67, 53),
            (39227, 67, 0),
            (39236, 64, 0),
            (39907, 62, 57),
            (39919, 59, 55),
            (43298, 59, 0),
            (43330, 62, 0),
            (43855, 69, 60),
            (43855, 66, 64),
            (47929, 66, 0),
            (47940, 69, 0),
        ]
        .into_iter()
        .map(|(offset_ms, note, velocity)| MidiEvidence {
            offset_ms,
            note,
            velocity,
            channel: 0,
        })
        .collect();
        let Answer::Performance(p) = &ex.answer else {
            panic!()
        };
        let report = p.accompaniment_report(&evidence, 90);
        assert_eq!(
            (report.pitches, report.timing, report.holds, report.extras),
            (12, 12, 12, 0)
        );
        let expected = ex.expected_score.as_ref().unwrap();
        for shift in [0, 12] {
            let evidence: Vec<_> = evidence
                .iter()
                .cloned()
                .map(|mut e| {
                    e.note -= shift;
                    e
                })
                .collect();
            let notes: Vec<_> = evidence
                .iter()
                .filter(|e| e.velocity > 0)
                .map(|e| e.note)
                .collect();
            let actual = response(&ex, &notes, &evidence, 90, true);
            assert_eq!(actual.hands, expected.hands);
            assert_eq!(actual.ticks, expected.ticks);
            assert_eq!(actual.chords, expected.chords);
            assert_eq!(actual.events.len(), expected.events.len());
            for (got, wanted) in actual.events.iter().zip(&expected.events) {
                assert_eq!(
                    (got.tick, got.duration, got.hand, got.incorrect),
                    (wanted.tick, wanted.duration, wanted.hand, false)
                );
                assert_eq!(
                    got.notes(),
                    wanted.notes().iter().map(|n| n - shift).collect::<Vec<_>>()
                );
            }
            // Rendering must not replace the recorded MIDI durations.
            assert!(played(&ex, &notes, &evidence, 90)
                .events
                .iter()
                .any(|e| e.duration != 8));
        }
    }
}
