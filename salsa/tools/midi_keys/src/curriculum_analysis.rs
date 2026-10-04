//! Opt-in scheduler audit using the production graph and scheduler. Simulations
//! start fresh or replay a read-only profile snapshot; no progress is saved.
//! Run the trainer_test binary with --ignored --nocapture.
use super::*;
use std::collections::BTreeSet;

fn audit_revision() -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for source in [
        include_str!("learner.rs"),
        include_str!("courses.rs"),
        include_str!("curriculum.rs"),
    ] {
        for byte in source.bytes() {
            hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
    }
    format!("{hash:016x}")
}

fn ancestors<'a>(id: &'a str, graph: &'a [Skill], visited: &mut BTreeSet<&'a str>) {
    if !visited.insert(id) {
        return;
    }
    let skill = graph.iter().find(|s| s.id == id).unwrap();
    for req in &skill.requires {
        ancestors(&req.skill, graph, visited);
    }
}
#[test]
#[ignore = "opt-in learning-path and scheduler cost report"]
fn report_learning_paths() {
    let graph = curriculum();
    let targets = [
        "melody.0.5",
        "melody.0.8",
        "harmony.0.major.progression.4",
        "dictation.0.natural-minor.5",
        "harmony.0.natural-minor.progression.4",
    ];
    for id in targets {
        let mut prerequisites = BTreeSet::new();
        ancestors(id, &graph, &mut prerequisites);
        eprintln!("PATH {id}: {} prerequisite skills, {} perfect answers to unlock, {} to establish via shortest path",prerequisites.len()-1,(prerequisites.len()-1)*6,(prerequisites.len()-1)*6+12);
    }
    let mut joint = BTreeSet::new();
    for id in [targets[0], targets[2]] {
        ancestors(id, &graph, &mut joint);
    }
    eprintln!("JOINT shortest path (5-note melody + 4-chord progression, C major): {} skills, {} answers to establish both",joint.len(),(joint.len()-2)*6+24);
    for percent in [100, 90] {
        let mut profile = Profile::default();
        let mut rng = exercise::Random(87654);
        let mut first = BTreeSet::new();
        let mut mastered = BTreeSet::new();
        for turn in 0..50000 {
            let Some((skill, _)) = profile.next(&graph, 1000) else {
                break;
            };
            if targets.contains(&skill.id.as_str()) && first.insert(skill.id.clone()) {
                eprintln!(
                    "SCHEDULE {percent}% first {} at exercise {}",
                    skill.id,
                    turn + 1
                );
            }
            let correct = rng.take(100) < percent;
            profile.record(
                skill,
                Attempt {
                    title: String::new(),
                    prompt: String::new(),
                    spelled_answer: vec![],
                    number: 0,
                    skill: skill.id.clone(),
                    variant: turn.to_string(),
                    correct,
                    assisted: false,
                    at: 1000,
                    answer: vec![],
                    expected: vec![],
                    evidence: vec![],
                    bpm: None,
                    pitch_correct: correct,
                    timing_correct: true,
                    duration_correct: true,
                },
            );
            // The scheduler does not read history; keep the audit's memory bounded.
            profile.recent_attempts.clear();
            if targets.contains(&skill.id.as_str())
                && profile.skills[&skill.id].mastered()
                && mastered.insert(skill.id.clone())
            {
                eprintln!(
                    "SCHEDULE {percent}% established {} at exercise {}",
                    skill.id,
                    turn + 1
                );
            }
            if mastered.len() == targets.len() {
                break;
            }
            if (turn + 1) % 10000 == 0 {
                eprintln!(
                    "SCHEDULE {percent}% reached {} exercises; {}/{} milestones established",
                    turn + 1,
                    mastered.len(),
                    targets.len()
                );
            }
        }
    }
}

#[test]
#[ignore = "opt-in repeated 90-percent scheduler simulation"]
fn report_ninety_percent_distribution() {
    simulate_distribution(&Profile::default(), 90, "fresh");
}

