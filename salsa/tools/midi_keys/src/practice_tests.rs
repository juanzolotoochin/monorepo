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
    session.start(now);
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
    session.start(now);
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

#[test]
fn quarter_notes_follow_continuous_drums_without_a_listening_phase() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("learner.json")).unwrap();
    let skill = session
        .graph
        .iter()
        .find(|s| s.id == "rhythm.0.60")
        .unwrap();
    session.exercise = Some(Exercise::generate(skill, 42));
    let start = Instant::now();
    session.start(start);
    assert!(session.phase == Phase::Answering);
    assert!(session.exercise.as_ref().unwrap().playback.is_empty());
    for beat in 0..3 {
        let events = session.tick(start + Duration::from_secs(beat)).unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::DrumBeat { .. })));
        assert_eq!(session.profile.completed, 0);
    }
    // Join on any later beat, not at a prescribed end of a demo/count-in.
    for beat in 3..11 {
        let at = start + Duration::from_secs(beat);
        assert!(session
            .tick(at)
            .unwrap()
            .iter()
            .any(|e| matches!(e, Event::DrumBeat { .. })));
        session.input(
            Event::Note {
                channel: 0,
                note: 60,
                velocity: 90,
            },
            at,
        );
        session.input(
            Event::Note {
                channel: 0,
                note: 60,
                velocity: 0,
            },
            at + Duration::from_millis(940),
        );
    }
    session.tick(start + Duration::from_millis(11600)).unwrap();
    assert!(session.phase == Phase::Feedback);
    assert!(session.profile.recent_attempts[0].correct);
    assert!(session
        .tick(start + Duration::from_secs(12))
        .unwrap()
        .iter()
        .all(|e| !matches!(e, Event::DrumBeat { .. })));
}

#[test]
fn pulse_checks_alignment_to_backing_and_quarter_note_lengths() {
    let ex = generated(Practice::Rhythm {
        kind: Rhythm::Pulse,
        bpm: 60,
    });
    let Answer::Performance(ref score) = ex.answer else {
        panic!()
    };
    let (_, onsets, evidence, start) = performed(score);
    assert!(ex.performance_timing_correct(&onsets, Some(start)));
    let delayed: Vec<_> = onsets
        .iter()
        .map(|at| *at + Duration::from_secs(3))
        .collect();
    assert!(ex.performance_timing_correct(&delayed, Some(start)));
    let offbeat: Vec<_> = onsets
        .iter()
        .map(|at| *at + Duration::from_millis(500))
        .collect();
    assert!(!ex.performance_timing_correct(&offbeat, Some(start)));
    assert!(ex.holds_correct(&evidence));
    let mut short = evidence.clone();
    for (i, event) in short.iter_mut().filter(|e| e.velocity == 0).enumerate() {
        event.offset_ms = i as u64 * 1000 + 100;
    }
    assert!(!ex.holds_correct(&short));
}

#[test]
fn rhythm_report_uses_grading_anchor_and_labels_missing_extra_and_short_notes() {
    let ex = generated(Practice::Rhythm {
        kind: Rhythm::Pulse,
        bpm: 60,
    });
    let Answer::Performance(ref score) = ex.answer else {
        panic!()
    };
    let (_, onsets, mut evidence, start) = performed(score);
    let delayed: Vec<_> = onsets
        .iter()
        .map(|at| *at + Duration::from_millis(3100))
        .collect();
    let report = ex.rhythm_report(&delayed, Some(start), &evidence).unwrap();
    assert!(report.rows.iter().all(|r| r.correct(score.tolerance_ms)));
    assert!(report.rows.iter().all(|r| r.timing_label() == "100ms late"));
    let early: Vec<_> = onsets
        .iter()
        .map(|at| *at + Duration::from_millis(2900))
        .collect();
    assert_eq!(
        ex.rhythm_report(&early, Some(start), &evidence)
            .unwrap()
            .rows[0]
            .timing_label(),
        "100ms early"
    );
    evidence[1].offset_ms = 100;
    assert!(!ex
        .rhythm_report(&onsets, Some(start), &evidence)
        .unwrap()
        .rows[0]
        .correct(score.tolerance_ms));
    let mut missing = onsets.clone();
    missing.remove(3);
    evidence.drain(6..8);
    let report = ex.rhythm_report(&missing, Some(start), &evidence).unwrap();
    assert_eq!(report.rows[3].timing_label(), "MISSING");
    assert!(report.rows[4..]
        .iter()
        .all(|r| r.timing_label() == "on time"));
    let mut extra = missing.clone();
    extra.push(start + Duration::from_millis(8500));
    evidence.push(crate::learner::MidiEvidence {
        offset_ms: 8500,
        channel: 0,
        note: 61,
        velocity: 90,
    });
    let report = ex.rhythm_report(&extra, Some(start), &evidence).unwrap();
    assert_eq!(report.rows.last().unwrap().timing_label(), "EXTRA");
    assert_eq!(report.rows.last().unwrap().actual_hold_ms, None);
}

