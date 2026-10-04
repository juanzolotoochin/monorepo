# MIDI Keys

A Rust ear-training and theory program for Linux, played through a MIDI keyboard
with a sampled Yamaha C5. The trainer chooses the next exercise from a dependency
graph of specific skills, records performance, and saves after every result.
There is no exercise picker. `--free-play` opens the original Ratatui piano,
including held-note chips, velocity, sustain, and a scrolling note trail.
Middle C is C4 (MIDI 60).

From the repository root:

```sh
# List MIDI inputs, then connect to a port from that list.
bazel run //salsa/tools/midi_keys -- --list
bazel run //salsa/tools/midi_keys -- --port 24:0

# Train: tap and release a key to select the MIDI device, then follow the prompt.
bazel run //salsa/tools/midi_keys

# Free piano / visualizer, or a separate learner profile.
bazel run //salsa/tools/midi_keys -- --free-play
bazel run //salsa/tools/midi_keys -- --profile /path/to/learner.json

# Play the piano demo without a MIDI device.
bazel run //salsa/tools/midi_keys -- --demo

# Visualize silently, print a frame, or export an eight-second piano preview.
bazel run //salsa/tools/midi_keys -- --free-play --silent
bazel run //salsa/tools/midi_keys -- --snapshot
bazel run //salsa/tools/midi_keys -- --render-demo /tmp/piano.wav
```

Build with `bazel build //salsa/tools/midi_keys`, using the repository's normal
configuration. After building, the executable is also available at
`bazel-bin/salsa/tools/midi_keys/midi_keys`.

## Adaptive training

All musical answers use MIDI. For untimed exercises, after the last key is
released, a 450 ms pause submits a complete answer. Timed phrases finish at their
scheduled end, including rests and held notes; an empty fixed-time answer counts
as a missed attempt. Enter can submit sooner, or after a partial answer.
Feedback advances automatically after two seconds for an independent correct
answer, or four seconds for a mistake or assisted answer. Enter continues sooner.
The next prompt waits until all physical keys are released.
The device-selection note is not counted as an answer.

The recent-exercise log shows the prompt, your played notes, and ✓/✗ results,
newest first. Hint-assisted answers are marked. New results retain their prompt
and note spelling across restarts; older saved results lack the original prompt.
Written exercises use contextual note names (for example, C down a major 2nd
shows C–Bb), including in keyboard labels and feedback.

| Key | Training action |
| --- | --- |
| `r` / `R` | Replay or restart the listening prompt/count-in, and clear the current answer |
| Backspace | Clear an answer and try again before submission |
| `h` | Show a hint; the attempt receives no mastery credit |
| `x` | Record "don't know" and show feedback |
| Enter | Submit / continue after feedback |

The curriculum contains 6,645 separately tracked skill variants. Keys, scale
families, directions, listening/construction, and tempos have independent scores:

- Interval construction and recognition, including ascending, descending, and
  simultaneous presentations. Construction specifies a starting note and
  direction, accepts any starting octave, and requires the exact interval
  (G3–A3 and G4–A4 are major seconds; G3–A4 is not).
  Recognition accepts any two notes with the same
  interval, in either direction, without requiring absolute pitch.
- Chord construction and recognition in all twelve keys, progressing from
  triads to seventh and ninth chords. Quality recognition accepts any root and
  inversion, including octave doubling, but not missing or extra chord tones.
- Construction and recognition of chord inversions. The lowest played pitch
  determines the inversion. Symmetrical augmented and diminished-seventh chords
  have construction exercises but no ambiguous inversion-recognition exercises.
- Scale construction and listening recognition: major, natural minor, major and
  minor pentatonic, harmonic minor, jazz melodic minor (same notes both ways),
  Dorian, Mixolydian, Lydian, Phrygian, Locrian, and whole tone. Construction has
  ascending, descending, and both-direction variants, in any starting octave.
  Untimed, 60, 90, and 120 BPM are separate skills. Timed scales have four
  count-in beats and an ongoing piano pulse. Note onsets must stay within a
  quarter-beat of the target grid, measured from the first played note; both
  pitch and timing must pass. Scale recognition accepts the heard pattern
  transposed to any starting note.
- Tonal foundations: find the tonic of a major/minor progression, then recognize
  individual notes after a scale reference. Tonic and fifth precede the third,
  followed by the other scale tones. Every tone in every scale family is tracked
  independently; hearing the key is never treated as proof of playing it.
- Two-, three-, five-, and eight-note dictation in each scale family. Two-note
  phrases use tonic/fifth; three-note phrases add the third; longer phrases
  require the remaining tones. Major and minor develop separately. Melodies
  accept a whole-octave shift but still require the correct contour and intervals.
