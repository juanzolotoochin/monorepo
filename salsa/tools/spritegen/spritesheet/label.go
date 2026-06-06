package spritesheet

import (
	"image"
	"image/color"
	"image/draw"

	"golang.org/x/image/font"
	"golang.org/x/image/font/basicfont"
	"golang.org/x/image/math/fixed"
)

var labelFace = basicfont.Face7x13

const labelCharW = 7  // basicfont.Face7x13 advance per glyph
const labelCharH = 13 // basicfont.Face7x13 glyph height

// labelBestScale returns the largest integer scale where the rendered text
// fits within a w×h box (at least 1×).
func labelBestScale(text string, w, h int) int {
	textW := font.MeasureString(labelFace, text).Round()
	if textW == 0 {
		return 1
	}
	scaleH := h / labelCharH
	scaleW := w / textW
	s := scaleH
	if scaleW < s {
		s = scaleW
	}
	if s < 1 {
		s = 1
	}
	return s
}

// renderLabel draws text as large as possible inside a w×h NRGBA image using
// nearest-neighbor upscaling. Background is transparent; text is dark gray.
func renderLabel(text string, w, h int) *image.NRGBA {
	s := labelBestScale(text, w, h)
	ascent := labelFace.Metrics().Ascent.Round()
	textW := font.MeasureString(labelFace, text).Round()

	// Render at 1× into a minimal bitmap.
	small := image.NewNRGBA(image.Rect(0, 0, textW, labelCharH))
	draw.Draw(small, small.Bounds(), image.Transparent, image.Point{}, draw.Src)
	(&font.Drawer{
		Dst:  small,
		Src:  image.NewUniform(color.NRGBA{40, 40, 40, 255}),
		Face: labelFace,
		Dot:  fixed.P(0, ascent),
	}).DrawString(text)

	// Nearest-neighbor scale-up.
	sw, sh := textW*s, labelCharH*s
	scaled := image.NewNRGBA(image.Rect(0, 0, sw, sh))
	for sy := range labelCharH {
		for sx := range textW {
			c := small.NRGBAAt(sx, sy)
			for dy := range s {
				for dx := range s {
					scaled.SetNRGBA(sx*s+dx, sy*s+dy, c)
				}
			}
		}
	}

	// Center in the output cell.
	out := image.NewNRGBA(image.Rect(0, 0, w, h))
	draw.Draw(out, out.Bounds(), image.Transparent, image.Point{}, draw.Src)
	ox := (w - sw) / 2
	oy := (h - sh) / 2
	draw.Draw(out, image.Rect(ox, oy, ox+sw, oy+sh), scaled, image.Point{}, draw.Over)
	return out
}
