//! Phrase grading. Simultaneous notes are unordered within each frame, but
//! frames stay ordered. Durations use MIDI key releases, not sustain-pedal audio.
use crate::learner::MidiEvidence;
use std::collections::BTreeSet;
use std::time::Instant;

#[derive(Clone, Debug)]
pub struct Frame {
    pub choices: Vec<Vec<usize>>,
    pub at_ms: u64,
    pub hold_ms: Option<u64>,
}
#[derive(Clone, Debug)]
pub struct Performance {
    pub frames: Vec<Frame>,
    pub exact_register: bool,
    pub any_pitch: bool,
    pub timed: bool,
    pub fixed_start: bool,
    pub tolerance_ms: u64,
    pub smooth: bool,
    pub preserve_contour: bool,
}
impl Performance {
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
            return played.iter().all(|n| *n == played[0]);
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
                for &(index, on) in &ons[offset..end] {
                    let release = evidence[index + 1..]
                        .iter()
                        .find(|e| e.channel == on.channel && e.note == on.note);
                    let Some(off) = release.filter(|e| e.velocity == 0) else {
                        return false;
                    };
                    if off
                        .offset_ms
                        .saturating_sub(on.offset_ms)
                        .abs_diff(duration)
                        > self.tolerance_ms
                    {
                        return false;
                    }
                }
            }
            offset = end;
        }
        true
    }
}