- Major/minor motifs: repeated notes, steps, leaps, contour, and transposition.
  Separate rhythmic dictation scores pitch, attacks, and held lengths.
- Rhythm: pulse, eighth notes, rests, ties/long holds, syncopation, and maintaining
  a pulse after the click disappears, at 60/90/120 BPM. Answers use any single
  repeated MIDI key. Meter exercises ask for downbeats in 2/4, 3/4, and 4/4.
- Harmony in major/minor: tonic-triad resolutions, hearing diatonic chord
  functions, two-/four-chord progressions, tonic/half-cadence phrases, bass lines,
  and roots of inverted chords. Chord phrases require three-note attacks in
  order; chord tones within each attack can arrive in any order within 180 ms.
- Voicing: 1–7 shells, 3–5 pairs, and I–IV–V–I voice leading. The latter accepts
  inversions while limiting each sorted voice's movement to five semitones.
- Harmonization: one melody note, then four notes, with any diatonic triad
  containing each corresponding melody note accepted. This is an introductory
  membership rubric, not a claim that all accepted progressions sound equally good.
- Accompaniment: four, then twelve bars of melody over a displayed diatonic chord
  chart, using 1–7 or 3–5 voicings at 60 BPM. Play on each bar's downbeat and hold
  3½ beats. These are guided accompaniment exercises, not free-form harmonization
  or a stylistic blues arrangement.
- Bass/melody coordination: hold a bass beneath four upper notes, then change
  bass across four bars. This explicitly specifies registers; MIDI can verify
  overlapping notes and timing, but cannot identify which physical hand was used.
- Advanced contextual intervals: hear a major scale and two notes, then play
  the actual pitch classes within that key, in order, in any octave.

Timed phrase grading separates pitch, onset timing, and physical key-hold
duration. Echo exercises start when you play; downbeat, disappearing-click,
coordination, and accompaniment exercises are anchored to the supplied count-in.
Backing audio continues while MIDI answers are collected. Replaying restarts the
entire timeline and clears the draft; panic cancels playback and its deadline.
Sustain-pedal tails do not count as holding a key. Chord-attack tolerance is
180 ms; timed-phrase onset/release tolerances are 60–250 ms depending on the task.

Listening prompts are never written to the visual keyboard or note trail.
The UI cannot access the grading answer through its read-only training view.
The keyboard is hidden below 34 terminal rows to leave room for instructions.
In free-play mode, the original compact layout is preserved.
Below the 64×22 training minimum, training pauses and silences its backing audio;
it never switches to a free-play-looking screen while continuing hidden exercises.
Resizing back restarts an unfinished prompt without scoring it. Saved feedback
keeps its remaining display time.

### Mastery policy

These are explicit first-version heuristics, not calibrated psychometric scores:

- Each skill starts at 0/10. An independent correct answer updates the score
  to `0.8 * old_score + 2`; a wrong or assisted answer uses `0.8 * old_score`.
- A prerequisite needs at least six attempts and a score of 7/10 to unlock a
  dependent skill. The first qualification is remembered; a later setback
  requests practice without repeatedly closing that path. Introduced skills
  also remain available. Scores are never copied between related skills.
- A skill is established at 9/10 with at least twelve attempts and a current
  streak of five independent successes. It leaves regular practice.
- An established skill returns after fourteen days or when a direct dependent
  is answered incorrectly. Related mistakes request review; they do not silently
  alter the prerequisite's score.
- Requested reviews and retention checks receive priority. Waiting practice has
  uncapped aging, with slower aging for consolidation than initial fluency.
  New skills are chosen by stage, then oldest prerequisite qualification, then
  authored order. Introductions are separated by six completed answers and pause
  at twelve skills below basic fluency or twenty-four unfinished skills. An idle
  learner does not wait merely to satisfy the introduction cadence.
  One result updates one target skill. Pitch and timing evidence are recorded
  separately even when both are required for success.

The root exercise is constructing an ascending major second. Introduction order
prioritizes tonal foundations and short phrases. Scale families unlock from
relevant prerequisites in the current key; no family requires mastery of all
twelve keys of another family. Existing introduced skills and saved mastery are
preserved, while new listening and rhythm skills begin with their own evidence.
Stages cover whole families: first vocabulary (seconds, thirds, fourths, fifths,
octaves, early tonal hearing, triads and pulse), then the remaining intervals and
foundations, then fluency and longer phrases in familiar keys, then broader key
transfer and advanced material. Every prerequisite inherits the earliest stage
that needs it, including chord prerequisites in other keys. Interval listening
directions are parallel skills after construction, rather than descending
recognition waiting for ascending recognition.

