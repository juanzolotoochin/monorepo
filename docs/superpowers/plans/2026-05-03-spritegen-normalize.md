# spritegen Normalize Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `spritesheet.Normalize` and a `spritegen normalize <file> --output=<file>` CLI command that produces a uniform-grid spritesheet with transparent background and bottom-center aligned sprites.

**Architecture:** `Normalize` composes existing helpers — `Slice` for row detection, `detectBackground`+`RemoveBackground` for transparency — then lays out a fresh `*image.NRGBA` grid using `image/draw` from the stdlib. The CLI command reads the input, calls `Normalize`, writes the PNG, and prints offset/sprite-size metadata.

**Tech Stack:** Go stdlib (`image`, `image/color`, `image/draw`, `image/png`), Cobra, Bazel, Graphite (`gt`) for commits.

**IMPORTANT — commits:** All changes go on a new Graphite branch. Use `gt create spritegen-normalize -m "..."` for the first commit in Task 1, then `gt modify --no-interactive -m "..."` for Task 2. **Never use `git commit`.**

---

## File Map

| File | Change |
|------|--------|
| `salsa/tools/spritegen/spritesheet/spritesheet.go` | Add `NormalizedSheet` type and `Normalize` function; add `"image/draw"` import |
| `salsa/tools/spritegen/spritesheet/spritesheet_test.go` | Add `TestNormalize` |
| `salsa/tools/spritegen/main.go` | Add `normalizeCmd`; register in `main()` |

No new files. No BUILD changes (no new external dependencies).

---

## Task 1: Add `NormalizedSheet` and `Normalize` to the `spritesheet` package (TDD)

**Files:**
- Modify: `salsa/tools/spritegen/spritesheet/spritesheet_test.go`
- Modify: `salsa/tools/spritegen/spritesheet/spritesheet.go`

### Background — how the existing code works

- `Slice(img image.Image) ([]Row, error)` — detects row and column groups, returns tight-bounded `Label` and `Sprites` rectangles per row.
- `detectBackground(img) color.RGBA` — median of corner patches; unexported.
- `bgTolerance = 30` — package constant.
- `RemoveBackground(img image.Image, bg color.RGBA, tolerance int) *image.NRGBA` — replaces bg pixels with alpha=0.
- `fillRect(img *image.RGBA, r image.Rectangle, c color.RGBA)` — test helper already defined.

### Grid dimension formulas

Given `labelW`, `spriteW`, `cellH`, `maxCols`, `numRows`, `padding`:
```
outW = padding*(maxCols+2) + labelW + maxCols*spriteW
outH = padding*(numRows+1) + numRows*cellH
```

Cell top-left positions for row `i`, padding `p`:
- Label cell: `cellX = p`, `cellY = p + i*(cellH+p)`
- Sprite `j`:  `cellX = 2*p + labelW + j*(spriteW+p)`, `cellY` same as label

Bottom-center draw offset within a cell of size (`cellW`, `cellH`) for a sprite of size (`w`, `h`):
- `drawX = cellX + (cellW - w) / 2`
- `drawY = cellY + cellH - h`

- [ ] **Step 1: Write the failing test**

Add `TestNormalize` to `salsa/tools/spritegen/spritesheet/spritesheet_test.go`, after `TestRemoveBackground`:

```go
func TestNormalize(t *testing.T) {
	bg := color.RGBA{21, 23, 31, 255}
	content := color.RGBA{200, 100, 50, 255}
	img := image.NewRGBA(image.Rect(0, 0, 200, 100))
	fillRect(img, img.Bounds(), bg)
	// Row 0 (y 10-50): label 20×10, sprite0 30×40, sprite1 25×35
	fillRect(img, image.Rect(0, 10, 20, 20), content)
	fillRect(img, image.Rect(30, 10, 60, 50), content)
	fillRect(img, image.Rect(70, 15, 95, 50), content)
	// Row 1 (y 60-100): label 18×8, sprite0 32×36
	fillRect(img, image.Rect(0, 60, 18, 68), content)
	fillRect(img, image.Rect(30, 64, 62, 100), content)

	result, err := spritesheet.Normalize(img, 4)
	require.NoError(t, err)

	// labelW=20, spriteW=32, cellH=40, maxCols=2, numRows=2, padding=4
	// outW = 4*(2+2) + 20 + 2*32 = 100
	// outH = 4*(2+1) + 2*40 = 92
	assert.Equal(t, image.Rect(0, 0, 100, 92), result.Image.Bounds())
	assert.Equal(t, 20, result.LabelWidth)
	assert.Equal(t, 32, result.SpriteWidth)
	assert.Equal(t, 40, result.SpriteHeight)
	assert.Equal(t, 4, result.Padding)

	// Row 0, sprite 0: cellX=2*4+20=28, cellY=4, sprite 30×40
	// drawX=28+(32-30)/2=29, drawY=4+(40-40)=4
	// Bottom-center pixel: (29+15, 4+39) = (44, 43) — must be opaque
	assert.Equal(t, uint8(255), result.Image.NRGBAAt(44, 43).A, "row 0 sprite 0 bottom should be opaque")

	// Row 1, sprite 1 cell (empty): cellX=2*4+20+1*(32+4)=64, cellY=4+1*(40+4)=48
	// Center pixel: (64+16, 48+20) = (80, 68) — must be transparent
	assert.Equal(t, uint8(0), result.Image.NRGBAAt(80, 68).A, "empty cell should be transparent")
}
```

No new imports needed — `image`, `image/color`, `spritesheet`, `assert`, `require` are already imported in the test file.