#[test]
fn rhythm_comparison_can_pause_auto_advance_and_does_not_leak_into_next_attempt() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("profile.json")).unwrap();
    let skill = session
        .graph
        .iter()
        .find(|s| s.id == "rhythm.0.60")
        .unwrap();
    session.exercise = Some(Exercise::generate(skill, 42));
    let start = Instant::now();
    session.start(start);
    session.input(
        Event::Note {
            channel: 0,
            note: 60,
            velocity: 90,
        },
        start,
    );
    session.input(
        Event::Note {
            channel: 0,
            note: 60,
            velocity: 0,
        },
        start + Duration::from_millis(100),
    );
    session.tick(start + Duration::from_secs(9)).unwrap();
    assert!(session.view().rhythm_report.is_some());
    assert!(session.advance_at.is_some());
    session.pause_comparison();
    session.tick(start + Duration::from_secs(60)).unwrap();
    assert!(session.phase == Phase::Feedback);
    assert!(session.view().comparison_paused);
    session.advance();
    assert!(session.view().rhythm_report.is_none());
    assert!(!session.view().comparison_paused);
}

#[test]
fn rhythm_accepts_the_reported_scale_with_overlapping_releases() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("profile.json")).unwrap();
    let skill = session
        .graph
        .iter()
        .find(|s| s.id == "rhythm.0.60")
        .unwrap();
    session.exercise = Some(Exercise::generate(skill, 42));
    let start = Instant::now();
    session.start(start);
    let notes = [60, 62, 64, 65, 67, 69, 71, 72];
    let attack_errors = [-65i64, -12, 28, -21, -15, -16, -38, -36];
    let holds = [1070, 1067, 941, 1024, 1046, 1005, 1048, 969];
    let mut events = Vec::new();
    for i in 0..8 {
        let attack = (3000 + i as i64 * 1000 + attack_errors[i]) as u64;
        events.push((
            attack,
            Event::Note {
                channel: 0,
                note: notes[i],
                velocity: 90,
            },
        ));
        events.push((
            attack + holds[i],
            Event::Note {
                channel: 0,
                note: notes[i],
                velocity: 0,
            },
        ));
    }
    events.sort_by_key(|(ms, _)| *ms);
    for (ms, event) in events {
        let at = start + Duration::from_millis(ms);
        session.tick(at).unwrap();
        session.input(event, at);
    }
    session.tick(start + Duration::from_secs(12)).unwrap();
    assert!(session.phase == Phase::Feedback);
    assert_eq!(session.feedback, "Correct!");
    let result = session.profile.recent_attempts.last().unwrap();
    assert!(
        result.correct && result.pitch_correct && result.timing_correct && result.duration_correct
    );
    assert_eq!(result.answer, notes);
    assert!(session
        .view()
        .rhythm_report
        .unwrap()
        .rows
        .iter()
        .all(|r| r.correct(166)));
}

#[test]
fn all_rhythm_only_exercises_ignore_pitch_but_still_require_the_exact_note_count() {
    let graph = curriculum();
    let mut checked = 0;
    for skill in &graph {
        let ex = Exercise::generate(skill, 42);
        let Answer::Performance(score) = &ex.answer else {
            continue;
        };
        if !score.any_pitch {
            continue;
        }
        let notes: Vec<_> = (0..score.note_count()).map(|i| 60 + i % 12).collect();
        assert!(ex.correct(&notes), "{}", skill.id);
        assert!(!ex.correct(&notes[..notes.len() - 1]), "{}", skill.id);
        let mut extra = notes;
        extra.push(60);
        assert!(!ex.correct(&extra), "{}", skill.id);
        checked += 1;
    }
    assert!(checked > 0);
}

#[test]
fn accompaniment_keeps_all_bars_visible_and_exposes_its_id() {
    let mut session = Session::preview("harmony.7.major.accompany.12.true").unwrap();
    let now = Instant::now();
    session.start(now);
    session.tick(now + Duration::from_secs(4)).unwrap();
    session.tick(now + Duration::from_secs(25)).unwrap();
    let view = session.view();
    assert_eq!(view.skill_id, "harmony.7.major.accompany.12.true");
    assert_eq!(view.score_page, 0);
    assert_eq!(view.score_pages, 1);
    assert_eq!(view.display_score.unwrap().chords.len(), 12);
    assert_eq!(view.current_bar, Some(5));
}