An opt-in audit uses the real scheduler with a fresh in-memory learner:

```sh
bazel build //salsa/tools/midi_keys:trainer_test
bazel-bin/salsa/tools/midi_keys/trainer_test --ignored --nocapture report_learning_paths
```

Basic seconds and thirds in both listening directions are part of the early
foundation, even when isolated interval recognition is not a hard prerequisite
for a melody. Two-note melodies introduce both directions between tonic and
fifth. Three-note melodies require separate ascending and descending two-note
listening practice within the tonic triad; five-note melodies additionally
require both directions between scale tones. These contextual skills depend on
the individual tones they use, and remain separate across keys and scale families.
Chord progressions require chord-function recognition (which in turn requires
chord-quality recognition) and bass-line listening. Bass lines and four-chord
progressions vary among four diatonic phrases using the taught functions.
Cadences require only tonic and dominant function recognition. Four-note bass
lines build on three-note phrases and scale movement in both directions; they
do not wait for five-note melodic dictation.

Run the audit for current cumulative scheduler counts; they are not human
learning-time measurements. It excludes calendar-based retention reviews.
Current mastery allows replays and draft retries, so it does not certify success
on the first listen or generalization to arbitrary unfamiliar chord progressions.
Existing evidence and skill IDs are preserved. Previously introduced skills
remain available after curriculum changes; new prerequisites do not erase scores.

### Persistence and architecture

The default profile is `$XDG_DATA_HOME/midi_keys/learner.json`, falling back to
`~/.local/share/midi_keys/learner.json`. Use `--profile FILE` for another learner.
The versioned JSON stores mastery, timestamps, attempts, assistance, pitch and
timing/duration results, and timestamped MIDI note-on/off evidence. It is updated through
a synced temporary file, atomic rename, and directory sync before feedback or
advancement. A process lock prevents concurrent writers. Invalid JSON or an
unsupported schema version produces an error without overwriting the profile.
Quitting midway through an exercise does not score it. Completed attempts are
retained; the initial JSON backend rewrites the profile per result.

The training library has no terminal or audio dependencies:

- `curriculum.rs`: stable skill IDs, task parameters, prerequisites, DAG validation.
- `courses.rs`: tonal, rhythm, and harmony branches and introduction order.
- `music.rs`: shared scale families, degrees, and diatonic chord vocabulary.
- `exercise.rs`: stimulus generation and pitch/tempo grading contracts.
- `practice.rs`: tonal references, rhythm examples, and phrase/backing generation.
- `performance.rs`: ordered chord frames, onset, voice-leading, and hold grading.
- `learner.rs`: evidence, mastery policy, scheduling, and atomic profile storage.
- `trainer.rs`: private session state and the waiting/listening/answer/feedback
  lifecycle; only a read-only presentation view is exposed to the UI.

New curriculum entries should append stable IDs rather than renaming existing
ones. Additive persisted fields have backward-compatible defaults; incompatible
schema changes require a versioned migration. Sight reading, expressive dynamics,
pedaling, unrestricted improvisation, and richer stylistic accompaniment remain
future work. Scheduling and timing tolerances are initial heuristics, not
pedagogically or perceptually calibrated measurements.

## Free piano and visualizer

With no arguments, the app listens to all available inputs. The first positive-velocity
note-on selects its device and immediately highlights that note. Note releases,
MIDI clocks, and controllers do not select a device. Other inputs are disconnected
after selection. `--port` still lets you choose explicitly.

Use a Unicode terminal with true color support, at least 64 columns by 22 rows.
Keys use three columns and at most six rows. Widening the window adds keys
immediately, including partial octaves: about three octaves at 80 columns,
five at 120, and the maximum eight at 179 columns. Auto-follow uses the entire visible range. The layout
adapts to resizing; at 32 rows it adds a six-second note trail and instrument
details. The display
refreshes at 30 fps using incremental updates; audio keeps playing while resizing.
The trail uses Unicode half-blocks for two semitones per terminal row, with
octave and range labels. Its pitch window follows recent notes; notes outside
that window are hidden rather than collapsed onto an edge.
Bars extend while keys are held and scroll away after release. Like the keyboard
highlights, durations show physical key holds rather than sustain-pedal tails.

| Key | Action |
| --- | --- |
| `[` / `]` | Move the view down / up one octave, disabling auto-follow |
| `f` | Follow newly played notes outside the visible range |
| `-` / `+` | Lower / raise piano volume |
| `m` | Mute / unmute piano audio |
| `n` | Show / hide note names on the keyboard (hidden by default) |
| Space | Panic: silence all notes, release sustain, clear highlights |
| `q` or Ctrl-C | Quit and restore the terminal |