- [ ] **Step 2: Run the test to verify it fails**

```bash
bazel test //salsa/tools/spritegen/spritesheet:spritesheet_test --test_filter=TestNormalize --test_output=errors 2>&1 | tail -20
```

Expected: FAIL — `spritesheet.Normalize undefined` and `spritesheet.NormalizedSheet undefined`.

- [ ] **Step 3: Add `NormalizedSheet` type and `Normalize` function to `spritesheet.go`**

Add `"image/draw"` to the import block in `salsa/tools/spritegen/spritesheet/spritesheet.go`:

```go
import (
	"context"
	"fmt"
	"image"
	"image/color"
	"image/draw"
	"sort"
)
```

Then append `NormalizedSheet` and `Normalize` at the end of the file, after `absDiff`:

```go
// NormalizedSheet is the result of normalizing a sprite sheet.
type NormalizedSheet struct {
	Image        *image.NRGBA
	LabelWidth   int // width of the label column
	SpriteWidth  int // normalized sprite cell width
	SpriteHeight int // normalized sprite cell height
	Padding      int // gap between cells and around the sheet edges
}

// Normalize detects sprite rows, removes the background, and renders a new
// spritesheet with uniform cell dimensions. Sprites are bottom-center aligned
// within each cell. padding is the gap in pixels between cells and around edges.
func Normalize(img image.Image, padding int) (*NormalizedSheet, error) {
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

	sub, ok := img.(interface {
		SubImage(image.Rectangle) image.Image
	})
	if !ok {
		return nil, fmt.Errorf("spritesheet: image does not support SubImage")
	}

	bg := detectBackground(img)

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

	for i, row := range rows {
		cellY := padding + i*(cellH+padding)

		if !row.Label.Empty() {
			src := RemoveBackground(sub.SubImage(row.Label), bg, bgTolerance)
			w, h := src.Bounds().Dx(), src.Bounds().Dy()
			dx := padding + (labelW-w)/2
			dy := cellY + cellH - h
			draw.Draw(out, image.Rect(dx, dy, dx+w, dy+h), src, src.Bounds().Min, draw.Src)
		}

		for j, sprite := range row.Sprites {
			src := RemoveBackground(sub.SubImage(sprite), bg, bgTolerance)
			w, h := src.Bounds().Dx(), src.Bounds().Dy()
			dx := 2*padding + labelW + j*(spriteW+padding) + (spriteW-w)/2
			dy := cellY + cellH - h
			draw.Draw(out, image.Rect(dx, dy, dx+w, dy+h), src, src.Bounds().Min, draw.Src)
		}
	}

	return &NormalizedSheet{
		Image:        out,
		LabelWidth:   labelW,
		SpriteWidth:  spriteW,
		SpriteHeight: cellH,
		Padding:      padding,
	}, nil
}
```

- [ ] **Step 4: Run all tests to verify they pass**

```bash
bazel test //salsa/tools/spritegen/spritesheet:spritesheet_test --test_output=errors 2>&1 | tail -20
```

Expected: ALL tests PASS.

- [ ] **Step 5: Commit on a new branch**

```bash
gt create spritegen-normalize -m "feat(spritegen): add Normalize to spritesheet package"
```

---

## Task 2: Add `normalize` command to the CLI

**Files:**
- Modify: `salsa/tools/spritegen/main.go`

- [ ] **Step 1: Read the current `main.go`**

Read `salsa/tools/spritegen/main.go` in full before making any changes.

- [ ] **Step 2: Add `normalizeCmd`**

Add the following variable declaration to `main.go`, after `sliceCmd` and before `labelSanitizeRe`:

```go
var normalizeCmd = &cobra.Command{
	Use:   "normalize <file>",
	Short: "Normalize a sprite sheet into a uniform grid with transparent background",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		outputPath, _ := cmd.Flags().GetString("output")
		padding, _ := cmd.Flags().GetInt("padding")
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

		result, err := spritesheet.Normalize(img, padding)
		if err != nil {
			return err
		}

		out, err := os.Create(outputPath)
		if err != nil {
			return err
		}
		defer out.Close()

		if err := png.Encode(out, result.Image); err != nil {
			return err
		}

		fmt.Printf("Offset:      %d\n", result.LabelWidth)
		fmt.Printf("Sprite size: %d x %d\n", result.SpriteWidth, result.SpriteHeight)
		fmt.Printf("Written to:  %s\n", outputPath)
		return nil
	},
}
```

- [ ] **Step 3: Register the command and its flags in `main()`**

In the `main()` function, add before `rootCmd.AddCommand(infoCmd)`:

```go
normalizeCmd.Flags().String("output", "", "path to write the normalized PNG (required)")
_ = normalizeCmd.MarkFlagRequired("output")
normalizeCmd.Flags().Int("padding", 8, "gap in pixels between cells and around the sheet edges")
rootCmd.AddCommand(normalizeCmd)
```

- [ ] **Step 4: Run all tests**

```bash
bazel test //salsa/tools/spritegen/... --test_output=errors 2>&1 | tail -20
```

Expected: ALL tests PASS.

- [ ] **Step 5: Verify the binary builds and the new command appears in help**

```bash
bazel run //salsa/tools/spritegen:spritegen -- --help 2>&1 | tail -15
```

Expected output includes `normalize` in the available commands list.

```bash
bazel run //salsa/tools/spritegen:spritegen -- normalize --help 2>&1 | tail -15
```

Expected output includes `--output`, `--padding` flags.

- [ ] **Step 6: Commit**

```bash
gt modify --no-interactive -m "feat(spritegen): add normalize command to CLI"
```
