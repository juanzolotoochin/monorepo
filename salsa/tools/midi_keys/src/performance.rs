//! Phrase grading. Simultaneous notes are unordered within each frame, but
//! frames stay ordered. Durations use MIDI key releases, not sustain-pedal audio.
use crate::learner::MidiEvidence;
use std::collections::BTreeSet;
use std::time::Instant;

#[derive(Clone, Debug, serde::Serialize)]
pub struct Frame {
    pub choices: Vec<Vec<usize>>,
    pub at_ms: u64,
    pub hold_ms: Option<u64>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct Performance {
    pub frames: Vec<Frame>,
    /// Bar-by-bar accompaniment: allow detached holds and align corrections by time.
    pub accompaniment: bool,
    pub exact_register: bool,
    /// Grade rhythm only: each attack may use any pitch. Count still matters.
    pub any_pitch: bool,
    pub timed: bool,
    pub fixed_start: bool,
    pub tolerance_ms: u64,
    pub smooth: bool,
    pub preserve_contour: bool,
}
impl Performance {
    pub fn hold_matches(&self, target: u64, held: Option<u64>) -> bool {
        held.is_some_and(|held| {
            if self.accompaniment {
                // Accompaniment alone permits a three-beat hold in a four-beat bar,
                // with the exercise timing tolerance at that release boundary.
                ((target * 3 / 4).saturating_sub(self.tolerance_ms)..=target + target / 8)
                    .contains(&held)
            } else {
                held.abs_diff(target) <= self.tolerance_ms
            }
        })
    }

