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
    _worker: Option<AudioThread>,
}
impl Sound {
    #[cfg(test)]
    pub fn silent() -> Self {
        Self {
            muted: true,
            active: false,
            _worker: None,
        }
    }
    pub fn new() -> Self {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };

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
