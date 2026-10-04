use super::*;
use curriculum::{Direction, Task};
use exercise::Answer;
use learner::{Attempt, Mastery};

fn skill(task: Task) -> Skill {
    Skill {
        stage: 0,
        id: "test".into(),
        title: "test".into(),
        task,
        requires: vec![],
    }
}
fn result(id: &str, correct: bool, turn: u64) -> Attempt {
    Attempt {
        title: String::new(),
        prompt: String::new(),
        spelled_answer: vec![],
        number: 0,
        skill: id.into(),
        variant: format!("{turn}"),
        correct,
        assisted: false,
        at: 1000 + turn,
        answer: vec![],
        expected: vec![],
        evidence: vec![],
        bpm: None,
        pitch_correct: correct,
        timing_correct: true,
        duration_correct: true,
    }
}
fn established() -> Mastery {
    Mastery {
        introduced_at: 0,
        first_qualified_at: Some(0),
        score: 9.5,
        attempts: 20,
        correct: 20,
        streak: 20,
        variants: ["a".into(), "b".into(), "c".into()].into(),
        last_practiced: 1000,
        last_turn: 0,
        review_requested: false,
    }
}

#[test]
fn curriculum_is_a_valid_connected_dag_with_specific_skills() {
    let graph = curriculum();
    curriculum::validate(&graph).unwrap();
    assert_eq!(graph.len(), 6645);
    let profile = Profile::default();
    let (first, _) = profile.next(&graph, 1000).unwrap();
    assert_eq!(first.id, "interval.build.2.up");
    let mut profile = Profile::default();
    for _ in 0..graph.len() {
        let before = profile.skills.len();
        for skill in &graph {
            if profile.unlocked(skill) {
                profile.skills.insert(skill.id.clone(), established());
            }
        }
        if profile.skills.len() == before {
            break;
        }
    }
    assert_eq!(
        profile.skills.len(),
        graph.len(),
        "Every skill is reachable"
    );
}

#[test]
fn all_foundations_and_core_listening_finish_before_advanced_branches_dominate() {
    let graph = curriculum();
    let mut profile = Profile::default();
    let milestones = [
        "melody.0.5",
        "dictation.0.natural-minor.5",
        "harmony.0.major.progression.4",
        "harmony.0.natural-minor.progression.4",
    ];
    // This covers the whole foundation rather than optimizing only four goals.
    // There are 160 foundational skills, each requiring at least 12 answers.
    for turn in 0..4000 {
        let skill = profile.next(&graph, 1000).unwrap().0;
        let mut attempt = result(&skill.id, true, turn);
        attempt.at = 1000;
        profile.record(skill, attempt);
        if milestones
            .iter()
            .all(|id| profile.skills.get(*id).is_some_and(|s| s.mastered()))
            && graph
                .iter()
                .filter(|s| s.stage <= 1)
                .all(|s| profile.skills.get(&s.id).is_some_and(|s| s.mastered()))
        {
            return;
        }
    }
    panic!("Core listening milestones should be reached before unrelated advanced branches dominate practice");
}

#[test]
fn graph_validation_rejects_cycles_and_missing_prerequisites() {
    let mut graph = curriculum();
    let id = graph[0].id.clone();
    graph[0].requires.push(curriculum::Requirement {
        skill: id,
        score: 7.0,
        attempts: 6,
    });
    assert!(curriculum::validate(&graph).is_err());
    graph[0].requires[0].skill = "missing".into();
    assert!(curriculum::validate(&graph).is_err());
}

#[test]
fn six_independent_successes_unlock_and_established_skills_retire_until_due() {
    let graph = curriculum();
    let first = &graph[0];
    let mut profile = Profile::default();
    let child = graph.iter().find(|s| s.id == "interval.hear.2.up").unwrap();
    for turn in 0..5 {
        profile.record(first, result(&first.id, true, turn));
    }
    assert!(!profile.unlocked(child));
    profile.record(first, result(&first.id, true, 5));
    assert!(profile.unlocked(child));
    for turn in 6..12 {
        profile.record(first, result(&first.id, true, turn));
    }
    assert!(profile.skills[&first.id].mastered());
    let only = [first.clone()];
    assert!(profile.next(&only, 1012).is_none());
    assert_eq!(
        profile.next(&only, 1011 + 14 * 86400).unwrap().1,
        "Retention check"
    );
}

