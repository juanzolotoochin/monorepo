"""Rebuild music_glyphs.rs from the pinned Bravura OTF; development tool only.

Requires fonttools==4.60.1. See NOTATION.md for source/hash and license.
Usage: python extract_music_glyphs.py Bravura.otf ../src/music_glyphs.rs
"""
import hashlib
from pathlib import Path
import sys
from fontTools.ttLib import TTFont
from fontTools.pens.basePen import BasePen

source = Path(sys.argv[1])
assert hashlib.sha256(source.read_bytes()).hexdigest() == "cdf0f893ee1fdb64b7f6713d71ee0dcfc349c0ac01429a8e451b01a9e79f5f3b"
font = TTFont(source)
glyphs = font.getGlyphSet()
cmap = font.getBestCmap()
class Flatten(BasePen):
    def __init__(self):
        super().__init__(glyphs)
        self.contours = []
    def _moveTo(self, p): self.contours.append([p])
    def _lineTo(self, p): self.contours[-1].append(p)
    def _closePath(self): pass
    def _curveToOne(self, a, b, c):
        start = self._getCurrentPoint()
        for step in range(1, 17):
            t = step / 16
            self._lineTo(tuple((1-t)**3*start[k]+3*(1-t)**2*t*a[k]+3*(1-t)*t*t*b[k]+t**3*c[k] for k in [0,1]))
    def _qCurveToOne(self, a, b):
        start = self._getCurrentPoint()
        for step in range(1,17):
            t = step / 16
            self._lineTo(tuple((1-t)**2*start[k]+2*(1-t)*t*a[k]+t*t*b[k] for k in [0,1]))

symbols = {"TREBLE":0xe050,"BASS":0xe062,"BLACK":0xe0a4,"HALF":0xe0a3,"WHOLE":0xe0a2,"SHARP":0xe262,"FLAT":0xe260,"NATURAL":0xe261,"DOUBLE_SHARP":0xe263,"DOUBLE_FLAT":0xe264,"REST_QUARTER":0xe4e5,"REST_HALF":0xe4e4,"REST_WHOLE":0xe4e3,"REST_EIGHTH":0xe4e6,"FLAG_UP":0xe240,"FLAG_DOWN":0xe241,"FOUR":0xe084}
output = ['// Generated MIDI Keys Symbols: outlines derived from Bravura (OFL-1.1).', '// Copyright Steinberg Media Technologies GmbH. See assets/BRAVURA-LICENSE.txt.', '// Do not edit; regenerate with assets/extract_music_glyphs.py.', '#![allow(clippy::approx_constant)]']
for name, code in symbols.items():
    pen = Flatten()
    glyphs[cmap[code]].draw(pen)
    contours = []
    for contour in pen.contours:
        points = ','.join(f'({x/250:.4f},{-y/250:.4f})' for x,y in contour)
        contours.append('&['+points+']')
    output.append('#[rustfmt::skip]')
    output.append(f'pub const {name}: &[&[(f64,f64)]] = &[{",".join(contours)}];')
Path(sys.argv[2]).write_text('\n'.join(output)+'\n')

# Optional chord lettering, normalized to em units for the same vector renderer.
if len(sys.argv) > 3:
    chord_source = Path(sys.argv[3])
    assert hashlib.sha256(chord_source.read_bytes()).hexdigest() == "baccc64becc3eb7d104b7c84d99f5314a0a1f896e2b3ea6c2f22fc08d2003bee"
    font = TTFont(chord_source)
    glyphs = font.getGlyphSet()
    cmap = font.getBestCmap()
    units = font["head"].unitsPerEm
    output = ["// Generated MIDI Keys Chord Lettering, derived from Liberation Sans 2.1.5 (OFL-1.1).", "// Copyright Google Corporation (2010), Red Hat, Inc. (2012).", "// See assets/LIBERATION-LICENSE.txt and assets/NOTATION.md.", "#![allow(clippy::approx_constant)]"]
    characters = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789#()+/- "
    for char in characters:
        pen = Flatten()
        glyphs[cmap[ord(char)]].draw(pen)
        contours = []
        for contour in pen.contours:
            points = ','.join(f'({x/units:.5f},{-y/units:.5f})' for x,y in contour)
            contours.append('&['+points+']')
        output += ['#[rustfmt::skip]', f'const C{ord(char)}: &[&[(f64,f64)]] = &[{",".join(contours)}];']
    output += ['#[rustfmt::skip]', "pub fn glyph(ch: char) -> Option<(f64, &'static [&'static [(f64, f64)]])> {", '    match ch {']
    for char in characters:
        advance = font['hmtx'][cmap[ord(char)]][0] / units
        output.append(f"        '{char}' => Some(({advance:.5f}, C{ord(char)})),")
    output += ['        _ => None,', '    }', '}']
    Path(sys.argv[2]).with_name('chord_glyphs.rs').write_text('\n'.join(output)+'\n')
