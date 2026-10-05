//! Spoken words mapped to MIDI notes in a small, embedded SoundFont.
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::io::Cursor;
use std::sync::Arc;

pub struct Voice {
    synth: Synthesizer,
}
impl Voice {
    pub fn load() -> Result<Self, String> {
        let font = SoundFont::new(&mut Cursor::new(count_in_data::SOUNDFONT))
            .map_err(|e| format!("Cannot read count-in SoundFont: {e}"))?;
        let mut settings = SynthesizerSettings::new(48000);
        settings.maximum_polyphony = 8;
        settings.block_size = 64;
        settings.enable_reverb_and_chorus = false;
        let synth = Synthesizer::new(&Arc::new(font), &settings).map_err(|e| e.to_string())?;
        Ok(Self { synth })
    }
    pub fn start(&mut self, number: u8) {
        self.stop();
        if (1..=4).contains(&number) {
            // The font's root key equals its trigger key, preserving speech pitch.
            self.synth
                .process_midi_message(0, 0x90, 59 + i32::from(number), 127);
        }
    }
    pub fn stop(&mut self) {
        self.synth.note_off_all(true);
    }
    pub fn volume(&mut self, value: f32) {
        self.synth.set_master_volume(value.clamp(0.0, 1.0));
    }
    pub fn render(&mut self, left: &mut [f32], right: &mut [f32]) {
        self.synth.render(left, right);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn each_midi_key_plays_a_distinct_word_and_stops_cleanly() {
        let mut voice = Voice::load().unwrap();
        voice.volume(1.0);
        let mut left = vec![0.0; 48000];
        let mut right = left.clone();
        let mut words = vec![];
        for n in 1..=4 {
            voice.start(n);
            voice.render(&mut left, &mut right);
            assert!(left.iter().any(|v| v.abs() > 0.05));
            assert!(left[46000..].iter().all(|v| v.abs() < 0.0001));
            assert!(
                left[..1920].iter().any(|v| v.abs() > 0.001),
                "Leading silence on word {n}"
            );
            words.push(left.clone());
            voice.start(n);
            voice.stop();
            // Drain a possible partial internal synth block before testing silence.
            voice.render(&mut left, &mut right);
            assert!(left[64..].iter().all(|v| v.abs() < 0.0001));
        }
        for i in 0..4 {
            for j in 0..i {
                assert_ne!(words[i], words[j]);
            }
        }
        voice.start(0);
        voice.render(&mut left, &mut right);
        assert!(left.iter().all(|v| *v == 0.0));
        voice.volume(0.0);
        voice.start(4);
        voice.render(&mut left, &mut right);
        assert!(left.iter().all(|v| *v == 0.0));
    }
}
