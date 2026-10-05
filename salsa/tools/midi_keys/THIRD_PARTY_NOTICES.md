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

## GeneralUser GS sampled drums

- Creator: S. Christian Collins.
- Version: 2.0.3; revision `684543d5e5efaef08d02be50dcda8d552478fa60`.
- Source: https://github.com/mrbumpy409/GeneralUser-GS
- License: GeneralUser GS License v2.0, preserved in runfiles as
  `@generaluser_drums//:license` (`documentation/LICENSE.txt`).
- Archive SHA-256:
  `aef2e2901ab399061ac3765e259ab4c3409bdcb24b7cf814b4b54aaad549dbff`.

The app uses the unmodified SF2's standard GM percussion kit. The license permits
use in software projects; retain its full terms, including the upstream notes
on sample provenance, with redistributions.

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

Spoken count-in: recordings by Dvortygirl from Wikimedia Commons. One, two,
and three are CC BY-SA 3.0; four is public domain. The adapted PCM files retain
those terms. Source links and conversion details: `assets/count-in/NOTICE.txt`.
