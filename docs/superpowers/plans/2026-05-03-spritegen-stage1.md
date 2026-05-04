# spritegen Stage 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a `spritegen info <file>` CLI that detects background color and counts state rows in an unnormalized sprite sheet PNG.

**Architecture:** A `spritesheet` Go library performs all analysis (background detection via corner sampling, row counting via horizontal scan). A thin `main.go` cobra CLI wraps it. Tests use synthetic `image.RGBA` images for unit tests and Bazel runfiles to access `testing/main-character.png` for the golden-path test.

**Tech Stack:** Go stdlib (`image`, `image/png`, `image/color`), cobra (`@com_github_spf13_cobra`), testify (`@com_github_stretchr_testify//assert`), rules_go runfiles (`@io_bazel_rules_go//go/tools/bazel`), Bazel `go_library` / `go_binary` / `go_test`.

---

## File Map

| File | Role |
|------|------|
| `salsa/tools/spritegen/testing/BUILD.bazel` | `filegroup` exporting `main-character.png` for test data |
| `salsa/tools/spritegen/spritesheet/spritesheet.go` | Library: `Info` struct, `Analyze`, helpers |
| `salsa/tools/spritegen/spritesheet/spritesheet_test.go` | Unit + golden-path tests |
| `salsa/tools/spritegen/spritesheet/BUILD.bazel` | `go_library` + `go_test` with data dep |
| `salsa/tools/spritegen/main.go` | cobra root + `info` subcommand |
| `salsa/tools/spritegen/BUILD.bazel` | `go_binary` |

---

## Task 1: Scaffold — testing filegroup, library types, and BUILD files

**Files:**
- Create: `salsa/tools/spritegen/testing/BUILD.bazel`
- Create: `salsa/tools/spritegen/spritesheet/spritesheet.go`
- Create: `salsa/tools/spritegen/spritesheet/BUILD.bazel`

- [ ] **Step 1: Create the testing filegroup**

`salsa/tools/spritegen/testing/BUILD.bazel`:
```python
filegroup(
    name = "testdata",
    srcs = ["main-character.png"],
    visibility = ["//visibility:public"],
)
```

- [ ] **Step 2: Create the library stub**

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
	count := 0
	inContent := false
	for y := bounds.Min.Y; y < bounds.Max.Y; y++ {
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
		isContent := float64(nonBg)/float64(width) > contentThreshold
		if isContent && !inContent {
			count++
		}
		inContent = isContent
	}
	return count
}

func absDiff(a, b int) int {
	if a > b {
		return a - b
	}
	return b - a
}
```

- [ ] **Step 3: Create the library BUILD file**

`salsa/tools/spritegen/spritesheet/BUILD.bazel`:
```python
load("@io_bazel_rules_go//go:def.bzl", "go_library", "go_test")

go_library(
    name = "spritesheet",
    srcs = ["spritesheet.go"],
    importpath = "github.com/juanique/monorepo/salsa/tools/spritegen/spritesheet",
    visibility = ["//visibility:public"],
)

go_test(
    name = "spritesheet_test",
    srcs = ["spritesheet_test.go"],
    data = ["//salsa/tools/spritegen/testing:testdata"],
    deps = [
        ":spritesheet",
        "@com_github_stretchr_testify//assert",
        "@io_bazel_rules_go//go/tools/bazel",
    ],
)
```

- [ ] **Step 4: Verify the library builds**

```bash
bazel build //salsa/tools/spritegen/spritesheet
```

Expected: build succeeds with no errors.

- [ ] **Step 5: Commit the scaffold**

```bash
git add salsa/tools/spritegen/testing/BUILD.bazel \
        salsa/tools/spritegen/spritesheet/spritesheet.go \
        salsa/tools/spritegen/spritesheet/BUILD.bazel
git commit -m "feat(spritegen): scaffold spritesheet library"
```

---

## Task 2: Unit tests — synthetic images

**Files:**
- Create: `salsa/tools/spritegen/spritesheet/spritesheet_test.go`

- [ ] **Step 1: Write the failing tests**

`salsa/tools/spritegen/spritesheet/spritesheet_test.go`:
```go
package spritesheet_test

import (
	"image"
	"image/color"
	"testing"

	"github.com/juanique/monorepo/salsa/tools/spritegen/spritesheet"
	"github.com/stretchr/testify/assert"
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
	// Background should be close to the fill color
	assert.InDelta(t, 21, int(info.Background.R), 5)
}

