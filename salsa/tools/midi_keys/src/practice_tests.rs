use super::*;
use curriculum::{Direction, Task};
use exercise::Answer;
use music::Scale;
use performance::Performance;
use practice::{Harmony, Motif, Practice, Rhythm};

fn generated(task: Practice) -> Exercise {
    Exercise::generate(
        &Skill {
            stage: 0,
            id: "test".into(),
            title: "test".into(),
            task: Task::Practice(task),
            requires: vec![],
        },
        42,
    )
}
fn performed(score: &Performance) -> (Vec<usize>, Vec<Instant>, Vec<MidiEvidence>, Instant) {
    let start = Instant::now();
    let mut notes = vec![];
    let mut onsets = vec![];
    let mut evidence = vec![];
    for frame in &score.frames {
        for &note in &frame.choices[0] {
            notes.push(note);
            onsets.push(start + Duration::from_millis(frame.at_ms));
            evidence.push(MidiEvidence {
                offset_ms: frame.at_ms,
                channel: 0,
                note,
                velocity: 90,
            });
            evidence.push(MidiEvidence {
                offset_ms: frame.at_ms + frame.hold_ms.unwrap_or(180),
                channel: 0,
                note,
                velocity: 0,
            });
        }
    }
    evidence.sort_by_key(|e| (e.offset_ms, e.velocity > 0));
    (notes, onsets, evidence, start)
}
#[test]
fn every_new_exercise_has_a_valid_sounding_and_gradeable_example() {
    let graph = curriculum();
    eprintln!("Curriculum contains {} skills", graph.len());
    for skill in graph.iter().filter(|s| matches!(s.task, Task::Practice(_))) {
        for seed in [1, 42, 777] {
            let ex = Exercise::generate(skill, seed);
            let example = ex.expected_evidence();
            assert!(example.iter().all(|n| *n < 128), "{}", skill.id);
            assert!(ex.correct(&example), "{}: {:?}", skill.id, example);
            let mut wrong = example.clone();
            wrong.pop();
            assert!(!ex.correct(&wrong), "{} accepts missing note", skill.id);
            if let Answer::Performance(score) = &ex.answer {
                let (_, onsets, evidence, start) = performed(score);
                assert!(score.timing_correct(&onsets, Some(start)), "{}", skill.id);
                assert!(score.holds_correct(&evidence), "{}", skill.id);
            }
        }
    }
}
#[test]
fn individual_tones_precede_melodies_and_minor_does_not_inherit_major_mastery() {
    let graph = curriculum();
    let get = |id: &str| graph.iter().find(|s| s.id == id).unwrap();
    let mut profile = Profile::default();
    for skill in &graph {
        if skill.id.contains("major") || skill.id == "melody.0.3" {
            profile.skills.insert(
                skill.id.clone(),
                learner::Mastery {
                    score: 10.0,
                    attempts: 20,
                    ..Default::default()
                },
            );
        }
    }
    assert!(!profile.unlocked(get("dictation.0.natural-minor.2")));
    let phrase = get("dictation.0.major.2");
    assert!(phrase.requires.iter().any(|r| r.skill == "tone.0.major.1"));
    assert!(phrase.requires.iter().any(|r| r.skill == "tone.0.major.5"));
    assert!(get("melody.0.3")
        .requires
        .iter()
        .any(|r| r.skill == phrase.id));
    assert!(get("melody.0.3")
        .requires
        .iter()
        .any(|r| r.skill == "tone.0.major.3"));
    assert!(!get("scale.0.natural-minor.up")
        .requires
        .iter()
        .any(|r| r.skill.contains("major")));
    assert!(get("scale.0.dorian.up")
        .requires
        .iter()
        .all(|r| !r.skill.starts_with("scale.1.")));
}
#[test]
fn early_phrases_use_only_introduced_tones_and_do_not_require_a_register() {
    for scale in Scale::ALL {
        for length in [2, 3] {
            let task = Practice::Melody {
                root: 0,
                scale,
                length,
                motif: Motif::Free,
                rhythmic: false,
            };
            for seed in 1..50 {
                let ex = Exercise::generate(
                    &Skill {
                        stage: 0,
                        id: "test".into(),
                        title: "test".into(),
                        task: Task::Practice(task.clone()),
                        requires: vec![],
                    },
                    seed,
                );
                let notes = ex.expected_evidence();
                let fifth = *scale
                    .steps()
                    .iter()
                    .find(|n| **n == 7)
                    .unwrap_or(&scale.steps()[2]);
                let third = *scale
                    .steps()
                    .iter()
                    .find(|n| **n == 3 || **n == 4)
                    .unwrap_or(&scale.steps()[1]);
                let allowed = if length == 2 {
                    vec![0, fifth]
                } else {
                    vec![0, third, fifth]
                };
                assert!(notes.iter().all(|n| allowed.contains(&(n % 12))));
                let shifted: Vec<_> = notes.iter().map(|n| n + 12).collect();
                assert!(ex.correct(&shifted));
                let mut wrong = shifted.clone();
                wrong[1] += 12;
                assert!(!ex.correct(&wrong));
            }
        }
    }
}
#[test]
fn scale_family_recognition_accepts_transposition_but_rejects_the_other_family() {
    let major = generated(Practice::Scale {
        root: 0,
        scale: Scale::Major,
        direction: Direction::Up,
        hear: true,
        bpm: None,
    });
    assert!(major.correct(&[62, 64, 66, 67, 69, 71, 73, 74]));
    assert!(!major.correct(&[62, 64, 65, 67, 69, 70, 72, 74]));
    let minor = generated(Practice::Scale {
        root: 0,
        scale: Scale::Minor,
        direction: Direction::Up,
        hear: true,
        bpm: None,
    });
    assert!(minor.correct(&[62, 64, 65, 67, 69, 70, 72, 74]));
    assert_eq!(major.prompt, minor.prompt);
    for n in 0..128 {
        assert_eq!(major.spelling.note_name(n), minor.spelling.note_name(n));
    }
}
#[test]
fn chord_frames_are_ordered_but_notes_within_a_chord_are_not() {
    let ex = generated(Practice::Harmony {
        root: 0,
        scale: Scale::Major,
        kind: Harmony::Progression(4),
    });
    let Answer::Performance(score) = &ex.answer else {
        panic!()
    };
    let (mut notes, mut onsets, _, start) = performed(score);
    for chunk in notes.chunks_mut(3) {
        chunk.reverse();
    }
    assert!(ex.correct(&notes));
    notes.rotate_left(3);
    assert!(!ex.correct(&notes));
    onsets[2] += Duration::from_millis(500);
    assert!(!score.timing_correct(&onsets, Some(start)));
}
#[test]
fn harmony_accepts_alternative_harmonizations_but_rejects_wrong_notes_and_large_leaps() {
    let ex = generated(Practice::Harmony {
        root: 0,
        scale: Scale::Major,
        kind: Harmony::Harmonize(1),
    });
    assert!(ex.correct(&[60, 64, 67])); // I contains the melody's C.
    assert!(ex.correct(&[57, 60, 64])); // vi is also valid.
    assert!(!ex.correct(&[62, 66, 69])); // Chromatic triad.
    assert!(!ex.correct(&[62, 65, 69])); // Diatonic, but does not contain C.
    let leading = generated(Practice::Harmony {
        root: 0,
        scale: Scale::Major,
        kind: Harmony::VoiceLeading,
    });
    let mut played = leading.expected_evidence();
    assert!(leading.correct(&played));
    for n in &mut played[3..6] {
        *n += 12;
    }
    assert!(!leading.correct(&played));
}
#[test]
fn rhythm_grades_onsets_and_releases_independently() {
    let ex = generated(Practice::Rhythm {
        kind: Rhythm::Ties,
        bpm: 60,
    });
    let Answer::Performance(score) = &ex.answer else {
        panic!()
    };
    let (notes, mut onsets, mut evidence, start) = performed(score);
    assert!(score.pitch_correct(&notes));
    onsets[1] += Duration::from_millis(400);
    assert!(!score.timing_correct(&onsets, Some(start)));
    assert!(score.holds_correct(&evidence));
    evidence
        .iter_mut()
        .find(|e| e.velocity == 0)
        .unwrap()
        .offset_ms = 100;
    assert!(!score.holds_correct(&evidence));
    assert!(score.pitch_correct(&notes));
}
#[test]
fn coordination_requires_holding_the_bass_while_the_melody_moves() {
    let ex = generated(Practice::Harmony {
        root: 0,
        scale: Scale::Major,
        kind: Harmony::Coordination(false),
    });
    let Answer::Performance(score) = &ex.answer else {
        panic!()
    };
    let (notes, onsets, mut evidence, start) = performed(score);
    assert!(score.pitch_correct(&notes));
    assert!(score.timing_correct(&onsets, Some(start)));
    assert!(score.holds_correct(&evidence));
    evidence
        .iter_mut()
        .find(|e| e.note == notes[0] && e.velocity == 0)
        .unwrap()
        .offset_ms = 300;
    assert!(!score.holds_correct(&evidence));
}
#[test]
fn fixed_phrases_do_not_accept_a_correct_phrase_started_late() {
    let ex = generated(Practice::Meter { beats: 3, bpm: 90 });
    let Answer::Performance(score) = &ex.answer else {
        panic!()
    };
    let (_, onsets, _, start) = performed(score);
    let late: Vec<_> = onsets
        .iter()
        .map(|t| *t + Duration::from_millis(600))
        .collect();
    assert!(!score.timing_correct(&late, Some(start)));
    assert!(!score.timing_correct(&onsets, None));
}
#[test]
fn accompaniment_collects_midi_during_backing_and_saves_only_after_the_phrase() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("learner.json")).unwrap();
    let skill = session
        .graph
        .iter()
        .find(|s| s.id == "harmony.0.major.accompany.12.false")
        .unwrap();
    session.exercise = Some(Exercise::generate(skill, 42));
    let ex = session.exercise.as_ref().unwrap();
    let start_ms = ex.answer_after_ms.unwrap();
    let end_ms = ex.fixed_duration_ms().unwrap();
    assert!(ex.playback.last().unwrap().0 > start_ms);
    let Answer::Performance(score) = &ex.answer else {
        panic!()
    };
    let (_, _, evidence, now) = performed(score);
    session.ready(now);
    session.tick(now + Duration::from_millis(start_ms)).unwrap();
    assert!(session.phase == Phase::Answering);
    for event in evidence {
        let at = now + Duration::from_millis(start_ms + event.offset_ms);
        session.tick(at).unwrap();
        session.input(
            Event::Note {
                channel: 0,
                note: event.note,
                velocity: event.velocity,
            },
            at,
        );
        assert_eq!(session.profile.completed, 0);
    }
    session
        .tick(now + Duration::from_millis(start_ms + end_ms))
        .unwrap();
    assert!(session.phase == Phase::Feedback);
    assert_eq!(session.profile.completed, 1);
    let attempt = session.profile.recent_attempts.last().unwrap();
    assert!(
        attempt.correct
            && attempt.pitch_correct
            && attempt.timing_correct
            && attempt.duration_correct
    );
    assert!(session
        .playback
        .iter()
        .all(|(_, e)| matches!(e, Event::ClearChannel(15))));
}
#[test]
fn fixed_phrase_timeout_and_restart_do_not_leave_old_backing_or_award_empty_answers() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("learner.json")).unwrap();
    let skill = session
        .graph
        .iter()
        .find(|s| s.id == "rhythm.5.60")
        .unwrap();
    session.exercise = Some(Exercise::generate(skill, 42));
    let now = Instant::now();
    session.ready(now);
    session.tick(now + Duration::from_secs(5)).unwrap();
    session.input(
        Event::Note {
            channel: 0,
            note: 60,
            velocity: 90,
        },
        now + Duration::from_secs(5),
    );
    session.replay(now + Duration::from_secs(6));
    assert!(session.played.is_empty());
    assert_eq!(session.profile.completed, 0);
    assert!(session
        .playback
        .iter()
        .all(|(at, _)| *at >= now + Duration::from_secs(6)));
    let deadline = session.finish_at.unwrap();
    session.tick(deadline).unwrap();
    assert_eq!(session.profile.completed, 1);
    assert!(!session.profile.recent_attempts.last().unwrap().correct);
}

