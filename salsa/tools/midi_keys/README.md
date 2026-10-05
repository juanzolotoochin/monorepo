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

## Exercise browser

Run `bazel run //salsa/tools/midi_keys -- --exercises` to search and preview any
exercise independently of adaptive training. No learner profile is opened,
locked, or saved; `--profile` and `--free-play` cannot be combined with this mode.
All curriculum exercises are available, including ones not yet unlocked.

To skip the browser, pass an exact skill ID:

```sh
bazel run //salsa/tools/midi_keys -- --exercise rhythm.0.60
bazel run //salsa/tools/midi_keys -- --exercise chord.build.3.11
```

`--exercise ID` uses the same ungraded preview mode and can be combined with
`--port CLIENT:PORT`. Unknown IDs produce an error before opening devices.
Press `b` to return to its category with that exercise selected.

Use Left/Right or Tab to choose a category: intervals, chords and inversions,
scales, tonal hearing, melodies, rhythm and meter, harmony, or accompaniment.
All exercises remains available for a global search. Wide terminals show a
category sidebar; compact terminals show the current category above the search.

Type words from an exercise title or its skill ID (for example `rhythm.0.60`),
use Up/Down to select, and press Enter to try it with the normal MIDI/audio flow.
Search is scoped to the selected category. Backspace edits the search; Ctrl-U
clears it; Ctrl-C quits the browser.
During a preview, `b` returns to the list, `v` generates another variant, and `r`
replays the same variant. Feedback stays visible until Enter starts another
variant or you return to the list. Normal volume, mute, and panic controls remain
available while previewing. Results never change scores, history, or insights.

## Sight reading and lead sheets

The browser has **Sight reading** and **Lead sheets** categories, with 42 new
skills. Right hand, left hand, hands together, and chord-symbol/voicing execution
have separate skill IDs and mastery. Existing profiles need no reset.

- `reading.right.0` / `reading.left.0`: single notes in treble/bass clef.
- Hand levels 1–3: wider registers, steps, and skips, without a tempo requirement.
- Levels 4–6: quarter notes, half notes/rests, and eighth notes/rests at 60 BPM.
- Levels 7–8: G major and F major key signatures; level 9: familiar triads;
  level 10: ties across a barline (one attack, held through the tie).
- `reading.together.0` through `.4`: alternating hands, simultaneous notes,
  melody over held bass, independent rhythms, then chord accompaniment.
- `reading.symbol.ROOT.QUALITY.VOICING`: read C/F/G major-triad symbols, then
  Cmaj7/Fmaj7/G7 root–7th and 3rd–7th shells. Voicing IDs are `triad`, `root7`,
  and `third7`; examples include `reading.symbol.0.0.triad` and
  `reading.symbol.7.4.third7`.
- `reading.lead.0` through `.5`: melody over roots, familiar triads, changing
  triads, root–7th shells, changing shells, then half-note 3rd–7th shells.
  The melody is notated; left-hand notes are inferred from chord symbols and
  the stated voicing/rhythm. Each voicing prerequisite requires score ≥9 and
  at least 12 attempts before the relevant lead sheet is introduced.

For example:

```sh
bazel run //salsa/tools/midi_keys -- --exercise reading.right.0
bazel run //salsa/tools/midi_keys -- --exercise reading.together.2
bazel run //salsa/tools/midi_keys -- --exercise reading.lead.2
```

Use Ghostty or Kitty directly, or inside tmux with `allow-passthrough` enabled.
GNU Screen is not supported. The build embeds licensed
music-symbol outlines; no host fonts, downloads, or external renderer are needed.
Unsupported terminals skip reading skills during adaptive practice and pause
explicit reading previews. They never substitute note names for a reading test.

There is **no target playback before answering**. Start when ready. Timed reading
starts its metronome immediately, before you play, and uses ±180ms attack tolerance.
Listen for as long as you need, then start on any beat. The click keeps its phase;
your first note starts the phrase timer.
Untimed reading completes on the final expected attack; timed phrases wait for
their last held duration.
Press `r` to restart as practice (no first-attempt mastery credit); clearing an
answer after playing also makes it practice. In feedback, `r` plays a model,
`c` pauses advancement, `j/k` scroll details, and Enter continues.

