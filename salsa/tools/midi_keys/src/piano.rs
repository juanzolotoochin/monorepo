use keyboard::Event;
use rustysynth::{SoundFont, Synthesizer, SynthesizerSettings};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const RATE: usize = 48_000;
const BLOCK: usize = 256;
const FONT: &str = env!("MIDI_PIANO_SF2");

unsafe extern "C" {
    fn piano_audio_start(
        handle: *mut *mut c_void,
        render: unsafe extern "C" fn(*mut c_void, *mut f32, u32),
        state: *mut c_void,
    ) -> c_int;
    fn piano_audio_stop(handle: *mut c_void);
    fn piano_audio_error(code: c_int) -> *const c_char;
}

/// Resolve Bazel runfiles without depending on the caller's working directory.
fn soundfont_path() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut directories: Vec<PathBuf> = ["RUNFILES_DIR", "TEST_SRCDIR"]
        .iter()
        .filter_map(std::env::var_os)
        .map(PathBuf::from)
        .collect();
    directories.push(PathBuf::from(format!("{}.runfiles", executable.display())));
    for directory in directories {
        let path = directory.join(FONT);
        if path.is_file() {
            return Ok(path);
        }
    }
    let manifest = std::env::var_os("RUNFILES_MANIFEST_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(format!("{}.runfiles_manifest", executable.display())));
    if let Ok(contents) = std::fs::read_to_string(manifest) {
        if let Some(path) = contents.lines().find_map(|line| {
            let (key, path) = line.split_once(' ')?;
            (key == FONT).then(|| PathBuf::from(path))
        }) {
            return Ok(path);
        }
    }
    Err("Piano samples not found. Run with `bazel run //salsa/tools/midi_keys` so its runfiles are available.".into())
}

struct Renderer {
    synth: Synthesizer,
    left: [f32; BLOCK],
    right: [f32; BLOCK],
}

impl Renderer {
    fn load() -> Result<Self, String> {
        let path = soundfont_path()?;
        let file = File::open(&path).map_err(|e| format!("Cannot open piano samples: {e}"))?;
        let font = SoundFont::new(&mut BufReader::new(file))
            .map_err(|e| format!("Cannot read Salamander piano: {e}"))?;
        let mut settings = SynthesizerSettings::new(RATE as i32);
        settings.maximum_polyphony = 256;
        settings.block_size = 64;
        settings.enable_reverb_and_chorus = true;
        let synth = Synthesizer::new(&Arc::new(font), &settings).map_err(|e| e.to_string())?;
        let mut renderer = Self {
            synth,
            left: [0.0; BLOCK],
            right: [0.0; BLOCK],
        };
        renderer.reset();
        renderer.volume(0.6);
        Ok(renderer)
    }

    fn reset(&mut self) {
        self.synth.reset();
        for channel in 0..16 {
            // A little room around the piano, with no chorus or instrument changes.
            self.synth.process_midi_message(channel, 0xb0, 91, 24);
            self.synth.process_midi_message(channel, 0xb0, 93, 0);
        }
    }

    fn volume(&mut self, value: f32) {
        // The sample bank has generous recording headroom. Leave space for
        // chords while bringing a normal key strike to a useful listening level.
        self.synth.set_master_volume(value.clamp(0.0, 1.0) * 2.0);
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Note {
                channel,
                note,
                velocity,
            } => {
                self.synth
                    .process_midi_message(channel as i32, 0x90, note as i32, velocity as i32);
            }
            Event::Sustain { channel, down } => {
                self.synth.process_midi_message(
                    channel as i32,
                    0xb0,
                    64,
                    if down { 127 } else { 0 },
                );
            }
            Event::ClearChannel(channel) => self.synth.note_off_all_channel(channel as i32, true),
            Event::ResetControllers(channel) => {
                self.synth.reset_all_controllers_channel(channel as i32)
            }
            Event::Reset => self.reset(),
        }
    }

    fn render(&mut self, output: &mut [f32]) {
        for chunk in output.chunks_mut(BLOCK * 2) {
            let frames = chunk.len() / 2;
            self.synth
                .render(&mut self.left[..frames], &mut self.right[..frames]);
            for (index, frame) in chunk.chunks_exact_mut(2).enumerate() {
                frame[0] = self.left[index].clamp(-1.0, 1.0);
                frame[1] = self.right[index].clamp(-1.0, 1.0);
            }
        }
    }
}