#[test]
fn contextual_movements_preserve_direction_key_and_octave_freedom() {
    let graph = curriculum();
    for skill in &graph {
        let Task::Practice(Practice::Movement {
            direction,
            root,
            scale,
            ..
        }) = &skill.task
        else {
            continue;
        };
        for seed in 1..=16 {
            let ex = Exercise::generate(skill, seed);
            let notes = ex.expected_evidence();
            assert_eq!(notes.len(), 2);
            assert_eq!(
                notes[0] < notes[1],
                *direction == Direction::Up,
                "{}",
                skill.id
            );
            assert_ne!(notes[0], notes[1]);
            assert!(notes
                .iter()
                .all(|n| scale.steps().contains(&((n + 12 - root) % 12))));
            assert!(ex.correct(&notes.iter().map(|n| n + 12).collect::<Vec<_>>()));
            assert!(!ex.correct(&[notes[1], notes[0]]));
            assert!(!ex.correct(&[notes[0], notes[1] + 12]));
        }
    }
}

#[test]
fn longer_listening_requires_both_directions_and_bass_before_progressions() {
    let graph = curriculum();
    for skill in &graph {
        match &skill.task {
            Task::Practice(Practice::Melody {
                root,
                scale,
                length: 3 | 5,
                ..
            }) => {
                let Task::Practice(Practice::Melody { length, .. }) = &skill.task else {
                    unreachable!()
                };
                for direction in ["up", "down"] {
                    let id = format!(
                        "movement.{root}.{}.{}.{direction}",
                        scale.id(),
                        if *length == 3 { "triad" } else { "scale" }
                    );
                    // Rhythmic and motif variants inherit these requirements through the base melody.
                    let base = if scale == &Scale::Major {
                        format!("melody.{root}.{length}")
                    } else {
                        format!("dictation.{root}.{}.{length}", scale.id())
                    };
                    let base = graph.iter().find(|s| s.id == base).unwrap();
                    assert!(
                        base.requires.iter().any(|r| r.skill == id),
                        "{} missing {id}",
                        base.id
                    );
                }
            }
            Task::Practice(Practice::Harmony {
                root,
                scale,
                kind: Harmony::Progression(2),
            }) => {
                assert!(skill
                    .requires
                    .iter()
                    .any(|r| r.skill == format!("harmony.{root}.{}.bass.false", scale.id())));
                for degree in [0, 3, 4, 5] {
                    assert!(skill
                        .requires
                        .iter()
                        .any(|r| r.skill
                            == format!("harmony.{root}.{}.function.{degree}", scale.id())));
                }
            }
            _ => {}
        }
    }
}

