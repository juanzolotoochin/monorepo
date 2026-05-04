# spritegen Stage 2 — Row Slicing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `spritesheet.Slice` to detect label and sprite bounding boxes per row, and a `spritegen slice <file> --output=<dir>` CLI command that writes those subimages as PNG files.

**Architecture:** Refactor `countRows` to expose row y-ranges via a new `findRowRanges` helper. `Slice` builds on that to also detect column groups within each row using the same scan+gap-bridging approach transposed to the x-axis. The CLI crops subimages via `image.SubImage` and writes them with `png.Encode`. No new external dependencies.

**Tech Stack:** Go stdlib (`image`, `image/png`, `image/color`), cobra, Bazel `go_library`/`go_binary`/`go_test`, Graphite (`gt`) for commits.

**IMPORTANT — commits:** This is Stage 2 work. All changes go on a new Graphite branch `spritegen-stage2`. Use `gt create spritegen-stage2 -m "..."` for the first commit in Task 1, then `gt modify --no-interactive` for all subsequent tasks. **Never use `git commit`.**

---

## File Map

| File | Change |
|------|--------|
| `salsa/tools/spritegen/spritesheet/spritesheet.go` | Add `Row` type, `minGapCols`, `Slice`, `findColRanges`, `tightenRect`; refactor `countRows` to call `findRowRanges` |
| `salsa/tools/spritegen/spritesheet/spritesheet_test.go` | Add `TestSlice_TwoRows` and `TestSlice_MainCharacter` |
| `salsa/tools/spritegen/main.go` | Add `sliceCmd`, `writeSubImage`; change `_ "image/png"` to `"image/png"` |

No new files. No BUILD file changes needed (no new deps).

---

## Task 1: Refactor `countRows` to extract `findRowRanges`

**Files:**
- Modify: `salsa/tools/spritegen/spritesheet/spritesheet.go`

This is a pure refactor — no behavior change. The existing row-detection logic moves into `findRowRanges` which returns `[]image.Rectangle` instead of a count. `countRows` becomes a one-liner calling it.

- [ ] **Step 1: Replace `spritesheet.go` with the refactored version**

`salsa/tools/spritegen/spritesheet/spritesheet.go`:
```go
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

	merged := make([]bool, height)
	copy(merged, isContent)
	for i := 0; i < height; {
		if !merged[i] {
			gapStart := i
			for i < height && !merged[i] {
				i++
			}
			gapLen := i - gapStart
			if gapLen < minGapRows && gapStart > 0 && i < height {
				for j := gapStart; j < i; j++ {
					merged[j] = true
				}
			}
		} else {
			i++
		}
	}

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

// findColRanges returns tight bounding rectangles of content column groups
// within rowRect.
func findColRanges(img image.Image, rowRect image.Rectangle, bg color.RGBA, tolerance int) []image.Rectangle {
	bounds := img.Bounds()
	width := bounds.Dx()
	height := rowRect.Dy()

	isContent := make([]bool, width)
	for i, x := 0, bounds.Min.X; x < bounds.Max.X; i, x = i+1, x+1 {
		nonBg := 0
		for y := rowRect.Min.Y; y < rowRect.Max.Y; y++ {
			r, g, b, _ := img.At(x, y).RGBA()
			c := color.RGBA{R: uint8(r >> 8), G: uint8(g >> 8), B: uint8(b >> 8)}
			if absDiff(int(c.R), int(bg.R)) > tolerance ||
				absDiff(int(c.G), int(bg.G)) > tolerance ||
				absDiff(int(c.B), int(bg.B)) > tolerance {
				nonBg++
			}
		}
		isContent[i] = float64(nonBg)/float64(height) > contentThreshold
	}

	merged := make([]bool, width)
	copy(merged, isContent)
	for i := 0; i < width; {
		if !merged[i] {
			gapStart := i
			for i < width && !merged[i] {
				i++
			}
			gapLen := i - gapStart
			if gapLen < minGapCols && gapStart > 0 && i < width {
				for j := gapStart; j < i; j++ {
					merged[j] = true
				}
			}
		} else {
			i++
		}
	}

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
			r, g, b, _ := img.At(x, y).RGBA()
			c := color.RGBA{R: uint8(r >> 8), G: uint8(g >> 8), B: uint8(b >> 8)}
			if absDiff(int(c.R), int(bg.R)) > tolerance ||
				absDiff(int(c.G), int(bg.G)) > tolerance ||
				absDiff(int(c.B), int(bg.B)) > tolerance {
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
```

