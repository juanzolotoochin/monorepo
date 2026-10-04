use keyboard::{Event, InputSelection, Keyboard};
use piano::Piano;
use std::ffi::{c_char, c_int, c_void, CStr};
use std::io::{self, IsTerminal, Write};
use std::time::{Duration, Instant};

// All FFI is confined to wrappers below. Native handles stay on this thread;
// buffers and pointers remain alive for each synchronous call.
unsafe extern "C" {
    fn mk_open(seq: *mut *mut c_void) -> c_int;
    fn mk_close(seq: *mut c_void);
    fn mk_error(code: c_int) -> *const c_char;
    fn mk_port(
        seq: *mut c_void,
        index: c_int,
        client: *mut c_int,
        port: *mut c_int,
        name: *mut c_char,
        size: usize,
    ) -> c_int;
    fn mk_connect(seq: *mut c_void, client: c_int, port: c_int) -> c_int;
    fn mk_disconnect(seq: *mut c_void, client: c_int, port: c_int);
    fn mk_event(seq: *mut c_void, message: *mut u8, client: *mut c_int, port: *mut c_int) -> c_int;
    fn mk_terminal_enter() -> c_int;
    fn mk_terminal_leave();
    fn mk_key() -> c_int;
    fn mk_terminal_size(columns: *mut c_int, rows: *mut c_int);
}

fn checked(code: c_int) -> Result<c_int, String> {
    if code < 0 {
        // ALSA returns a static, NUL-terminated error description.
        Err(unsafe { CStr::from_ptr(mk_error(code)) }
            .to_string_lossy()
            .into_owned())
    } else {
        Ok(code)
    }
}

struct Midi(*mut c_void);
struct Port {
    client: c_int,
    port: c_int,
    name: String,
}

impl Port {
    fn address(&self) -> String {
        format!("{}:{}", self.client, self.port)
    }
}

impl Midi {
    fn open() -> Result<Self, String> {
        let mut handle = std::ptr::null_mut();
        checked(unsafe { mk_open(&mut handle) }).map_err(|error| format!(
            "Cannot open MIDI: {error}. Run on a Linux host with /dev/snd/seq accessible, or use --demo."))?;
        Ok(Self(handle))
    }

    fn ports(&self) -> Vec<Port> {
        let mut ports = Vec::new();
        loop {
            let (mut client, mut port) = (0, 0);
            let mut name = [0 as c_char; 256];
            let found = unsafe {
                mk_port(
                    self.0,
                    ports.len() as c_int,
                    &mut client,
                    &mut port,
                    name.as_mut_ptr(),
                    name.len(),
                )
            };
            if found == 0 {
                break;
            }
            let name = unsafe { CStr::from_ptr(name.as_ptr()) }
                .to_string_lossy()
                .chars()
                .map(|c| {
                    if c.is_ascii_graphic() || c == ' ' {
                        c
                    } else {
                        '?'
                    }
                })
                .collect();
            ports.push(Port { client, port, name });
        }
        ports
    }

    fn connect(&self, port: &Port) -> Result<(), String> {
        checked(unsafe { mk_connect(self.0, port.client, port.port) })
            .map_err(|error| format!("Cannot connect to {}: {error}", port.address()))?;
        Ok(())
    }

    fn drain(
        &self,
        keyboard: &mut Keyboard,
        selection: &mut InputSelection,
        ports: &[Port],
        piano: Option<&Piano>,
        trainer: &mut Option<trainer::Session>,
    ) -> Result<bool, String> {
        let mut changed = false;
        // Bound each batch so MIDI clocks or a noisy device cannot starve input.
        for _ in 0..512 {
            let mut message = [0u8; 3];
            let (mut client, mut port) = (0, 0);
            let result =
                checked(unsafe { mk_event(self.0, message.as_mut_ptr(), &mut client, &mut port) })
                    .map_err(|error| {
                        format!("MIDI input stopped: {error}. Check the connection and restart.")
                    })?;
            if result == 0 {
                break;
            }
            if result == 3 && selection.source == Some((client, port)) {
                return Err("Selected MIDI device disconnected. Reconnect it and restart.".into());
            }
            if result == 1 {
                if let Some(event) = Event::from_midi(message[0], message[1], message[2]) {
                    let was_selecting = selection.source.is_none();
                    if selection.accepts((client, port), event) {
                        if was_selecting {
                            for other in ports {
                                if (other.client, other.port) != (client, port) {
                                    unsafe { mk_disconnect(self.0, other.client, other.port) };
                                }
                            }
                        }
                        apply_event(keyboard, piano, event)?;
                        if !was_selecting {
                            if let Some(trainer) = trainer {
                                trainer.input(event, Instant::now());
                            }
                        }
                        changed = true;
                    }
                }
            }
        }
        Ok(changed)
    }
}

