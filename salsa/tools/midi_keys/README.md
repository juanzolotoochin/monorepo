# MIDI Keys

A small Rust TUI for Linux: play a MIDI keyboard and see held notes on an
ASCII piano. Chords highlight together; the readout includes note names,
MIDI numbers, channels, and velocities. Middle C is C4 (MIDI 60).

From the repository root:

```sh
# List MIDI inputs, then connect to a port from that list.
bazel run //salsa/tools/midi_keys -- --list
bazel run //salsa/tools/midi_keys -- --port 24:0

# Without arguments, press a key on your MIDI keyboard to select it.
bazel run //salsa/tools/midi_keys

# Try the UI without hardware, or print one plain ASCII frame.
bazel run //salsa/tools/midi_keys -- --demo
bazel run //salsa/tools/midi_keys -- --snapshot
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
| Space | Clear held notes |
| `q` or Ctrl-C | Quit and restore the terminal |

The display tracks physical note-on/note-off state on all 16 MIDI channels.
Sustain does not keep released keys highlighted. MIDI all-notes-off,
all-sound-off, and reset messages clear the appropriate state. Unplugging the
selected device ends the session; reconnect it and restart to select its new
port. This app visualizes input; it does not synthesize sound.

## Build and runtime dependencies

Rust 1.92.0 and the LLVM C toolchain are managed by Bazel. ALSA is statically
linked from the pinned Bzlmod module `alsa_lib` 1.2.9.bcr.4, exposed through
`//third_party/alsa`. No host Rust installation, ALSA development package,
`pkg-config`, `aseqdump`, `stty`, or installed `libasound.so` is used.

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
Ctrl-C, SIGTERM, and terminal restoration with no host utilities on PATH.
Live note delivery additionally needs a hardware check on a host with `/dev/snd/seq`.