#[test]
#[ignore = "opt-in read-only continuation from a trusted history prefix"]
fn report_trusted_history_continuation() {
    let path = std::env::var_os("MIDI_AUDIT_PROFILE")
        .expect("Set MIDI_AUDIT_PROFILE to a profile snapshot");
    let trusted: usize = std::env::var("MIDI_AUDIT_TRUSTED")
        .expect("Set MIDI_AUDIT_TRUSTED explicitly")
        .parse()
        .unwrap();
    let saved: Profile = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(trusted > 0 && trusted <= saved.recent_attempts.len());
    let graph = curriculum();
    let mut profile = Profile::default();
    for (index, attempt) in saved.recent_attempts.iter().take(trusted).enumerate() {
        assert_eq!(
            attempt.number,
            index as u64 + 1,
            "A full contiguous history is required"
        );
        let skill = graph
            .iter()
            .find(|s| s.id == attempt.skill)
            .expect("History refers to unknown skill");
        profile.record(skill, attempt.clone());
    }
    let correct = profile
        .recent_attempts
        .iter()
        .filter(|a| a.correct && !a.assisted)
        .count();
    eprintln!(
        "TRUSTED PREFIX {} attempts, {} independent correct; no profile writes",
        trusted, correct
    );
    let rate = std::env::var("MIDI_AUDIT_ACCURACY")
        .ok()
        .map(|s| s.parse::<usize>().unwrap());
    assert!(rate.is_none_or(|r| r <= 100));
    if rate.is_none() || rate == Some(90) {
        simulate_distribution(&profile, 90, "trusted-prefix-90");
    }
    if rate.is_none() || rate == Some(correct * 100 / trusted) {
        simulate_distribution(
            &profile,
            correct * 100 / trusted,
            "trusted-prefix-observed-rate",
        );
    }
    assert!(
        rate.is_none_or(|r| r == 90 || r == correct * 100 / trusted),
        "Choose 90 or the observed rate"
    );
}

fn category(skill: &Skill) -> &'static str {
    match &skill.task {
        curriculum::Task::BuildInterval { .. } => "interval construction",
        curriculum::Task::HearInterval { .. } => "interval recognition",
        curriculum::Task::Practice(practice::Practice::Tonic { .. }) => "tonic",
        curriculum::Task::Practice(practice::Practice::Degree { .. }) => "single scale tone",
        curriculum::Task::Practice(practice::Practice::Movement { .. }) => "contextual movement",
        curriculum::Task::Practice(practice::Practice::Melody { .. }) => "melody",
        _ => "other",
    }
}