impl Drop for Midi {
    fn drop(&mut self) {
        unsafe { mk_close(self.0) };
    }
}

struct Terminal;

impl Terminal {
    fn enter() -> Result<Self, String> {
        checked(unsafe { mk_terminal_enter() }).map_err(|error| {
            format!(
                "An interactive terminal is required: {error}. Use --snapshot for plain output."
            )
        })?;
        let guard = Self;
        write!(io::stdout(), "\x1b[?1049h\x1b[?25l").map_err(|e| e.to_string())?;
        io::stdout().flush().map_err(|e| e.to_string())?;
        Ok(guard)
    }

    fn size(&self) -> (i32, i32) {
        let (mut columns, mut rows) = (80, 24);
        unsafe { mk_terminal_size(&mut columns, &mut rows) };
        (columns, rows)
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        unsafe { mk_terminal_leave() };
        let _ = write!(io::stdout(), "\x1b[0m\x1b[?25h\x1b[?1049l");
        let _ = io::stdout().flush();
    }
}

const HELP: &str = "MIDI Keys - adaptive ear training and theory practice

Usage: midi_keys [--list | --port CLIENT:PORT | --demo | --snapshot]
                 [--free-play] [--silent] [--note-names] [--profile FILE]
                 [--trace-events FILE]
       midi_keys --render-demo FILE.wav

  no options    Adaptive training. Press a MIDI key to select its input
  --free-play   Piano and visualizer without training
  --profile FILE  Use a separate learner profile (JSON)
  --trace-events FILE  Diagnose audio by logging its input events to a new file
  --list        List available MIDI input ports
  --port 24:0   Connect to a specific input (see --list)
  --demo        Play example piano chords without a MIDI device
  --snapshot    Print a demo frame and exit; no terminal or device required
  --silent      Visualize only, without loading samples or opening audio
  --note-names  Show note names on the piano keys (hidden by default)
  --render-demo FILE.wav  Render a piano preview without an audio device

Controls: [ / ] octave, f follow, space panic, -/+ volume, m mute, n names, q quit
Training: release keys to submit; next exercise starts automatically.
          r replay/restart listening, Enter submit/next now, Backspace clear,
          h hint (no mastery credit), x don't know
Middle C is C4 (MIDI 60). Highlights show held keys, not sustain pedal state.
Use at least 64 columns by 22 rows; 100 x 32 shows the full instrument panel.";

fn apply_event(keyboard: &mut Keyboard, piano: Option<&Piano>, event: Event) -> Result<(), String> {
    if let Some(piano) = piano {
        piano.event(event)?;
    }
    keyboard.apply(event);
    Ok(())
}

fn apply_demo_event(
    keyboard: &mut Keyboard,
    piano: Option<&Piano>,
    event: Event,
) -> Result<(), String> {
    if let Some(piano) = piano {
        piano.event_from(event, "demo")?;
    }
    keyboard.apply(event);
    Ok(())
}

fn demo_chord(keyboard: &mut Keyboard, piano: Option<&Piano>, step: usize) -> Result<(), String> {
    const CHORDS: [[usize; 3]; 4] = [[60, 64, 67], [62, 65, 69], [61, 65, 68], [59, 62, 67]];
    for note in 0..128 {
        if keyboard.velocity(note) > 0 {
            apply_demo_event(
                keyboard,
                piano,
                Event::Note {
                    channel: 0,
                    note,
                    velocity: 0,
                },
            )?;
        }
    }
    if step % 2 == 0 {
        for (index, &note) in CHORDS[(step / 2) % CHORDS.len()].iter().enumerate() {
            apply_demo_event(
                keyboard,
                piano,
                Event::Note {
                    channel: 0,
                    note,
                    velocity: 80 + index as u8 * 12,
                },
            )?;
        }
    }
    Ok(())
}