#[test]
fn related_mistakes_request_review_without_falsely_scoring_prerequisites() {
    let graph = curriculum();
    let child = graph.iter().find(|s| s.id == "interval.hear.2.up").unwrap();
    let mut profile = Profile::default();
    profile
        .skills
        .insert("interval.build.2.up".into(), established());
    profile.record(child, result(&child.id, false, 0));
    assert!(profile.skills["interval.build.2.up"].review_requested);
    assert_eq!(profile.skills["interval.build.2.up"].score, 9.5);
    assert_eq!(
        profile.next(&graph, 1000).unwrap().0.id,
        "interval.build.2.up"
    );
}

#[test]
fn hints_do_not_unlock_skills_and_scheduling_is_fair_to_existing_skills() {
    let graph = curriculum();
    let mut profile = Profile::default();
    for turn in 0..12 {
        let mut attempt = result(&graph[0].id, true, turn);
        attempt.assisted = true;
        profile.record(&graph[0], attempt);
    }
    assert_eq!(profile.skills[&graph[0].id].score, 0.0);
    assert_eq!(profile.skills[&graph[0].id].correct, 0);
    assert!(!profile.unlocked(&graph[1]));
    // Broadly run the automatic course: no introduced weak skill should be
    // permanently starved by the supply of new unlocked skills.
    for turn in 12..400 {
        let next = profile.next(&graph, 1000).unwrap().0;
        profile.record(next, result(&next.id, true, turn));
    }
    assert!(profile.skills["interval.build.2.up"].mastered());
    assert!(profile.skills.len() > 10);
}

#[test]
fn recognition_accepts_transposition_and_inversions_without_leaking_the_answer() {
    let interval = Exercise::generate(
        &skill(Task::HearInterval {
            semitones: 4,
            direction: Direction::Down,
        }),
        42,
    );
    assert!(interval.correct(&[55, 59]));
    assert!(interval.correct(&[72, 68]));
    assert!(!interval.correct(&[60, 63]));
    assert!(!interval.correct(&[60, 76]));
    assert!(!interval.prompt.contains("third"));
    assert!(!interval.title.contains("third"));
    let chord = Exercise::generate(
        &skill(Task::HearChord {
            root: 0,
            quality: 0,
        }),
        42,
    );
    assert!(chord.correct(&[62, 66, 69]));
    assert!(chord.correct(&[66, 69, 74, 78]));
    assert!(!chord.correct(&[62, 65, 69]));
    assert!(!chord.correct(&[62, 66, 69, 73]));
    assert!(!chord.prompt.contains("major"));
}

#[test]
fn interval_construction_accepts_any_octave_but_requires_root_direction_and_distance() {
    for direction in [Direction::Up, Direction::Down] {
        for semitones in 1..=12 {
            let exercise = Exercise::generate(
                &skill(Task::BuildInterval {
                    semitones,
                    direction,
                }),
                42,
            );
            let Answer::AnchoredInterval { root, .. } = exercise.answer else {
                panic!()
            };
            for start in [36 + root, 60 + root, 84 + root] {
                let end = if direction == Direction::Down {
                    start - semitones
                } else {
                    start + semitones
                };
                assert!(exercise.correct(&[start, end]));
                assert!(!exercise.correct(&[start + 1, end + 1]));
                assert!(!exercise.correct(&[start, end + 12]));
                assert!(!exercise.correct(&[end, start]));
                assert!(!exercise.correct(&[start]));
                assert!(!exercise.correct(&[start, end, end]));
            }
            assert!(exercise.correct(&exercise.expected_evidence()));
            assert_eq!(exercise.minimum_notes(), 2);
            assert!(exercise.prompt.contains("Any starting octave"));
            assert_eq!(exercise.prompt_highlights.len(), 3);
            let note = &exercise.prompt[exercise.prompt_highlights[0].range.clone()];
            assert!(!note.chars().any(|c| c.is_ascii_digit()));
        }
    }
}

