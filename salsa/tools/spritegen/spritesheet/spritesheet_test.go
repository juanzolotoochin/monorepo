package spritesheet_test

import (
	"context"
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	_ "image/png"
	"os"
	"os/exec"
	"testing"

	"github.com/bazelbuild/rules_go/go/tools/bazel"
	"github.com/juanique/monorepo/salsa/tools/spritegen/spritesheet"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// rembgToolBatch implements spritesheet.RembgBatch using the rembg Python tool binary.
type rembgToolBatch struct{ toolPath string }

func (r *rembgToolBatch) Process(items []spritesheet.RembgBatchItem) error {
	data, err := spritesheet.ManifestJSON(items)
	if err != nil {
		return err
	}
	// Verify ManifestJSON round-trips correctly.
	var check []struct {
		In  string `json:"in"`
		Out string `json:"out"`
	}
	if err := json.Unmarshal(data, &check); err != nil {
		return err
	}

	manifestFile, err := os.CreateTemp("", "rembg-manifest-*.json")
	if err != nil {
		return err
	}
	manifestPath := manifestFile.Name()
	manifestFile.Close()
	defer os.Remove(manifestPath)

	if err := os.WriteFile(manifestPath, data, 0644); err != nil {
		return err
	}
	c := exec.Command(r.toolPath, manifestPath)
	c.Stdout = os.Stderr
	c.Stderr = os.Stderr
	return c.Run()
}

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
	require.NoError(t, err)
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
	require.NoError(t, err)
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
	require.NoError(t, err)
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
	require.NoError(t, err)
	assert.Equal(t, 7, info.RowCount)
	assert.Equal(t, 1536, info.Width)
	assert.Equal(t, 1024, info.Height)
	// Background should be a dark color (all channels < 50)
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
		fillRect(img, image.Rect(0, rowY[0], 20, rowY[1]), content)   // label
		fillRect(img, image.Rect(30, rowY[0], 70, rowY[1]), content)  // sprite 0
		fillRect(img, image.Rect(80, rowY[0], 120, rowY[1]), content) // sprite 1
	}

	rows, err := spritesheet.Slice(img)
	require.NoError(t, err)
	require.Len(t, rows, 2)
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
	require.NoError(t, err)
	require.Len(t, rows, 7)
	for i, row := range rows {
		assert.False(t, row.Label.Empty(), "row %d label empty", i)
		assert.Greater(t, len(row.Sprites), 0, "row %d has no sprites", i)
	}
}

type fakeLabelReader struct {
	labels []string
	idx    int
}

func (f *fakeLabelReader) ReadLabel(_ context.Context, _ image.Image) (string, error) {
	if f.idx >= len(f.labels) {
		return "", fmt.Errorf("fakeLabelReader: unexpected call %d (only %d labels configured)", f.idx, len(f.labels))
	}
	label := f.labels[f.idx]
	f.idx++
	return label, nil
}

func TestSlicer_WithFakeReader(t *testing.T) {
	bg := color.RGBA{21, 23, 31, 255}
	content := color.RGBA{200, 100, 50, 255}
	img := image.NewRGBA(image.Rect(0, 0, 200, 100))
	fillRect(img, img.Bounds(), bg)

	for _, rowY := range [][2]int{{5, 45}, {55, 95}} {
		fillRect(img, image.Rect(0, rowY[0], 20, rowY[1]), content)   // label
		fillRect(img, image.Rect(30, rowY[0], 70, rowY[1]), content)  // sprite 0
		fillRect(img, image.Rect(80, rowY[0], 120, rowY[1]), content) // sprite 1
	}

	slicer := spritesheet.Slicer{
		LabelReader: &fakeLabelReader{labels: []string{"idle", "walk"}},
	}
	rows, err := slicer.Slice(context.Background(), img)
	require.NoError(t, err)
	require.Len(t, rows, 2)
	assert.Equal(t, "idle", rows[0].LabelText)
	assert.Equal(t, "walk", rows[1].LabelText)
	assert.Len(t, rows[0].Sprites, 2)
	assert.Len(t, rows[1].Sprites, 2)
}