- [ ] **Step 2: Run existing tests to confirm no regression**

```bash
bazel test //salsa/tools/spritegen/spritesheet:spritesheet_test
```

Expected: all 4 existing tests pass (`TestAnalyze_ZeroRows`, `TestAnalyze_OneRow`, `TestAnalyze_ThreeRows`, `TestAnalyze_MainCharacter`).

- [ ] **Step 3: Create the Stage 2 Graphite branch with this change**

```bash
git add salsa/tools/spritegen/spritesheet/spritesheet.go
gt create spritegen-stage2 -m "feat(spritegen): add row slicing (stage 2)"
```

Expected: new branch `spritegen-stage2` created stacked on `spritegen-stage1`.

---

## Task 2: Synthetic test for `Slice`

**Files:**
- Modify: `salsa/tools/spritegen/spritesheet/spritesheet_test.go`

- [ ] **Step 1: Write the failing test**

Add `TestSlice_TwoRows` to `spritesheet_test.go`. The complete file (replace the existing file with this):

```go
package spritesheet_test

import (
	"image"
	"image/color"
	_ "image/png"
	"os"
	"testing"

	"github.com/bazelbuild/rules_go/go/tools/bazel"
	"github.com/juanique/monorepo/salsa/tools/spritegen/spritesheet"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// fillRect fills a rectangle in img with c.
func fillRect(img *image.RGBA, r image.Rectangle, c color.RGBA) {
	for y := r.Min.Y; y < r.Max.Y; y++ {
		for x := r.Min.X; x < r.Max.X; x++ {
			img.SetRGBA(x, y, c)
		}
	}
}

func TestAnalyze_ZeroRows(t *testing.T) {
	bg := color.RGBA{21, 23, 31, 255}
	img := image.NewRGBA(image.Rect(0, 0, 20, 20))
	fillRect(img, img.Bounds(), bg)

	info, err := spritesheet.Analyze(img)
	assert.NoError(t, err)
	assert.Equal(t, 0, info.RowCount)
	assert.Equal(t, 20, info.Width)
	assert.Equal(t, 20, info.Height)
	assert.InDelta(t, 21, int(info.Background.R), 5)
	assert.InDelta(t, 23, int(info.Background.G), 5)
	assert.InDelta(t, 31, int(info.Background.B), 5)
}

func TestAnalyze_OneRow(t *testing.T) {
	bg := color.RGBA{21, 23, 31, 255}
	content := color.RGBA{200, 100, 50, 255}
	img := image.NewRGBA(image.Rect(0, 0, 100, 30))
	fillRect(img, img.Bounds(), bg)
	fillRect(img, image.Rect(0, 10, 100, 16), content)

	info, err := spritesheet.Analyze(img)
	assert.NoError(t, err)
	assert.Equal(t, 1, info.RowCount)
}

func TestAnalyze_ThreeRows(t *testing.T) {
	bg := color.RGBA{21, 23, 31, 255}
	content := color.RGBA{200, 100, 50, 255}
	img := image.NewRGBA(image.Rect(0, 0, 100, 100))
	fillRect(img, img.Bounds(), bg)
	fillRect(img, image.Rect(0, 5, 100, 15), content)
	fillRect(img, image.Rect(0, 35, 100, 45), content)
	fillRect(img, image.Rect(0, 65, 100, 75), content)

	info, err := spritesheet.Analyze(img)
	assert.NoError(t, err)
	assert.Equal(t, 3, info.RowCount)
}

func TestAnalyze_MainCharacter(t *testing.T) {
	path, err := bazel.Runfile("salsa/tools/spritegen/testing/main-character.png")
	require.NoError(t, err)

	f, err := os.Open(path)
	require.NoError(t, err)
	defer f.Close()

	img, _, err := image.Decode(f)
	require.NoError(t, err)

	info, err := spritesheet.Analyze(img)
	assert.NoError(t, err)
	assert.Equal(t, 7, info.RowCount)
	assert.Equal(t, 1536, info.Width)
	assert.Equal(t, 1024, info.Height)
	assert.Less(t, int(info.Background.R), 50)
	assert.Less(t, int(info.Background.G), 50)
	assert.Less(t, int(info.Background.B), 50)
}

func TestSlice_TwoRows(t *testing.T) {
	bg := color.RGBA{21, 23, 31, 255}
	content := color.RGBA{200, 100, 50, 255}
	// 200-wide, 100-tall image with background everywhere.
	img := image.NewRGBA(image.Rect(0, 0, 200, 100))
	fillRect(img, img.Bounds(), bg)

	// Row 0: y 5-44. Row 1: y 55-94.
	// Within each row: label x 0-19, sprite0 x 30-69, sprite1 x 80-119.
	for _, rowY := range [][2]int{{5, 45}, {55, 95}} {
		fillRect(img, image.Rect(0, rowY[0], 20, rowY[1]), content)  // label
		fillRect(img, image.Rect(30, rowY[0], 70, rowY[1]), content) // sprite 0
		fillRect(img, image.Rect(80, rowY[0], 120, rowY[1]), content) // sprite 1
	}

	rows, err := spritesheet.Slice(img)
	assert.NoError(t, err)
	assert.Len(t, rows, 2)
	for i, row := range rows {
		assert.False(t, row.Label.Empty(), "row %d label empty", i)
		assert.Len(t, row.Sprites, 2, "row %d sprite count", i)
		assert.False(t, row.Sprites[0].Empty(), "row %d sprite 0 empty", i)
		assert.False(t, row.Sprites[1].Empty(), "row %d sprite 1 empty", i)
	}
}

func TestSlice_MainCharacter(t *testing.T) {
	path, err := bazel.Runfile("salsa/tools/spritegen/testing/main-character.png")
	require.NoError(t, err)

	f, err := os.Open(path)
	require.NoError(t, err)
	defer f.Close()

	img, _, err := image.Decode(f)
	require.NoError(t, err)

	rows, err := spritesheet.Slice(img)
	assert.NoError(t, err)
	assert.Len(t, rows, 7)
	for i, row := range rows {
		assert.False(t, row.Label.Empty(), "row %d label empty", i)
		assert.Greater(t, len(row.Sprites), 0, "row %d has no sprites", i)
	}
}
```