fn run() -> Result<(), String> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let silent = args.iter().any(|arg| arg == "--silent");
    args.retain(|arg| arg != "--silent");
    let mut note_names = args.iter().any(|arg| arg == "--note-names");
    args.retain(|arg| arg != "--note-names");
    let free_play = args.iter().any(|arg| arg == "--free-play");
    args.retain(|arg| arg != "--free-play");
    let profile_path = if let Some(index) = args.iter().position(|arg| arg == "--profile") {
        args.remove(index);
        if index >= args.len() || args[index].starts_with("--") {
            return Err("--profile requires a file path".into());
        }
        Some(std::path::PathBuf::from(args.remove(index)))
    } else {
        None
    };
    let trace_path = if let Some(index) = args.iter().position(|arg| arg == "--trace-events") {
        args.remove(index);
        if index >= args.len() || args[index].starts_with("--") {
            return Err("--trace-events requires a new file path".into());
        }
        Some(std::path::PathBuf::from(args.remove(index)))
    } else {
        None
    };
    let option = args.first().map(String::as_str).unwrap_or("");
    if matches!(option, "--help" | "-h") && args.len() == 1 {
        println!("{HELP}");
        return Ok(());
    }
    let valid = match option {
        "" => args.is_empty(),
        "--list" | "--demo" | "--snapshot" => args.len() == 1,
        "--port" | "--render-demo" => args.len() == 2,
        _ => false,
    };
    if !valid {
        return Err(format!("Invalid arguments.\n\n{HELP}"));
    }
    if free_play && option == "--demo" {
        return Err("--free-play and --demo are different modes. Omit --demo for a piano without automatic playback.".into());
    }
    if trace_path.is_some()
        && (silent || matches!(option, "--list" | "--snapshot" | "--render-demo"))
    {
        return Err("--trace-events requires interactive piano audio (omit --silent).".into());
    }
    let mut keyboard = Keyboard::default();
    if option == "--snapshot" {
        demo_chord(&mut keyboard, None, 0)?;
        print!("{}", keyboard.render("Demo | C major", false));
        return Ok(());
    }
    if option == "--render-demo" {
        eprintln!("Loading Salamander Grand Piano...");
        piano::render_demo(std::path::Path::new(&args[1]))?;
        println!("Saved piano preview to {}", args[1]);
        return Ok(());
    }
    if option != "--list" && (!io::stdin().is_terminal() || !io::stdout().is_terminal()) {
        return Err("An interactive terminal is required. Use --snapshot or --render-demo for noninteractive output.".into());
    }
    let training = !free_play && matches!(option, "" | "--port");
    if training && silent {
        return Err(
            "Ear training needs sound. Use --free-play --silent for the silent visualizer.".into(),
        );
    }
    let mut trainer = if training {
        Some(trainer::Session::open(match profile_path {
            Some(path) => path,
            None => trainer::default_profile_path()?,
        })?)
    } else {
        None
    };
    let piano = if silent || option == "--list" {
        None
    } else {
        eprintln!("Loading Salamander Grand Piano...");
        Some(Piano::start(
            trace_path.as_deref(),
            if training {
                "training"
            } else if option == "--demo" {
                "demo"
            } else {
                "free-play"
            },
        )?)
    };
    let mut volume = 0.6f32;
    let mut muted = false;
    let midi = if option == "--demo" {
        None
    } else {
        Some(Midi::open()?)
    };
    let mut selection = InputSelection::default();
    let mut ports = Vec::new();
    if let Some(midi) = &midi {
        let available = midi.ports();
        if option == "--list" {
            println!("MIDI inputs:");
            for port in &available {
                println!("  {:>7}  {}", port.address(), port.name);
            }
            if available.is_empty() {
                println!("  No inputs found. Connect a keyboard, or try --demo.");
            }
            return Ok(());
        }
        if available.is_empty() {
            return Err("No readable MIDI inputs found. Connect a keyboard or use --demo.".into());
        }
        if option == "--port" {
            let port = available
                .into_iter()
                .find(|p| p.address() == args[1])
                .ok_or_else(|| {
                    format!(
                        "Unknown input port '{}'. Use --list to see available inputs.",
                        args[1]
                    )
                })?;
            midi.connect(&port)?;
            selection.source = Some((port.client, port.port));
            ports.push(port);
        } else {
            let mut errors = Vec::new();
            for port in available {
                match midi.connect(&port) {
                    Ok(()) => ports.push(port),
                    Err(error) => errors.push(error),
                }
            }
            if ports.is_empty() {
                return Err(errors.join("\n"));
            }
        }
    }
    let terminal = Terminal::enter()?;
    let mut display = ui::Display::new().map_err(|e| e.to_string())?;
    let mut last_frame = Instant::now();
    let mut size = terminal.size();
    let mut dirty = true;
    let mut step = 0;
    let mut next_demo = Instant::now();
    let mut escape_sequence = 0;
    loop {
        let new_size = terminal.size();
        dirty |= new_size != size;
        size = new_size;
        if let Some(trainer) = &mut trainer {
            let fits = ui::training_fits(
                size.0.clamp(0, u16::MAX as i32) as u16,
                size.1.clamp(0, u16::MAX as i32) as u16,
            );
            if let Some(event) = trainer.set_visible(fits, Instant::now()) {
                if let Some(piano) = &piano {
                    piano.event_from(event, "training")?;
                }
            }
        }
        keyboard.set_visible_span(ui::visible_span(size.0.clamp(0, u16::MAX as i32) as u16));
        if selection.source.is_some() && (0..128).all(|note| keyboard.velocity(note) == 0) {
            if let Some(trainer) = &mut trainer {
                trainer.ready(Instant::now());
            }
        }
        if let Some(midi) = &midi {
            dirty |= midi.drain(
                &mut keyboard,
                &mut selection,
                &ports,
                piano.as_ref(),
                &mut trainer,
            )?;
        } else if Instant::now() >= next_demo {
            demo_chord(&mut keyboard, piano.as_ref(), step)?;
            step += 1;
            next_demo = Instant::now() + Duration::from_millis(700);
            dirty = true;
        }
        if let Some(trainer) = &mut trainer {
            for event in trainer.tick(Instant::now())? {
                // Listening prompts go only to audio: the visual keyboard and
                // note trail must never reveal an ear-training answer.
                if let Some(piano) = &piano {
                    piano.event_from(event, "training")?;
                }
            }
        }
        if dirty || last_frame.elapsed() >= Duration::from_millis(33) {
            let source = ports
                .iter()
                .find(|p| Some((p.client, p.port)) == selection.source)
                .map(|port| format!("{}  /  {}", port.address(), port.name))
                .unwrap_or_default();
            let view = ui::View {
                keyboard: &keyboard,
                source: &source,
                connected: selection.source.is_some(),
                demo: midi.is_none(),
                silent,
                muted,
                note_names,
                volume,
                now: Instant::now(),
            };
            if let Some(trainer) = &trainer {
                display.draw_training(&view, &trainer.view())
            } else {
                display.draw(&view)
            }
            .map_err(|e| e.to_string())?;
            last_frame = Instant::now();
            dirty = false;
        }
        let key = unsafe { mk_key() };
        if matches!(key, -1 | 3 | 4) {
            break;
        }
        if key == 0 {
            continue;
        }
        // Ignore terminal escape sequences so an arrow's '[' is not treated
        // as the octave control. Handles both CSI and SS3 sequences.
        if escape_sequence == 1 {
            escape_sequence = if matches!(key, 91 | 79) { 2 } else { 0 };
            continue;
        }
        if escape_sequence == 2 {
            if (0x40..=0x7e).contains(&key) {
                escape_sequence = 0;
            }
            continue;
        }
        if let Some(trainer) = &mut trainer {
            let handled = match key {
                10 | 13 => {
                    if trainer.phase() == trainer::Phase::Feedback {
                        apply_event(&mut keyboard, piano.as_ref(), Event::Reset)?;
                        trainer.advance();
                    } else {
                        trainer.submit(false)?;
                    }
                    true
                }
                114 | 82 => {
                    if matches!(
                        trainer.phase(),
                        trainer::Phase::Answering
                            | trainer::Phase::Waiting
                            | trainer::Phase::Listening
                    ) {
                        apply_event(&mut keyboard, piano.as_ref(), Event::Reset)?;
                        trainer.replay(Instant::now());
                    }
                    true
                }
                8 | 127 => {
                    trainer.clear_answer();
                    apply_event(&mut keyboard, piano.as_ref(), Event::Reset)?;
                    true
                }
                104 => {
                    trainer.hint();
                    true
                }
                120 => {
                    trainer.submit(true)?;
                    true
                }
                _ => false,
            };
            if handled {
                dirty = true;
                continue;
            }
        }
        match key {
            27 => escape_sequence = 1,
            113 | 81 => break,
            91 => {
                keyboard.shift(false);
                dirty = true;
            }
            93 => {
                keyboard.shift(true);
                dirty = true;
            }
            102 => {
                keyboard.follow = true;
                dirty = true;
            }
            32 => {
                apply_event(&mut keyboard, piano.as_ref(), Event::Reset)?;
                if let Some(trainer) = &mut trainer {
                    trainer.panic();
                }
                dirty = true;
            }
            109 => {
                muted = !muted;
                if let Some(piano) = &piano {
                    piano.volume(if muted { 0.0 } else { volume })?;
                }
                dirty = true;
            }
            110 => {
                note_names = !note_names;
                dirty = true;
            }
            43 | 61 | 45 => {
                volume = (volume + if key == 45 { -0.05 } else { 0.05 }).clamp(0.0, 1.0);
                if let Some(piano) = &piano {
                    piano.volume(if muted { 0.0 } else { volume })?;
                }
                dirty = true;
            }
            _ => {}
        }
    }
    Ok(())
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("midi_keys: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