Pass `--note-names` to start with keyboard labels enabled. The `n` toggle applies
to the current session. Held-note chips and the range labels remain visible.

To diagnose unexpected sound during explicit free play, use a new trace file:

```sh
bazel run //salsa/tools/midi_keys -- --free-play --trace-events /tmp/midi-free-play.log
```

The trace records the process ID, mode, and timestamped events sent to the piano,
tagged as `midi/control`, `training`, or `demo`. It does not record audio. Explicit
`--free-play` never constructs a training session; a free-play trace should contain
no `training` or `demo` events. Existing trace files are not overwritten. The
`--demo` mode plays automatic example chords and cannot be combined with `--free-play`.

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
bazel test //salsa/tools/midi_keys:all
```

Rust tests cover chords, independent channels, velocity-zero release, MIDI
reset controls, invalid messages, the full note range, and rendering. PTY tests
exercise the real executable, octave controls, terminal resizing, normal exit,
Ctrl-C, SIGTERM, and terminal restoration with no host utilities on PATH, using
`--silent` so no physical audio device is needed. The offline piano test renders
the real sample bank and verifies silence, velocity dynamics, sustain, release,
panic, and volume. `--render-demo` also works without a MIDI or audio device.
Live note delivery additionally needs a hardware check on a host with `/dev/snd/seq`.

UI tests render wide, compact, and undersized terminals into Ratatui's test
backend. To export the actual cell layout as an SVG without MIDI or audio:

```sh
bazel run //salsa/tools/midi_keys:ui_preview -- /tmp/midi-keys.svg 120 38
bazel run //salsa/tools/midi_keys:ui_preview -- /tmp/trainer.svg 120 38 --training
```

Trainer tests cover graph validity and reachability, mastery/unlock/review
behavior, transposition, inversions, extensions, tempo grading, count-in input
isolation, exact/contextual pitch rules, profile locking, corruption, save
failures, duplicate submission, and persistence across restart.

To audit curriculum pacing without MIDI, audio, or profile writes:

```sh
bazel build //salsa/tools/midi_keys:trainer_test
MIDI_AUDIT_OUTPUT_DIR=/tmp/midi-audit \
  bazel-bin/salsa/tools/midi_keys/trainer_test --ignored --nocapture report_ninety_percent_distribution
MIDI_AUDIT_PROFILE=/path/to/profile-snapshot.json MIDI_AUDIT_TRUSTED=150 \
  MIDI_AUDIT_OUTPUT_DIR=/tmp/midi-audit \
  bazel-bin/salsa/tools/midi_keys/trainer_test --ignored --nocapture report_trusted_history_continuation
MIDI_AUDIT_OUTPUT_DIR=/tmp/midi-audit \
  bazel-bin/salsa/tools/midi_keys/trainer_test --ignored --nocapture validate_exported_pacing
```

The first audit runs 20 fresh learners at 90% accuracy. The second replays only
the specified contiguous history prefix in memory, then runs 20 continuations
each at 90% and the prefix's observed accuracy (rounded down to a whole percent).
The validator checks all 60 interval variants and all 160 foundation skills,
including delayed/missing introductions, missing mastery events, duplicate rows,
and inconsistent exercise numbers. Its pacing guardrails apply to these
86–90% scenarios and are engineering regression limits, not learning benchmarks.
Exports include a fingerprint of the scheduler and curriculum sources; the
validator rejects old results after a policy change.
`MIDI_AUDIT_TRIALS` allows smaller exploratory runs (the validator requires 20).
`MIDI_AUDIT_ACCURACY=90` or the observed percentage selects one continuation
scenario so independent audits can run concurrently.
Optional JSON exports contain every skill in every trial, its first exercise
number and first mastery exercise number, the random seed, and the run's ending
exercise number. Counts include the trusted prefix. Null means not reached by
the end of that run; mastery is the first threshold crossing, not permanent
mastery. Each run continues until every foundation skill and the five reported
milestones have reached mastery, or stops at 10,000 exercises. These are not
full-curriculum completion simulations; the validator reports missing foundations.
Accuracy is independent and constant; the simulated clock is fixed, excluding
calendar-based retention reviews. These audits measure scheduler behavior, not
human learning speed.

Export the production dependency graph for visualization without reading a profile:

```sh
MIDI_GRAPH_OUTPUT=/tmp/midi-curriculum.json \
  bazel-bin/salsa/tools/midi_keys/trainer_test --ignored export_curriculum_graph
```

The compact JSON contains the policy fingerprint, prerequisite thresholds, and
nodes as `[id, title, stage, prerequisite_node_indices]`. Edges point from each
prerequisite to the skill that requires it.