func TestSlicer_NilLabelReader(t *testing.T) {
	bg := color.RGBA{21, 23, 31, 255}
	content := color.RGBA{200, 100, 50, 255}
	img := image.NewRGBA(image.Rect(0, 0, 200, 100))
	fillRect(img, img.Bounds(), bg)

	for _, rowY := range [][2]int{{5, 45}, {55, 95}} {
		fillRect(img, image.Rect(0, rowY[0], 20, rowY[1]), content)
		fillRect(img, image.Rect(30, rowY[0], 70, rowY[1]), content)
		fillRect(img, image.Rect(80, rowY[0], 120, rowY[1]), content)
	}

	slicer := spritesheet.Slicer{} // LabelReader is nil
	rows, err := slicer.Slice(context.Background(), img)
	require.NoError(t, err)
	require.Len(t, rows, 2)
	assert.Equal(t, "", rows[0].LabelText)
	assert.Equal(t, "", rows[1].LabelText)
}

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

func TestNormalize_MainCharacter(t *testing.T) {
	path, err := bazel.Runfile("salsa/tools/spritegen/testing/main-character.png")
	require.NoError(t, err)

	f, err := os.Open(path)
	require.NoError(t, err)
	defer f.Close()

	img, _, err := image.Decode(f)
	require.NoError(t, err)

	rows, err := spritesheet.Slice(img)
	require.NoError(t, err)

	maxCols := 0
	for _, row := range rows {
		if len(row.Sprites) > maxCols {
			maxCols = len(row.Sprites)
		}
	}
	numRows := len(rows)

	result, err := spritesheet.Normalize(img, 8)
	require.NoError(t, err)

	expectedW := 8*(maxCols+2) + result.LabelWidth + maxCols*result.SpriteWidth
	expectedH := 8*(numRows+1) + numRows*result.CellHeight
	assert.Equal(t, expectedW, result.Image.Bounds().Dx(), "output width should match formula")
	assert.Equal(t, expectedH, result.Image.Bounds().Dy(), "output height should match formula")
}

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
	assert.Equal(t, 40, result.CellHeight)
	assert.Equal(t, 4, result.Padding)

	// Row 0, sprite 0: cellX=2*4+20=28, cellY=4, sprite 30×40
	// drawX=28+(32-30)/2=29, drawY=4+(40-40)=4
	// Bottom-center pixel: (29+15, 4+39) = (44, 43) — must be opaque
	assert.Equal(t, uint8(255), result.Image.NRGBAAt(44, 43).A, "row 0 sprite 0 bottom should be opaque")

	// Row 1, sprite 1 cell (empty): cellX=2*4+20+1*(32+4)=64, cellY=4+1*(40+4)=48
	// Center pixel: (64+16, 48+20) = (80, 68) — must be transparent
	assert.Equal(t, uint8(0), result.Image.NRGBAAt(80, 68).A, "empty cell should be transparent")
}

func TestPack_EmptyRows(t *testing.T) {
	sheet, err := spritesheet.Pack(nil, 4)
	require.NoError(t, err)
	assert.Equal(t, image.Rect(0, 0, 0, 0), sheet.Bounds())
}

func TestPack_NegativePadding(t *testing.T) {
	_, err := spritesheet.Pack([]spritesheet.FrameRow{{Label: "idle", Frames: []image.Image{image.NewRGBA(image.Rect(0, 0, 10, 10))}}}, -1)
	assert.Error(t, err)
}