Sight-reading holds are assessed against the full written note length, allowing
detached releases up to half a beat early (capped at half the note value)
and overlap up to a quarter beat late (capped at a quarter of the note value).
At 60 BPM, a whole note accepts 3500–4250 ms and a quarter note 500–1250 ms.
These are practice tolerances, not prescribed articulation: unmarked notes
permit both detached and connected playing. Long bass notes and ties still
require sustained holds; an extra beat of holding is rejected. Attack timing
remains ±180 ms; dedicated rhythm drills retain their own timing and articulation
rules. Playback's 60 ms articulation
gap is not a sight-reading grading target.

Written notes require the notated register. Chord symbols accept the documented
closed-position triad or shell in supported octaves below middle C. Parts use
separate registers (right ≥C4, left <C4), so MIDI evidence can be assigned to each
part; MIDI cannot verify the physical hand or fingering. This initial version
covers short C-major phrases, G/F major staff reading, and the chord vocabulary
above, not arbitrary MusicXML, hand crossing, or unrestricted accompaniment.

Results distinguish right-part pitch reading, left-part pitch/voicing execution,
chord-symbol execution, timing, held lengths, and coordination. Their independent
0–10 states are saved in `reading_components`; per-attempt evidence is stored in
`reading`. A wrong melody does not lower the correct chord component or request
blanket review of known chord prerequisites. Symbol interpretation and voicing
execution share the observable played-chord evidence; the program cannot infer
which mental step caused a wrong chord. Overall skill mastery still requires the
whole exercise to pass. Old history is preserved without inventing reading credit.

## Adaptive training

All musical answers use MIDI. Intervals and chord recognition go directly to
answering. Tonic exercises first allow ungraded exploration: try notes freely,
then press Enter once to start a fresh answer. Fixed-length untimed
answers submit on the final note attack (one for a single pitch, two for an
interval); no second Enter or key release is needed. Chords submit after enough
notes, all keys released, and a 450 ms pause to collect the whole voicing.
Untimed partial answers can pause indefinitely.
Replaying a tonic exercise returns to exploration. Timed phrases finish at their
scheduled end, including rests and held notes; an empty fixed-time answer counts
as a missed attempt. Enter can submit sooner, or after a partial answer.
Feedback advances automatically after two seconds only for an independent, fully
correct answer. Mistakes, imperfect graded answers, and assisted attempts stay
on screen until `r` retries or Enter continues. A prominent red **NEEDS WORK**
banner identifies unsuccessful results; graded results retain their numeric score.
Feedback keeps the exercise’s original theory, score, or rhythm layout. Score
comparisons put Played above Expected; rhythm retains its attack/hold table.
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
| Enter | Start answer during exploration / submit answer / continue after feedback |
| `i` | Switch Insights / recent history on compact terminals |
| `j` / `k` | Browse insights |

### Mixed recognition and Insights

Interval recognition uses seven cumulative sets defined in `src/recognition.rs`:
seconds; add thirds; add perfect fifths and octaves; add perfect fourths; add
sixths; add sevenths; add tritones. Ascending, descending, and simultaneous
presentations progress independently. Every set member must be available before
mixed practice starts. Every other exercise is reserved for mixed
recognition when available; other work continues between those exercises.

Individual scores are retained, but a mastered interval returns in each expanded
set. Set mastery requires evidence for **every** member: at least six answers for
newly introduced intervals and three fresh answers for previously established
intervals, at least 80% independent correctness over its last ten answers in that
set, and correct answers from at least two starting-pitch variants. A successful
check stays credited within that set; a recurring confusion or
three consecutive errors on that interval requests a new check. Evidence does
not transfer between sets or presentations. These are scheduling heuristics, not a
claim of calibrated proficiency or first-listen mastery; replay is still allowed.

Wrong two-note responses record the interval actually played. Three occurrences
of the same directional confusion, making up at least 25% of the last ten source
interval prompts in the current set, trigger extra sampling of both intervals
when they belong to that set. Age-based coverage prevents focused practice from
excluding the rest of the set. Hints do not establish mastery or count as a
confusion; incomplete responses cannot identify an alternative interval.

