//! Durable learner evidence and deterministic scheduling policy.
use crate::curriculum::Skill;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const INTRODUCTION_SPACING: u64 = 6;
const MAX_INITIAL_LEARNING: usize = 12;
const MAX_UNFINISHED: usize = 24;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Mastery {
    #[serde(default)]
    pub introduced_at: u64,
    #[serde(default)]
    pub first_qualified_at: Option<u64>,
    pub score: f32,
    pub attempts: u32,
    pub correct: u32,
    pub streak: u32,
    pub variants: BTreeSet<String>,
    pub last_practiced: u64,
    pub last_turn: u64,
    pub review_requested: bool,
}
impl Mastery {
    pub fn mastered(&self) -> bool {
        self.score >= 9.0 && self.attempts >= 12 && self.streak >= 5
    }
    fn observe(&mut self, correct: bool, attempt: &Attempt) {
        self.observe_quality(correct, if correct { 1.0 } else { 0.0 }, attempt);
    }
    fn observe_quality(&mut self, correct: bool, quality: f32, attempt: &Attempt) {
        if self.attempts == 0 {
            self.introduced_at = attempt.number;
        }
        self.attempts += 1;
        let independent = correct && !attempt.assisted;
        self.score = (self.score * 0.8 + if attempt.assisted { 0.0 } else { 2.0 * quality })
            .clamp(0.0, 10.0);
        if self.score >= 7.0 && self.attempts >= 6 {
            self.first_qualified_at.get_or_insert(attempt.number);
        }
        if independent {
            self.correct += 1;
            self.streak += 1;
            self.variants.insert(attempt.variant.clone());
        } else {
            self.streak = 0;
        }
        self.last_turn = attempt.number;
        self.last_practiced = attempt.at;
        self.review_requested = false;
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Attempt {
    /// Optional 0–100 grade; old histories and discrete exercises stay binary.
    #[serde(default)]
    pub performance_score: Option<u8>,
    #[serde(default)]
    pub reading: Option<crate::reading::ReadingResult>,
    #[serde(default)]
    pub recognition: Option<crate::recognition::Context>,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub spelled_answer: Vec<String>,
    pub number: u64,
    pub skill: String,
    pub variant: String,
    pub correct: bool,
    pub assisted: bool,
    pub at: u64,
    pub answer: Vec<usize>,
    pub expected: Vec<usize>,
    #[serde(default)]
    pub evidence: Vec<MidiEvidence>,
    #[serde(default)]
    pub bpm: Option<u32>,
    #[serde(default)]
    pub pitch_correct: bool,
    #[serde(default)]
    pub timing_correct: bool,
    #[serde(default = "default_true")]
    pub duration_correct: bool,
}
impl Attempt {
    pub fn successful(&self) -> bool {
        self.performance_score
            .map_or(self.correct, |score| score >= 90)
    }
}
fn default_true() -> bool {
    true
}
pub use crate::evidence::MidiEvidence;
#[derive(Clone, Serialize, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub reading_components: BTreeMap<String, Mastery>,
    #[serde(default)]
    pub recognition: crate::recognition::Recognition,
    pub version: u32,
    pub completed: u64,
    pub skills: BTreeMap<String, Mastery>,
    pub recent_attempts: Vec<Attempt>,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            reading_components: BTreeMap::new(),
            recognition: Default::default(),
            version: 1,
            completed: 0,
            skills: BTreeMap::new(),
            recent_attempts: Vec::new(),
        }
    }
}
impl Profile {
    pub fn insights(&self, graph: &[Skill]) -> Vec<String> {
        let mut insights = self.recognition.insights();
        let mut mastered: Vec<_> = graph
            .iter()
            .filter_map(|skill| {
                if matches!(skill.task, crate::curriculum::Task::HearInterval { .. }) {
                    return None;
                }
                self.skills
                    .get(&skill.id)
                    .filter(|s| s.mastered())
                    .map(|s| (s.last_turn, &skill.title))
            })
            .collect();
        mastered.sort_by_key(|(turn, _)| std::cmp::Reverse(*turn));
        for (_, title) in mastered.into_iter().take(1).rev() {
            insights.insert(0, format!("Mastered: {title}"));
        }
        insights
    }
    pub fn unlocked(&self, skill: &Skill) -> bool {
        // Initial qualification opens a path permanently. Later weakness lowers
        // scores and requests review without repeatedly closing that path.
        self.skills.get(&skill.id).is_some_and(|s| s.attempts > 0)
            || skill.requires.iter().all(|r| {
                self.skills.get(&r.skill).is_some_and(|s| {
                    (s.score >= r.score && s.attempts >= r.attempts)
                        || (r.score <= 7.0 && r.attempts <= 6 && s.first_qualified_at.is_some())
                })
            })
    }
    pub fn next<'a>(&self, graph: &'a [Skill], now: u64) -> Option<(&'a Skill, &'static str)> {
        let recognition = self.recognition.next(self, graph, now);
        if self.completed % 2 == 1 {
            if let Some(candidate) = recognition {
                return Some(candidate);
            }
        }
        let mut candidates = Vec::new();
        let mut new_skills = Vec::new();
        let mut learning = 0;
        let mut unfinished = 0;
        let unpracticed = Mastery::default();
        let last_introduction = self
            .skills
            .values()
            .map(|s| s.introduced_at)
            .max()
            .unwrap_or(0);
        for (index, skill) in graph.iter().enumerate() {
            if matches!(skill.task, crate::curriculum::Task::HearInterval { .. }) {
                continue;
            }
            if !self.unlocked(skill) {
                continue;
            }
            let state = self.skills.get(&skill.id).unwrap_or(&unpracticed);
            if state.attempts == 0 {
                // The first qualification timestamp survives later practice and
                // temporary prerequisite weakness; old profiles use turn zero.
                let ready_at = skill
                    .requires
                    .iter()
                    .filter_map(|r| self.skills.get(&r.skill))
                    .map(|s| s.first_qualified_at.unwrap_or(0))
                    .max()
                    .unwrap_or(0);
                new_skills.push((skill.stage, ready_at, index, skill));
                continue;
            }
            if state.attempts < 6 || state.score < 7.0 {
                learning += 1;
            }
            if !state.mastered() {
                unfinished += 1;
            }
            let due = state.mastered() && now.saturating_sub(state.last_practiced) >= 14 * 86400;
            if state.mastered() && !due && !state.review_requested {
                continue;
            }
            let reason = if state.review_requested {
                "Review after a related mistake"
            } else if due {
                "Retention check"
            } else {
                "Building fluency"
            };
            let priority = if state.review_requested {
                60.0
            } else if due {
                45.0
            } else {
                10.0 - state.score
            };
            // No cap: even a low-priority practice eventually gets its turn.
            let consolidating =
                state.score >= 7.0 && state.attempts >= 6 && !state.review_requested && !due;
            let spacing = self.completed.saturating_sub(state.last_turn) as f32
                * if consolidating { 1.5 } else { 3.0 };
            candidates.push((priority + spacing, index, skill, reason));
        }
        candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        // Admit one new skill per six answers with at most twelve skills still
        // below basic fluency. Consolidation to mastery continues separately;
        // a long success streak must not block all introductions. Also bound
        // the total unfinished pool so consolidation cannot grow indefinitely.
        if candidates.is_empty()
            || (learning < MAX_INITIAL_LEARNING
                && unfinished < MAX_UNFINISHED
                && self.completed.saturating_sub(last_introduction) >= INTRODUCTION_SPACING)
        {
            new_skills.sort_by_key(|(stage, ready, index, _)| (*stage, *ready, *index));
            if let Some((_, _, _, skill)) = new_skills.first() {
                return Some((skill, "New skill unlocked"));
            }
        }
        candidates
            .first()
            .map(|(_, _, skill, reason)| (*skill, *reason))
            .or(recognition)
    }
    pub fn record(&mut self, skill: &Skill, mut attempt: Attempt) {
        self.completed += 1;
        attempt.number = self.completed;
        self.recognition.record(skill, &mut attempt);
        if let Some(reading) = &attempt.reading {
            for (part, correct) in reading.components() {
                let state = self
                    .reading_components
                    .entry(format!("{}:{part}", skill.id))
                    .or_default();
                state.observe(correct, &attempt);
            }
        }
        let state = self.skills.entry(skill.id.clone()).or_default();
        let quality = attempt
            .performance_score
            .map_or(if attempt.correct { 1.0 } else { 0.0 }, |score| {
                f32::from(score.min(100)) / 100.0
            });
        state.observe_quality(attempt.successful(), quality, &attempt);
        let independent = attempt.successful() && !attempt.assisted;
        if !independent && attempt.reading.is_none() {
            for prerequisite in &skill.requires {
                if let Some(state) = self.skills.get_mut(&prerequisite.skill) {
                    state.review_requested = true;
                }
            }
        }
        self.recent_attempts.push(attempt);
    }
}