func TestPack_Layout(t *testing.T) {
	// Two rows, two frames each. Row 0: 20×30 frames. Row 1: 15×20 frames.
	// Cell size = max(20,15) × max(30,20) = 20×30. Padding = 4.
	// outW = 4*(2+1) + 2*20 = 52
	// outH = 4*(2+1) + 2*30 = 72
	frame := func(w, h int, c color.RGBA) image.Image {
		img := image.NewRGBA(image.Rect(0, 0, w, h))
		fillRect(img, img.Bounds(), c)
		return img
	}
	red := color.RGBA{255, 0, 0, 255}
	blue := color.RGBA{0, 0, 255, 255}

	rows := []spritesheet.FrameRow{
		{Label: "attack", Frames: []image.Image{frame(20, 30, red), frame(20, 30, red)}},
		{Label: "idle", Frames: []image.Image{frame(15, 20, blue), frame(15, 20, blue)}},
	}
	sheet, err := spritesheet.Pack(rows, 4)
	require.NoError(t, err)
	assert.Equal(t, image.Rect(0, 0, 52, 72), sheet.Bounds())

	// Row 0, frame 0: cellX=4, cellY=4, frame 20×30 — bottom-center aligned (fits exactly)
	// Center of that cell: (4+10, 4+15) = (14, 19) — should be red and opaque
	c := sheet.NRGBAAt(14, 19)
	assert.Equal(t, uint8(255), c.R, "row 0 frame 0 should be red")
	assert.Equal(t, uint8(255), c.A, "row 0 frame 0 should be opaque")

	// Row 1, frame 1: cellX=4+20+4=28, cellY=4+30+4=38
	// Frame is 15×20, bottom-center in 20×30 cell: dstX=28+(20-15)/2=30, dstY=38+(30-20)=48
	// Center of that frame: (30+7, 48+10) = (37, 58) — should be blue
	c = sheet.NRGBAAt(37, 58)
	assert.Equal(t, uint8(255), c.B, "row 1 frame 1 should be blue")
	assert.Equal(t, uint8(255), c.A, "row 1 frame 1 should be opaque")

	// Gap between rows (row 0 ends at y=33, row 1 starts at y=38): y=35 col 14 — transparent
	c = sheet.NRGBAAt(14, 35)
	assert.Equal(t, uint8(0), c.A, "gap between rows should be transparent")
}

func TestPack_TransparentSourcePreserved(t *testing.T) {
	// Frames with alpha transparency: center opaque, corners transparent.
	img := image.NewRGBA(image.Rect(0, 0, 10, 10))
	for y := 0; y < 10; y++ {
		for x := 0; x < 10; x++ {
			if x >= 2 && x < 8 && y >= 2 && y < 8 {
				img.SetRGBA(x, y, color.RGBA{100, 200, 50, 255})
			}
			// corners stay transparent (zero value)
		}
	}
	rows := []spritesheet.FrameRow{{Label: "run", Frames: []image.Image{img}}}
	sheet, err := spritesheet.Pack(rows, 2)
	require.NoError(t, err)

	// Frame placed at x=2, y=2 (padding=2, one frame, one row, frame fills cell exactly).
	// Corner pixel (2,2) in the sheet corresponds to frame pixel (0,0) — transparent.
	assert.Equal(t, uint8(0), sheet.NRGBAAt(2, 2).A, "transparent corner should stay transparent")
	// Center pixel (2+5, 2+5) = (7,7) — opaque.
	assert.Equal(t, uint8(255), sheet.NRGBAAt(7, 7).A, "opaque center should remain opaque")
}

// TestNormalizeWithRembg_BackgroundTransparent is an integration test that verifies
// the per-sprite rembg path produces a transparent background. The 30×30 square at
// (375, 490) in the output (padding=1) is a known background-only region.
func TestNormalizeWithRembg_BackgroundTransparent(t *testing.T) {
	toolPath, err := bazel.Runfile("salsa/tools/spritegen/rembg_tool")
	require.NoError(t, err)

	imgPath, err := bazel.Runfile("salsa/tools/spritegen/testing/main-character.png")
	require.NoError(t, err)

	f, err := os.Open(imgPath)
	require.NoError(t, err)
	defer f.Close()

	img, _, err := image.Decode(f)
	require.NoError(t, err)

	result, err := spritesheet.NormalizeWithRembg(img, &rembgToolBatch{toolPath: toolPath}, 1)
	require.NoError(t, err)

	for y := 490; y < 520; y++ {
		for x := 375; x < 405; x++ {
			c := result.Image.NRGBAAt(x, y)
			assert.Equal(t, uint8(0), c.A, "pixel at (%d,%d) should be transparent (background)", x, y)
		}
	}
}
