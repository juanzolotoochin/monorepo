package spritesheet

import (
	"context"
	"fmt"
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

// LabelReader reads the text label visible in an image.
type LabelReader interface {
	ReadLabel(ctx context.Context, img image.Image) (string, error)
}

// LabeledRow is a Row with an optional text label read by OCR.
type LabeledRow struct {
	Row
	LabelText string // empty when Slicer.LabelReader is nil
}

// Slicer slices a sprite sheet and optionally reads labels via an injected LabelReader.
type Slicer struct {
	LabelReader LabelReader // nil → labels are not read
}

// Slice slices img into labeled rows. If s.LabelReader is non-nil, it reads
// the label text for each row by calling ReadLabel on the label subimage.
func (s *Slicer) Slice(ctx context.Context, img image.Image) ([]LabeledRow, error) {
	rows, err := Slice(img)
	if err != nil {
		return nil, err
	}

	labeled := make([]LabeledRow, len(rows))
	for i, row := range rows {
		labeled[i] = LabeledRow{Row: row}
	}

	if s.LabelReader == nil {
		return labeled, nil
	}

	sub, ok := img.(interface {
		SubImage(image.Rectangle) image.Image
	})
	if !ok {
		return nil, fmt.Errorf("spritesheet: image does not support SubImage")
	}

	for i, row := range rows {
		text, err := s.LabelReader.ReadLabel(ctx, sub.SubImage(row.Label))
		if err != nil {
			return nil, fmt.Errorf("spritesheet: row %d: %w", i, err)
		}
		labeled[i].LabelText = text
	}
	return labeled, nil
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
				r, g, b, a := img.At(o[0]+dx, o[1]+dy).RGBA()
				if a == 0 {
					continue // skip transparent pixels (pre-masked input)
				}
				rs = append(rs, int(r>>8))
				gs = append(gs, int(g>>8))
				bs = append(bs, int(b>>8))
			}
		}
	}
	if len(rs) == 0 {
		return color.RGBA{A: 255} // fully transparent image; colour doesn't matter
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

// isBackground checks if the pixel at (x, y) is background: either fully transparent
// (pre-masked input) or within tolerance of bg's RGB values.
func isBackground(img image.Image, x, y int, bg color.RGBA, tolerance int) bool {
	r, g, b, a := img.At(x, y).RGBA()
	if a == 0 {
		return true
	}
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

// RemoveBackground returns a new *image.NRGBA with the same bounds as img.
// Each pixel whose R, G, B channels are all within tolerance of bg is replaced
// with full transparency; all other pixels are copied with A=255.
func RemoveBackground(img image.Image, bg color.RGBA, tolerance int) *image.NRGBA {
	bounds := img.Bounds()
	out := image.NewNRGBA(bounds)
	for y := bounds.Min.Y; y < bounds.Max.Y; y++ {
		for x := bounds.Min.X; x < bounds.Max.X; x++ {
			if isBackground(img, x, y, bg, tolerance) {
				out.SetNRGBA(x, y, color.NRGBA{R: 0, G: 0, B: 0, A: 0})
			} else {
				r, g, b, _ := img.At(x, y).RGBA()
				out.SetNRGBA(x, y, color.NRGBA{R: uint8(r >> 8), G: uint8(g >> 8), B: uint8(b >> 8), A: 255})
			}
		}
	}
	return out
}

func absDiff(a, b int) int {
	if a > b {
		return a - b
	}
	return b - a
}

// NormalizedSheet is the result of normalizing a sprite sheet.
type NormalizedSheet struct {
	Image        *image.NRGBA
	LabelWidth   int // width of the label column
	SpriteWidth  int // normalized sprite cell width
	CellHeight   int // normalized cell height (max across sprites and labels)
	Padding      int // gap between cells and around the sheet edges
}

// identifyBackgroundPixels returns a flat byte slice (indexed by (y-bounds.Min.Y)*width +
// (x-bounds.Min.X)) where 1 means the pixel is background: it both matches bg within
// tolerance AND is reachable from the image boundary via flood-fill through similarly
// matching pixels. Enclosed dark pixels that happen to be close to the background color
// (e.g. character shadows) are not reachable and therefore not marked as background.
func identifyBackgroundPixels(img image.Image, bg color.RGBA, tolerance int) []byte {
	b := img.Bounds()
	w, h := b.Dx(), b.Dy()
	mask := make([]byte, w*h) // 0=unvisited, 1=background, 2=visited content

	type pt struct{ x, y int }
	queue := make([]pt, 0, 2*(w+h))

	visit := func(x, y int) {
		if x < b.Min.X || x >= b.Max.X || y < b.Min.Y || y >= b.Max.Y {
			return
		}
		i := (y-b.Min.Y)*w + (x-b.Min.X)
		if mask[i] != 0 {
			return
		}
		if isBackground(img, x, y, bg, tolerance) {
			mask[i] = 1
			queue = append(queue, pt{x, y})
		} else {
			mask[i] = 2
		}
	}

	for x := b.Min.X; x < b.Max.X; x++ {
		visit(x, b.Min.Y)
		visit(x, b.Max.Y-1)
	}
	for y := b.Min.Y + 1; y < b.Max.Y-1; y++ {
		visit(b.Min.X, y)
		visit(b.Max.X-1, y)
	}

	for len(queue) > 0 {
		p := queue[0]
		queue = queue[1:]
		visit(p.x+1, p.y)
		visit(p.x-1, p.y)
		visit(p.x, p.y+1)
		visit(p.x, p.y-1)
	}

	return mask
}

// NormalizeWithMask is like Normalize but uses maskImg's alpha channel to determine
// which pixels are foreground (alpha > 0) vs background (alpha == 0).
// Row/column detection uses img so that label/sprite bounds are accurate.
// Pixel colors are copied from img (preserving original palette).
func NormalizeWithMask(img image.Image, maskImg image.Image, padding int) (*NormalizedSheet, error) {
	if padding < 0 {
		return nil, fmt.Errorf("spritesheet: padding must be non-negative")
	}

	rows, err := Slice(img)
	if err != nil {
		return nil, err
	}

	if len(rows) == 0 {
		return &NormalizedSheet{
			Image:   image.NewNRGBA(image.Rect(0, 0, 0, 0)),
			Padding: padding,
		}, nil
	}

	labelW, spriteW, cellH, maxCols := 0, 0, 0, 0
	for _, row := range rows {
		if w := row.Label.Dx(); w > labelW {
			labelW = w
		}
		if h := row.Label.Dy(); h > cellH {
			cellH = h
		}
		if len(row.Sprites) > maxCols {
			maxCols = len(row.Sprites)
		}
		for _, s := range row.Sprites {
			if w := s.Dx(); w > spriteW {
				spriteW = w
			}
			if h := s.Dy(); h > cellH {
				cellH = h
			}
		}
	}

	numRows := len(rows)
	outW := padding*(maxCols+2) + labelW + maxCols*spriteW
	outH := padding*(numRows+1) + numRows*cellH
	out := image.NewNRGBA(image.Rect(0, 0, outW, outH))

	copyContent := func(rect image.Rectangle, dstX, dstY int) {
		for sy := rect.Min.Y; sy < rect.Max.Y; sy++ {
			for sx := rect.Min.X; sx < rect.Max.X; sx++ {
				_, _, _, a := maskImg.At(sx, sy).RGBA()
				if a > 0 {
					r, g, b, _ := img.At(sx, sy).RGBA()
					out.SetNRGBA(dstX+(sx-rect.Min.X), dstY+(sy-rect.Min.Y),
						color.NRGBA{uint8(r >> 8), uint8(g >> 8), uint8(b >> 8), 255})
				}
			}
		}
	}

	for i, row := range rows {
		cellY := padding + i*(cellH+padding)
		if !row.Label.Empty() {
			w, h := row.Label.Dx(), row.Label.Dy()
			copyContent(row.Label, padding+(labelW-w)/2, cellY+cellH-h)
		}
		for j, sprite := range row.Sprites {
			w, h := sprite.Dx(), sprite.Dy()
			dx := 2*padding + labelW + j*(spriteW+padding) + (spriteW-w)/2
			copyContent(sprite, dx, cellY+cellH-h)
		}
	}

	return &NormalizedSheet{
		Image:       out,
		LabelWidth:  labelW,
		SpriteWidth: spriteW,
		CellHeight:  cellH,
		Padding:     padding,
	}, nil
}

// Normalize detects sprite rows, removes the background, and renders a new
// spritesheet with uniform cell dimensions. Sprites are bottom-center aligned
// within each cell. padding is the gap in pixels between cells and around edges.
//
// Background removal uses flood-fill from the image edges so that dark character
// pixels enclosed by other content are preserved even when close to the background color.
func Normalize(img image.Image, padding int) (*NormalizedSheet, error) {
	if padding < 0 {
		return nil, fmt.Errorf("spritesheet: padding must be non-negative")
	}

	rows, err := Slice(img)
	if err != nil {
		return nil, err
	}

	if len(rows) == 0 {
		return &NormalizedSheet{
			Image:   image.NewNRGBA(image.Rect(0, 0, 0, 0)),
			Padding: padding,
		}, nil
	}

	bg := detectBackground(img)
	bgMask := identifyBackgroundPixels(img, bg, bgTolerance)
	bounds := img.Bounds()
	bw := bounds.Dx()

	// Compute grid dimensions.
	labelW, spriteW, cellH, maxCols := 0, 0, 0, 0
	for _, row := range rows {
		if w := row.Label.Dx(); w > labelW {
			labelW = w
		}
		if h := row.Label.Dy(); h > cellH {
			cellH = h
		}
		if len(row.Sprites) > maxCols {
			maxCols = len(row.Sprites)
		}
		for _, s := range row.Sprites {
			if w := s.Dx(); w > spriteW {
				spriteW = w
			}
			if h := s.Dy(); h > cellH {
				cellH = h
			}
		}
	}

	numRows := len(rows)
	outW := padding*(maxCols+2) + labelW + maxCols*spriteW
	outH := padding*(numRows+1) + numRows*cellH
	out := image.NewNRGBA(image.Rect(0, 0, outW, outH))

	copyContent := func(rect image.Rectangle, dstX, dstY int) {
		for sy := rect.Min.Y; sy < rect.Max.Y; sy++ {
			for sx := rect.Min.X; sx < rect.Max.X; sx++ {
				if bgMask[(sy-bounds.Min.Y)*bw+(sx-bounds.Min.X)] != 1 {
					r, g, b, _ := img.At(sx, sy).RGBA()
					out.SetNRGBA(dstX+(sx-rect.Min.X), dstY+(sy-rect.Min.Y),
						color.NRGBA{uint8(r >> 8), uint8(g >> 8), uint8(b >> 8), 255})
				}
			}
		}
	}

	for i, row := range rows {
		cellY := padding + i*(cellH+padding)
		if !row.Label.Empty() {
			w, h := row.Label.Dx(), row.Label.Dy()
			copyContent(row.Label, padding+(labelW-w)/2, cellY+cellH-h)
		}
		for j, sprite := range row.Sprites {
			w, h := sprite.Dx(), sprite.Dy()
			dx := 2*padding + labelW + j*(spriteW+padding) + (spriteW-w)/2
			copyContent(sprite, dx, cellY+cellH-h)
		}
	}

	return &NormalizedSheet{
		Image:       out,
		LabelWidth:  labelW,
		SpriteWidth: spriteW,
		CellHeight:  cellH,
		Padding:     padding,
	}, nil
}