#[test]
fn written_intervals_preserve_note_letters_and_octave_boundaries() {
    for (root, semitones, direction, played, expected) in [
        (8, 2, Direction::Up, [68, 70], ["Ab4", "Bb4"]),
        (0, 2, Direction::Down, [60, 58], ["C4", "Bb3"]),
        (1, 2, Direction::Down, [61, 59], ["Db4", "Cb4"]),
        (6, 6, Direction::Up, [54, 60], ["F#3", "B#3"]),
        (8, 1, Direction::Down, [68, 67], ["Ab4", "G4"]),
    ] {
        let names = NoteSpelling::for_task(
            &Task::BuildInterval {
                semitones,
                direction,
            },
            root,
        );
        assert_eq!(played.map(|n| names.note_name(n)), expected);
    }
}

#[test]
fn scales_and_chords_use_their_written_key_but_listening_answers_do_not_leak() {
    let names = NoteSpelling::for_task(
        &Task::Scale {
            root: 6,
            minor: false,
            direction: Direction::Up,
        },
        0,
    );
    assert_eq!(
        [66, 68, 70, 71, 73, 75, 77, 78].map(|n| names.note_name(n)),
        ["F#4", "G#4", "A#4", "B4", "C#5", "D#5", "E#5", "F#5"]
    );
    let names = NoteSpelling::for_task(
        &Task::BuildChord {
            root: 0,
            quality: 8,
        },
        0,
    );
    assert_eq!(
        [60, 63, 66, 69].map(|n| names.note_name(n)),
        ["C4", "Eb4", "Gb4", "Bbb4"]
    );
    let first = NoteSpelling::for_task(
        &Task::HearChord {
            root: 8,
            quality: 0,
        },
        0,
    );
    let second = NoteSpelling::for_task(
        &Task::HearChord {
            root: 6,
            quality: 1,
        },
        0,
    );
    for note in 0..128 {
        assert_eq!(first.note_name(note), second.note_name(note));
    }
}

#[test]
fn feedback_advances_automatically_once_with_time_to_read_mistakes_and_hints() {
    for (correct, assisted, seconds) in [(true, false, 2), (false, false, 4), (true, true, 4)] {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::open(dir.path().join("learner.json")).unwrap();
        let now = Instant::now();
        session.ready(now);
        session.played = session.exercise.as_ref().unwrap().expected_evidence();
        if assisted {
            session.hint();
        }
        session.finish(!correct, now).unwrap();
        session
            .tick(now + Duration::from_millis(seconds * 1000 - 1))
            .unwrap();
        assert!(session.phase == Phase::Feedback);
        let events = session.tick(now + Duration::from_secs(seconds)).unwrap();
        assert!(events.iter().any(|e| matches!(e, Event::Reset)));
        assert!(session.phase == Phase::Waiting);
        assert!(session.played.is_empty());
        assert!(session.advance_at.is_none());
        assert_eq!(session.profile.completed, 1);
        session.tick(now + Duration::from_secs(20)).unwrap();
        assert!(session.phase == Phase::Waiting);
        assert_eq!(session.profile.completed, 1);
    }
}

#[test]
fn replay_restarts_audio_and_clears_draft_without_scoring_or_erasing_hint_usage() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("learner.json")).unwrap();
    let hearing = session
        .graph
        .iter()
        .find(|s| s.id == "interval.hear.2.up")
        .unwrap();
    session.exercise = Some(Exercise::generate(hearing, 42));
    let now = Instant::now();
    session.ready(now);
    session.tick(now).unwrap();
    let restart = now + Duration::from_millis(200);
    session.replay(restart);
    let events = session.tick(restart).unwrap();
    assert!(matches!(events.first(), Some(Event::ClearChannel(15))));
    assert!(matches!(
        events.get(1),
        Some(Event::Note { velocity: 90, .. })
    ));
    // The old timeline's first note-off (400 ms) must have been discarded.
    assert!(session
        .tick(now + Duration::from_millis(400))
        .unwrap()
        .is_empty());
    session.tick(now + Duration::from_secs(2)).unwrap();
    assert!(session.phase == Phase::Answering);
    session.hint();
    session.input(
        Event::Note {
            channel: 0,
            note: 60,
            velocity: 90,
        },
        now + Duration::from_secs(2),
    );
    session.replay(now + Duration::from_secs(3));
    assert!(session.phase == Phase::Listening);
    assert!(session.played.is_empty() && session.evidence.is_empty() && session.down.is_empty());
    assert!(session.assisted);
    assert_eq!(session.profile.completed, 0);
}

