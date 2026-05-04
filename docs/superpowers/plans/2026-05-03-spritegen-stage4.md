# spritegen Stage 4 — Transparent Background Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `spritesheet.RemoveBackground` and a `--transparent-bg` flag to `spritegen slice` that replaces background-colored pixels in each output PNG with full transparency.

**Architecture:** `RemoveBackground` is a pure function in the `spritesheet` package that takes an image plus background color/tolerance and returns a new `*image.NRGBA` with background pixels zeroed out. The CLI flag wires it in: when set, `Analyze` is called once to get background info, and every subimage is passed through `RemoveBackground` before `png.Encode`. The existing `writeSubImage` helper is renamed to `writeImage(img image.Image, path string)` to decouple writing from SubImage extraction.

**Tech Stack:** Go stdlib (`image`, `image/color`, `image/png`), Cobra, Bazel, Graphite (`gt`) for commits.

**IMPORTANT — commits:** All Stage 4 changes go on a new Graphite branch. Use `gt create spritegen-stage4 -m "..."` for the first commit in Task 1, then `gt modify --no-interactive -m "..."` for Task 2. **Never use `git commit`.**

---

## File Map

| File | Change |
|------|--------|
| `salsa/tools/spritegen/spritesheet/spritesheet.go` | Add `RemoveBackground` function |
| `salsa/tools/spritegen/spritesheet/spritesheet_test.go` | Add `TestRemoveBackground` |
| `salsa/tools/spritegen/main.go` | Rename `writeSubImage` → `writeImage`; add `--transparent-bg` flag; apply `RemoveBackground` in write loop |

No new files. No BUILD changes (no new dependencies).

---

## Task 1: Add `RemoveBackground` to the `spritesheet` package (TDD)

**Files:**
- Modify: `salsa/tools/spritegen/spritesheet/spritesheet_test.go`
- Modify: `salsa/tools/spritegen/spritesheet/spritesheet.go`

- [ ] **Step 1: Write the failing test**

Add `TestRemoveBackground` to `salsa/tools/spritegen/spritesheet/spritesheet_test.go`, after the last test (`TestSlicer_NilLabelReader`):

```go
func TestRemoveBackground(t *testing.T) {
	bg := color.RGBA{21, 23, 31, 255}
	content := color.RGBA{200, 100, 50, 255}
	img := image.NewRGBA(image.Rect(0, 0, 40, 20))
	fillRect(img, img.Bounds(), bg)
	contentRect := image.Rect(10, 5, 30, 15)
	fillRect(img, contentRect, content)

	result := spritesheet.RemoveBackground(img, bg, 30)

	bounds := result.Bounds()
	for y := bounds.Min.Y; y < bounds.Max.Y; y++ {
		for x := bounds.Min.X; x < bounds.Max.X; x++ {
			c := result.NRGBAAt(x, y)
			if (image.Point{X: x, Y: y}).In(contentRect) {
				assert.Equal(t, uint8(200), c.R, "content R at (%d,%d)", x, y)
				assert.Equal(t, uint8(100), c.G, "content G at (%d,%d)", x, y)
				assert.Equal(t, uint8(50), c.B, "content B at (%d,%d)", x, y)
				assert.Equal(t, uint8(255), c.A, "content A at (%d,%d)", x, y)
			} else {
				assert.Equal(t, uint8(0), c.A, "bg alpha at (%d,%d)", x, y)
			}
		}
	}
}
```

No new imports needed — `image`, `image/color`, and `spritesheet` are already imported in the test file.

- [ ] **Step 2: Run the test to verify it fails**

```bash
bazel test //salsa/tools/spritegen/spritesheet:spritesheet_test --test_filter=TestRemoveBackground --test_output=short 2>&1 | tail -20
```

Expected: FAIL — `spritesheet.RemoveBackground undefined`.

- [ ] **Step 3: Implement `RemoveBackground`**

