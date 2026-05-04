package spritesheet_test

import (
	"context"
	"fmt"
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
