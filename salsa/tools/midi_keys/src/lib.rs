#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Note {
        channel: usize,
        note: usize,
        velocity: u8,
    },
    ClearChannel(usize),
    Reset,
}

impl Event {
    /// Decode the channel messages used by the keyboard, including MIDI's
    /// velocity-zero note-on convention. Ignore invalid data and other events.
    pub fn from_midi(status: u8, key: u8, value: u8) -> Option<Self> {
        if status == 0xff {
            return Some(Self::Reset);
        }
        if key > 127 || value > 127 {
            return None;
        }
        let channel = (status & 0x0f) as usize;
        match status & 0xf0 {
            0x80 | 0x90 => Some(Self::Note {
                channel,
                note: key as usize,
                velocity: if status & 0xf0 == 0x90 { value } else { 0 },
            }),
            0xb0 if matches!(key, 120 | 123) => Some(Self::ClearChannel(channel)),
            _ => None,
        }
    }
}

#[derive(Default)]
pub struct InputSelection {
    pub source: Option<(i32, i32)>,
}

impl InputSelection {
    /// Select only on a positive-velocity note, then isolate that source.
    pub fn accepts(&mut self, source: (i32, i32), event: Event) -> bool {
        if self.source.is_none()
            && matches!(
                event,
                Event::Note {
                    velocity: 1..=127,
                    ..
                }
            )
        {
            self.source = Some(source);
        }
        self.source == Some(source)
    }
}

pub fn note_name(note: usize) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!("{}{}", NAMES[note % 12], note as i32 / 12 - 1)
}

pub struct Keyboard {
    velocities: [[u8; 128]; 16],
    pub start: usize,
    pub follow: bool,
    pub last: String,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            velocities: [[0; 128]; 16],
            start: 48,
            follow: true,
            last: "Waiting for a note...".into(),
        }
    }
}

impl Keyboard {
    pub fn apply(&mut self, event: Event) {
        match event {
            Event::Note {
                channel,
                note,
                velocity,
            } if channel < 16 && note < 128 && velocity < 128 => {
                self.velocities[channel][note] = velocity;
                self.last = format!(
                    "{}  MIDI {}  ch {}  velocity {}  {}",
                    note_name(note),
                    note,
                    channel + 1,
                    velocity,
                    if velocity > 0 { "ON" } else { "OFF" }
                );
                if velocity > 0 && self.follow && (note < self.start || note > self.start + 24) {
                    self.start = (note / 12 * 12).min(108);
                }
            }
            Event::ClearChannel(channel) if channel < 16 => self.velocities[channel].fill(0),
            Event::Reset => self.clear(),
            _ => {}
        }
    }

    pub fn clear(&mut self) {
        self.velocities = [[0; 128]; 16];
        self.last = "Notes cleared".into();
    }

    pub fn velocity(&self, note: usize) -> u8 {
        self.velocities
            .iter()
            .map(|channel| channel[note])
            .max()
            .unwrap_or(0)
    }

    pub fn shift(&mut self, up: bool) {
        self.follow = false;
        self.start = if up {
            (self.start + 12).min(108)
        } else {
            self.start.saturating_sub(12)
        };
    }

