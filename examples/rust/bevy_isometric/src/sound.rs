//! Small procedural sound bank backed by the repository's hermetic miniaudio.
use bevy::prelude::*;

unsafe extern "C" {
    fn kestrel_audio_start() -> i32;
    fn kestrel_audio_play(cue: i32);
    fn kestrel_audio_stop();
}
#[derive(Clone, Copy)]
pub enum Cue {
    Swing = 0,
    Clang = 1,
    Step = 2,
}
#[derive(Resource)]
pub struct Sound {
    pub muted: bool,
    active: bool,
}
impl Sound {
    #[cfg(test)]
    pub fn silent() -> Self {
        Self {
            muted: true,
            active: false,
        }
    }
    pub fn new() -> Self {
        // The C backend owns its mixer and joins its device thread on shutdown.
        let active = unsafe { kestrel_audio_start() == 0 };
        if !active {
            eprintln!("Audio device unavailable; continuing without sound.");
        }
        Self {
            muted: !active,
            active,
        }
    }
    pub fn play(&self, cue: Cue) {
        if self.active && !self.muted {
            unsafe {
                kestrel_audio_play(cue as i32);
            }
        }
    }
}
impl Drop for Sound {
    fn drop(&mut self) {
        if self.active {
            unsafe {
                kestrel_audio_stop();
            }
        }
    }
}