#[test]
fn construction_context_and_dictation_have_distinct_grading_contracts() {
    let build = Exercise::generate(
        &skill(Task::BuildChord {
            root: 0,
            quality: 0,
        }),
        42,
    );
    assert!(build.correct(&[64, 67, 72]));
    assert!(!build.correct(&[62, 66, 69]));
    let scale = Exercise::generate(
        &skill(Task::Scale {
            root: 0,
            minor: false,
            direction: Direction::Both,
        }),
        42,
    );
    let Answer::OctaveSequence(notes) = &scale.answer else {
        panic!()
    };
    assert_eq!(notes.len(), 15);
    assert_eq!(notes[0], notes[14]);
    assert!(scale.correct(notes));
    assert!(!scale.correct(&notes[..14]));
    let context = Exercise::generate(
        &skill(Task::ContextInterval {
            root: 0,
            first: 0,
            second: 2,
        }),
        42,
    );
    assert!(context.correct(&[48, 64]));
    assert!(!context.correct(&[62, 66]));
    let melody = Exercise::generate(
        &skill(Task::Practice(practice::Practice::Melody {
            root: 0,
            scale: music::Scale::Major,
            length: 5,
            motif: practice::Motif::Free,
            rhythmic: false,
        })),
        42,
    );
    let Answer::OctaveSequence(notes) = &melody.answer else {
        panic!()
    };
    assert!(melody.correct(notes));
    let mut wrong = notes.clone();
    wrong[2] += 1;
    assert!(!melody.correct(&wrong));
}

#[test]
fn all_generated_audio_stays_in_range_and_releases_every_note() {
    for skill in curriculum() {
        for seed in 1..8 {
            let exercise = Exercise::generate(&skill, seed);
            let mut active = BTreeSet::new();
            let mut previous = 0;
            for (at, event) in exercise.playback {
                assert!(at >= previous);
                previous = at;
                if let Event::Note {
                    channel,
                    note,
                    velocity,
                } = event
                {
                    assert!(note < 128);
                    assert_eq!(channel, 15);
                    if velocity > 0 {
                        assert!(
                            active.insert(note),
                            "Overlapping playback of note {note} in {}",
                            skill.id
                        );
                    } else {
                        active.remove(&note);
                    }
                }
            }
            assert!(active.is_empty());
        }
    }
}

#[test]
fn profile_roundtrip_locking_and_corruption_preserve_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("learner.json");
    let (store, mut profile) = Store::open(path.clone()).unwrap();
    assert!(Store::open(path.clone()).is_err());
    let graph = curriculum();
    profile.record(&graph[0], result(&graph[0].id, true, 1));
    store.save(&profile).unwrap();
    drop(store);
    let (store, loaded) = Store::open(path.clone()).unwrap();
    assert_eq!(loaded.completed, 1);
    drop(store);
    std::fs::write(&path, "broken JSON").unwrap();
    assert!(Store::open(path.clone()).is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "broken JSON");
}

#[test]
fn unsupported_profile_version_is_not_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("learner.json");
    let mut profile = Profile::default();
    profile.version = 99;
    let bytes = serde_json::to_vec(&profile).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    assert!(Store::open(path.clone()).is_err());
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}

#[test]
fn profile_with_lock_extension_has_a_distinct_lock_across_atomic_saves() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("learner.lock");
    let (store, profile) = Store::open(path.clone()).unwrap();
    store.save(&profile).unwrap();
    assert!(Store::open(path.clone()).is_err());
    drop(store);
    assert!(Store::open(path).is_ok());
}