Add the following function at the end of `salsa/tools/spritegen/spritesheet/spritesheet.go`, before the final `absDiff` helper (or after it — order doesn't matter):

```go
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
```

No new imports needed — `image` and `image/color` are already imported.

- [ ] **Step 4: Run the test to verify it passes**

```bash
bazel test //salsa/tools/spritegen/spritesheet:spritesheet_test --test_output=short 2>&1 | tail -20
```

Expected: All tests PASS (including the new `TestRemoveBackground`).

- [ ] **Step 5: Commit on a new branch**

```bash
gt create spritegen-stage4 -m "feat(spritegen): add RemoveBackground to spritesheet package"
```

---

## Task 2: Add `--transparent-bg` flag to `spritegen slice`

**Files:**
- Modify: `salsa/tools/spritegen/main.go`

- [ ] **Step 1: Read the current `main.go` to understand context**

Read `salsa/tools/spritegen/main.go` in full before making changes.

- [ ] **Step 2: Rename `writeSubImage` to `writeImage` and update its signature**

Replace the existing `writeSubImage` function:

```go
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
```

With:

```go
func writeImage(img image.Image, path string) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	return png.Encode(f, img)
}
```

- [ ] **Step 3: Update the write loop in `sliceCmd` to use `writeImage` and support `--transparent-bg`**

In `sliceCmd.RunE`, add a line to read the new flag right after the existing flag reads at the top:

```go
transparentBg, _ := cmd.Flags().GetBool("transparent-bg")
```

Then, after the label-reading block (after `if err != nil { return err }`), add an `info` lookup:

```go
var info *spritesheet.Info
if transparentBg {
    info, err = spritesheet.Analyze(img)
    if err != nil {
        return err
    }
}
```

Then replace the entire write loop. The current loop is:

```go
for i, row := range labeledRows {
    labelPart := ""
    if row.LabelText != "" {
        labelPart = sanitizeLabel(row.LabelText) + "_"
    }
    if !row.Label.Empty() {
        labelPath := filepath.Join(outputDir, fmt.Sprintf("%02d_%slabel.png", i, labelPart))
        if err := writeSubImage(sub, row.Label, labelPath); err != nil {
            return err
        }
    }
    for j, sprite := range row.Sprites {
        spritePath := filepath.Join(outputDir, fmt.Sprintf("%02d_%s%02d.png", i, labelPart, j))
        if err := writeSubImage(sub, sprite, spritePath); err != nil {
            return err
        }
    }
}
```

Replace it with:

```go
for i, row := range labeledRows {
    labelPart := ""
    if row.LabelText != "" {
        labelPart = sanitizeLabel(row.LabelText) + "_"
    }
    if !row.Label.Empty() {
        labelPath := filepath.Join(outputDir, fmt.Sprintf("%02d_%slabel.png", i, labelPart))
        subImg := sub.SubImage(row.Label)
        if transparentBg {
            subImg = spritesheet.RemoveBackground(subImg, info.Background, info.BgTolerance)
        }
        if err := writeImage(subImg, labelPath); err != nil {
            return err
        }
    }
    for j, sprite := range row.Sprites {
        spritePath := filepath.Join(outputDir, fmt.Sprintf("%02d_%s%02d.png", i, labelPart, j))
        subImg := sub.SubImage(sprite)
        if transparentBg {
            subImg = spritesheet.RemoveBackground(subImg, info.Background, info.BgTolerance)
        }
        if err := writeImage(subImg, spritePath); err != nil {
            return err
        }
    }
}
```

- [ ] **Step 4: Register the new flag in `main()`**

In the `main()` function, after the existing `sliceCmd.Flags().Bool("read-labels", ...)` line, add:

```go
sliceCmd.Flags().Bool("transparent-bg", false, "Replace background-colored pixels with transparency in output PNGs.")
```

- [ ] **Step 5: Run all tests to verify**

```bash
bazel test //salsa/tools/spritegen/... --test_output=short 2>&1 | tail -20
```

Expected: All tests PASS.

- [ ] **Step 6: Verify the binary builds**

```bash
bazel build //salsa/tools/spritegen:spritegen 2>&1 | tail -10
```

Expected: Build succeeds with no errors.

- [ ] **Step 7: Commit**

```bash
gt modify --no-interactive -m "feat(spritegen): add --transparent-bg flag to slice command"
```