func TestAnalyze_OneRow(t *testing.T) {
	bg := color.RGBA{21, 23, 31, 255}
	content := color.RGBA{200, 100, 50, 255}
	img := image.NewRGBA(image.Rect(0, 0, 100, 30))
	fillRect(img, img.Bounds(), bg)
	// One band of content in the middle rows 10-15
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
	// Three separate bands: rows 5-14, 35-44, 65-74
	fillRect(img, image.Rect(0, 5, 100, 15), content)
	fillRect(img, image.Rect(0, 35, 100, 45), content)
	fillRect(img, image.Rect(0, 65, 100, 75), content)

	info, err := spritesheet.Analyze(img)
	assert.NoError(t, err)
	assert.Equal(t, 3, info.RowCount)
}
```

- [ ] **Step 2: Run the tests**

```bash
bazel test //salsa/tools/spritegen/spritesheet:spritesheet_test
```

Expected: all 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add salsa/tools/spritegen/spritesheet/spritesheet_test.go
git commit -m "test(spritegen): add synthetic unit tests for Analyze"
```

---

## Task 3: Golden-path test with the real sprite

**Files:**
- Modify: `salsa/tools/spritegen/spritesheet/spritesheet_test.go`

- [ ] **Step 1: Replace `spritesheet_test.go` with the complete final version**

`salsa/tools/spritegen/spritesheet/spritesheet_test.go`:
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
	assert.NoError(t, err)

	f, err := os.Open(path)
	assert.NoError(t, err)
	defer f.Close()

	img, _, err := image.Decode(f)
	assert.NoError(t, err)

	info, err := spritesheet.Analyze(img)
	assert.NoError(t, err)
	assert.Equal(t, 7, info.RowCount)
	assert.Equal(t, 1536, info.Width)
	assert.Equal(t, 1024, info.Height)
	// Background should be a dark color (all channels < 50)
	assert.Less(t, int(info.Background.R), 50)
	assert.Less(t, int(info.Background.G), 50)
	assert.Less(t, int(info.Background.B), 50)
}
```

- [ ] **Step 2: Run the golden-path test**

```bash
bazel test //salsa/tools/spritegen/spritesheet:spritesheet_test --test_output=streamed
```

Expected: all 4 tests pass including `TestAnalyze_MainCharacter`.

If `bazel.Runfile` returns a "file not found" error, check the actual runfiles path by printing `os.Getenv("RUNFILES_DIR")` and listing contents — the path prefix may need adjustment (e.g., `_main/salsa/...` in bzlmod workspaces).

- [ ] **Step 3: Commit**

```bash
git add salsa/tools/spritegen/spritesheet/spritesheet_test.go
git commit -m "test(spritegen): add golden-path test against main-character.png"
```

---

## Task 4: CLI binary

**Files:**
- Create: `salsa/tools/spritegen/main.go`
- Create: `salsa/tools/spritegen/BUILD.bazel`

- [ ] **Step 1: Write main.go**

`salsa/tools/spritegen/main.go`:
```go
package main

import (
	"fmt"
	"image"
	_ "image/png"
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

func main() {
	rootCmd.AddCommand(infoCmd)
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
```

- [ ] **Step 2: Write the binary BUILD file**

`salsa/tools/spritegen/BUILD.bazel`:
```python
load("@io_bazel_rules_go//go:def.bzl", "go_binary")

go_binary(
    name = "spritegen",
    srcs = ["main.go"],
    importpath = "github.com/juanique/monorepo/salsa/tools/spritegen",
    visibility = ["//visibility:public"],
    deps = [
        "//salsa/tools/spritegen/spritesheet",
        "@com_github_spf13_cobra//:cobra",
    ],
)
```

- [ ] **Step 3: Build the binary**

```bash
bazel build //salsa/tools/spritegen
```

Expected: build succeeds.

- [ ] **Step 4: Run it against the test sprite**

```bash
bazel run //salsa/tools/spritegen -- info \
  $(pwd)/salsa/tools/spritegen/testing/main-character.png
```

Expected output:
```
File:       main-character.png
Size:       1536 x 1024
Background: #17181F  (tolerance ±30)
Rows:       7
```

(Exact hex may vary slightly based on median sampling; channels should all be < 50.)

- [ ] **Step 5: Commit**

```bash
git add salsa/tools/spritegen/main.go salsa/tools/spritegen/BUILD.bazel
git commit -m "feat(spritegen): add CLI binary with info command"
```