The Insights panel highlights recently mastered individual skills, established
recognition sets with their presentation, recurring confusions with counts, and
progress through the expanded set. Wide terminals show it alongside history;
compact terminals use `i` to switch panels. `j`/`k` cycle the visible insights.
Insights never reveal the currently playing target interval.

New attempts save a versioned recognition context alongside the existing history.
Old scores and attempts are preserved when loading, but old results without set
context do not establish mixed-set mastery. Recognition state is saved atomically
with the answer. Chord and scale recognition still use their existing individual
skill scheduling; cumulative sets currently apply to intervals.

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
  count-in beats and an ongoing metronome pulse. Note onsets must stay within a
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
- Basic pulse exercises use a continuous sampled kick/snare/hi-hat backing:
  join on any beat and play eight quarter notes using any MIDI notes. There is no
  listening phase; timing is checked against the backing grid and notes should
  be held almost a full beat. The kit requires no additional sample assets.
  In Ghostty or Kitty, pulse exercises display a one-line percussion score
  (two bars of quarter notes) using the Kitty graphics protocol. The renderer
  is built into the binary; no host fonts, image tools, or downloads are needed.
  Other terminals and GNU Screen get a text rhythm score. Staff
  notation for the separate reading curriculum is described below.
  Rhythm results show each expected beat, the played key, early/late attack in
  milliseconds, and played/expected held duration. Missing and extra notes are
  identified separately. Timing and release tolerances match grading; sustain
  pedal duration does not count as holding a key. Press `c` to pause automatic
  advancement, `j`/`k` to scroll the comparison, and Enter to continue. Preview
  results already wait for Enter. A missing beat is aligned by time for feedback,
  although grading still requires the complete ordered answer.
- Rhythm: pulse, eighth notes, rests, ties/long holds, syncopation, and maintaining
  a pulse after the click disappears, at 60/90/120 BPM. Pitches may change between notes; only note count, timing, and
  required held lengths are graded. Meter exercises ask for downbeats in 2/4, 3/4, and 4/4.
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
  chart, using root/seventh or third/fifth voicings at 60 BPM. Chord names
  (e.g. G, C, D) label the bars. Play on each bar's downbeat and hold four beats. These are guided accompaniment exercises, not free-form harmonization
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
- An individual skill is established at 9/10 with at least twelve attempts and a
  current streak of five independent successes. It leaves ordinary practice;
  interval recognition still returns for the mixed-set checks described above.
- An established skill returns after fourteen days or when a direct dependent
  is answered incorrectly. Related mistakes request review; they do not silently
  alter the prerequisite's score.
- Requested reviews and retention checks receive priority. Waiting practice has
  uncapped aging, with slower aging for consolidation than initial fluency.
  New skills are chosen by stage, then oldest prerequisite qualification, then
  authored order. Introductions are separated by six completed answers and pause
  at twelve skills below basic fluency or twenty-four unfinished skills in the
  ordinary practice pool. Mixed recognition uses its own bounded set. An idle
  learner does not wait merely to satisfy the introduction cadence.
  One result updates one target skill and, for interval recognition, its current
  set evidence. Pitch and timing evidence are recorded
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
- `recognition.rs`: cumulative interval sets, contextual mastery, confusion
  detection, mixed sampling, and listening insights.
- `trainer.rs`: private session state and the waiting/listening/answer/feedback
  lifecycle; only a read-only presentation view is exposed to the UI.

New curriculum entries should append stable IDs rather than renaming existing
ones. Additive persisted fields have backward-compatible defaults; incompatible
schema changes require a versioned migration. Expressive dynamics,
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

The keyboard uses **Salamander Grand Piano**, created by Alexander Holm,
with the FreePats SF2 conversion by Roberto. It is a sampled Yamaha C5 with
multiple velocity layers. RustySynth renders it at 48 kHz in stereo, with up to
256 sample voices, subtle reverb, and no chorus. Synth rendering runs on the
audio callback thread; terminal drawing cannot block it on a UI lock.