fn simulate_distribution(initial: &Profile, percent: usize, label: &str) {
    let graph = curriculum();
    let revision = audit_revision();
    let trials: u64 = std::env::var("MIDI_AUDIT_TRIALS")
        .map(|s| s.parse().unwrap())
        .unwrap_or(20);
    assert!(trials > 0);
    let now = initial.recent_attempts.last().map(|a| a.at).unwrap_or(1000);
    eprintln!(
        "SCENARIO {label} starting_completed={} accuracy={percent}%",
        initial.completed
    );
    let targets = [
        "dictation.0.major.2",
        "melody.0.3",
        "melody.0.5",
        "melody.0.8",
        "harmony.0.major.progression.4",
    ];
    let mut firsts: Vec<Vec<usize>> = vec![vec![]; targets.len()];
    let mut established: Vec<Vec<usize>> = vec![vec![]; targets.len()];
    let mut interval_counts = vec![];
    let mut paired_totals = vec![];
    let mut export = vec![];
    let mut prefix_first = std::collections::BTreeMap::new();
    let mut prefix_mastered = std::collections::BTreeMap::new();
    let mut replay = Profile::default();
    for attempt in &initial.recent_attempts {
        let skill = graph.iter().find(|s| s.id == attempt.skill).unwrap();
        prefix_first
            .entry(skill.id.clone())
            .or_insert(attempt.number as usize);
        replay.record(skill, attempt.clone());
        if replay.skills[&skill.id].mastered() {
            prefix_mastered
                .entry(skill.id.clone())
                .or_insert(attempt.number as usize);
        }
    }
    for trial in 0..trials {
        let seed = if trial == 0 {
            87654
        } else {
            trial.wrapping_mul(0x9e3779b97f4a7c15)
        };
        let mut rng = exercise::Random(seed);
        let mut profile = initial.clone();
        let mut all_first = prefix_first.clone();
        let mut all_mastered = prefix_mastered.clone();
        let mut first = vec![None; targets.len()];
        let mut mastered = vec![None; targets.len()];
        let mut categories = std::collections::BTreeMap::<String, usize>::new();
        for attempt in &initial.recent_attempts {
            let skill = graph.iter().find(|s| s.id == attempt.skill).unwrap();
            *categories.entry(category(skill).into()).or_default() += 1;
        }
        for turn in initial.completed as usize + 1..=10000 {
            let (skill, _) = profile.next(&graph, now).unwrap();
            all_first.entry(skill.id.clone()).or_insert(turn);
            *categories.entry(category(skill).into()).or_default() += 1;
            let target = targets.iter().position(|id| *id == skill.id);
            if let Some(i) = target {
                first[i].get_or_insert(turn);
            }
            let correct = rng.take(100) < percent;
            profile.record(
                skill,
                Attempt {
                    title: String::new(),
                    prompt: String::new(),
                    spelled_answer: vec![],
                    number: 0,
                    skill: skill.id.clone(),
                    variant: turn.to_string(),
                    correct,
                    assisted: false,
                    at: now,
                    answer: vec![],
                    expected: vec![],
                    evidence: vec![],
                    bpm: None,
                    pitch_correct: correct,
                    timing_correct: true,
                    duration_correct: true,
                },
            );
            profile.recent_attempts.clear();
            if profile.skills[&skill.id].mastered() {
                all_mastered.entry(skill.id.clone()).or_insert(turn);
            }
            if let Some(i) = target {
                if profile.skills[&skill.id].mastered() {
                    mastered[i].get_or_insert(turn);
                }
            }
            if turn == 200 {
                let intervals = categories
                    .get("interval construction")
                    .copied()
                    .unwrap_or(0)
                    + categories.get("interval recognition").copied().unwrap_or(0);
                interval_counts.push(intervals);
                eprintln!(
                    "AT200 trial={trial} categories={categories:?} introduced={} established={}",
                    profile.skills.len(),
                    profile.skills.values().filter(|s| s.mastered()).count()
                );
            }
            if mastered.iter().all(Option::is_some)
                && graph
                    .iter()
                    .filter(|s| s.stage <= 1)
                    .all(|s| all_mastered.contains_key(&s.id))
            {
                break;
            }
        }
        for i in 0..targets.len() {
            if let Some(n) = first[i] {
                firsts[i].push(n);
            }
            if let Some(n) = mastered[i] {
                established[i].push(n);
            }
        }
        if let (Some(melody), Some(chords)) = (mastered[2], mastered[4]) {
            paired_totals.push(melody.max(chords));
        }
        eprintln!("TRIAL {trial}: first={first:?} established={mastered:?}");
        if std::env::var_os("MIDI_AUDIT_OUTPUT_DIR").is_some() {
            for skill in &graph {
                export.push(serde_json::json!({
                    "scenario": label, "accuracy_percent": percent, "trial": trial,
                    "policy_revision": revision,
                    "seed": seed.to_string(), "starting_completed": initial.completed,
                    "ending_completed": profile.completed,
                    "skill_id": skill.id, "title": skill.title, "category": category(skill),
                    "stage": skill.stage,
                    "introduced_at": all_first.get(&skill.id),
                    "first_mastered_at": all_mastered.get(&skill.id),
                }));
            }
        }
    }
    if let Some(dir) = std::env::var_os("MIDI_AUDIT_OUTPUT_DIR") {
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::path::Path::new(&dir).join(format!("{label}.json"));
        std::fs::write(path, serde_json::to_vec(&export).unwrap()).unwrap();
    }
    let summarize = |values: &mut Vec<usize>| {
        values.sort_unstable();
        if values.is_empty() {
            return "none reached within 10,000 exercises".to_string();
        }
        let n = values.len();
        let median = if n % 2 == 0 {
            (values[n / 2 - 1] + values[n / 2]) as f64 / 2.0
        } else {
            values[n / 2] as f64
        };
        format!(
            "n={n}/{trials} min={} median={median} max={}",
            values[0],
            values[n - 1]
        )
    };
    eprintln!("INTERVALS AT 200 {}", summarize(&mut interval_counts));
    for (i, id) in targets.iter().enumerate() {
        eprintln!(
            "DISTRIBUTION {id} first [{}] established [{}]",
            summarize(&mut firsts[i]),
            summarize(&mut established[i])
        );
    }
    eprintln!(
        "BOTH MELODY5 AND PROGRESSION4 cumulative [{}]",
        summarize(&mut paired_totals)
    );
    for count in &mut paired_totals {
        *count -= initial.completed as usize;
    }
    eprintln!(
        "BOTH MELODY5 AND PROGRESSION4 additional [{}]",
        summarize(&mut paired_totals)
    );
}

// Engineering pacing guardrails for the 86–90% audits, not claims about human
// learning speed. Inspect all foundations rather than a few favorable goals.
fn introduction_deadline(skill: &Skill) -> Option<u64> {
    match &skill.task {
        curriculum::Task::BuildInterval { semitones, .. }
        | curriculum::Task::HearInterval { semitones, .. } => {
            Some(if [1, 2, 3, 4, 5, 7, 12].contains(semitones) {
                1000
            } else {
                3000
            })
        }
        curriculum::Task::HearChord {
            root: 0,
            quality: 0 | 1,
        } => Some(1500),
        _ if skill.stage == 0 => Some(2000),
        _ if skill.stage == 1 => Some(5000),
        _ => None,
    }
}

