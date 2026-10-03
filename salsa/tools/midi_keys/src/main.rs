use keyboard::{Event, InputSelection, Keyboard};
use std::ffi::{c_char, c_int, c_void, CStr};
use std::io::{self, Write};
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
                        keyboard.apply(event);
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

const HELP: &str = "MIDI Keys - ASCII piano for Linux MIDI input

Usage: midi_keys [--list | --port CLIENT:PORT | --demo | --snapshot]

  no options    Press a key on your MIDI keyboard to select its input
  --list        List available MIDI input ports
  --port 24:0   Connect to a specific input (see --list)
  --demo        Animate example chords without a MIDI device
  --snapshot    Print a demo frame and exit; no terminal or device required

Controls: [ / ] octave, f follow notes, space clear, q / Ctrl-C quit
Middle C is C4 (MIDI 60). Highlights show held keys, not sustain pedal state.
Use a terminal at least 64 columns by 23 rows.";

fn demo_chord(keyboard: &mut Keyboard, step: usize) {
    const CHORDS: [[usize; 3]; 4] = [[60, 64, 67], [62, 65, 69], [61, 65, 68], [59, 62, 67]];
    keyboard.clear();
    if step % 2 == 0 {
        for (index, &note) in CHORDS[(step / 2) % CHORDS.len()].iter().enumerate() {
            keyboard.apply(Event::Note {
                channel: 0,
                note,
                velocity: 80 + index as u8 * 12,
            });
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let option = args.first().map(String::as_str).unwrap_or("");
    if matches!(option, "--help" | "-h") && args.len() == 1 {
        println!("{HELP}");
        return Ok(());
    }
    let valid = match option {
        "" => args.is_empty(),
        "--list" | "--demo" | "--snapshot" => args.len() == 1,
        "--port" => args.len() == 2,
        _ => false,
    };
    if !valid {
        return Err(format!("Invalid arguments.\n\n{HELP}"));
    }
    let mut keyboard = Keyboard::default();
    if option == "--snapshot" {
        demo_chord(&mut keyboard, 0);
        print!("{}", keyboard.render("Demo | C major", false));
        return Ok(());
    }
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
    let mut size = terminal.size();
    let mut dirty = true;
    let mut step = 0;
    let mut next_demo = Instant::now();
    let mut escape_sequence = 0;
    loop {
        if let Some(midi) = &midi {
            dirty |= midi.drain(&mut keyboard, &mut selection, &ports)?;
        } else if Instant::now() >= next_demo {
            demo_chord(&mut keyboard, step);
            step += 1;
            next_demo = Instant::now() + Duration::from_millis(700);
            dirty = true;
        }
        let new_size = terminal.size();
        dirty |= new_size != size;
        size = new_size;
        if dirty {
            let frame = if size.0 < 64 || size.1 < 23 {
                "Resize to 64 x 23.\nq to quit.\n".to_owned()
            } else {
                let source = if midi.is_none() {
                    "Demo | example chords (no MIDI device)".to_owned()
                } else if let Some(port) = ports
                    .iter()
                    .find(|p| Some((p.client, p.port)) == selection.source)
                {
                    format!("{} | {}", port.address(), port.name)
                        .chars()
                        .take(60)
                        .collect()
                } else {
                    "Press a key on your MIDI keyboard to select it.".to_owned()
                };
                keyboard.render(&source, true)
            };
            let mut stdout = io::stdout().lock();
            write!(stdout, "\x1b[H{}\x1b[J", frame.replace('\n', "\x1b[K\r\n"))
                .map_err(|e| e.to_string())?;
            stdout.flush().map_err(|e| e.to_string())?;
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
                keyboard.clear();
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
