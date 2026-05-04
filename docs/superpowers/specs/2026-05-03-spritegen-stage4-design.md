# spritegen Stage 4 — Transparent Background

## Overview

Add a `--transparent-bg` flag to `spritegen slice` that replaces background-colored pixels in each output subimage with full transparency. A new `RemoveBackground` function in the `spritesheet` package implements the pixel transformation. No new dependencies.

---

## Library Changes — `spritesheet` package

### New Exported Function

```go
func RemoveBackground(img image.Image, bg color.RGBA, tolerance int) *image.NRGBA
```

- Allocates a new `*image.NRGBA` of the same bounds as `img`.
- For each pixel: if all three channels are within `tolerance` of `bg`, write `color.NRGBA{0, 0, 0, 0}` (fully transparent); otherwise copy the pixel with `A: 255`.
- Returns the new image. The original is not modified.

Uses `*image.NRGBA` (non-premultiplied RGBA) because `png.Encode` preserves alpha correctly with this type and it is the standard Go representation for RGBA images with transparency.

Background detection and tolerance come from the caller — no internal detection. This keeps the function pure and testable.

---

## CLI Changes — `main.go`

New flag on `sliceCmd`:

```
--transparent-bg   (bool, default false) Replace background-colored pixels with transparency in output PNGs.
```

When `--transparent-bg` is set:
- Call `spritesheet.Analyze(img)` to obtain `info.Background` and `info.BgTolerance`.
- For each subimage written (both label and sprite files), apply `spritesheet.RemoveBackground(sub.SubImage(rect), info.Background, info.BgTolerance)` and pass the result to `png.Encode` instead of the raw subimage.

When `--transparent-bg` is false, behavior is unchanged.

`Analyze` is called once per invocation, before the write loop, only when the flag is set.

---

## File Map

| File | Change |
|------|--------|
| `salsa/tools/spritegen/spritesheet/spritesheet.go` | Add `RemoveBackground` |
| `salsa/tools/spritegen/spritesheet/spritesheet_test.go` | Add `TestRemoveBackground` |
| `salsa/tools/spritegen/main.go` | Add `--transparent-bg` flag, apply `RemoveBackground` in write loop |

No new files. No new dependencies. No BUILD changes.

---

## Tests

**`TestRemoveBackground`** (in `spritesheet_test.go`):

- Construct a 40×20 `*image.RGBA`:
  - Background: `color.RGBA{21, 23, 31, 255}` — fills entire image.
  - Content block: `color.RGBA{200, 100, 50, 255}` — fills `image.Rect(10, 5, 30, 15)`.
- Call `spritesheet.RemoveBackground(img, bg, 30)`.
- Assert every pixel outside the content block has alpha `0`.
- Assert every pixel inside the content block has RGB `{200, 100, 50}` and alpha `255`.

---

## Out of Scope

- Anti-aliasing or feathering at background/content edges
- Per-pixel tolerance (uniform tolerance only)
- Any output format other than PNG