    /// Match chord attacks to their musical time, so a correction does not
    /// shift every subsequent bar. Extras remain errors without erasing credit.
    pub fn accompaniment_report(
        &self,
        evidence: &[MidiEvidence],
        origin_ms: i64,
    ) -> AccompanimentReport {
        let mut groups: Vec<Vec<usize>> = vec![];
        for (i, on) in evidence.iter().enumerate().filter(|(_, e)| e.velocity > 0) {
            if let Some(group) = groups
                .last_mut()
                .filter(|g| on.offset_ms.saturating_sub(evidence[g[0]].offset_ms) <= 180)
            {
                group.push(i);
            } else {
                groups.push(vec![i]);
            }
        }
        // origin_ms is measured from evidence offset zero, including leading releases.
        let times: Vec<_> = groups
            .iter()
            .map(|g| evidence[g[0]].offset_ms as i64 + origin_ms)
            .collect();
        let targets: Vec<_> = self.frames.iter().map(|f| f.at_ms as i64).collect();
        let mut report = AccompanimentReport {
            total: self.frames.len(),
            ..Default::default()
        };
        for (target, input) in crate::rhythm::align_attacks(&targets, &times, 60) {
            let Some(target) = target else {
                report.extras += 1;
                continue;
            };
            let Some(input) = input else {
                report.issues.push(format!("bar {} missing", target + 1));
                continue;
            };
            let frame = &self.frames[target];
            let group = &groups[input];
            let normalize = |n: usize| if self.exact_register { n } else { n % 12 };
            let mut notes: Vec<_> = group.iter().map(|&i| normalize(evidence[i].note)).collect();
            notes.sort_unstable();
            let pitch = frame.choices.iter().any(|choice| {
                let mut expected: Vec<_> = choice.iter().map(|&n| normalize(n)).collect();
                expected.sort_unstable();
                expected == notes
            });
            let timing = group.iter().all(|&i| {
                (evidence[i].offset_ms as i64 + origin_ms).abs_diff(targets[target])
                    <= self.tolerance_ms
            });
            let holds = frame.hold_ms.is_none_or(|duration| {
                group.iter().all(|&i| {
                    let held = crate::evidence::held_ms(evidence, i);
                    self.hold_matches(duration, held)
                })
            });
            report.pitches += usize::from(pitch);
            report.timing += usize::from(timing);
            report.holds += usize::from(holds);
            let mut issues = vec![];
            if !pitch {
                issues.push("notes");
            }
            if !timing {
                issues.push("timing");
            }
            if !holds {
                issues.push("hold");
            }
            if !issues.is_empty() {
                report
                    .issues
                    .push(format!("bar {}: {}", target + 1, issues.join("/")));
            }
        }
        report
    }
    pub fn note_count(&self) -> usize {
        self.frames.iter().map(|f| f.choices[0].len()).sum()
    }
    pub fn example(&self) -> Vec<usize> {
        self.frames
            .iter()
            .flat_map(|f| f.choices[0].clone())
            .collect()
    }
    pub fn end_ms(&self) -> u64 {
        self.frames
            .iter()
            .map(|f| f.at_ms + f.hold_ms.unwrap_or(250))
            .max()
            .unwrap_or(0)
    }
    pub fn pitch_correct(&self, played: &[usize]) -> bool {
        if played.len() != self.note_count() {
            return false;
        }
        if self.any_pitch {
            return true;
        }
        if self.preserve_contour {
            let expected = self.example();
            if !played
                .iter()
                .zip(&expected)
                .all(|(a, b)| *a as i32 - played[0] as i32 == *b as i32 - expected[0] as i32)
            {
                return false;
            }
        }
        let mut offset = 0;
        let mut previous: Option<Vec<usize>> = None;
        for frame in &self.frames {
            let end = offset + frame.choices[0].len();
            let current = &played[offset..end];
            let normalize = |notes: &[usize]| -> BTreeSet<usize> {
                notes
                    .iter()
                    .map(|n| if self.exact_register { *n } else { n % 12 })
                    .collect()
            };
            if !frame
                .choices
                .iter()
                .any(|choice| normalize(choice) == normalize(current))
            {
                return false;
            }
            let mut sorted = current.to_vec();
            sorted.sort_unstable();
            if self.smooth
                && previous
                    .as_ref()
                    .is_some_and(|prev| prev.iter().zip(&sorted).any(|(a, b)| a.abs_diff(*b) > 5))
            {
                return false;
            }
            previous = Some(sorted);
            offset = end;
        }
        true
    }
    pub fn timing_correct(&self, onsets: &[Instant], start: Option<Instant>) -> bool {
        if onsets.len() != self.note_count() {
            return false;
        }
        let Some(first) = onsets.first().copied() else {
            return false;
        };
        let anchor = if self.fixed_start {
            let Some(at) = start else {
                return false;
            };
            at
        } else {
            first
        };
        let mut offset = 0;
        for frame in &self.frames {
            let end = offset + frame.choices[0].len();
            let group = &onsets[offset..end];
            // A chord must be played together (180 ms spread), not as a melody.
            if group.last().unwrap().duration_since(group[0]).as_millis() > 180 {
                return false;
            }
            if self.timed
                && group.iter().any(|at| {
                    let actual = if *at >= anchor {
                        at.duration_since(anchor).as_millis() as i128
                    } else {
                        -(anchor.duration_since(*at).as_millis() as i128)
                    };
                    (actual - frame.at_ms as i128).abs() > self.tolerance_ms as i128
                })
            {
                return false;
            }
            offset = end;
        }
        true
    }
    pub fn holds_correct(&self, evidence: &[MidiEvidence]) -> bool {
        let ons: Vec<_> = evidence
            .iter()
            .enumerate()
            .filter(|(_, e)| e.velocity > 0)
            .collect();
        if ons.len() != self.note_count() {
            return false;
        }
        let mut offset = 0;
        for frame in &self.frames {
            let end = offset + frame.choices[0].len();
            if let Some(duration) = frame.hold_ms {
                for &(index, _) in &ons[offset..end] {
                    if !self.hold_matches(duration, crate::evidence::held_ms(evidence, index)) {
                        return false;
                    }
                }
            }
            offset = end;
        }
        true
    }
}

#[derive(Default, Debug)]
pub struct AccompanimentReport {
    pub total: usize,
    pub pitches: usize,
    pub timing: usize,
    pub holds: usize,
    pub extras: usize,
    pub issues: Vec<String>,
}
impl AccompanimentReport {
    /// Pitch 60%, attacks 25%, holds 15%; a missing bar loses all three.
    /// An extra chord costs one bar's share. This is independent of the strict
    /// per-event error flags used to annotate the score.
    pub fn score(&self) -> u8 {
        if self.total == 0 {
            return 0;
        }
        let points =
            (60.0 * self.pitches as f64 + 25.0 * self.timing as f64 + 15.0 * self.holds as f64
                - 100.0 * self.extras as f64)
                / self.total as f64;
        let perfect = self.pitches == self.total
            && self.timing == self.total
            && self.holds == self.total
            && self.extras == 0;
        points
            .round()
            .clamp(0.0, if perfect { 100.0 } else { 99.0 }) as u8
    }
    pub fn feedback(&self) -> String {
        let mut details = self.issues.join("; ");
        if self.extras > 0 {
            if !details.is_empty() {
                details.push_str("; ");
            }
            details.push_str(&format!("{} extra chord attack(s)", self.extras));
        }
        format!(
            "Chords {}/{} · on time {}/{} · holds {}/{}. {}",
            self.pitches, self.total, self.timing, self.total, self.holds, self.total, details
        )
    }
}

#[cfg(test)]
mod grade_tests {
    use super::*;
    #[test]
    fn local_mistakes_lose_proportional_points() {
        let mut r = AccompanimentReport {
            total: 12,
            pitches: 12,
            timing: 12,
            holds: 12,
            ..Default::default()
        };
        assert_eq!(r.score(), 100);
        r.holds = 11;
        assert_eq!(r.score(), 99);
        r.holds = 12;
        r.pitches = 11;
        assert_eq!(r.score(), 95);
        r.timing = 11;
        r.holds = 11;
        assert_eq!(r.score(), 92); // One missing bar, with eleven good bars.
        r.extras = 1;
        assert!(r.score() < 92);
        r.extras = 100;
        assert_eq!(r.score(), 0);
        assert_eq!(AccompanimentReport::default().score(), 0);
    }
}
