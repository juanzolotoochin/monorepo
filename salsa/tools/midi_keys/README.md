# MIDI Keys

A small Rust piano for Linux: play a MIDI keyboard, hear a sampled Yamaha C5,
and see held notes on an ASCII piano. Chords highlight together; the readout includes note names,
MIDI numbers, channels, and velocities. Middle C is C4 (MIDI 60).

From the repository root:

```sh
# List MIDI inputs, then connect to a port from that list.
bazel run //salsa/tools/midi_keys -- --list
bazel run //salsa/tools/midi_keys -- --port 24:0

# Without arguments, press a key on your MIDI keyboard to select it.
bazel run //salsa/tools/midi_keys

# Play the piano demo without a MIDI device.
bazel run //salsa/tools/midi_keys -- --demo

# Visualize silently, print a frame, or export an eight-second piano preview.
bazel run //salsa/tools/midi_keys -- --silent
bazel run //salsa/tools/midi_keys -- --snapshot
bazel run //salsa/tools/midi_keys -- --render-demo /tmp/piano.wav
```

Use `--config=full_local` after `run` to disable the repository's remote cache
and build-event upload. After building, the executable is also available at
`bazel-bin/salsa/tools/midi_keys/midi_keys`.

With no arguments, the app listens to all available inputs. The first positive-velocity
note-on selects its device and immediately highlights that note. Note releases,
MIDI clocks, and controllers do not select a device. Other inputs are disconnected
after selection. `--port` still lets you choose explicitly.

The terminal must support ANSI escapes and be at least 64 columns by 23 rows.
Pressed keys turn cyan and show `*`; unpressed black keys show `#`.

| Key | Action |
| --- | --- |
| `[` / `]` | Move the view down / up one octave, disabling auto-follow |
| `f` | Follow newly played notes outside the visible range |
| `-` / `+` | Lower / raise piano volume |
| `m` | Mute / unmute piano audio |
| Space | Panic: silence all notes, release sustain, clear highlights |
| `q` or Ctrl-C | Quit and restore the terminal |

The display tracks physical note-on/note-off state on all 16 MIDI channels.
The sustain pedal holds the sound, but does not keep released keys highlighted. MIDI all-notes-off,
all-sound-off, and reset messages clear the appropriate state. Unplugging the
selected device ends the session; reconnect it and restart to select its new
port.

## Piano sound

There is one instrument: **Salamander Grand Piano**, created by Alexander Holm,
with the FreePats SF2 conversion by Roberto. It is a sampled Yamaha C5 with
multiple velocity layers. RustySynth renders it at 48 kHz in stereo, with up to
256 sample voices, subtle reverb, and no chorus. Synth rendering runs on the
audio callback thread; terminal drawing cannot block it on a UI lock.

Sound goes to the desktop's default output through PipeWire's PulseAudio service
or PulseAudio. Select the UltraLite (or headphones) in your desktop audio settings.
The app requests 256-frame audio periods and two periods of buffering; actual
latency depends on the audio server and hardware. MIDI polling is bounded to 2 ms.
`--demo` tests sound independently of the physical MIDI connection. `--silent`
does not load the samples or open an audio device.

The first Bazel build downloads a **296 MiB** archive, which expands to about
**1.18 GiB**. It stays in Bazel's external cache, outside Git. Startup loads the
samples into memory. Both the archive and synth version are pinned and verified
by checksum. Run the executable with its Bazel runfiles present (normally via
`bazel run`); copying only the binary omits the piano.

Samples: [FreePats Salamander Grand Piano](https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html),
version V3+20200602, [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/).
The SF2 conversion omits the original SFZ's pedal/hammer/release-noise features.
Attribution and pinned versions are also recorded in `THIRD_PARTY_NOTICES.md`.

## Build and runtime dependencies

Rust 1.92.0 and the LLVM C toolchain are managed by Bazel. ALSA is statically
linked from the pinned Bzlmod module `alsa_lib` 1.2.9.bcr.4, exposed through
`//third_party/alsa`. No host Rust installation, ALSA development package,
`pkg-config`, `aseqdump`, `stty`, or installed `libasound.so` is used.
Audio uses pinned RustySynth 1.3.7 (pure Rust) and miniaudio 0.11.25 (built
with the same LLVM toolchain). No host audio development headers are needed.
At runtime, audible modes require `libpulse.so.0` and a running desktop audio
server. The application uses its API directly and does not launch an external
synthesizer or audio command.

A small C adapter keeps ALSA types and terminal handling behind an ABI boundary;
Rust handles the keyboard state, UI, and lifecycle. The adapter passes an inline
configuration to `snd_seq_open_lconf`, so it does not need `/usr/share/alsa` or
the user's `.asoundrc`. This is the Linux legacy MIDI sequencer API (MIDI 1.0).
See the [ALSA sequencer documentation](https://www.alsa-project.org/alsa-doc/alsa-lib/group___sequencer.html).

Running live input still requires a Linux kernel with ALSA sequencer support,
permission to access `/dev/snd/seq`, and a connected MIDI source. A container
must expose that device. The binary also uses the normal Linux glibc runtime;
a hermetic build does not mean a fully static executable. `--demo` and
`--snapshot` work without MIDI hardware.

## Verification

```sh
bazel test --config=full_local //salsa/tools/midi_keys:all
```

Rust tests cover chords, independent channels, velocity-zero release, MIDI
reset controls, invalid messages, the full note range, and rendering. PTY tests
exercise the real executable, octave controls, terminal resizing, normal exit,
Ctrl-C, SIGTERM, and terminal restoration with no host utilities on PATH, using
`--silent` so no physical audio device is needed. The offline piano test renders
the real sample bank and verifies silence, velocity dynamics, sustain, release,
panic, and volume. `--render-demo` also works without a MIDI or audio device.
Live note delivery additionally needs a hardware check on a host with `/dev/snd/seq`.