Metronome and rhythm backing use recorded drum samples from **GeneralUser GS
2.0.3** by S. Christian Collins: closed hi-hat for the steady click, with acoustic
kick and snare for the backing pattern. A separate RustySynth instance renders
the standard GM drum kit without reverb or chorus; these sounds do not use the
piano bank or procedural noise. The immutable upstream revision and archive
SHA-256 are pinned in `MODULE.bazel`. The approximately 62 MiB archive stays in
Bazel's external cache; the roughly 31 MiB SF2 and its license ship as runfiles.

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

## Notation across the curriculum

Notation follows the purpose of each exercise:

| Exercise family | Before answering | After answering |
| --- | --- | --- |
| Scale construction | `guided.` lessons show the scale; the matching existing lesson asks for it from memory | Expected and played pitches |
| Timed scales | From memory, after the untimed scale prerequisites | Expected and played notation |
| Interval/chord construction, inversions, shells | Written instruction without giving away the answer | An example solution and the played answer; accepted voicings/octaves can differ |
| Interval/chord/scale recognition, tonic, scale degrees, melodic dictation, harmonic function/progressions | Listening target stays hidden | Expected and played scores |
| Quarter-note pulse | Percussion notation with continuous drums | Existing detailed attack/hold comparison |
| Voice leading and harmonization | Task/context without a completed solution | An example solution and the played answer |
| Rhythm imitation and meter | Listen first; no printed target | Existing timing comparison |
| Bass/melody coordination | Separate treble and bass staves with explicit offbeat rests | Expected and played scores |
| Accompaniment | Written voicings and chord symbols alongside the backing melody | Expected and played scores |
| Sight reading and lead sheets | Unheard notation; lead sheets give chord symbols rather than written left-hand answers | Expected/played scores plus separate part assessments |

There are 432 additive guided-scale skills, covering each existing untimed
scale/key/direction combination. For example:

```sh
bazel run //salsa/tools/midi_keys -- --exercise guided.scale.0.natural-minor.up
bazel run //salsa/tools/midi_keys -- --exercise harmony.0.major.coordination.true
```

A new learner qualifies on the guided version before the matching memory task.
Previously practiced skills retain their IDs, history, and unlocked status;
no guided mastery is inferred from old attempts. Timed variants still follow
memory practice.

Simple accompaniment keeps its full four- or twelve-bar chord chart visible,
with the current bar highlighted. Other scores paginate in two-bar sections. `p`/`n` browse pages; timed written parts
advance pages with the performance clock until manually browsed. Feedback has
EXPECTED and PLAYED staves on the same horizontal grid. `c` pauses automatic
advancement; browsing feedback pages also pauses it. Exercise previews already
wait for Enter. Untimed notation shows pitch order/voicing without a time
signature; note spacing is not a rhythm requirement. Timed feedback rounds to an
eighth-note grid, so notation is not a substitute for exact timing measurements.

Chord-function lessons now have musical labels rather than `Function(0)`.
The initial dependency chain is tonic → dominant → subdominant → submediant,
before two-chord phrases; the remaining diatonic functions follow. In natural
minor the labels use the actual diatonic qualities (including minor v).
Coordination progresses from one fixed harmony/bar to four changing bars.
The bass is held continuously while the upper notes fall on each `&` in
`1 & 2 & 3 & 4 &`, not in a single serial sequence with the bass.

The renderer uses embedded Bravura-derived outlines and the existing Kitty
image protocol; it adds no runtime font, browser, or system rendering dependency.
See `assets/NOTATION.md` for provenance and the checked glyph-generation process.

### Scores inside tmux

Enable `tmux set -g allow-passthrough on`, or put `set -g allow-passthrough on`
in `~/.tmux.conf`. Start tmux from Ghostty or Kitty so it inherits
`GHOSTTY_RESOURCES_DIR` or `KITTY_WINDOW_ID`, then restart the MIDI app.
The renderer wraps individual graphics chunks and deletion commands in tmux
DCS passthrough. Unicode image placeholders live in the normal TUI cell buffer,
so tmux handles pane offsets, moves, visibility, and text redraws. Images use
process-specific IDs to avoid collisions between MIDI apps in different panes.
The outer terminal must support Kitty Unicode placeholders; tmux must preserve
true-color foreground values (RGB terminal support). Nested multiplexers are
not supported. No tmux settings are changed by the app.

