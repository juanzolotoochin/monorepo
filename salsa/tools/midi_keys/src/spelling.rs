//! Display spelling follows the written task. MIDI grading remains pitch based.
use crate::curriculum::{Direction, Task, CHORDS, ROOTS};

const NATURALS: [i32; 7] = [0, 2, 4, 5, 7, 9, 11];
const LETTERS: [char; 7] = ['C', 'D', 'E', 'F', 'G', 'A', 'B'];

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
struct SpelledNote {
    letter: usize,
    accidental: i32,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct NoteSpelling([SpelledNote; 12]);

impl Default for NoteSpelling {
    fn default() -> Self {
        Self(std::array::from_fn(|pc| {
            let letter = NATURALS.iter().rposition(|&n| n <= pc as i32).unwrap();
            SpelledNote {
                letter,
                accidental: pc as i32 - NATURALS[letter],
            }
        }))
    }
}

impl NoteSpelling {
    fn root_letter(root: usize) -> usize {
        LETTERS
            .iter()
            .position(|&letter| ROOTS[root].starts_with(letter))
            .unwrap()
    }

    fn set(&mut self, pitch: i32, letter: usize) {
        let pc = pitch.rem_euclid(12) as usize;
        let accidental = (pitch - NATURALS[letter] + 6).rem_euclid(12) - 6;
        self.0[pc] = SpelledNote { letter, accidental };
    }

    pub(crate) fn for_task(task: &Task, interval_root: usize) -> Self {
        let mut names = Self::default();
        match *task {
            Task::GuidedScale(ref task) => return Self::for_task(task, interval_root),
            Task::Reading(_) => {}
            Task::Practice(ref practice) => return practice.spelling(),
            Task::BuildInterval {
                semitones,
                direction,
            } => {
                let root = interval_root % 12;
                let letter = Self::root_letter(root);
                let steps = [1, 1, 2, 2, 3, 3, 4, 5, 5, 6, 6, 7][semitones - 1];
                let sign = if direction == Direction::Down { -1 } else { 1 };
                names.set(root as i32, letter);
                names.set(
                    root as i32 + sign * semitones as i32,
                    (letter as i32 + sign * steps).rem_euclid(7) as usize,
                );
            }
            Task::Scale { root, minor, .. } | Task::TimedScale { root, minor, .. } => {
                names.scale(root, minor);
            }
            Task::ContextInterval { root, .. } => names.scale(root, false),
            Task::BuildChord { root, quality }
            | Task::Inversion {
                root,
                quality,
                hearing: false,
                ..
            } => {
                let letter = Self::root_letter(root);
                for (degree, semitones) in CHORDS[quality].iter().enumerate() {
                    names.set((root + semitones) as i32, (letter + degree * 2) % 7);
                }
            }
            // An unseen listening answer must not influence displayed spelling.
            Task::HearInterval { .. }
            | Task::HearChord { .. }
            | Task::Inversion { hearing: true, .. } => {}
        }
        names
    }

    fn scale(&mut self, root: usize, minor: bool) {
        let degrees = if minor {
            [0, 2, 3, 5, 7, 8, 10]
        } else {
            [0, 2, 4, 5, 7, 9, 11]
        };
        let letter = Self::root_letter(root);
        for (degree, semitones) in degrees.into_iter().enumerate() {
            self.set((root + semitones) as i32, (letter + degree) % 7);
        }
    }
    pub(crate) fn for_scale(root: usize, scale: crate::music::Scale) -> Self {
        let mut names = Self::default();
        let letter = Self::root_letter(root);
        for (&semitones, &degree) in scale.steps().iter().zip(scale.letters()) {
            names.set((root + semitones) as i32, (letter + degree) % 7);
        }
        names
    }

    pub fn staff_position(&self, midi: usize) -> (i32, i8) {
        let note = self.0[midi % 12];
        let octave = (midi as i32 - NATURALS[note.letter] - note.accidental).div_euclid(12);
        (octave * 7 + note.letter as i32, note.accidental as i8)
    }
    pub fn note_name(&self, midi: usize) -> String {
        let note = self.0[midi % 12];
        let accidental = if note.accidental < 0 { "b" } else { "#" }
            .repeat(note.accidental.unsigned_abs() as usize);
        // B# and Cb cross the MIDI octave boundary in written notation.
        let octave = (midi as i32 - NATURALS[note.letter] - note.accidental).div_euclid(12) - 1;
        format!("{}{accidental}{octave}", LETTERS[note.letter])
    }
}