#[test]
fn complete_answer_submits_automatically_but_partial_answers_can_pause() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("learner.json");
    let mut session = Session::open(path.clone()).unwrap();
    let now = Instant::now();
    session.ready(now);
    let notes = session.exercise.as_ref().unwrap().expected_evidence();
    for (index, note) in notes.into_iter().enumerate() {
        let at = now + Duration::from_secs(index as u64 * 31);
        session.input(
            Event::Note {
                channel: 0,
                note,
                velocity: 90,
            },
            at,
        );
        session.input(
            Event::Note {
                channel: 0,
                note,
                velocity: 0,
            },
            at,
        );
        if index == 0 {
            session.tick(at + Duration::from_secs(30)).unwrap();
            assert!(session.phase == Phase::Answering);
            assert_eq!(session.profile.completed, 0);
        }
    }
    session.tick(now + Duration::from_secs(32)).unwrap();
    assert!(session.phase == Phase::Feedback);
    assert_eq!(session.profile.completed, 1);
    session.submit(false).unwrap();
    assert_eq!(session.profile.completed, 1);
    let view = session.view();
    assert_eq!(view.recent_exercises.len(), 1);
    assert!(view.recent_exercises[0].correct);
    assert_eq!(view.recent_exercises[0].prompt, view.prompt);
    let saved_answer = view.recent_exercises[0].entered.clone();
    let saved_prompt = view.prompt.to_string();
    drop(session);
    let session = Session::open(path).unwrap();
    assert_eq!(session.profile.completed, 1);
    let view = session.view();
    assert_eq!(view.recent_exercises[0].entered, saved_answer);
    assert_eq!(view.recent_exercises[0].prompt, saved_prompt);
    assert_eq!(view.recent_exercises[0].number, 1);
}

#[test]
fn failed_save_does_not_advance_or_claim_credit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("learner.json");
    let mut session = Session::open(path.clone()).unwrap();
    session.ready(Instant::now());
    std::fs::create_dir(&path).unwrap(); // Persist cannot overwrite a directory.
    assert!(session.submit(true).is_err());
    assert_eq!(session.profile.completed, 0);
    assert!(session.phase == Phase::Answering);
    assert!(session.advance_at.is_none());
    assert!(session.view().recent_exercises.is_empty());
}

#[test]
fn hidden_training_silences_backing_ignores_inputs_and_restarts_without_scoring() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("learner.json")).unwrap();
    let skill = session
        .graph
        .iter()
        .find(|s| s.id == "harmony.0.major.accompany.4.false")
        .unwrap();
    session.exercise = Some(Exercise::generate(skill, 42));
    let now = Instant::now();
    session.ready(now);
    session.tick(now + Duration::from_secs(5)).unwrap();
    assert!(session.phase == Phase::Answering);
    session.hint();
    assert_eq!(
        session.set_visible(false, now + Duration::from_secs(5)),
        Some(Event::ClearChannel(15))
    );
    let later = now + Duration::from_secs(600);
    session.input(
        Event::Note {
            channel: 0,
            note: 60,
            velocity: 90,
        },
        later,
    );
    session.ready(later);
    session.replay(later);
    session.submit(true).unwrap();
    assert!(session.tick(later).unwrap().is_empty());
    assert!(session.played.is_empty());
    assert_eq!(session.profile.completed, 0);
    session.set_visible(true, later);
    session.ready(later);
    assert!(session.phase == Phase::Listening);
    assert!(session.assisted);
    assert_eq!(
        session.answer_started_at,
        Some(later + Duration::from_secs(4))
    );
}

#[test]
fn resizing_during_feedback_preserves_the_result_and_its_remaining_delay() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("learner.json")).unwrap();
    let now = Instant::now();
    session.ready(now);
    session.finish(true, now).unwrap();
    session.set_visible(false, now + Duration::from_secs(1));
    let later = now + Duration::from_secs(600);
    session.tick(later).unwrap();
    assert!(session.phase == Phase::Feedback);
    assert_eq!(session.profile.completed, 1);
    session.set_visible(true, later);
    session.tick(later + Duration::from_secs(2)).unwrap();
    assert!(session.phase == Phase::Feedback);
    session.tick(later + Duration::from_secs(3)).unwrap();
    assert!(session.phase == Phase::Waiting);
}

