//! Presentation evidence derived from the same clock and frames as grading.
use crate::{
    exercise::{Answer, Exercise},
    learner::MidiEvidence,
};
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct RhythmScore {
    pub bpm: u32,
    pub tolerance_ms: u64,
    pub notes: Vec<(u64, Option<u64>)>,
    pub quarter_notes: bool,
}
#[derive(Clone, Debug)]
pub struct RhythmRow {
    pub expected_ms: Option<u64>,
    pub expected_hold_ms: Option<u64>,
    pub actual_ms: Option<i64>,
    pub actual_hold_ms: Option<u64>,
    pub note: Option<usize>,
}
#[derive(Clone, Debug)]
pub struct RhythmReport {
    pub rows: Vec<RhythmRow>,
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