enum Command {
    Event(Event),
    Volume(f32),
}
struct Callback {
    renderer: Renderer,
    commands: Receiver<Command>,
}

unsafe extern "C" fn render_callback(state: *mut c_void, output: *mut f32, frames: u32) {
    // miniaudio owns this thread, provides exactly frames*2 floats, and stops
    // it before Piano drops the Box. Only this callback touches the renderer.
    let state = unsafe { &mut *(state as *mut Callback) };
    let output = unsafe { std::slice::from_raw_parts_mut(output, frames as usize * 2) };
    for _ in 0..1024 {
        match state.commands.try_recv() {
            Ok(Command::Event(event)) => state.renderer.event(event),
            Ok(Command::Volume(value)) => state.renderer.volume(value),
            Err(_) => break,
        }
    }
    state.renderer.render(output);
}

pub struct Piano {
    device: *mut c_void,
    // Kept at a stable address while the device calls into Rust.
    _callback: Box<Callback>,
    commands: SyncSender<Command>,
    trace: Option<Mutex<File>>,
    started: Instant,
}

impl Piano {
    pub fn start(trace_path: Option<&Path>, mode: &str) -> Result<Self, String> {
        let trace = trace_path
            .map(|path| {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .map_err(|e| format!("Cannot create event trace {}: {e}", path.display()))?;
                writeln!(
                    file,
                    "# MIDI Keys pid={} mode={} started_unix={}\n# elapsed_us source event",
                    std::process::id(),
                    mode,
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                )
                .map_err(|e| e.to_string())?;
                Ok::<_, String>(Mutex::new(file))
            })
            .transpose()?;
        let renderer = Renderer::load()?;
        let (commands, receiver) = mpsc::sync_channel(1024);
        let mut callback = Box::new(Callback {
            renderer,
            commands: receiver,
        });
        let mut device = std::ptr::null_mut();
        let result = unsafe {
            piano_audio_start(
                &mut device,
                render_callback,
                (&mut *callback as *mut Callback).cast(),
            )
        };
        if result != 0 {
            let error = unsafe { CStr::from_ptr(piano_audio_error(result)) }.to_string_lossy();
            return Err(format!("Cannot start piano audio: {error}. Check your desktop audio output (PipeWire/PulseAudio), or use --silent."));
        }
        Ok(Self {
            device,
            _callback: callback,
            commands,
            trace,
            started: Instant::now(),
        })
    }

    fn send(&self, command: Command) -> Result<(), String> {
        // Fail explicitly instead of silently losing note-offs and hanging notes.
        self.commands
            .try_send(command)
            .map_err(|_| "Piano audio queue overflowed or stopped. Restart the app.".into())
    }

    pub fn event(&self, event: Event) -> Result<(), String> {
        self.event_from(event, "midi/control")
    }
    /// Optional diagnostic logging happens on the caller, never the audio callback.
    pub fn event_from(&self, event: Event, source: &str) -> Result<(), String> {
        self.send(Command::Event(event))?;
        if let Some(trace) = &self.trace {
            writeln!(
                trace.lock().map_err(|e| e.to_string())?,
                "{} {} {:?}",
                self.started.elapsed().as_micros(),
                source,
                event
            )
            .map_err(|e| format!("Cannot write audio event trace: {e}"))?;
        }
        Ok(())
    }
    pub fn volume(&self, value: f32) -> Result<(), String> {
        self.send(Command::Volume(value))
    }
}