#[test]
fn older_profiles_load_without_log_presentation_fields() {
    let mut profile = Profile::default();
    profile.record(&curriculum()[0], result("interval.build.2.up", true, 1));
    let mut json = serde_json::to_value(profile).unwrap();
    let attempt = json["recent_attempts"][0].as_object_mut().unwrap();
    for field in ["title", "prompt", "spelled_answer"] {
        attempt.remove(field);
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("learner.json");
    std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    let session = Session::open(path).unwrap();
    let view = session.view();
    assert_eq!(view.recent_exercises.len(), 1);
    assert_eq!(
        view.recent_exercises[0].prompt,
        "Prompt unavailable (older result)"
    );
}

#[test]
fn inversions_require_the_correct_bass_and_extended_chords_require_all_tones() {
    let first = Exercise::generate(
        &skill(Task::Inversion {
            root: 0,
            quality: 0,
            inversion: 1,
            hearing: false,
        }),
        42,
    );
    assert!(first.correct(&[64, 67, 72]));
    assert!(!first.correct(&[60, 64, 67]));
    assert!(!first.correct(&[66, 69, 74]));
    let hearing = Exercise::generate(
        &skill(Task::Inversion {
            root: 0,
            quality: 0,
            inversion: 1,
            hearing: true,
        }),
        42,
    );
    assert!(hearing.correct(&[66, 69, 74]));
    assert!(!hearing.correct(&[62, 66, 69]));
    let ninth = Exercise::generate(
        &skill(Task::HearChord {
            root: 0,
            quality: 10,
        }),
        42,
    );
    assert!(ninth.correct(&[60, 64, 67, 70, 74]));
    assert!(ninth.correct(&[62, 66, 69, 72, 76]));
    assert!(!ninth.correct(&[60, 64, 67, 70]));
    assert_eq!(ninth.minimum_notes(), 5);
}

#[test]
fn tempo_grades_pitch_and_rhythm_independently_and_unlocks_in_stages() {
    let exercise = Exercise::generate(
        &skill(Task::TimedScale {
            root: 0,
            minor: false,
            direction: Direction::Up,
            bpm: 120,
        }),
        42,
    );
    let start = Instant::now();
    let steady: Vec<_> = (0..8)
        .map(|i| start + Duration::from_millis(i * 500))
        .collect();
    assert!(exercise.timing_correct(&steady));
    let slow: Vec<_> = (0..8)
        .map(|i| start + Duration::from_millis(i * 1000))
        .collect();
    assert!(!exercise.timing_correct(&slow));
    let mut uneven = steady.clone();
    uneven[4] += Duration::from_millis(200);
    assert!(!exercise.timing_correct(&uneven));
    let graph = curriculum();
    let mut profile = Profile::default();
    profile
        .skills
        .insert("scale.0.major.up".into(), established());
    assert!(profile.unlocked(
        graph
            .iter()
            .find(|s| s.id == "scale.0.major.up.bpm60")
            .unwrap()
    ));
    assert!(!profile.unlocked(
        graph
            .iter()
            .find(|s| s.id == "scale.0.major.up.bpm120")
            .unwrap()
    ));
}

#[test]
fn prompts_ignore_midi_answers_and_tempo_results_preserve_timestamps() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("learner.json")).unwrap();
    let skill = session
        .graph
        .iter()
        .find(|s| s.id == "scale.0.major.up.bpm120")
        .unwrap();
    session.exercise = Some(Exercise::generate(skill, 42));
    let start = Instant::now();
    session.ready(start);
    assert!(session.phase == Phase::Listening);
    session.input(
        Event::Note {
            channel: 0,
            note: 60,
            velocity: 90,
        },
        start,
    );
    assert!(session.played.is_empty());
    let notes = session.exercise.as_ref().unwrap().expected_evidence();
    for (i, note) in notes.into_iter().enumerate() {
        let at = start + Duration::from_millis(2000 + i as u64 * 500);
        session.tick(at).unwrap();
        session.input(
            Event::Note {
                channel: 0,
                note,
                velocity: 90,
            },
            at,
        );
        session.input(
            Event::Note {
                channel: 0,
                note,
                velocity: 0,
            },
            at + Duration::from_millis(100),
        );
    }
    session.tick(start + Duration::from_secs(7)).unwrap();
    let attempt = session.profile.recent_attempts.last().unwrap();
    assert!(attempt.correct && attempt.pitch_correct && attempt.timing_correct);
    assert_eq!(attempt.bpm, Some(120));
    assert_eq!(attempt.evidence.len(), 16);
    assert_eq!(attempt.evidence[2].offset_ms, 500);
    assert!(session.metronome_at.is_none());
}

#[test]
fn basic_descending_intervals_are_introduced_early_at_ninety_percent() {
    let graph = curriculum();
    for seed in [87654, 42, 777] {
        let mut profile = Profile::default();
        let mut rng = exercise::Random(seed);
        for turn in 0..500 {
            let skill = profile.next(&graph, 1000).unwrap().0;
            let mut attempt = result(&skill.id, rng.take(100) < 90, turn);
            attempt.at = 1000;
            profile.record(skill, attempt);
        }
        for semitones in [1, 2, 3, 4] {
            for direction in ["up", "down"] {
                let id = format!("interval.hear.{semitones}.{direction}");
                assert!(
                    profile.skills.contains_key(&id),
                    "{id} delayed beyond 500 exercises, seed {seed}"
                );
            }
        }
    }
}