#[test]
fn pacing_guardrails_include_every_interval_presentation() {
    let graph = curriculum();
    let mut checked = 0;
    for skill in &graph {
        if matches!(
            skill.task,
            curriculum::Task::BuildInterval { .. } | curriculum::Task::HearInterval { .. }
        ) {
            assert!(introduction_deadline(skill).is_some());
            checked += 1;
        }
    }
    assert_eq!(checked, 60);
    let fifth = graph.iter().find(|s| s.id == "interval.hear.7.up").unwrap();
    assert!(
        2067 > introduction_deadline(fifth).unwrap(),
        "The previously missed regression must fail the audit"
    );
}

#[test]
#[ignore = "validate all skills in three completed 20-run simulation exports"]
fn validate_exported_pacing() {
    let directory = std::env::var_os("MIDI_AUDIT_OUTPUT_DIR").expect("Set MIDI_AUDIT_OUTPUT_DIR");
    let graph = curriculum();
    let revision = audit_revision();
    let skills: std::collections::BTreeMap<_, _> =
        graph.iter().map(|s| (s.id.as_str(), s)).collect();
    let mut failures = vec![];
    for scenario in ["fresh", "trusted-prefix-90", "trusted-prefix-observed-rate"] {
        let path = std::path::Path::new(&directory).join(format!("{scenario}.json"));
        let rows: Vec<serde_json::Value> =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(rows.len(), 20 * graph.len());
        let mut seen = BTreeSet::new();
        for row in &rows {
            assert_eq!(
                row["policy_revision"].as_str(),
                Some(revision.as_str()),
                "Rerun simulations after changing the policy or curriculum"
            );
            let trial = row["trial"].as_u64().unwrap();
            let id = row["skill_id"].as_str().unwrap();
            assert!(trial < 20 && seen.insert((trial, id)));
            let skill = skills[id];
            assert_eq!(
                row["stage"].as_u64(),
                Some(u64::from(skill.stage)),
                "Export from a different curriculum"
            );
            let first = row["introduced_at"].as_u64();
            let mastered = row["first_mastered_at"].as_u64();
            let end = row["ending_completed"].as_u64().unwrap();
            assert!(first.is_none_or(|n| n > 0 && n <= end));
            assert!(mastered.is_none_or(|n| first.is_some_and(|f| f <= n) && n <= end));
            if let Some(deadline) = introduction_deadline(skill) {
                if first.is_none_or(|n| n > deadline) {
                    failures.push(format!(
                        "{scenario} trial {trial}: {id} first {first:?}, deadline {deadline}"
                    ));
                }
            }
            if skill.stage <= 1 && mastered.is_none() {
                failures.push(format!(
                    "{scenario} trial {trial}: {id} never mastered by {end}"
                ));
            }
        }
        eprintln!("AUDIT {scenario}: checked every skill in 20 runs, including all 60 interval variants and all foundation mastery events");
    }
    assert!(
        failures.is_empty(),
        "Pacing regressions:\n{}",
        failures.join("\n")
    );
}

#[test]
#[ignore = "export the production skill graph for visualization"]
fn export_curriculum_graph() {
    let graph = curriculum();
    curriculum::validate(&graph).unwrap();
    let path = std::env::var_os("MIDI_GRAPH_OUTPUT").expect("Set MIDI_GRAPH_OUTPUT");
    let index: std::collections::BTreeMap<_, _> = graph
        .iter()
        .enumerate()
        .map(|(i, skill)| (skill.id.as_str(), i))
        .collect();
    let nodes: Vec<_> = graph
        .iter()
        .map(|skill| {
            serde_json::json!([
                skill.id,
                skill.title,
                skill.stage,
                skill
                    .requires
                    .iter()
                    .map(|r| index[r.skill.as_str()])
                    .collect::<Vec<_>>()
            ])
        })
        .collect();
    // Keep thresholds explicit and verify that a single legend describes every edge.
    assert!(graph
        .iter()
        .flat_map(|s| &s.requires)
        .all(|r| r.score == 7.0 && r.attempts == 6));
    let export = serde_json::json!({"revision": audit_revision(), "score": 7, "attempts": 6, "nodes": nodes});
    std::fs::write(path, serde_json::to_vec(&export).unwrap()).unwrap();
}