#[test]
fn early_two_note_phrases_use_both_directions_and_progressions_vary() {
    let graph = curriculum();
    for id in ["dictation.0.major.2", "dictation.0.natural-minor.2"] {
        let skill = graph.iter().find(|s| s.id == id).unwrap();
        let mut directions = std::collections::BTreeSet::new();
        for seed in 1..=64 {
            let notes = Exercise::generate(skill, seed).expected_evidence();
            directions.insert(notes[0] < notes[1]);
        }
        assert_eq!(directions.len(), 2);
    }
    for scale in [Scale::Major, Scale::Minor] {
        for suffix in ["progression.4", "bass.false", "bass.true"] {
            let id = format!("harmony.0.{}.{suffix}", scale.id());
            let skill = graph.iter().find(|s| s.id == id).unwrap();
            let examples: std::collections::BTreeSet<_> = (1..=64)
                .map(|seed| Exercise::generate(skill, seed).expected_evidence())
                .collect();
            assert!(examples.len() >= 4, "{id} repeats a fixed phrase");
        }
    }
}

#[test]
fn cadences_and_bass_lines_do_not_wait_for_longer_phrase_exercises() {
    let graph = curriculum();
    for skill in &graph {
        match &skill.task {
            Task::Practice(Practice::Harmony {
                root,
                scale,
                kind: Harmony::Cadence(_),
            }) => {
                assert_eq!(skill.requires.len(), 2);
                for degree in [0, 4] {
                    assert!(skill
                        .requires
                        .iter()
                        .any(|r| r.skill
                            == format!("harmony.{root}.{}.function.{degree}", scale.id())));
                }
            }
            Task::Practice(Practice::Harmony {
                root,
                scale,
                kind: Harmony::Bass(false),
            }) => {
                assert!(!skill
                    .requires
                    .iter()
                    .any(|r| r.skill == format!("melody.{root}.5")
                        || r.skill == format!("dictation.{root}.{}.5", scale.id())));
                for direction in ["up", "down"] {
                    assert!(skill
                        .requires
                        .iter()
                        .any(|r| r.skill
                            == format!("movement.{root}.{}.scale.{direction}", scale.id())));
                }
            }
            _ => {}
        }
    }
}
