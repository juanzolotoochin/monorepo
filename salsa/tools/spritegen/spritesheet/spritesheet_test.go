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
	// Background should be a dark color (all channels < 50)
	assert.Less(t, int(info.Background.R), 50)
	assert.Less(t, int(info.Background.G), 50)
	assert.Less(t, int(info.Background.B), 50)
}