Score panels use compact, bounded heights instead of expanding with the window.
Expected and played answers have separate images with native terminal headings;
chord names are engraved directly above their matching bar or beat, in the same
image as the staff. Embedded vector lettering keeps names sharp and aligned
when resized. The current chord is underlined during playback. Musical notation
is rasterized at twice the vector scene's dimensions (four times the
pixels), with antialiasing. Both comparison images have independent transport
IDs/caches and share the same pitch-staff set and horizontal timing grid.

Timed response scores include rests for silent gaps and missing parts. Red notes
or rests indicate pitch, attack-timing, or hold errors; the text feedback explains
which condition failed. Highlights use the original MIDI timestamps, so errors
remain visible even when notation rounding produces the same written duration.
Missing and extra attacks are aligned by time in reading feedback instead of
shifting every later note's comparison.

### Share a reproducible exercise debug capture

```sh
bazel run //salsa/tools/midi_keys -- --exercise rhythm.0.60 --debug-exercise /tmp/midi-last-exercise.json
```

`--debug-exercise FILE` also works with `--exercises` and with `--silent`.
It is restricted to preview mode and does not read or update your learner profile.
Share the resulting JSON to investigate an exercise or grading issue.

The versioned file contains the exact generated prompt/variant, structured answer
rules and tolerances, expected notes, written scores, planned playback, actual
exercise output events, played answer, grading evidence and results. It also
includes terminal keys, terminal dimensions/audio settings, timing anchors,
and every ALSA input event delivered to the app, with its source and monotonic
receipt timestamp. Events ignored during listening, exploration, hidden UI, or
feedback are retained; so are releases arriving after automatic submission.
Note/controller messages include MIDI bytes; other events include the ALSA event
type and full data payload (including SysEx). This is application-level input
capture, not a USB/MIDI wire recorder. `decoded_input` records interpretation
separately from `midi_input`; do not count them as two physical events.

The file updates atomically about every 250 ms while input changes, immediately
on grading, and before quitting through normal keyboard controls. It includes
incomplete attempts. Starting another exercise/variant replaces the capture;
replaying the same variant retains its event timeline and replaces the latest
grade. Returning to the browser leaves the last exercise file intact. An existing
exercise-debug file may be replaced; unrelated existing files are rejected.
No MIDI/profile history from earlier application runs is included.

### Score rendering performance

Notation uses an even-odd scanline rasterizer with the same subpixel samples as
its reference renderer. Kitty uploads use lossless zlib compression (`o=z`),
including through tmux; the Rust compressor is already pinned in the repository
lockfile. Resolution and antialiasing are unchanged.

A local benchmark of `reading.together.2` measured about 46 ms to rasterize,
compress, encode and frame a new two-staff score (previously about 780 ms).
The resulting terminal stream is about 54 KB rather than 6.18 MB per image.
These measurements exclude terminal I/O and Ghostty rendering. Unchanged images
continue to use the existing cache and send no graphics data.

Reproduce without MIDI hardware:

```sh
bazel build //salsa/tools/midi_keys:ui_test
bazel-bin/salsa/tools/midi_keys/ui_test --ignored --nocapture report_score_render_performance
```

Tests compare scanline pixels with the original reference algorithm and verify
that compressed image payloads decode to the exact original RGB bytes. Timing
numbers are reported by an explicit benchmark, not enforced as flaky CI limits.

Every exercise page displays a selectable `ID: ...` line at the bottom, including
score and rhythm views. Use that ID with `--exercise ID` or in a bug report.

Accompaniment uses a bundled spoken “four, three, two, one” count-in at 60 BPM.
Its result is graded out of 100: correct chords contribute 60%, on-time changes
25%, and sufficient holds 15%, averaged across all bars. Missing bars lose all
three components; extra chord attacks cost one bar's share. Local mistakes stay
highlighted. A 95/100 attempt contributes 95% of full mastery evidence, and 90+
qualifies for a successful independent streak. Assisted attempts give no mastery
credit. Other exercise families retain their existing grading. Old histories
without a performance score continue to load and use their original binary result;
no history is retrospectively rescored.

