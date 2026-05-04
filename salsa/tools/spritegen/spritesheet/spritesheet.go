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

// minGapCols is the minimum number of consecutive background-only columns required
// to be treated as a real gap between sprites. Smaller gaps are bridged over.
const minGapCols = 5

// Info holds the analysis results for a sprite sheet.
type Info struct {
	Width       int
	Height      int
	Background  color.RGBA
	BgTolerance int
	RowCount    int
}

// Row holds the bounding rectangles for one state row's label and sprites.
type Row struct {
	Label   image.Rectangle
	Sprites []image.Rectangle
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

// Slice finds the bounding rectangles of each state row's label and sprites.
// The first content column group in each row is the label; the rest are sprites.
func Slice(img image.Image) ([]Row, error) {
	bg := detectBackground(img)
	rowRanges := findRowRanges(img, bg, bgTolerance)
	rows := make([]Row, 0, len(rowRanges))
	for _, rowRect := range rowRanges {
		colRanges := findColRanges(img, rowRect, bg, bgTolerance)
		if len(colRanges) == 0 {
			continue
		}
		rows = append(rows, Row{
			Label:   colRanges[0],
			Sprites: colRanges[1:],
		})
	}
	return rows, nil
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

// findRowRanges returns the bounding rectangles of each state row.
// Each rectangle spans the full image width and the y-range of the content band.
func findRowRanges(img image.Image, bg color.RGBA, tolerance int) []image.Rectangle {
	bounds := img.Bounds()
	width := bounds.Dx()
	height := bounds.Dy()

	isContent := make([]bool, height)
	for i, y := 0, bounds.Min.Y; y < bounds.Max.Y; i, y = i+1, y+1 {
		nonBg := 0
		for x := bounds.Min.X; x < bounds.Max.X; x++ {
			if !isBackground(img, x, y, bg, tolerance) {
				nonBg++
			}
		}
		isContent[i] = float64(nonBg)/float64(width) > contentThreshold
	}

	merged := make([]bool, height)
	copy(merged, isContent)
	bridgeSmallGaps(merged, minGapRows)

	var ranges []image.Rectangle
	inRow := false
	rowStart := 0
	for i, c := range merged {
		if c && !inRow {
			inRow = true
			rowStart = i
		} else if !c && inRow {
			inRow = false
			ranges = append(ranges, image.Rect(
				bounds.Min.X, bounds.Min.Y+rowStart,
				bounds.Max.X, bounds.Min.Y+i,
			))
		}
	}
	if inRow {
		ranges = append(ranges, image.Rect(
			bounds.Min.X, bounds.Min.Y+rowStart,
			bounds.Max.X, bounds.Max.Y,
		))
	}
	return ranges
}

func countRows(img image.Image, bg color.RGBA, tolerance int) int {
	return len(findRowRanges(img, bg, tolerance))
}

// isBackground checks if the pixel at (x, y) matches the background color within tolerance.
func isBackground(img image.Image, x, y int, bg color.RGBA, tolerance int) bool {
	r, g, b, _ := img.At(x, y).RGBA()
	return absDiff(int(r>>8), int(bg.R)) <= tolerance &&
		absDiff(int(g>>8), int(bg.G)) <= tolerance &&
		absDiff(int(b>>8), int(bg.B)) <= tolerance
}

// bridgeSmallGaps fills in small gaps (sequences of false values) in the flags slice.
// If a gap is shorter than minGap and has content on both sides, it is filled.
func bridgeSmallGaps(flags []bool, minGap int) {
	n := len(flags)
	for i := 0; i < n; {
		if !flags[i] {
			gapStart := i
			for i < n && !flags[i] {
				i++
			}
			if i-gapStart < minGap && gapStart > 0 && i < n {
				for j := gapStart; j < i; j++ {
					flags[j] = true
				}
			}
		} else {
			i++
		}
	}
}

// findColRanges returns tight bounding rectangles of content column groups
// within rowRect.
func findColRanges(img image.Image, rowRect image.Rectangle, bg color.RGBA, tolerance int) []image.Rectangle {
	bounds := img.Bounds()
	// Column scan spans full image width; rowRect restricts only the y range.
	width := bounds.Dx()
	height := rowRect.Dy()

	isContent := make([]bool, width)
	for i, x := 0, bounds.Min.X; x < bounds.Max.X; i, x = i+1, x+1 {
		nonBg := 0
		for y := rowRect.Min.Y; y < rowRect.Max.Y; y++ {
			if !isBackground(img, x, y, bg, tolerance) {
				nonBg++
			}
		}
		isContent[i] = float64(nonBg)/float64(height) > contentThreshold
	}

	merged := make([]bool, width)
	copy(merged, isContent)
	bridgeSmallGaps(merged, minGapCols)

	var colRanges []image.Rectangle
	inCol := false
	colStart := 0
	for i, c := range merged {
		if c && !inCol {
			inCol = true
			colStart = i
		} else if !c && inCol {
			inCol = false
			colRanges = append(colRanges, tightenRect(img, image.Rect(
				bounds.Min.X+colStart, rowRect.Min.Y,
				bounds.Min.X+i, rowRect.Max.Y,
			), bg, tolerance))
		}
	}
	if inCol {
		colRanges = append(colRanges, tightenRect(img, image.Rect(
			bounds.Min.X+colStart, rowRect.Min.Y,
			bounds.Max.X, rowRect.Max.Y,
		), bg, tolerance))
	}
	return colRanges
}

// tightenRect shrinks rect to the smallest bounding box of non-background pixels within it.
func tightenRect(img image.Image, rect image.Rectangle, bg color.RGBA, tolerance int) image.Rectangle {
	minX, minY := rect.Max.X, rect.Max.Y
	maxX, maxY := rect.Min.X, rect.Min.Y
	for y := rect.Min.Y; y < rect.Max.Y; y++ {
		for x := rect.Min.X; x < rect.Max.X; x++ {
			if !isBackground(img, x, y, bg, tolerance) {
				if x < minX {
					minX = x
				}
				if x+1 > maxX {
					maxX = x + 1
				}
				if y < minY {
					minY = y
				}
				if y+1 > maxY {
					maxY = y + 1
				}
			}
		}
	}
	if minX >= maxX || minY >= maxY {
		return image.Rectangle{}
	}
	return image.Rect(minX, minY, maxX, maxY)
}

func absDiff(a, b int) int {
	if a > b {
		return a - b
	}
	return b - a
}
