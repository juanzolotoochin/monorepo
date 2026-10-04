//! Shared musical vocabulary; independent of scheduling and presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scale {
    Major,
    Minor,
    MajorPentatonic,
    MinorPentatonic,
    HarmonicMinor,
    MelodicMinor,
    Dorian,
    Mixolydian,
    Lydian,
    Phrygian,
    Locrian,
    WholeTone,
}
impl Scale {
    pub const ALL: [Self; 12] = [
        Self::Major,
        Self::Minor,
        Self::MajorPentatonic,
        Self::MinorPentatonic,
        Self::HarmonicMinor,
        Self::MelodicMinor,
        Self::Dorian,
        Self::Mixolydian,
        Self::Lydian,
        Self::Phrygian,
        Self::Locrian,
        Self::WholeTone,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "natural-minor",
            Self::MajorPentatonic => "major-pentatonic",
            Self::MinorPentatonic => "minor-pentatonic",
            Self::HarmonicMinor => "harmonic-minor",
            Self::MelodicMinor => "jazz-melodic-minor",
            Self::Dorian => "dorian",
            Self::Mixolydian => "mixolydian",
            Self::Lydian => "lydian",
            Self::Phrygian => "phrygian",
            Self::Locrian => "locrian",
            Self::WholeTone => "whole-tone",
        }
    }
    pub fn steps(self) -> &'static [usize] {
        match self {
            Self::Major => &[0, 2, 4, 5, 7, 9, 11],
            Self::Minor => &[0, 2, 3, 5, 7, 8, 10],
            Self::MajorPentatonic => &[0, 2, 4, 7, 9],
            Self::MinorPentatonic => &[0, 3, 5, 7, 10],
            Self::HarmonicMinor => &[0, 2, 3, 5, 7, 8, 11],
            Self::MelodicMinor => &[0, 2, 3, 5, 7, 9, 11],
            Self::Dorian => &[0, 2, 3, 5, 7, 9, 10],
            Self::Mixolydian => &[0, 2, 4, 5, 7, 9, 10],
            Self::Lydian => &[0, 2, 4, 6, 7, 9, 11],
            Self::Phrygian => &[0, 1, 3, 5, 7, 8, 10],
            Self::Locrian => &[0, 1, 3, 5, 6, 8, 10],
            Self::WholeTone => &[0, 2, 4, 6, 8, 10],
        }
    }
    pub fn letters(self) -> &'static [usize] {
        match self {
            Self::MajorPentatonic => &[0, 1, 2, 4, 5],
            Self::MinorPentatonic => &[0, 2, 3, 4, 6],
            Self::WholeTone => &[0, 1, 2, 3, 4, 5],
            _ => &[0, 1, 2, 3, 4, 5, 6],
        }
    }
    pub fn parent(self) -> Self {
        match self {
            Self::Minor | Self::Major => self,
            Self::MinorPentatonic | Self::HarmonicMinor | Self::Dorian | Self::Phrygian => {
                Self::Minor
            }
            Self::MelodicMinor => Self::HarmonicMinor,
            Self::Locrian => Self::Phrygian,
            Self::WholeTone => Self::Lydian,
            _ => Self::Major,
        }
    }
    pub fn triad(self, root: usize, degree: usize) -> Vec<usize> {
        let steps = self.steps();
        [degree, degree + 2, degree + 4]
            .into_iter()
            .map(|i| root + steps[i % steps.len()] + 12 * (i / steps.len()))
            .collect()
    }
}