    /// ASCII-only geometry, with optional ANSI color layered onto held keys.
    pub fn render(&self, source: &str, color: bool) -> String {
        let end = (self.start + 24).min(127);
        let white: Vec<usize> = (self.start..=end)
            .filter(|note| !matches!(note % 12, 1 | 3 | 6 | 8 | 10))
            .collect();
        let width = white.len() * 4 + 1;
        let mut grid = vec![vec![(' ', false); width]; 7];
        for (index, &note) in white.iter().enumerate() {
            let x = index * 4;
            let held = self.velocity(note) > 0;
            for row in &mut grid {
                row[x] = ('|', false);
                row[x + 4] = ('|', false);
            }
            for col in x + 1..x + 4 {
                grid[0][col] = ('_', false);
                grid[6][col] = ('_', held);
            }
            for ch in 0..3 {
                grid[4][x + 1 + ch] = (if held { '*' } else { ' ' }, held);
            }
            let label = format!("{:^3}", note_name(note));
            for (offset, ch) in label.chars().enumerate() {
                grid[5][x + 1 + offset] = (ch, held);
            }
        }
        for (index, &note) in white.iter().enumerate() {
            let x = index * 4;
            if note < end && matches!((note + 1) % 12, 1 | 3 | 6 | 8 | 10) {
                let black_held = self.velocity(note + 1) > 0;
                for row in grid.iter_mut().take(4).skip(1) {
                    for cell in &mut row[x + 3..=x + 5] {
                        *cell = (if black_held { '*' } else { '#' }, black_held);
                    }
                }
            }
        }
        let mut out = format!(
            "MIDI KEYS\n{source}\n\n{} - {} | {} | * pressed, # black key\n",
            note_name(self.start),
            note_name(end),
            if self.follow { "follow" } else { "manual" }
        );
        for row in grid {
            for (ch, held) in row {
                if color && held {
                    out.push_str("\x1b[1;30;46m");
                }
                out.push(ch);
                if color && held {
                    out.push_str("\x1b[0m");
                }
            }
            out.push('\n');
        }
        out.push_str(&format!("\nLast: {}\n", self.last));
        let held: Vec<String> = (0..128)
            .filter(|&n| self.velocity(n) > 0)
            .map(|n| format!("{}({})", note_name(n), self.velocity(n)))
            .collect();
        if held.is_empty() {
            out.push_str("Held: --\n");
        } else {
            for (index, chunk) in held.chunks(5).take(3).enumerate() {
                out.push_str(&format!(
                    "{}{}\n",
                    if index == 0 { "Held: " } else { "      " },
                    chunk.join("  ")
                ));
            }
            if held.len() > 15 {
                out.push_str(&format!("      +{} more notes\n", held.len() - 15));
            }
        }
        out.push_str("\n[ / ] octave   f follow   space clear   q / Ctrl-C quit\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_ignores_releases_and_controls_and_preserves_first_note() {
        let mut selection = InputSelection::default();
        let mut keyboard = Keyboard::default();
        for event in [
            Event::Reset,
            Event::ClearChannel(0),
            Event::from_midi(0x90, 60, 0).unwrap(),
            Event::from_midi(0x80, 60, 64).unwrap(),
        ] {
            assert!(!selection.accepts((14, 0), event));
            assert_eq!(selection.source, None);
        }
        let first = Event::from_midi(0x90, 60, 90).unwrap();
        assert!(selection.accepts((32, 0), first));
        keyboard.apply(first);
        assert_eq!(keyboard.velocity(60), 90);
        for source in [(14, 0), (32, 1)] {
            assert!(!selection.accepts(source, first));
            assert!(!selection.accepts(source, Event::Reset));
        }
        let release = Event::from_midi(0x80, 60, 0).unwrap();
        assert!(selection.accepts((32, 0), release));
        keyboard.apply(release);
        assert_eq!(keyboard.velocity(60), 0);
        assert_eq!(selection.source, Some((32, 0)));
    }

    #[test]
    fn chords_and_zero_velocity_release() {
        let mut keyboard = Keyboard::default();
        keyboard.apply(Event::from_midi(0x90, 60, 98).unwrap());
        keyboard.apply(Event::from_midi(0x90, 64, 75).unwrap());
        assert_eq!(keyboard.velocity(60), 98);
        assert_eq!(keyboard.velocity(64), 75);
        keyboard.apply(Event::from_midi(0x90, 60, 0).unwrap());
        assert_eq!(keyboard.velocity(60), 0);
        keyboard.apply(Event::from_midi(0x80, 64, 40).unwrap());
        assert_eq!(keyboard.velocity(64), 0);
    }

    #[test]
    fn channels_release_independently_and_panic_controls_clear() {
        let mut keyboard = Keyboard::default();
        for (channel, velocity) in [(0, 80), (1, 100), (0, 0)] {
            keyboard.apply(Event::Note {
                channel,
                note: 60,
                velocity,
            });
        }
        assert_eq!(keyboard.velocity(60), 100);
        keyboard.apply(Event::from_midi(0xb1, 123, 0).unwrap());
        assert_eq!(keyboard.velocity(60), 0);
        assert_eq!(Event::from_midi(0xb0, 120, 0), Some(Event::ClearChannel(0)));
        assert_eq!(Event::from_midi(0xff, 0, 0), Some(Event::Reset));
    }

    #[test]
    fn ignores_unrelated_and_malformed_messages() {
        for (status, key, value) in [
            (0xf8, 0, 0),
            (0xfe, 0, 0),
            (0x90, 128, 2),
            (0x90, 60, 255),
            (0xb0, 64, 127),
            (0xe0, 0, 64),
        ] {
            assert_eq!(Event::from_midi(status, key, value), None);
        }
    }

    #[test]
    fn follows_full_midi_range_and_respects_manual_view() {
        let mut keyboard = Keyboard::default();
        for note in 0..128 {
            keyboard.apply(Event::Note {
                channel: 0,
                note,
                velocity: 90,
            });
            assert!(note >= keyboard.start && note <= keyboard.start + 24);
            assert!(keyboard.render("test", false).contains(&note_name(note)));
            assert!(keyboard
                .render("test", false)
                .lines()
                .all(|line| line.len() < 64));
        }
        keyboard.shift(false);
        let start = keyboard.start;
        keyboard.apply(Event::Note {
            channel: 0,
            note: 0,
            velocity: 90,
        });
        assert_eq!(keyboard.start, start);
        assert_eq!(note_name(60), "C4");
        assert_eq!(note_name(0), "C-1");
        assert_eq!(note_name(127), "G9");
    }

    #[test]
    fn shows_white_and_black_keys_and_clears_highlights() {
        let mut keyboard = Keyboard::default();
        for note in [60, 61, 64] {
            keyboard.apply(Event::Note {
                channel: 0,
                note,
                velocity: 90,
            });
        }
        let frame = keyboard.render("demo", false);
        assert!(frame.contains("***"));
        assert!(frame.contains("C4(90)  C#4(90)  E4(90)"));
        assert!(!frame.contains('\x1b'));
        keyboard.clear();
        assert!(keyboard.render("demo", false).contains("Held: --"));
        assert!(!keyboard.render("demo", false).contains("***"));
    }
}
