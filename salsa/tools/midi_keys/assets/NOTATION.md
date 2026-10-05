# MIDI Keys Symbols

Bazel generates the music-symbol and chord-lettering Rust crates from pinned
font inputs. The normal `bazel build //salsa/tools/midi_keys` build invokes
`assets:extract_music_glyphs` through a declared action; no manual regeneration
or generated Rust sources in Git are needed.

The action uses the repository's Python 3.11 toolchain and the hash-locked
`fonttools` dependency in `requirements_lock.txt` (currently 4.62.1). It receives
both fonts and output paths explicitly. The generator does not download fonts
or discover installed fonts. The repository-wide rules_python Bash bootstrap
starts the hermetic interpreter without host Python. Bazel verifies fonts during
repository resolution and caches generation by its declared inputs and tool.

Build only the outlines with:

```
bazel build //salsa/tools/midi_keys/assets:glyphs
```

Outputs live under `bazel-bin/salsa/tools/midi_keys/assets/generated/`.
The Rust renderer links the generated crates; Python and the original fonts are
not runtime dependencies.

## Music symbols

The embedded polygon outlines are derived from Bravura by Steinberg Media
Technologies GmbH, licensed under OFL-1.1. The derived collection is named
MIDI Keys Symbols. The license and original font change log accompany it.

Source: https://github.com/steinbergmedia/bravura
Revision: 37b194378b710cc40e406ab6c4b07608bb9548ae
File: redist/otf/Bravura.otf
SHA-256: cdf0f893ee1fdb64b7f6713d71ee0dcfc349c0ac01429a8e451b01a9e79f5f3b

## Chord lettering

The derived MIDI Keys Chord Lettering subset uses Liberation Sans 2.1.5,
Copyright Google Corporation (2010) and Red Hat, Inc. (2012), OFL-1.1.
See `LIBERATION-LICENSE.txt`.

Upstream: https://github.com/liberationfonts/liberation-fonts
Release: https://github.com/liberationfonts/liberation-fonts/files/7261482/liberation-fonts-ttf-2.1.5.tar.gz
Archive SHA-256: 7191c669bf38899f73a2094ed00f7b800553364f90e2637010a69c0e268f25d0
File: LiberationSans-Regular.ttf
File SHA-256: 76d04c18ea243f426b7de1f3ad208e927008f961dc5945e5aad352d0dfde8ee8

Chord names use the same vector coordinates, supersampling, and image as their
staff. They stay anchored to their musical tick when resized or paginated.
