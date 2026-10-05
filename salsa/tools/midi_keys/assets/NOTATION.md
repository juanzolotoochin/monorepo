# MIDI Keys Symbols

The embedded polygon outlines in `src/music_glyphs.rs` are derived from the
Bravura font by Steinberg Media Technologies GmbH, licensed under OFL-1.1.
The derived collection is named MIDI Keys Symbols. License and original font
change log accompany these outlines. The original font is not needed at runtime.

Source: https://github.com/steinbergmedia/bravura
Revision: 37b194378b710cc40e406ab6c4b07608bb9548ae
File: redist/otf/Bravura.otf
SHA256: cdf0f893ee1fdb64b7f6713d71ee0dcfc349c0ac01429a8e451b01a9e79f5f3b

Regenerate with Python and fonttools==4.60.1:

```
python extract_music_glyphs.py /path/to/Bravura.otf ../src/music_glyphs.rs
```

The script verifies the source hash. No downloads, font discovery, Python, or
external rendering programs are used by the Bazel build or running application.

## Chord lettering

`src/chord_glyphs.rs` embeds vector letter outlines derived from Liberation Sans
2.1.5. The derived subset is named MIDI Keys Chord Lettering. Copyright Google
Corporation (2010) and Red Hat, Inc. (2012), OFL-1.1; see
`LIBERATION-LICENSE.txt`. Upstream: https://github.com/liberationfonts/liberation-fonts

Source file: `LiberationSans-Regular.ttf`, version 2.1.5.
SHA-256: `baccc64becc3eb7d104b7c84d99f5314a0a1f896e2b3ea6c2f22fc08d2003bee`.

To regenerate both outline collections, pass the verified lettering font as the
optional third argument:

```
python extract_music_glyphs.py Bravura.otf ../src/music_glyphs.rs LiberationSans-Regular.ttf
```

Chord names use the same vector coordinates, supersampling, and image as their
staff. They stay anchored to their musical tick when resized or paginated.
No host font lookup occurs during build or runtime.
