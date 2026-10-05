//! Cumulative listening sets and recent confusion evidence, independent of UI/audio.
use crate::curriculum::{Direction, Skill, Task, INTERVALS};
use crate::learner::{Attempt, Profile};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const SETS: [&[usize]; 7] = [
    &[1, 2],
    &[1, 2, 3, 4],
    &[1, 2, 3, 4, 7, 12],
    &[1, 2, 3, 4, 5, 7, 12],
    &[1, 2, 3, 4, 5, 7, 8, 9, 12],
    &[1, 2, 3, 4, 5, 7, 8, 9, 10, 11, 12],
    &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
];
pub const LABELS: [&str; 7] = [
    "seconds",
    "seconds and thirds",
    "through fifths and octaves",
    "with perfect fourths",
    "with sixths",
    "with sevenths",
    "all simple intervals",
];

pub fn direction_name(direction: Direction) -> &'static str {
    match direction {
        Direction::Up => "ascending",
        Direction::Down => "descending",
        Direction::Together => "simultaneous",
        Direction::Both => "both",
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Context {
    pub version: u8,
    pub set: usize,
    pub direction: String,
    pub expected: usize,
    pub heard_as: Option<usize>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Recognition {
    pub tracks: BTreeMap<String, Track>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Track {
    pub stage: usize,
    pub last_turn: u64,
    pub sets: BTreeMap<usize, SetEvidence>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
pub struct SetEvidence {
    #[serde(default)]
    pub qualified_members: BTreeSet<usize>,
    pub members: BTreeMap<usize, Vec<Observation>>,
    pub mastered_at: Option<u64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Observation {
    pub correct: bool,
    pub heard_as: Option<usize>,
    pub variant: String,
    pub turn: u64,
}
impl SetEvidence {
    fn established(&self, interval: usize, stage: usize) -> bool {
        self.qualified_members.contains(&interval) || self.qualified(interval, stage)
    }
    fn qualified(&self, interval: usize, stage: usize) -> bool {
        let Some(rows) = self.members.get(&interval) else {
            return false;
        };
        let minimum = if stage > 0 && SETS[stage - 1].contains(&interval) {
            3
        } else {
            6
        };
        rows.len() >= minimum
            && rows.iter().filter(|r| r.correct).count() * 5 >= rows.len() * 4
            && rows
                .iter()
                .filter(|r| r.correct)
                .map(|r| &r.variant)
                .collect::<BTreeSet<_>>()
                .len()
                >= 2
    }
    fn confusion(&self, source: usize) -> Option<(usize, usize, usize)> {
        let rows = self.members.get(&source)?;
        let mut counts = BTreeMap::<usize, usize>::new();
        for row in rows.iter().filter(|r| !r.correct) {
            if let Some(target) = row.heard_as.filter(|t| *t != source) {
                *counts.entry(target).or_default() += 1;
            }
        }
        counts
            .into_iter()
            .filter(|(_, count)| *count >= 3 && count * 4 >= rows.len())
            .max_by_key(|(_, count)| *count)
            .map(|(target, count)| (target, count, rows.len()))
    }
}
impl Recognition {
    pub fn record(&mut self, skill: &Skill, attempt: &mut Attempt) {
        let Task::HearInterval {
            semitones,
            direction,
        } = skill.task
        else {
            return;
        };
        let key = direction_name(direction);
        let track = self.tracks.entry(key.into()).or_default();
        // Old individual results are retained, but never silently count as mixed-set evidence.
        if track.stage >= SETS.len() || !SETS[track.stage].contains(&semitones) {
            return;
        }
        let heard_as = if attempt.answer.len() == 2 {
            let distance = attempt.answer[0].abs_diff(attempt.answer[1]);
            (1..=12).contains(&distance).then_some(distance)
        } else {
            None
        };
        attempt.recognition = Some(Context {
            version: 1,
            set: track.stage,
            direction: key.into(),
            expected: semitones,
            heard_as,
        });
        track.last_turn = attempt.number;
        let evidence = track.sets.entry(track.stage).or_default();
        let rows = evidence.members.entry(semitones).or_default();
        rows.push(Observation {
            correct: attempt.correct && !attempt.assisted,
            heard_as: if attempt.assisted { None } else { heard_as },
            variant: attempt.variant.clone(),
            turn: attempt.number,
        });
        if rows.len() > 10 {
            rows.remove(0);
        }
        let repeated_errors = rows.iter().rev().take(3).filter(|r| !r.correct).count() == 3;
        if repeated_errors || evidence.confusion(semitones).is_some() {
            evidence.qualified_members.remove(&semitones);
        } else if evidence.qualified(semitones, track.stage) {
            evidence.qualified_members.insert(semitones);
        }
        if SETS[track.stage]
            .iter()
            .all(|n| evidence.established(*n, track.stage))
        {
            evidence.mastered_at.get_or_insert(attempt.number);
            if track.stage + 1 < SETS.len() {
                track.stage += 1;
            }
        }
    }

    /// A balanced mixed pool remains available even after individual skills retire.
    pub fn next<'a>(
        &self,
        profile: &Profile,
        graph: &'a [Skill],
        now: u64,
    ) -> Option<(&'a Skill, &'static str)> {
        let available: BTreeSet<_> = graph
            .iter()
            .filter_map(|skill| {
                if let Task::HearInterval {
                    semitones,
                    direction,
                } = skill.task
                {
                    profile
                        .unlocked(skill)
                        .then_some((direction_name(direction), semitones))
                } else {
                    None
                }
            })
            .collect();
        let mut candidates = Vec::new();
        for skill in graph {
            let Task::HearInterval {
                semitones,
                direction,
            } = skill.task
            else {
                continue;
            };
            if !profile.unlocked(skill) {
                continue;
            }
            let key = direction_name(direction);
            let track = self.tracks.get(key);
            let stage = track.map_or(0, |t| t.stage.min(SETS.len() - 1));
            if !SETS[stage].contains(&semitones)
                || !SETS[stage].iter().all(|n| available.contains(&(key, *n)))
            {
                continue;
            }
            let evidence = track.and_then(|t| t.sets.get(&stage));
            if evidence.is_some_and(|e| e.mastered_at.is_some())
                && profile.skills.get(&skill.id).is_some_and(|s| {
                    s.mastered()
                        && !s.review_requested
                        && now.saturating_sub(s.last_practiced) < 14 * 86400
                })
            {
                continue;
            }
            let rows = evidence.and_then(|e| e.members.get(&semitones));
            let last = rows.and_then(|r| r.last()).map_or(0, |r| r.turn);
            let focus = evidence.is_some_and(|e| {
                SETS[stage].iter().any(|source| {
                    e.confusion(*source)
                        .is_some_and(|(target, _, _)| *source == semitones || target == semitones)
                })
            });
            // Deterministic jitter avoids a predictable alternating answer sequence.
            let mut random = (profile.completed + 1).wrapping_mul(0x9e3779b97f4a7c15)
                ^ (semitones as u64 * 7919)
                ^ direction as u64;
            random ^= random >> 12;
            random ^= random << 25;
            random ^= random >> 27;
            let jitter = random.wrapping_mul(0x2545f4914f6cdd1d) % 29;
            let needs_evidence = evidence.is_none_or(|e| !e.established(semitones, stage));
            let priority = profile.completed.saturating_sub(last)
                + jitter
                + if focus { 18 } else { 0 }
                + if needs_evidence { 45 } else { 0 };
            candidates.push((track.map_or(0, |t| t.last_turn), priority, skill, focus));
        }
        // Rotate presentations; within one presentation, balance coverage and confusions.
        candidates.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
        candidates.first().map(|(_, _, skill, focus)| {
            (
                *skill,
                if *focus {
                    "Mixed recognition · checking recurring confusions"
                } else {
                    "Mixed recognition · checking the full listening set"
                },
            )
        })
    }

    pub fn insights(&self) -> Vec<String> {
        let mut mastery = Vec::new();
        let mut confusions = Vec::new();
        let mut progress = Vec::new();
        for (direction, track) in &self.tracks {
            let stage = track.stage.min(SETS.len() - 1);
            if let Some((set, evidence)) = track
                .sets
                .iter()
                .rev()
                .find(|(_, e)| e.mastered_at.is_some())
            {
                mastery.push((
                    evidence.mastered_at.unwrap(),
                    format!("Mastered: {} · {direction} recognition", LABELS[*set]),
                ));
            }
            if let Some(evidence) = track.sets.get(&stage) {
                for source in SETS[stage] {
                    if let Some((target, count, total)) = evidence.confusion(*source) {
                        confusions.push(format!(
                            "Confusion: {} → {} · {direction} ({count}/{total} recent {} prompts)",
                            INTERVALS[source - 1],
                            INTERVALS[target - 1],
                            INTERVALS[source - 1]
                        ));
                    }
                }
                let qualified = SETS[stage]
                    .iter()
                    .filter(|n| evidence.established(**n, stage))
                    .count();
                if evidence.mastered_at.is_none() {
                    progress.push(format!(
                        "Checking: {} · {direction} ({qualified}/{} intervals established)",
                        LABELS[stage],
                        SETS[stage].len()
                    ));
                }
            } else {
                progress.push(format!(
                    "Next set: {} · {direction}; gathering mixed evidence",
                    LABELS[stage]
                ));
            }
        }
        mastery.sort_by_key(|(turn, _)| std::cmp::Reverse(*turn));
        let mut result: Vec<_> = mastery.into_iter().take(1).map(|(_, text)| text).collect();
        result.extend(confusions.into_iter().take(1));
        result.extend(progress.into_iter().take(1));
        if result.is_empty() {
            result.push("Gathering evidence for mixed interval recognition.".into());
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::learner::Mastery;
    fn skill(n: usize, direction: Direction) -> Skill {
        Skill {
            stage: 0,
            id: format!("hear.{n}.{}", direction_name(direction)),
            title: INTERVALS[n - 1].into(),
            task: Task::HearInterval {
                semitones: n,
                direction,
            },
            requires: vec![],
        }
    }
    fn answer(profile: &mut Profile, n: usize, response: usize, direction: Direction) {
        let s = skill(n, direction);
        let mut attempt = crate::tests::result(&s.id, n == response, profile.completed + 1);
        attempt.answer = vec![60, 60 + response];
        profile.record(&s, attempt);
    }
    #[test]
    fn isolated_slips_keep_credit_but_repeated_errors_reopen_the_check() {
        let mut p = Profile::default();
        for _ in 0..6 {
            answer(&mut p, 2, 2, Direction::Up);
        }
        answer(&mut p, 2, 7, Direction::Up);
        assert!(p.recognition.tracks["ascending"].sets[&0].established(2, 0));
        answer(&mut p, 2, 7, Direction::Up);
        answer(&mut p, 2, 7, Direction::Up);
        assert!(!p.recognition.tracks["ascending"].sets[&0].established(2, 0));
    }
    #[test]
    fn every_member_needs_fresh_mixed_evidence_and_directions_are_separate() {
        let mut p = Profile::default();
        for _ in 0..6 {
            answer(&mut p, 2, 2, Direction::Up);
        }
        assert_eq!(p.recognition.tracks["ascending"].stage, 0);
        for _ in 0..6 {
            answer(&mut p, 1, 1, Direction::Up);
        }
        let track = &p.recognition.tracks["ascending"];
        assert_eq!(track.stage, 1);
        assert!(track.sets[&0].mastered_at.is_some());
        assert!(!track.sets.contains_key(&1));
        assert!(!p.recognition.tracks.contains_key("descending"));
        for _ in 0..6 {
            answer(&mut p, 3, 3, Direction::Up);
            answer(&mut p, 4, 4, Direction::Up);
        }
        assert_eq!(
            p.recognition.tracks["ascending"].stage, 1,
            "old seconds evidence must not pass the expanded set"
        );
        assert!(p
            .recognition
            .insights()
            .iter()
            .any(|s| s.contains("Mastered: seconds · ascending")));
    }
    #[test]
    fn repeated_directional_confusions_drive_practice_and_decay() {
        let mut p = Profile::default();
        p.recognition.tracks.insert(
            "ascending".into(),
            Track {
                stage: 2,
                ..Default::default()
            },
        );
        for _ in 0..2 {
            answer(&mut p, 4, 7, Direction::Up);
        }
        assert!(p.recognition.tracks["ascending"].sets[&2]
            .confusion(4)
            .is_none());
        answer(&mut p, 4, 7, Direction::Up);
        let evidence = &p.recognition.tracks["ascending"].sets[&2];
        assert_eq!(evidence.confusion(4), Some((7, 3, 3)));
        assert!(evidence.confusion(7).is_none());
        let graph: Vec<_> = SETS[2].iter().map(|n| skill(*n, Direction::Up)).collect();
        let mut focused = 0;
        for turn in 10..110 {
            p.completed = turn;
            let (s, reason) = p.recognition.next(&p, &graph, 0).unwrap();
            if matches!(
                s.task,
                Task::HearInterval {
                    semitones: 4 | 7,
                    ..
                }
            ) {
                focused += 1;
                assert!(reason.contains("confusions"));
            }
        }
        assert!(
            focused > 50,
            "repeated confusion should meaningfully bias selection: {focused}"
        );
        for _ in 0..10 {
            answer(&mut p, 4, 4, Direction::Up);
        }
        assert!(p.recognition.tracks["ascending"].sets[&2]
            .confusion(4)
            .is_none());
    }
    #[test]
    fn mastered_individual_intervals_return_in_expanded_sets_without_false_mastery() {
        let mut p = Profile::default();
        let graph: Vec<_> = SETS[1].iter().map(|n| skill(*n, Direction::Up)).collect();
        p.recognition.tracks.insert(
            "ascending".into(),
            Track {
                stage: 1,
                ..Default::default()
            },
        );
        for s in &graph {
            p.skills.insert(
                s.id.clone(),
                Mastery {
                    score: 10.0,
                    attempts: 12,
                    streak: 12,
                    last_practiced: 100,
                    ..Default::default()
                },
            );
        }
        assert!(p.recognition.next(&p, &graph, 100).is_some());
        assert!(!p
            .recognition
            .insights()
            .iter()
            .any(|s| s.starts_with("Mastered:")));
        let encoded = serde_json::to_string(&p).unwrap();
        let restored: Profile = serde_json::from_str(&encoded).unwrap();
        assert_eq!(restored.recognition.tracks["ascending"].stage, 1);
    }
    #[test]
    fn curriculum_sets_are_cumulative_and_cover_every_simple_interval() {
        for pair in SETS.windows(2) {
            assert!(pair[0].iter().all(|n| pair[1].contains(n)));
        }
        assert_eq!(SETS.last().unwrap().len(), 12);
    }
}

#[cfg(test)]
mod evidence_tests {
    use super::*;
    #[test]
    fn hints_and_incomplete_answers_do_not_create_confusions_or_set_mastery() {
        let graph = crate::curriculum::curriculum();
        let skill = graph.iter().find(|s| s.id == "interval.hear.2.up").unwrap();
        let mut profile = Profile::default();
        for turn in 0..12 {
            let mut attempt = crate::tests::result(&skill.id, true, turn);
            attempt.assisted = true;
            attempt.answer = vec![60, 67];
            profile.record(skill, attempt);
        }
        let evidence = &profile.recognition.tracks["ascending"].sets[&0];
        assert!(evidence.mastered_at.is_none());
        assert!(evidence.confusion(2).is_none());
        assert!(!evidence.qualified(2, 0));
        let attempt = profile.recent_attempts.last().unwrap();
        assert_eq!(attempt.recognition.as_ref().unwrap().set, 0);
        let json = serde_json::to_string(&profile).unwrap();
        let restored: Profile = serde_json::from_str(&json).unwrap();
        assert_eq!(
            restored
                .recent_attempts
                .last()
                .unwrap()
                .recognition
                .as_ref()
                .unwrap()
                .heard_as,
            Some(7)
        );
        let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
        old.as_object_mut().unwrap().remove("recognition");
        for attempt in old["recent_attempts"].as_array_mut().unwrap() {
            attempt.as_object_mut().unwrap().remove("recognition");
        }
        let restored: Profile = serde_json::from_value(old).unwrap();
        assert!(restored.recognition.tracks.is_empty());
        assert_eq!(restored.completed, 12);
        assert_eq!(restored.skills[&skill.id].attempts, 12);
    }
    #[test]
    fn mixed_recognition_waits_until_all_members_are_available() {
        let graph = crate::curriculum::curriculum();
        let mut profile = Profile::default();
        let major = graph
            .iter()
            .find(|s| s.id == "interval.build.2.up")
            .unwrap();
        for turn in 0..6 {
            profile.record(major, crate::tests::result(&major.id, true, turn));
        }
        assert!(profile.recognition.next(&profile, &graph, 1000).is_none());
        let minor = graph
            .iter()
            .find(|s| s.id == "interval.build.1.up")
            .unwrap();
        for turn in 0..6 {
            profile.record(minor, crate::tests::result(&minor.id, true, turn));
        }
        assert!(profile.recognition.next(&profile, &graph, 1000).is_some());
    }
    #[test]
    fn mixed_scheduler_reaches_all_sets_without_retiring_old_members() {
        let graph: Vec<_> = crate::curriculum::curriculum()
            .into_iter()
            .filter(|s| matches!(s.task, Task::HearInterval { .. }))
            .map(|mut s| {
                s.requires.clear();
                s
            })
            .collect();
        let mut profile = Profile::default();
        let mut rng = crate::exercise::Random(42);
        for turn in 0..2500 {
            let (skill, _) = profile.recognition.next(&profile, &graph, 1000).unwrap();
            let Task::HearInterval { semitones, .. } = skill.task else {
                unreachable!()
            };
            let correct = rng.take(100) < 90;
            let mut attempt = crate::tests::result(&skill.id, correct, turn);
            attempt.answer = vec![
                60,
                60 + if correct {
                    semitones
                } else if semitones == 4 {
                    7
                } else {
                    4
                },
            ];
            profile.record(skill, attempt);
            profile.recent_attempts.clear();
            if profile.recognition.tracks.len() == 3
                && profile.recognition.tracks.values().all(|t| {
                    t.stage == 6 && t.sets.get(&6).is_some_and(|s| s.mastered_at.is_some())
                })
            {
                return;
            }
        }
        panic!("All seven sets in all three presentations should finish within 2500 recognition answers at 90% accuracy");
    }
}