The spoken count-in now uses MIDI note-on messages into an embedded SoundFont
(bank 0/program 0, keys 60–63 = one–four), through the same RustySynth engine as
the piano and drums. It retains the existing shared event scheduling; SoundFont
playback does not provide automatic speech time-stretching. The shared count-in
policy below selects speech only at tempos where every word fits.

Timed exercise startup is shared across the curriculum. Exercises declare a
count-in through `Exercise::add_count_in`; `tempo.rs` owns the count, deadlines,
and spoken/click selection. Up to 75 BPM, four-beat count-ins use the spoken
SoundFont; faster tempos use clicks to avoid truncating the words. Timed count-ins
and fixed-start performances show the prompt and score and wait for Enter.
Device readiness, resizing, and entering a preview cannot start their clock.
Enter starts the scheduled sequence; panic returns these exercises to preparation.

`evidence.rs` owns recorded MIDI facts and key-release matching, shared by
reading, rhythm, performance grading, and score feedback. Evidence offsets are
relative to capture start, including leading releases. `tempo::BeatClock` derives
metronome deadlines from one monotonic origin rather than accumulating rounded
beat lengths. This prevents drift; audio commands still pass through the app loop
and are not a sample-accurate audio-clock sequencer.

Regression coverage includes every generated count-in, waiting without audio or
grading, Enter and resize behavior, captured MIDI failures, release/retrigger
semantics, and fractional-tempo drift. The trainer tests use an explicit simulated
start when exercising an already-running timeline; separate flow tests exercise
automatic device readiness and the actual Enter entry point.

Retry controls are the same in training and preview: `r` restarts the same
exercise, including after feedback. `e` plays the sight-reading example after
feedback without starting a new attempt. `c` pauses any result to review it.
Retries after seeing the answer are marked assisted and do not earn mastery credit.
At startup, tap and release one MIDI key to select the input device; that note
is only for connection, not an exercise answer. `--port CLIENT:PORT` skips selection.

All exercise families share the same outer training screen. Guided scales,
sight reading, lead sheets, accompaniment, and rhythm render inside the exercise
area, alongside the same history, insights, and piano as theory questions.
At widths of 120 columns or more, history and insights appear together; at
narrower widths, `i` switches the shared panel. The piano appears at 34 rows or
more. These size rules are consistent across exercise families and phases.

Rhythm and meter exercises now use a 0–100 grade, saved in the same optional
`performance_score` field as accompaniment. Attacks contribute 70% and graded
holds 30%; exercises without hold targets use attack credit only. Within the
tolerance they earn full credit; outside it, credit falls linearly over one beat.
A missing or extra attack costs one note's share. Imperfect attempts cap at 99,
and grades of 90 or more qualify as successful for mastery. Imperfect attempts
still wait for explicit retry/next so their feedback can be reviewed.

Rhythm tolerance is one fifth of a beat (minimum 60 ms): ±200 ms at 60 BPM.
The played percussion score appears above the expected score; red notes/rests
and the exact-timing table use the same aligned report as grading. Notation
rounds to eighth-note ticks; small timing errors can therefore have the same
written duration but a red symbol. Compact terminals use colored text notation.
Other exercise families retain their existing grading policies.

Insights summarize the last 24 hours, the last 40 independent attempts, repeated
weaknesses, current mastery evidence, mixed-interval recognition, mastered skills,
and a nearby unlock requirement. Assisted attempts do not establish weaknesses;
a single miss is explicitly distinguished from a pattern. Historical results
retain their original grading; changing tolerances does not rewrite history.
Use **Shift+J / Shift+K** to browse insights even while a result's timing table
uses lowercase `j/k`. The historical profile itself is never regraded by these
summaries.


Build-time assets are generated by Bazel: music and chord glyphs from pinned
font downloads, and the spoken countdown SoundFont from checked-in adapted PCM
recordings. The actions use the repository's pinned Python toolchain and locked
fontTools dependency; they never invoke host Python or discover installed fonts.
The app embeds the generated assets and needs neither Python nor fonts at runtime.
See `assets/NOTATION.md` and `assets/count-in/NOTICE.txt` for provenance.
