# spritegen Stage 1 — Sprite Sheet Analysis

## Overview

A Go tool at `salsa/tools/spritegen` that analyzes an unnormalized sprite sheet and reports what it found: the detected background color and the number of state rows. This is Stage 1 of a larger sprite normalization pipeline.

## Package Layout

```
salsa/tools/spritegen/
├── testing/
│   └── main-character.png          (existing test asset)
├── spritesheet/
│   ├── spritesheet.go              (analysis library)
│   ├── spritesheet_test.go
│   └── BUILD.bazel
├── main.go                         (cobra CLI entry point)
└── BUILD.bazel
```

## Library: `spritesheet` package

Import path: `github.com/juanique/monorepo/salsa/tools/spritegen/spritesheet`

### Types

```go
type Info struct {
    Width       int
    Height      int
    Background  color.RGBA
    BgTolerance int
    RowCount    int
}
```

### API

```go
func Analyze(img image.Image) (*Info, error)
```

Only stdlib dependencies: `image`, `image/color`.

### Algorithm

**Background detection:**
1. Sample 5×5 pixel patches from each of the four corners of the image.
2. Compute the per-channel median of those 100 samples as the reference background color.
3. Use a fixed tolerance of 30 per channel (max absolute difference on any channel).

**Row counting:**
1. For each horizontal row of pixels, compute the fraction of pixels whose color differs from the background by more than the tolerance on any channel.
2. A row is "content" if that fraction exceeds 2%.
3. Find contiguous bands of content rows — each band is one state row.
4. Return the count of bands as `RowCount`.

## CLI

Binary name: `spritegen`
Command: `spritegen info <file>`

Uses [cobra](https://github.com/spf13/cobra) for flag/command parsing (already in go.mod).

**Example output:**
```
File:       main-character.png
Size:       1536 x 1024
Background: #17181F  (tolerance ±30)
Rows:       7
```

The background hex is derived from `Info.Background`.

## Build

`main.go` is a `go_binary` target. `spritesheet/` is a `go_library` target. Both use `@io_bazel_rules_go//go:def.bzl` following patterns already in the repo.

## Tests

`spritesheet_test.go` covers:
- **Golden path**: load `testing/main-character.png`, assert `RowCount == 7` and `Background` is in the expected dark range.
- **Synthetic 1-row**: construct a small `image.RGBA` with a solid background and one band of colored pixels, assert `RowCount == 1`.
- **Synthetic 0-row**: all-background image, assert `RowCount == 0`.

The `go_test` target in `spritesheet/BUILD.bazel` declares `testing/main-character.png` as a `data` dependency. The test resolves the file path at runtime using the `@io_bazel_rules_go//go/runfiles` package (or the `bazel.Runfile` helper from `github.com/bazelbuild/rules_go/go/tools/bazel`) so it works correctly under `bazel test` regardless of working directory.

## Out of Scope for Stage 1

- Clearing/replacing the background color
- Reading or detecting row label text (IDLE, WALK, etc.)
- Detecting the number of sprites per row
- Outputting JSON or machine-readable formats
