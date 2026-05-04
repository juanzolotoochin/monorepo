package spritesheet

import (
	"image"
	"image/color"
	"sort"
)

const bgTolerance = 30
const contentThreshold = 0.02
const cornerPatchSize = 5

// minGapRows is the minimum number of consecutive background-only rows required
// to be treated as a real gap between sprite rows. Smaller gaps (e.g. single-pixel
// background lines within a row) are bridged over.
const minGapRows = 5

// Info holds the analysis results for a sprite sheet.
type Info struct {
	Width       int
	Height      int
	Background  color.RGBA
	BgTolerance int
	RowCount    int
}

// Analyze detects the background color and counts state rows in img.
func Analyze(img image.Image) (*Info, error) {
	bounds := img.Bounds()
	bg := detectBackground(img)
	rows := countRows(img, bg, bgTolerance)
	return &Info{
		Width:       bounds.Dx(),
		Height:      bounds.Dy(),
		Background:  bg,
		BgTolerance: bgTolerance,
		RowCount:    rows,
	}, nil
}

func detectBackground(img image.Image) color.RGBA {
	bounds := img.Bounds()
	p := cornerPatchSize
	origins := [4][2]int{
		{bounds.Min.X, bounds.Min.Y},
		{bounds.Max.X - p, bounds.Min.Y},
		{bounds.Min.X, bounds.Max.Y - p},
		{bounds.Max.X - p, bounds.Max.Y - p},
	}
	var rs, gs, bs []int
	for _, o := range origins {
		for dy := 0; dy < p; dy++ {
			for dx := 0; dx < p; dx++ {
				r, g, b, _ := img.At(o[0]+dx, o[1]+dy).RGBA()
				rs = append(rs, int(r>>8))
				gs = append(gs, int(g>>8))
				bs = append(bs, int(b>>8))
			}
		}
	}
	sort.Ints(rs)
	sort.Ints(gs)
	sort.Ints(bs)
	mid := len(rs) / 2
	return color.RGBA{R: uint8(rs[mid]), G: uint8(gs[mid]), B: uint8(bs[mid]), A: 255}
}

func countRows(img image.Image, bg color.RGBA, tolerance int) int {
	bounds := img.Bounds()
	width := bounds.Dx()
	height := bounds.Dy()

	// First pass: compute per-row content flag.
	isContent := make([]bool, height)
	for i, y := 0, bounds.Min.Y; y < bounds.Max.Y; i, y = i+1, y+1 {
		nonBg := 0
		for x := bounds.Min.X; x < bounds.Max.X; x++ {
			r, g, b, _ := img.At(x, y).RGBA()
			c := color.RGBA{R: uint8(r >> 8), G: uint8(g >> 8), B: uint8(b >> 8)}
			if absDiff(int(c.R), int(bg.R)) > tolerance ||
				absDiff(int(c.G), int(bg.G)) > tolerance ||
				absDiff(int(c.B), int(bg.B)) > tolerance {
				nonBg++
			}
		}
		isContent[i] = float64(nonBg)/float64(width) > contentThreshold
	}

	// Second pass: bridge gaps shorter than minGapRows so that thin background
	// lines within a sprite row do not split it into multiple rows.
	merged := make([]bool, height)
	copy(merged, isContent)
	for i := 0; i < height; {
		if !merged[i] {
			// Count consecutive background rows.
			gapStart := i
			for i < height && !merged[i] {
				i++
			}
			gapLen := i - gapStart
			// If the gap is shorter than minGapRows and is surrounded by content
			// on both sides, fill it in.
			if gapLen < minGapRows && gapStart > 0 && i < height {
				for j := gapStart; j < i; j++ {
					merged[j] = true
				}
			}
		} else {
			i++
		}
	}

	// Count rising edges in the merged signal.
	count := 0
	inRow := false
	for _, c := range merged {
		if c && !inRow {
			count++
		}
		inRow = c
	}
	return count
}

func absDiff(a, b int) int {
	if a > b {
		return a - b
	}
	return b - a
}
