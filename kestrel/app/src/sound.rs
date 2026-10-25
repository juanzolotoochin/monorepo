//! Small procedural sound bank backed by the repository's hermetic miniaudio.
use bevy::prelude::*;
use std::{
    sync::mpsc::{self, SyncSender},
    thread::{self, JoinHandle},
};

unsafe extern "C" {
    fn kestrel_audio_start() -> i32;
    fn kestrel_audio_play(cue: i32);
    fn kestrel_audio_stop();
    fn kestrel_audio_cue_count() -> i32;
    fn kestrel_audio_synthesize(
        events: *const SynthEvent,
        count: u32,
        output: *mut f32,
        frames: u32,
    );
}

// Miniaudio initializes Windows COM as MTA. Winit needs STA on the window
// thread, so both audio initialization and destruction must live elsewhere.
// The C device has a separate real-time callback; cues remain lock-free.
struct AudioThread {
    stop: Option<SyncSender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl AudioThread {
    fn start(
        start: impl FnOnce() -> bool + Send + 'static,
        stop: impl FnOnce() + Send + 'static,
    ) -> Option<(bool, Self)> {
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let (stop_tx, stop_rx) = mpsc::sync_channel::<()>(0);
        let handle = thread::Builder::new()
            .name("kestrel-audio".into())
            .spawn(move || {
                let active = start();
                if ready_tx.send(active).is_ok() && active {
                    // Dropping the owner disconnects this channel.
                    let _ = stop_rx.recv();
                }
                if active {
                    stop();
                }
            })
            .ok()?;
        let active = ready_rx.recv().unwrap_or(false);
        Some((
            active,
            Self {
                stop: Some(stop_tx),
                handle: Some(handle),
            },
        ))
    }
}

impl Drop for AudioThread {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[repr(i32)]
#[derive(Clone, Copy, Debug)]
pub enum Cue {
    Swing = 0,
    Clang = 1,
    Step = 2,
    Toss = 6,
    Land = 7,
    Dispense = 8,
    Jetpack = 9,
    Stone = 3,
    Crystal = 4,
    Polymer = 5,
    GhoulHit = 10,
    Bow = 11,
    Spear = 12,
    Ballista = 13,
    Arc = 14,
    ArrowHit = 15,
    Decapitate = 16,
    Pierce = 17,
    Laser = 18,
    LaserImpact = 19,
}
impl Cue {
    pub const ALL: [Self; 20] = [
        Self::Swing, Self::Clang, Self::Step, Self::Stone, Self::Crystal,
        Self::Polymer, Self::Toss, Self::Land, Self::Dispense, Self::Jetpack,
        Self::GhoulHit, Self::Bow, Self::Spear, Self::Ballista, Self::Arc,
        Self::ArrowHit, Self::Decapitate, Self::Pierce, Self::Laser, Self::LaserImpact,
    ];
    pub fn name(id: i32) -> String {
        let cue = Self::ALL.get(id as usize).expect("recorded cue id");
        let mut name = String::new();
        for (index, character) in format!("{cue:?}").chars().enumerate() {
            if character.is_uppercase() && index > 0 { name.push('_'); }
            name.push(character.to_ascii_lowercase());
        }
        name
    }
}
#[repr(C)]
struct SynthEvent {
    sample: u64,
    kind: i32,
}
#[derive(Clone)]
struct RecordedCue {
    tick: u64,
    kind: i32,
}
#[derive(Resource)]
pub struct Sound {
    pub muted: bool,
    active: bool,
    _worker: Option<AudioThread>,
    recording: bool,
    tick: std::sync::atomic::AtomicU64,
    cues: std::sync::Mutex<Vec<RecordedCue>>,
}
impl Sound {
    pub fn silent() -> Self {
        Self {
            muted: true,
            active: false,
            _worker: None,
            recording: false,
            tick: std::sync::atomic::AtomicU64::new(0),
            cues: std::sync::Mutex::new(vec![]),
        }
    }
    pub fn new() -> Self {
        assert_eq!(unsafe { kestrel_audio_cue_count() } as usize, Cue::ALL.len());
        // The worker owns the native context/device for their entire lifetime.
        // Initialization completes before any cue can be submitted.
        let worker = AudioThread::start(
            || unsafe { kestrel_audio_start() == 0 },
            || unsafe { kestrel_audio_stop() },
        );
        let active = worker.as_ref().is_some_and(|(active, _)| *active);
        if !active {
            eprintln!("Audio device unavailable; continuing without sound.");
        }
        Self {
            muted: !active,
            active,
            _worker: worker.map(|(_, worker)| worker),
            recording: false,
            tick: std::sync::atomic::AtomicU64::new(0),
            cues: std::sync::Mutex::new(vec![]),
        }
    }
    pub fn recording() -> Self {
        Self {
            recording: true,
            muted: false,
            ..Self::silent()
        }
    }
    pub fn set_recording_tick(&self, tick: u64) {
        self.tick.store(tick, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn recorded_events(&self) -> Vec<(u64, i32)> {
        self.cues
            .lock()
            .unwrap()
            .iter()
            .map(|cue| (cue.tick, cue.kind))
            .collect()
    }
    pub fn write_wav(
        &self,
        path: &str,
        fps: u32,
        start_tick: u64,
        frames: usize,
    ) -> std::io::Result<()> {
        use std::io::Write;
        let sample_frames = (frames as u64 * 48000 / u64::from(fps)) as u32;
        let events: Vec<_> = self
            .cues
            .lock()
            .unwrap()
            .iter()
            .filter(|cue| cue.tick >= start_tick)
            .map(|cue| SynthEvent {
                sample: (cue.tick - start_tick) * 48000 / u64::from(fps),
                kind: cue.kind,
            })
            .filter(|event| event.sample < u64::from(sample_frames))
            .collect();
        let mut samples = vec![0.0_f32; sample_frames as usize * 2];
        unsafe {
            kestrel_audio_synthesize(
                events.as_ptr(),
                events.len() as u32,
                samples.as_mut_ptr(),
                sample_frames,
            );
        }
        let data_bytes = sample_frames * 4;
        let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
        file.write_all(b"RIFF")?;
        file.write_all(&(data_bytes + 36).to_le_bytes())?;
        file.write_all(b"WAVEfmt ")?;
        file.write_all(&16_u32.to_le_bytes())?;
        file.write_all(&1_u16.to_le_bytes())?;
        file.write_all(&2_u16.to_le_bytes())?;
        file.write_all(&48000_u32.to_le_bytes())?;
        file.write_all(&192000_u32.to_le_bytes())?;
        file.write_all(&4_u16.to_le_bytes())?;
        file.write_all(&16_u16.to_le_bytes())?;
        file.write_all(b"data")?;
        file.write_all(&data_bytes.to_le_bytes())?;
        for sample in samples {
            file.write_all(&((sample.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
        }
        file.flush()
    }
    pub fn play(&self, cue: Cue) {
        if self.recording {
            self.cues.lock().unwrap().push(RecordedCue {
                tick: self.tick.load(std::sync::atomic::Ordering::Relaxed),
                kind: cue as i32,
            });
        }
        if self.active && !self.muted {
            unsafe {
                kestrel_audio_play(cue as i32);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cue_ids_and_names_match_native_bank() {
        assert_eq!(unsafe { kestrel_audio_cue_count() } as usize, Cue::ALL.len());
        for (index, cue) in Cue::ALL.iter().enumerate() {
            assert_eq!(*cue as usize, index);
            assert!(!Cue::name(index as i32).is_empty());
        }
        assert_eq!(Cue::name(Cue::GhoulHit as i32), "ghoul_hit");
    }
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };

    #[test]
    fn offline_foley_is_deterministic_audible_and_silent_outside_cue_windows() {
        let events = [
            SynthEvent {
                sample: 2400,
                kind: Cue::Toss as i32,
            },
            SynthEvent {
                sample: 12000,
                kind: Cue::Land as i32,
            },
        ];
        let mut first = vec![0.0; 96000];
        let mut second = first.clone();
        unsafe {
            kestrel_audio_synthesize(events.as_ptr(), 2, first.as_mut_ptr(), 48000);
            kestrel_audio_synthesize(events.as_ptr(), 2, second.as_mut_ptr(), 48000);
        }
        assert_eq!(first, second);
        assert!(first.iter().all(|s| s.is_finite() && s.abs() < 1.0));
        assert!(first[..4800].iter().all(|s| *s == 0.0));
        assert!(first[4800..36000].iter().any(|s| s.abs() > 0.05));
        assert!(first[90000..].iter().all(|s| s.abs() < 0.00001));
        assert!(first[4800..36000]
            .chunks_exact(2)
            .any(|channels| (channels[0] - channels[1]).abs() > 0.001));
    }

    #[test]
    fn laser_contact_has_an_audible_transient() {
        let event = [SynthEvent { sample: 0, kind: Cue::LaserImpact as i32 }];
        let mut samples = vec![0.0; 48000 / 2];
        unsafe { kestrel_audio_synthesize(event.as_ptr(), 1, samples.as_mut_ptr(), 12000); }
        let peak = samples.iter().fold(0.0_f32, |peak, sample| peak.max(sample.abs()));
        assert!(peak > 0.15, "laser impact peak {peak} is too quiet");
        assert!(samples[18000..].iter().all(|sample| sample.abs() < 0.01));
    }

    #[test]
    fn audio_init_and_shutdown_share_a_dedicated_thread() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let init_events = events.clone();
        let stop_events = events.clone();
        let caller = thread::current().id();
        let (active, worker) = AudioThread::start(
            move || {
                init_events
                    .lock()
                    .unwrap()
                    .push(("init", thread::current().id()));
                true
            },
            move || {
                stop_events
                    .lock()
                    .unwrap()
                    .push(("stop", thread::current().id()));
            },
        )
        .unwrap();
        assert!(active);
        assert_eq!(events.lock().unwrap().len(), 1);
        drop(worker); // Must wait for native shutdown to complete.
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, "init");
        assert_eq!(events[1].0, "stop");
        assert_ne!(events[0].1, caller);
        assert_eq!(events[0].1, events[1].1);
    }

    #[test]
    fn unavailable_audio_does_not_destroy_an_uninitialized_device() {
        let stopped = Arc::new(AtomicBool::new(false));
        let stop_flag = stopped.clone();
        let (active, worker) = AudioThread::start(
            || false,
            move || {
                stop_flag.store(true, Ordering::SeqCst);
            },
        )
        .unwrap();
        assert!(!active);
        drop(worker);
        assert!(!stopped.load(Ordering::SeqCst));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn audio_startup_leaves_the_window_thread_compatible_with_ole() {
        #[link(name = "ole32")]
        unsafe extern "system" {
            fn OleInitialize(reserved: *mut std::ffi::c_void) -> i32;
            fn OleUninitialize();
        }
        // A fresh thread reproduces Sound::new() before winit's OleInitialize,
        // without inheriting any COM state from the test runner.
        thread::spawn(|| {
            let sound = Sound::new();
            let result = unsafe { OleInitialize(std::ptr::null_mut()) };
            if result >= 0 {
                unsafe {
                    OleUninitialize();
                }
            }
            drop(sound);
            assert!(
                result >= 0,
                "Window-thread OleInitialize failed: HRESULT 0x{:08X}",
                result as u32
            );
        })
        .join()
        .unwrap();
    }
}
