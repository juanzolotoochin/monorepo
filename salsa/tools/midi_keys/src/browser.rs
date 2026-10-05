//! Explicit exercise selection for previews; owns no learner profile or storage.
use crate::curriculum::{Skill, Task};
use crate::practice::{Harmony, Practice};

pub const CATEGORIES: &[&str] = &[
    "All exercises",
    "Intervals",
    "Chords & inversions",
    "Scales",
    "Tonal hearing",
    "Melodies",
    "Rhythm & meter",
    "Harmony",
    "Accompaniment",
    "Sight reading",
    "Lead sheets",
];
fn category(task: &Task) -> usize {
    match task {
        Task::GuidedScale(_) => 3,
        Task::Reading(task) => {
            if task.lead() {
                10
            } else {
                9
            }
        }
        Task::BuildInterval { .. } | Task::HearInterval { .. } => 1,
        Task::BuildChord { .. } | Task::HearChord { .. } | Task::Inversion { .. } => 2,
        Task::Scale { .. } | Task::TimedScale { .. } | Task::Practice(Practice::Scale { .. }) => 3,
        Task::ContextInterval { .. }
        | Task::Practice(
            Practice::Tonic { .. }
            | Practice::Degree { .. }
            | Practice::Movement { .. }
            | Practice::Resolve { .. },
        ) => 4,
        Task::Practice(Practice::Melody { .. }) => 5,
        Task::Practice(Practice::Rhythm { .. } | Practice::Meter { .. }) => 6,
        Task::Practice(Practice::Harmony {
            kind: Harmony::Accompany { .. } | Harmony::Coordination(_) | Harmony::Harmonize(_),
            ..
        }) => 8,
        Task::Practice(Practice::Harmony { .. }) => 7,
    }
}

pub struct Browser {
    pub query: String,
    pub category: usize,
    pub matches: Vec<usize>,
    pub selected: usize,
    pub open: bool,
    pub chosen: Option<usize>,
    pub variation: u64,
}
impl Browser {
    pub fn new(graph: &[Skill]) -> Self {
        Self {
            query: String::new(),
            category: 0,
            matches: (0..graph.len()).collect(),
            selected: 0,
            open: true,
            chosen: None,
            variation: 0,
        }
    }
    pub fn select_id(&mut self, id: &str, graph: &[Skill]) -> Result<(), String> {
        let index = graph
            .iter()
            .position(|skill| skill.id == id)
            .ok_or_else(|| {
                format!("Unknown exercise ID '{id}'. Use --exercises to browse available IDs.")
            })?;
        // Keep the full catalog available when the user returns with b.
        self.category = category(&graph[index].task);
        self.query.clear();
        self.filter(graph);
        self.selected = self.matches.iter().position(|i| *i == index).unwrap();
        self.select();
        Ok(())
    }
    pub fn edit(&mut self, key: u8, graph: &[Skill]) {
        match key {
            21 => self.query.clear(),
            8 | 127 => {
                self.query.pop();
            }
            32..=126 if self.query.len() < 120 => self.query.push(key as char),
            _ => return,
        }
        self.filter(graph);
    }
    pub fn change_category(&mut self, forward: bool, graph: &[Skill]) {
        self.category =
            (self.category + if forward { 1 } else { CATEGORIES.len() - 1 }) % CATEGORIES.len();
        self.filter(graph);
    }
    fn filter(&mut self, graph: &[Skill]) {
        let query = self.query.to_lowercase();
        self.matches = graph
            .iter()
            .enumerate()
            .filter_map(|(i, skill)| {
                let haystack = format!("{} {}", skill.title, skill.id).to_lowercase();
                ((self.category == 0 || category(&skill.task) == self.category)
                    && query.split_whitespace().all(|word| haystack.contains(word)))
                .then_some(i)
            })
            .collect();
        self.selected = 0;
    }
    pub fn navigate(&mut self, forward: bool) {
        if forward {
            self.selected = (self.selected + 1).min(self.matches.len().saturating_sub(1));
        } else {
            self.selected = self.selected.saturating_sub(1);
        }
    }
    pub fn select(&mut self) -> bool {
        let Some(index) = self.matches.get(self.selected).copied() else {
            return false;
        };
        self.chosen = Some(index);
        self.open = false;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn categories_partition_the_curriculum_and_search_stays_scoped() {
        let graph = crate::curriculum::curriculum();
        let mut browser = Browser::new(&graph);
        let mut seen = std::collections::BTreeSet::new();
        for index in 1..CATEGORIES.len() {
            browser.change_category(true, &graph);
            assert_eq!(browser.category, index);
            assert!(
                !browser.matches.is_empty(),
                "Empty category: {}",
                CATEGORIES[index]
            );
            for skill in &browser.matches {
                assert!(seen.insert(*skill), "Exercise in multiple categories");
            }
        }
        assert_eq!(seen.len(), graph.len());
        browser.change_category(true, &graph);
        assert_eq!(browser.category, 0);
        assert_eq!(browser.matches.len(), graph.len());
        for _ in 0..6 {
            browser.change_category(true, &graph);
        }
        for key in b"60" {
            browser.edit(*key, &graph);
        }
        assert!(!browser.matches.is_empty());
        assert!(browser
            .matches
            .iter()
            .all(|i| category(&graph[*i].task) == 6
                && format!("{} {}", graph[*i].title, graph[*i].id).contains("60")));
        browser.change_category(false, &graph);
        assert_eq!(browser.selected, 0);
        assert_eq!(browser.query, "60");
        browser.edit(21, &graph);
        assert!(!browser.matches.is_empty());
    }
}