- [ ] **Step 2: Run the tests**

```bash
bazel test //salsa/tools/spritegen/spritesheet:spritesheet_test --test_output=streamed
```

Expected: all 6 tests pass including `TestSlice_TwoRows` and `TestSlice_MainCharacter`.

If `TestSlice_MainCharacter` fails with wrong row count, debug the column detection. If it fails with `row N has no sprites`, the gap-bridging threshold may need adjustment — try increasing `minGapCols` or lowering `contentThreshold` for columns.

- [ ] **Step 3: Amend the Stage 2 commit**

```bash
git add salsa/tools/spritegen/spritesheet/spritesheet_test.go
gt modify --no-interactive
```

---

## Task 3: CLI `slice` subcommand

**Files:**
- Modify: `salsa/tools/spritegen/main.go`

- [ ] **Step 1: Replace `main.go` with the complete updated version**

`salsa/tools/spritegen/main.go`:
```go
package main

import (
	"fmt"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"
	"os"
	"path/filepath"

	"github.com/juanique/monorepo/salsa/tools/spritegen/spritesheet"
	"github.com/spf13/cobra"
)

var rootCmd = &cobra.Command{
	Use:   "spritegen",
	Short: "Sprite sheet management tool",
}

var infoCmd = &cobra.Command{
	Use:   "info <file>",
	Short: "Analyze a sprite sheet and print information about it",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		filePath := args[0]
		f, err := os.Open(filePath)
		if err != nil {
			return err
		}
		defer f.Close()

		img, _, err := image.Decode(f)
		if err != nil {
			return err
		}

		info, err := spritesheet.Analyze(img)
		if err != nil {
			return err
		}

		fmt.Printf("File:       %s\n", filepath.Base(filePath))
		fmt.Printf("Size:       %d x %d\n", info.Width, info.Height)
		fmt.Printf("Background: #%02X%02X%02X  (tolerance ±%d)\n",
			info.Background.R, info.Background.G, info.Background.B, info.BgTolerance)
		fmt.Printf("Rows:       %d\n", info.RowCount)
		return nil
	},
}

var sliceCmd = &cobra.Command{
	Use:   "slice <file>",
	Short: "Slice a sprite sheet into individual subimages",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		outputDir, _ := cmd.Flags().GetString("output")
		filePath := args[0]

		f, err := os.Open(filePath)
		if err != nil {
			return err
		}
		defer f.Close()

		img, _, err := image.Decode(f)
		if err != nil {
			return err
		}

		rows, err := spritesheet.Slice(img)
		if err != nil {
			return err
		}

		if err := os.MkdirAll(outputDir, 0755); err != nil {
			return err
		}

		sub, ok := img.(interface {
			SubImage(image.Rectangle) image.Image
		})
		if !ok {
			return fmt.Errorf("image does not support SubImage")
		}

		for i, row := range rows {
			labelPath := filepath.Join(outputDir, fmt.Sprintf("%02d_label.png", i))
			if err := writeSubImage(sub, row.Label, labelPath); err != nil {
				return err
			}
			for j, sprite := range row.Sprites {
				spritePath := filepath.Join(outputDir, fmt.Sprintf("%02d_%02d.png", i, j))
				if err := writeSubImage(sub, sprite, spritePath); err != nil {
					return err
				}
			}
		}

		fmt.Printf("Sliced %d rows to %s\n", len(rows), outputDir)
		return nil
	},
}

func writeSubImage(img interface {
	SubImage(image.Rectangle) image.Image
}, rect image.Rectangle, path string) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	return png.Encode(f, img.SubImage(rect))
}

func main() {
	sliceCmd.Flags().String("output", "", "directory to write subimages into (required)")
	_ = sliceCmd.MarkFlagRequired("output")
	rootCmd.AddCommand(infoCmd)
	rootCmd.AddCommand(sliceCmd)
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
```

Note: `_ "image/png"` is replaced with `"image/png"` (non-blank) since `png.Encode` is now used directly.

- [ ] **Step 2: Build the binary**

```bash
bazel build //salsa/tools/spritegen
```

Expected: build succeeds.

- [ ] **Step 3: Run the slice command against the test sprite**

```bash
mkdir -p /tmp/spritegen-out
bazel run //salsa/tools/spritegen -- slice \
  $(pwd)/salsa/tools/spritegen/testing/main-character.png \
  --output=/tmp/spritegen-out
```

Expected output:
```
Sliced 7 rows to /tmp/spritegen-out
```

Then verify the files exist:
```bash
ls /tmp/spritegen-out | head -20
```

Expected: files like `00_label.png`, `00_00.png`, `00_01.png`, ..., `06_label.png`, etc.

- [ ] **Step 4: Amend the Stage 2 commit**

```bash
git add salsa/tools/spritegen/main.go
gt modify --no-interactive
```