#[test]
fn qualification_persists_without_copying_mastery_or_losing_review() {
    let graph = curriculum();
    let parent = graph
        .iter()
        .find(|s| s.id == "interval.build.7.up")
        .unwrap();
    let child = graph
        .iter()
        .find(|s| s.id == "interval.hear.7.down")
        .unwrap();
    let mut profile = Profile::default();
    for turn in 0..6 {
        profile.record(parent, result(&parent.id, true, turn));
    }
    let qualified = profile.skills[&parent.id].first_qualified_at;
    assert_eq!(qualified, Some(6));
    assert!(profile.unlocked(child));
    profile.record(parent, result(&parent.id, false, 6));
    assert!(profile.skills[&parent.id].score < 7.0);
    assert!(profile.unlocked(child));
    assert!(!profile.skills.contains_key(&child.id));
    let mut profile: Profile =
        serde_json::from_str(&serde_json::to_string(&profile).unwrap()).unwrap();
    assert_eq!(profile.skills[&parent.id].first_qualified_at, qualified);
    profile.record(child, result(&child.id, false, 7));
    assert!(profile.skills[&parent.id].review_requested);
    let old: Mastery = serde_json::from_str(r#"{"score":8.0,"attempts":10,"correct":9,"streak":4,"variants":[],"last_practiced":1000,"last_turn":10,"review_requested":false}"#).unwrap();
    assert_eq!(old.first_qualified_at, None);
    assert_eq!(old.introduced_at, 0);
}

#[test]
fn oldest_ready_introduction_wins_within_a_stage_and_practice_is_bounded() {
    let mut early = skill(Task::BuildInterval {
        semitones: 2,
        direction: Direction::Up,
    });
    early.id = "early".into();
    let mut late = early.clone();
    late.id = "late".into();
    let mut first = early.clone();
    first.id = "first".into();
    first.requires = vec![curriculum::Requirement {
        skill: early.id.clone(),
        score: 7.0,
        attempts: 6,
    }];
    let mut second = first.clone();
    second.id = "second".into();
    second.requires[0].skill = late.id.clone();
    let mut profile = Profile::default();
    profile.completed = 100;
    let mut state = established();
    state.first_qualified_at = Some(10);
    profile.skills.insert(early.id.clone(), state.clone());
    state.first_qualified_at = Some(80);
    profile.skills.insert(late.id.clone(), state);
    let graph = [second, first];
    assert_eq!(profile.next(&graph, 1000).unwrap().0.id, "first");
    // Even an old waiting skill cannot flood a learner with weak new material.
    let mut graph = graph.to_vec();
    for i in 0..12 {
        let mut weak = early.clone();
        weak.id = format!("weak{i}");
        profile.skills.insert(
            weak.id.clone(),
            Mastery {
                attempts: 1,
                score: 2.0,
                last_turn: 90,
                ..Default::default()
            },
        );
        graph.push(weak);
    }
    assert!(profile.next(&graph, 1000).unwrap().0.id.starts_with("weak"));
    for state in profile.skills.values_mut() {
        state.score = 8.0;
        state.attempts = 6;
    }
    assert_eq!(profile.next(&graph, 1000).unwrap().0.id, "first");
    for i in 12..24 {
        let mut consolidating = early.clone();
        consolidating.id = format!("consolidating{i}");
        profile.skills.insert(
            consolidating.id.clone(),
            Mastery {
                attempts: 6,
                score: 8.0,
                last_turn: 90,
                ..Default::default()
            },
        );
        graph.push(consolidating);
    }
    let next = profile.next(&graph, 1000).unwrap().0;
    assert!(
        profile.skills.contains_key(&next.id),
        "Bound the consolidation backlog too"
    );
}

#[test]
fn curriculum_stages_cover_all_basics_and_promote_their_dependencies() {
    let graph = curriculum();
    for skill in &graph {
        for req in &skill.requires {
            let parent = graph.iter().find(|s| s.id == req.skill).unwrap();
            assert!(
                parent.stage <= skill.stage,
                "{} is gated by a later stage",
                skill.id
            );
        }
        match skill.task {
            Task::BuildInterval { .. } | Task::HearInterval { .. } => assert!(skill.stage <= 1),
            _ => {}
        }
    }
    for interval in 1..=12 {
        let listening: Vec<_> = graph
            .iter()
            .filter(
                |s| matches!(s.task, Task::HearInterval { semitones, .. } if semitones == interval),
            )
            .collect();
        assert_eq!(listening.len(), 3);
        for s in &listening {
            assert_eq!(s.requires.len(), 1);
            assert_eq!(s.requires[0].skill, format!("interval.build.{interval}.up"));
        }
    }
    let get = |id: &str| graph.iter().find(|s| s.id == id).unwrap();
    assert!(get("melody.0.8").stage < get("tonic.1.major").stage);
    assert!(get("melody.0.5").stage < get("scale.0.dorian.up").stage);
}

#[test]
fn tonic_exploration_is_ungraded_and_enter_starts_a_clean_answer() {
    let dir = tempfile::tempdir().unwrap();
    let mut session = Session::open(dir.path().join("learner.json")).unwrap();
    let tonic = session
        .graph
        .iter()
        .find(|s| s.id == "tonic.0.major")
        .unwrap();
    session.exercise = Some(Exercise::generate(tonic, 42));
    let start = Instant::now();
    session.ready(start);
    let after = start + Duration::from_secs(10);
    session.tick(after).unwrap();
    assert!(session.view().exploring);
    for note in [61, 62, 63, 60] {
        for velocity in [90, 0] {
            session.input(
                Event::Note {
                    channel: 0,
                    note,
                    velocity,
                },
                after,
            );
        }
    }
    session.tick(after + Duration::from_secs(60)).unwrap();
    assert_eq!(session.profile.completed, 0);
    assert!(session.played.is_empty() && session.evidence.is_empty());
    session.submit(false).unwrap();
    assert!(!session.view().exploring);
    assert!(session.phase == Phase::Answering);
    for velocity in [90, 0] {
        session.input(
            Event::Note {
                channel: 0,
                note: 60,
                velocity,
            },
            after,
        );
    }
    session.tick(after + Duration::from_millis(500)).unwrap();
    assert_eq!(session.profile.completed, 1);
    assert!(session.profile.recent_attempts[0].correct);
    assert_eq!(session.profile.recent_attempts[0].answer, vec![60]);
}

#[test]
fn answer_policies_keep_intervals_direct_and_scale_prompts_explicit() {
    for skill in curriculum() {
        let ex = Exercise::generate(&skill, 42);
        if matches!(
            skill.task,
            Task::BuildInterval { .. } | Task::HearInterval { .. }
        ) {
            assert!(ex.answer_policy == exercise::AnswerPolicy::Direct);
        }
        if matches!(skill.task, Task::Scale { .. }) || ex.title == "SCALE CONSTRUCTION" {
            assert!(ex.prompt.contains(" scale "), "{}: {}", skill.id, ex.prompt);
        }
    }
}

#[test]
fn fixed_length_answers_finish_on_last_attack_without_release_or_delay() {
    for id in ["interval.build.2.up", "tonic.0.major"] {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::open(dir.path().join("learner.json")).unwrap();
        let skill = session.graph.iter().find(|s| s.id == id).unwrap();
        session.exercise = Some(Exercise::generate(skill, 42));
        let now = Instant::now();
        session.ready(now);
        let now = now + Duration::from_secs(10);
        session.tick(now).unwrap();
        if session.exploring {
            session.submit(false).unwrap();
        }
        let notes = session.exercise.as_ref().unwrap().expected_evidence();
        for (i, note) in notes.iter().enumerate() {
            session.input(
                Event::Note {
                    channel: 0,
                    note: *note,
                    velocity: 90,
                },
                now,
            );
            if i + 1 == notes.len() {
                // A queued extra attack must not extend the completed answer.
                session.input(
                    Event::Note {
                        channel: 0,
                        note: 99,
                        velocity: 90,
                    },
                    now,
                );
            }
            session.tick(now).unwrap();
            assert_eq!(session.profile.completed, u64::from(i + 1 == notes.len()));
        }
        assert!(session.profile.recent_attempts[0].correct);
        assert_eq!(session.profile.recent_attempts[0].answer, notes);
    }
}