impl Drop for Piano {
    fn drop(&mut self) {
        unsafe { piano_audio_stop(self.device) };
    }
}

/// Offline rendering also makes audio verifiable without a sound device.
pub fn render_demo(path: &Path) -> Result<(), String> {
    let mut renderer = Renderer::load()?;
    let mut file = BufWriter::new(File::create(path).map_err(|e| e.to_string())?);
    let frames = RATE * 8;
    let bytes = (frames * 4) as u32;
    let mut header = Vec::new();
    header.extend(b"RIFF");
    header.extend((36 + bytes).to_le_bytes());
    header.extend(b"WAVEfmt ");
    header.extend(16u32.to_le_bytes());
    header.extend(1u16.to_le_bytes());
    header.extend(2u16.to_le_bytes());
    header.extend((RATE as u32).to_le_bytes());
    header.extend((RATE as u32 * 4).to_le_bytes());
    header.extend(4u16.to_le_bytes());
    header.extend(16u16.to_le_bytes());
    header.extend(b"data");
    header.extend(bytes.to_le_bytes());
    file.write_all(&header).map_err(|e| e.to_string())?;
    let mut block = [0.0; BLOCK * 2];
    let mut next_note = 0;
    let mut released = false;
    for offset in (0..frames).step_by(BLOCK) {
        if next_note < 6 && offset >= next_note * RATE / 2 {
            let note = [60, 64, 67, 72, 71, 67][next_note];
            renderer.event(Event::Note {
                channel: 0,
                note,
                velocity: 85,
            });
            next_note += 1;
        }
        if offset >= RATE * 4 && !released {
            renderer.synth.note_off_all(false);
            released = true;
        }
        let count = (frames - offset).min(BLOCK);
        renderer.render(&mut block[..count * 2]);
        for &sample in &block[..count * 2] {
            file.write_all(&((sample * 32767.0) as i16).to_le_bytes())
                .map_err(|e| e.to_string())?;
        }
    }
    file.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sampled_piano_has_dynamics_sustain_release_and_panic() {
        let mut piano = Renderer::load().unwrap();
        let mut samples = vec![0.0; RATE * 2];
        piano.render(&mut samples);
        assert!(samples.iter().all(|&v| v == 0.0));
        let energy = |data: &[f32]| data.iter().map(|v| (v * v) as f64).sum::<f64>();
        piano.event(Event::Note {
            channel: 0,
            note: 60,
            velocity: 30,
        });
        piano.render(&mut samples);
        let soft = energy(&samples);
        piano.reset();
        piano.event(Event::Note {
            channel: 0,
            note: 60,
            velocity: 110,
        });
        piano.render(&mut samples);
        assert!(energy(&samples) > soft * 1.5);
        assert!(samples.iter().all(|v| v.is_finite() && v.abs() <= 1.0));
        piano.reset();
        piano.event(Event::Sustain {
            channel: 0,
            down: true,
        });
        piano.event(Event::Note {
            channel: 0,
            note: 60,
            velocity: 90,
        });
        piano.render(&mut samples);
        piano.event(Event::Note {
            channel: 0,
            note: 60,
            velocity: 0,
        });
        piano.render(&mut samples);
        let held = energy(&samples);
        assert!(held > 0.01);
        piano.event(Event::Sustain {
            channel: 0,
            down: false,
        });
        for _ in 0..8 {
            piano.render(&mut samples);
        }
        assert!(energy(&samples) < held * 0.01);
        piano.event(Event::Reset);
        piano.render(&mut samples);
        assert!(samples.iter().all(|&v| v == 0.0));
        piano.event(Event::Note {
            channel: 0,
            note: 60,
            velocity: 90,
        });
        piano.volume(0.0);
        piano.render(&mut samples);
        assert!(samples.iter().all(|&v| v == 0.0));
    }
}
