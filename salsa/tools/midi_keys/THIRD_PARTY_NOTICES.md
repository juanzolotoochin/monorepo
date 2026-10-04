# Piano dependencies and attribution

## Salamander Grand Piano

- Original recordings: Alexander Holm, Yamaha C5, Salamander Grand Piano V3.
- SF2 conversion: Roberto, FreePats project, V3+20200602.
- Source: https://freepats.zenvoid.org/Piano/acoustic-grand-piano.html
- License: Creative Commons Attribution 3.0 Unported,
  https://creativecommons.org/licenses/by/3.0/
- Pinned archive SHA-256:
  `15edb061d7ba60d58332f72dba8f8ce40988048cc703f935e6320f37d650e213`.

The application uses the SF2 bank without changing it. Its original `readme.txt`
is preserved in the external repository `@salamander_piano`. The SF2 conversion
does not include all of the original SFZ release/pedal/hammer-noise features.
The sample archive is downloaded into Bazel's external cache, not committed here.

## RustySynth

- Version: 1.3.7.
- Source: https://github.com/sinshu/rustysynth
- License: MIT (`LICENSE.txt` in the downloaded crate).
- Crate integrity is pinned in `Cargo.lock`.

## miniaudio

- Version: 0.11.25.
- Source: https://github.com/mackron/miniaudio
- License: MIT or public domain; see the license in the pinned source archive.
- Archive SHA-256:
  `b900edcffe979816e2560a0580b9b1216d674b4f17fbadeca8f777a7f8ab0274`.

When redistributing, retain the upstream license files and the piano attribution
with the binary and its runfiles.
