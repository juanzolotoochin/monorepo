//! Read-only practice summaries. Recent evidence is bounded and assisted attempts
//! never establish a weakness or inflate independent success rates.
use crate::{
    curriculum::Skill,
    learner::{Attempt, Profile},
};
use std::collections::BTreeMap;

fn issues(attempt: &Attempt) -> String {
    if attempt.evidence.is_empty() {
        return "answer".into();
    }
    let mut labels = vec![];
    if !attempt.pitch_correct {
        labels.push("notes / count");
    }
    if !attempt.timing_correct {
        labels.push("attack timing");
    }
    if !attempt.duration_correct {
        labels.push("held lengths");
    }
    if labels.is_empty() {
        labels.push("answer");
    }
    labels.join(", ")
}

impl Profile {
    pub fn insights_at(&self, graph: &[Skill], now: u64, current: Option<&str>) -> Vec<String> {
        let title = |id: &str| {
            graph
                .iter()
                .find(|s| s.id == id)
                .map(|s| s.title.clone())
                .unwrap_or_else(|| id.into())
        };
        let known: Vec<_> = graph
            .iter()
            .filter_map(|s| self.skills.get(&s.id).map(|m| (s, m)))
            .collect();
        let mastered = known.iter().filter(|(_, m)| m.mastered()).count();
        let practicing = known
            .iter()
            .filter(|(_, m)| m.attempts > 0 && !m.mastered())
            .count();
        let day: Vec<_> = self
            .recent_attempts
            .iter()
            .filter(|a| a.at <= now && now.saturating_sub(a.at) <= 86400)
            .collect();
        let independent: Vec<_> = day.iter().copied().filter(|a| !a.assisted).collect();
        let passed = independent.iter().filter(|a| a.successful()).count();
        let mut result = vec![format!(
            "Progress: {mastered} skills mastered · {practicing} in practice"
        )];
        if independent.is_empty() {
            result.push(format!(
                "Last 24h: {} attempts; no independent results yet",
                day.len()
            ));
        } else {
            result.push(format!(
                "Last 24h: {} attempts · {passed}/{} independent passed ({}%) · {} assisted",
                day.len(),
                independent.len(),
                (100 * passed + independent.len() / 2) / independent.len(),
                day.len() - independent.len()
            ));
        }
        // Do not let a single miss or an old result masquerade as a pattern.
        let recent: Vec<_> = independent.iter().rev().copied().take(40).collect();
        let passed_recent = recent.iter().filter(|a| a.successful()).count();
        result.push(format!(
            "Recent: {passed_recent}/{} independent attempts passed",
            recent.len()
        ));
        let mut groups: BTreeMap<&str, Vec<&Attempt>> = BTreeMap::new();
        for attempt in &recent {
            groups.entry(&attempt.skill).or_default().push(attempt);
        }
        let mut weak: Vec<_> = groups
            .iter()
            .filter_map(|(id, attempts)| {
                let missed = attempts.iter().filter(|a| !a.successful()).count();
                (attempts.len() >= 3 && missed >= 2).then_some((*id, attempts, missed))
            })
            .collect();
        weak.sort_by(|a, b| {
            (b.2 * a.1.len())
                .cmp(&(a.2 * b.1.len()))
                .then_with(|| b.2.cmp(&a.2))
                .then_with(|| a.0.cmp(b.0))
        });
        for (id, attempts, missed) in weak.iter().take(2) {
            let mut kinds = BTreeMap::new();
            for a in attempts.iter().filter(|a| !a.successful()) {
                *kinds.entry(issues(a)).or_insert(0usize) += 1;
            }
            let kind = kinds
                .iter()
                .max_by_key(|(_, count)| *count)
                .map(|(kind, _)| kind.as_str())
                .unwrap_or("answer");
            result.push(format!(
                "Focus: {} · {missed}/{} missed in last {} independent attempts ({kind})",
                title(id),
                attempts.len(),
                recent.len()
            ));
        }
        if weak.is_empty() && !recent.is_empty() {
            if let Some(miss) = recent.iter().find(|a| !a.successful()) {
                result.push(format!(
                    "Last miss: {} · {} (saved result; not a repeated pattern yet)",
                    title(&miss.skill),
                    issues(miss)
                ));
            } else {
                result.push(format!(
                    "Recent: no misses in {} independent attempts; keep building consistency",
                    recent.len()
                ));
            }
        }
        if let Some(id) = current {
            let state = self.skills.get(id).cloned().unwrap_or_default();
            result.push(format!(
                "Working on: {} · {:.1}/10 evidence · {}/12 attempts · {}/5 correct streak",
                title(id),
                state.score,
                state.attempts.min(12),
                state.streak.min(5)
            ));
        }
        let recognition = self.recognition.insights();
        result.extend(
            recognition
                .iter()
                .filter(|s| s.starts_with("Confusion:"))
                .cloned(),
        );
        let mut established: Vec<_> = known
            .iter()
            .filter(|(s, m)| {
                m.mastered() && !matches!(s.task, crate::curriculum::Task::HearInterval { .. })
            })
            .collect();
        established.sort_by_key(|(_, m)| std::cmp::Reverse(m.last_turn));
        if let Some((skill, _)) = established.first() {
            result.push(format!("Mastered: {}", skill.title));
        }
        result.extend(
            recognition
                .into_iter()
                .filter(|s| !s.starts_with("Confusion:")),
        );
        // Explain one nearby dependency using the same qualification rule as unlocking.
        if let Some(skill) = graph
            .iter()
            .filter(|s| !s.requires.is_empty() && !self.unlocked(s))
            .filter(|s| {
                s.requires
                    .iter()
                    .all(|r| self.skills.get(&r.skill).is_some_and(|m| m.attempts > 0))
            })
            .min_by_key(|s| {
                (
                    s.requires
                        .iter()
                        .filter(|r| !self.requirement_met(r))
                        .count(),
                    s.stage,
                )
            })
        {
            if let Some(r) = skill.requires.iter().find(|r| !self.requirement_met(r)) {
                let state = &self.skills[&r.skill];
                result.push(format!("Unlock path: {} needs {} at {:.1}/10 and {} attempts; now {:.1}/10, {} attempts. Other requirements may remain.",
                    skill.title,title(&r.skill),r.score,r.attempts,state.score,state.attempts));
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::result;
    #[test]
    fn recent_patterns_exclude_assistance_old_results_and_respect_numeric_grades() {
        let graph = crate::curriculum::curriculum();
        let mut profile = Profile::default();
        let id = "interval.hear.2.up";
        let now = 200000;
        for (number, good, assisted, at, grade) in [
            (1, false, false, now, None),
            (2, false, false, now, None),
            (3, false, false, now, Some(99)),
            (4, false, true, now, None),
            (5, false, false, 1, None),
        ] {
            let mut attempt = result(id, good, number);
            attempt.number = number;
            attempt.at = at;
            attempt.assisted = assisted;
            attempt.performance_score = grade;
            profile.recent_attempts.push(attempt);
        }
        let insights = profile.insights_at(&graph, now, Some(id)).join("\n");
        assert!(insights.contains("4 attempts · 1/3 independent passed (33%) · 1 assisted"));
        assert!(insights.contains("2/3 missed in last 3 independent attempts"));
        assert!(insights.contains("Working on:"));
        profile.recent_attempts.remove(0);
        assert!(!profile
            .insights_at(&graph, now, None)
            .iter()
            .any(|s| s.starts_with("Focus:")));
    }
}