pub struct Store {
    pub path: PathBuf,
    _lock: File,
}
impl Store {
    pub fn default_path() -> Result<PathBuf, String> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|p| Path::new(p).is_absolute())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|p| PathBuf::from(p).join(".local/share")))
            .ok_or("Set HOME or XDG_DATA_HOME, or pass --profile FILE")?;
        Ok(base.join("midi_keys/learner.json"))
    }
    pub fn open(path: PathBuf) -> Result<(Self, Profile), String> {
        let parent = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::create_dir_all(parent).map_err(|e| format!("Cannot create profile directory: {e}"))?;
        // Append rather than replace the extension: a profile named *.lock
        // must not become its own lock file (atomic saves replace its inode).
        let mut lock_path = path.as_os_str().to_os_string();
        lock_path.push(".lock");
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(PathBuf::from(lock_path))
            .map_err(|e| e.to_string())?;
        lock.try_lock()
            .map_err(|e| format!("Learner profile is already in use or cannot be locked: {e}"))?;
        let profile = match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Profile>(&bytes).map_err(|e| {
                format!(
                    "Cannot read learner profile {}: {e}. The file was not changed.",
                    path.display()
                )
            })?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Profile::default(),
            Err(e) => return Err(format!("Cannot read {}: {e}", path.display())),
        };
        if profile.version != 1 {
            return Err(format!(
                "Unsupported learner profile version {}",
                profile.version
            ));
        }
        if profile
            .skills
            .values()
            .chain(profile.reading_components.values())
            .any(|s| !s.score.is_finite() || !(0.0..=10.0).contains(&s.score))
        {
            return Err("Invalid mastery score in learner profile".into());
        }
        if profile
            .recent_attempts
            .iter()
            .any(|a| a.performance_score.is_some_and(|s| s > 100))
        {
            return Err("Invalid performance score in learner profile".into());
        }
        Ok((Self { path, _lock: lock }, profile))
    }
    pub fn save(&self, profile: &Profile) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        serde_json::to_writer_pretty(&mut temp, profile).map_err(|e| e.to_string())?;
        temp.write_all(b"\n").map_err(|e| e.to_string())?;
        temp.as_file().sync_all().map_err(|e| e.to_string())?;
        temp.persist(&self.path)
            .map_err(|e| format!("Cannot save learner progress: {e}"))?;
        File::open(parent)
            .and_then(|file| file.sync_all())
            .map_err(|e| format!("Cannot sync learner progress: {e}"))?;
        Ok(())
    }
}
