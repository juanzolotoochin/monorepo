//! Presentation evidence derived from the same clock and frames as grading.
use crate::{
    exercise::{Answer, Exercise},
    learner::MidiEvidence,
};
use std::time::Instant;

#[derive(Clone, Debug, serde::Serialize)]
pub struct RhythmScore {
    pub bpm: u32,
    pub tolerance_ms: u64,
    pub notes: Vec<(u64, Option<u64>)>,
    pub quarter_notes: bool,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct RhythmRow {
    pub expected_ms: Option<u64>,
    pub expected_hold_ms: Option<u64>,
    pub actual_ms: Option<i64>,
    pub actual_hold_ms: Option<u64>,
    pub note: Option<usize>,
}
#[derive(Clone, Debug, serde::Serialize)]
pub struct RhythmReport {
    pub rows: Vec<RhythmRow>,
}
impl RhythmReport {
    pub fn count_correct(&self) -> bool {
        !self.rows.is_empty()
            && self
                .rows
                .iter()
                .all(|r| r.expected_ms.is_some() && r.actual_ms.is_some())
    }
    pub fn timing_correct(&self, tolerance: u64) -> bool {
        self.count_correct()
            && self
                .rows
                .iter()
                .all(|r| r.actual_ms.unwrap().abs_diff(r.expected_ms.unwrap() as i64) <= tolerance)
    }
    pub fn holds_correct(&self, tolerance: u64) -> bool {
        self.rows.iter().all(|r| {
            r.expected_hold_ms.is_none_or(|expected| {
                r.actual_hold_ms
                    .is_some_and(|actual| actual.abs_diff(expected) <= tolerance)
            })
        })
    }
    /// 70% attack accuracy, 30% hold accuracy. Untested holds contribute no weight.
    /// Beyond the tolerance, credit decreases continuously over one beat.
    /// Missing/extra attacks cost one complete note; imperfect results cap at 99.
    pub fn grade(&self, score: &RhythmScore) -> u8 {
        if score.notes.is_empty() {
            return 0;
        }
        let beat = 60000.0 / f64::from(score.bpm);
        let credit = |delta: u64| {
            (1.0 - delta.saturating_sub(score.tolerance_ms) as f64 / beat).clamp(0.0, 1.0)
        };
        let mut earned = 0.0;
        for row in &self.rows {
            match (row.expected_ms, row.actual_ms) {
                (Some(expected), Some(actual)) => {
                    let attack = credit(actual.abs_diff(expected as i64));
                    earned += if let Some(held) = row.expected_hold_ms {
                        0.7 * attack
                            + 0.3
                                * row
                                    .actual_hold_ms
                                    .map_or(0.0, |actual| credit(actual.abs_diff(held)))
                    } else {
                        attack
                    };
                }
                (None, Some(_)) => earned -= 1.0,
                _ => {}
            }
        }
        let ceiling = if self.rows.iter().all(|r| r.correct(score.tolerance_ms)) {
            100.0
        } else {
            99.0
        };
        (100.0 * earned / score.notes.len() as f64)
            .round()
            .clamp(0.0, ceiling) as u8
    }
    pub fn notation(&self, score: &RhythmScore) -> crate::WrittenScore {
        let expected = score.notation();
        let mut played = expected.clone();
        played.events.clear();
        for row in &self.rows {
            let expected_event = row
                .expected_ms
                .and_then(|ms| score.notes.iter().position(|n| n.0 == ms))
                .and_then(|i| {
                    expected
                        .events
                        .iter()
                        .filter(|e| !e.notes().is_empty())
                        .nth(i)
                });
            let at = row
                .actual_ms
                .unwrap_or(row.expected_ms.unwrap_or(0) as i64)
                .max(0) as u64;
            let correct = row.correct(score.tolerance_ms);
            let tick = if correct {
                expected_event.map_or_else(|| ticks(at, score.bpm), |e| e.tick)
            } else {
                ticks(at, score.bpm)
            };
            let duration = if correct || row.actual_ms.is_none() {
                expected_event.map_or(2, |e| e.duration)
            } else {
                row.actual_hold_ms
                    .map_or(1, |held| ticks(held, score.bpm).max(1))
            };
            played.events.push(crate::WrittenEvent {
                incorrect: !correct,
                hand: crate::Hand::Right,
                tick,
                duration,
                tie_tick: None,
                choices: row
                    .actual_ms
                    .map(|_| vec![vec![row.note.unwrap_or(60)]])
                    .unwrap_or_default(),
            });
        }
        played.ticks = played
            .ticks
            .max(
                played
                    .events
                    .iter()
                    .map(|e| e.tick + e.duration)
                    .max()
                    .unwrap_or(0),
            )
            .div_ceil(8)
            * 8;
        played.bars_per_page = played.ticks.div_ceil(8).max(1);
        crate::score_support::fill_rests(&mut played);
        played
    }
}
fn ticks(ms: u64, bpm: u32) -> u32 {
    ((ms as f64 * f64::from(bpm) / 30000.0).round() as u32).min(4096)
}
impl RhythmScore {
    pub fn notation(&self) -> crate::WrittenScore {
        let short = self
            .notes
            .windows(2)
            .map(|n| n[1].0.saturating_sub(n[0].0))
            .min()
            .unwrap_or(60000 / u64::from(self.bpm))
            .min(60000 / u64::from(self.bpm));
        let events: Vec<_> = self
            .notes
            .iter()
            .map(|&(at, hold)| crate::WrittenEvent {
                incorrect: false,
                hand: crate::Hand::Right,
                tick: ticks(at, self.bpm),
                duration: ticks(hold.unwrap_or(short), self.bpm).max(1),
                tie_tick: None,
                choices: vec![vec![60]],
            })
            .collect();
        let length = events
            .iter()
            .map(|e| e.tick + e.duration)
            .max()
            .unwrap_or(8)
            .div_ceil(8)
            * 8;
        let mut score = crate::WrittenScore {
            active_tick: None, bars_per_page: length.div_ceil(8).max(1), written_spelling: None,
            caption: "Rhythm only; pitches are unrestricted. Red marks attack or hold errors; exact timing is in the table.".into(),
            events, chords: vec![], hands: vec![crate::Hand::Right], key_fifths: 0,
            ticks: length, bpm: Some(self.bpm), lead: false, symbols_only: false,
        };
        crate::score_support::fill_rests(&mut score);
        score
    }
}

impl Exercise {
    pub fn rhythm_score(&self) -> Option<RhythmScore> {
        let Answer::Performance(score) = &self.answer else {
            return None;
        };
        if !score.any_pitch || !score.timed {
            return None;
        }
        Some(RhythmScore {
            bpm: self.bpm?,
            tolerance_ms: score.tolerance_ms,
            notes: score.frames.iter().map(|f| (f.at_ms, f.hold_ms)).collect(),
            quarter_notes: self.pulse_backing,
        })
    }
    pub fn rhythm_report(
        &self,
        onsets: &[Instant],
        start: Option<Instant>,
        evidence: &[MidiEvidence],
    ) -> Option<RhythmReport> {
        let score = self.rhythm_score()?;
        let anchor = self.performance_anchor(onsets, start);
        let attacks: Vec<_> = evidence
            .iter()
            .enumerate()
            .filter(|(_, e)| e.velocity > 0)
            .collect();
        let actual: Vec<_> = attacks
            .iter()
            .zip(onsets)
            .map(|(&(index, on), &at)| {
                let ms = anchor
                    .map(|anchor| {
                        if at >= anchor {
                            at.duration_since(anchor).as_millis() as i64
                        } else {
                            -(anchor.duration_since(at).as_millis() as i64)
                        }
                    })
                    .unwrap_or(0);
                let held = crate::evidence::held_ms(evidence, index);
                (ms, held, on.note)
            })
            .collect();
        let expected_times: Vec<_> = score.notes.iter().map(|n| n.0 as i64).collect();
        let actual_times: Vec<_> = actual.iter().map(|a| a.0).collect();
        let rows = align_attacks(&expected_times, &actual_times, score.bpm)
            .into_iter()
            .map(|(expected, played)| {
                let expected = expected.map(|i| score.notes[i]);
                let played = played.map(|i| actual[i]);
                RhythmRow {
                    expected_ms: expected.map(|e| e.0),
                    expected_hold_ms: expected.and_then(|e| e.1),
                    actual_ms: played.map(|a| a.0),
                    actual_hold_ms: played.and_then(|a| a.1),
                    note: played.map(|a| a.2),
                }
            })
            .collect();
        Some(RhythmReport { rows })
    }
}
impl RhythmRow {
    pub fn correct(&self, tolerance: u64) -> bool {
        matches!((self.expected_ms, self.actual_ms), (Some(e), Some(a)) if (a - e as i64).unsigned_abs() <= tolerance)
            && self.expected_hold_ms.is_none_or(|e| {
                self.actual_hold_ms
                    .is_some_and(|a| a.abs_diff(e) <= tolerance)
            })
    }
    pub fn timing_label(&self) -> String {
        match (self.expected_ms, self.actual_ms) {
            (Some(e), Some(a)) => {
                let delta = a - e as i64;
                if delta == 0 {
                    "on time".into()
                } else {
                    format!(
                        "{}ms {}",
                        delta.unsigned_abs(),
                        if delta < 0 { "early" } else { "late" }
                    )
                }
            }
            (Some(_), None) => "MISSING".into(),
            (None, Some(_)) => "EXTRA".into(),
            _ => String::new(),
        }
    }
}

/// Align attacks by time, explicitly retaining missing and extra events so one
/// mistake cannot shift every subsequent comparison. Indices refer to inputs.
pub(crate) fn align_attacks(
    expected: &[i64],
    actual: &[i64],
    bpm: u32,
) -> Vec<(Option<usize>, Option<usize>)> {
    let (n, m) = (expected.len(), actual.len());
    let gap = 60000.0 / f64::from(bpm) * 0.55;
    let mut cost = vec![vec![0.0; m + 1]; n + 1];
    for (i, row) in cost.iter_mut().enumerate() {
        row[0] = i as f64 * gap;
    }
    for j in 0..=m {
        cost[0][j] = j as f64 * gap;
    }
    for i in 1..=n {
        for j in 1..=m {
            let delta = expected[i - 1].abs_diff(actual[j - 1]) as f64;
            cost[i][j] = (cost[i - 1][j - 1] + delta)
                .min(cost[i - 1][j] + gap)
                .min(cost[i][j - 1] + gap);
        }
    }
    let (mut i, mut j) = (n, m);
    let mut rows = Vec::new();
    while i > 0 || j > 0 {
        let pair = i > 0
            && j > 0
            && (cost[i][j] - (cost[i - 1][j - 1] + expected[i - 1].abs_diff(actual[j - 1]) as f64))
                .abs()
                < 0.001;
        let expected = if pair || (i > 0 && (j == 0 || cost[i - 1][j] <= cost[i][j - 1])) {
            i -= 1;
            Some(i)
        } else {
            None
        };
        let actual = if pair || expected.is_none() {
            j -= 1;
            Some(j)
        } else {
            None
        };
        rows.push((expected, actual));
    }
    rows.reverse();
    rows
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pulse() -> RhythmScore {
        RhythmScore {
            bpm: 60,
            tolerance_ms: 200,
            notes: (0..8).map(|n| (n * 1000, Some(940))).collect(),
            quarter_notes: true,
        }
    }
    fn perfect() -> RhythmReport {
        RhythmReport {
            rows: (0..8)
                .map(|n| RhythmRow {
                    expected_ms: Some(n * 1000),
                    expected_hold_ms: Some(940),
                    actual_ms: Some(n as i64 * 1000),
                    actual_hold_ms: Some(940),
                    note: Some(60),
                })
                .collect(),
        }
    }
    #[test]
    fn tiny_errors_lose_little_credit_and_annotations_use_the_same_tolerance() {
        let score = pulse();
        let mut report = perfect();
        report.rows[5].actual_hold_ms = Some(1144);
        assert_eq!(report.grade(&score), 99);
        assert!(!report.holds_correct(200));
        let played = report.notation(&score);
        let bad: Vec<_> = played.events.iter().filter(|e| e.incorrect).collect();
        assert_eq!(bad.len(), 1);
        assert_eq!(bad[0].tick, 10);
        report.rows[5].actual_hold_ms = Some(1140);
        assert_eq!(report.grade(&score), 100);
        assert!(report.notation(&score).events.iter().all(|e| !e.incorrect));
        report.rows[5].actual_hold_ms = Some(940);
        report.rows[5].actual_ms = Some(5204);
        assert_eq!(report.grade(&score), 99);
    }
    #[test]
    fn missing_extra_and_unreleased_notes_cost_credit_without_poisoning_following_notes() {
        let score = pulse();
        let mut report = perfect();
        report.rows[3].actual_ms = None;
        report.rows[3].actual_hold_ms = None;
        assert_eq!(report.grade(&score), 88);
        let played = report.notation(&score);
        assert!(played
            .events
            .iter()
            .any(|e| e.tick == 6 && e.notes().is_empty() && e.incorrect));
        assert!(played.events.iter().any(|e| e.tick == 8 && !e.incorrect));
        let mut report = perfect();
        report.rows.push(RhythmRow {
            expected_ms: None,
            expected_hold_ms: None,
            actual_ms: Some(3500),
            actual_hold_ms: Some(100),
            note: Some(60),
        });
        assert_eq!(report.grade(&score), 88);
        assert!(report
            .notation(&score)
            .events
            .iter()
            .any(|e| e.tick == 7 && e.incorrect));
        let mut report = perfect();
        report.rows[3].actual_hold_ms = None;
        assert_eq!(report.grade(&score), 96);
        for row in &mut report.rows {
            row.actual_ms = None;
            row.actual_hold_ms = None;
        }
        assert_eq!(report.grade(&score), 0);
    }
}
