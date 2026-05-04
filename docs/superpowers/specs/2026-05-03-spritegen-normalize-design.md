# spritegen — Normalize

## Overview

Add a `Normalize` function to the `spritesheet` package and a `spritegen normalize <file> --output=<file>` CLI command. Normalize detects sprite rows, removes the background, computes a uniform grid (consistent row height, label column width, and sprite column width), and renders a new spritesheet with all cells bottom-center aligned and a fully transparent background.

---

## Library Changes — `spritesheet` package

### New Exported Type

```go
type NormalizedSheet struct {
    Image        *image.NRGBA
    LabelWidth   int // width of the label column
    SpriteWidth  int // normalized sprite cell width
    CellHeight   int // normalized cell height (max across sprites and labels)
    Padding      int // gap between cells and around the edges
}
```

### New Exported Function

```go
func Normalize(img image.Image, padding int) (*NormalizedSheet, error)
```

**Algorithm:**

1. Call `Slice(img)` to get `[]Row`. Each row has a tight-bounded `Label` rectangle and `[]Sprites` rectangles.
2. Call `detectBackground(img)` with the package constant `bgTolerance` to get background color info.
3. Compute grid dimensions:
   - `labelW` = max width of all `row.Label` rectangles (0 if all labels are empty)
   - `spriteW` = max width of all sprite rectangles across all rows
   - `cellH` = max height across all sprite and label rectangles
   - `maxCols` = max sprite count across all rows
4. Compute output image size:
   - `outW = padding*(maxCols+2) + labelW + maxCols*spriteW`
   - `outH = padding*(numRows+1) + numRows*cellH`
5. Allocate `image.NewNRGBA(image.Rect(0, 0, outW, outH))` — zero-initialized, fully transparent.
6. For each row `i` and each cell (label + sprites):
   - Crop the subimage via `img.(SubImager).SubImage(rect)`.
   - Apply `RemoveBackground(subImg, bg, bgTolerance)` to get a transparent `*image.NRGBA`.
   - Compute the cell's top-left corner in the output:
     - Label cell: `cellX = padding`, `cellY = padding + i*(cellH+padding)`
     - Sprite `j`: `cellX = 2*padding + labelW + j*(spriteW+padding)`, `cellY` same as label
   - Place the sprite bottom-center in the cell:
     - `drawX = cellX + (cellW - sprite.Bounds().Dx()) / 2`
     - `drawY = cellY + cellH - sprite.Bounds().Dy()`
   - Copy pixels using `draw.Draw` from `image/draw` (stdlib).
7. Return `&NormalizedSheet{Image: out, LabelWidth: labelW, SpriteWidth: spriteW, CellHeight: cellH, Padding: padding}`.

**Error conditions:**
- Returns an error if `img` does not implement `SubImage` (same check as `Slicer.Slice`).
- Returns an error if `Slice` fails.
- If there are no rows, returns an empty 0×0 transparent image with zero dimensions (not an error).

**No new external dependencies.** `image/draw` is part of the Go standard library.

---

## CLI Changes — `main.go`

### New Command

```
spritegen normalize <file> --output=<file> [--padding=8]
```

**Flags:**
- `--output` (string, required): path to write the normalized PNG.
- `--padding` (int, default 8): gap in pixels between cells and around the sheet edges.

**Behavior:**
1. Open and decode the input image.
2. Call `spritesheet.Normalize(img, padding)`.
3. Create the output file and `png.Encode` the result.
4. Print layout info to stdout:

```
Label width: <LabelWidth>
Sprite size: <SpriteWidth> x <CellHeight>
Written to:  <output-path>
```

---

## File Map

| File | Change |
|------|--------|
| `salsa/tools/spritegen/spritesheet/spritesheet.go` | Add `NormalizedSheet` type and `Normalize` function; add `image/draw` import |
| `salsa/tools/spritegen/spritesheet/spritesheet_test.go` | Add `TestNormalize` |
| `salsa/tools/spritegen/main.go` | Add `normalizeCmd` |

No new files. No BUILD changes (no new external dependencies).

---

## Tests

### `TestNormalize`

Construct a 200×100 `*image.RGBA`:
- Background: `color.RGBA{21, 23, 31, 255}` — fills entire image.
- Row 0 content (y 10–50):
  - Label: `fillRect(img, image.Rect(0, 10, 20, 20), content)` — 20×10
  - Sprite 0: `fillRect(img, image.Rect(30, 10, 60, 50), content)` — 30×40
  - Sprite 1: `fillRect(img, image.Rect(70, 15, 95, 50), content)` — 25×35
- Row 1 content (y 60–100):
  - Label: `fillRect(img, image.Rect(0, 60, 18, 68), content)` — 18×8
  - Sprite 0: `fillRect(img, image.Rect(30, 64, 62, 100), content)` — 32×36

Expected grid dimensions with `padding=4`:
- `labelW=20`, `spriteW=32`, `cellH=40`, `maxCols=2`
- `outW = 4*(2+2) + 20 + 2*32 = 100`
- `outH = 4*(2+1) + 2*40 = 92`

Assertions:
- `result.Image.Bounds()` == `image.Rect(0, 0, 100, 92)`
- `result.LabelWidth == 20`
- `result.SpriteWidth == 32`
- `result.CellHeight == 40`
- `result.Padding == 4`
- Pixel at the center-bottom of row 0 sprite 0's cell has `alpha == 255` (content present)
- Pixel in the middle of row 1 sprite 1's cell (the empty cell) has `alpha == 0` (transparent)

---

## Out of Scope

- Outputting a JSON metadata file alongside the PNG
- Configurable alignment (other than bottom-center)
- Anti-aliasing or feathering
- Any output format other than PNG
