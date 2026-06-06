package spritesheet

import (
	"encoding/json"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"os"
	"path/filepath"
)

// RembgBatch removes backgrounds from a batch of sprite images.
// Each item's In path is an input PNG; the result is written to Out.
// Implementations may run any AI or classical background-removal method.
type RembgBatch interface {
	Process(items []RembgBatchItem) error
}

// RembgBatchItem is a single sprite to process.
type RembgBatchItem struct {
	In  string `json:"in"`
	Out string `json:"out"`
}

// NormalizeWithRembg slices img, calls rb.Process with one item per sprite/label
// so the caller can apply AI background removal per-sprite, then composes a
// normalized sheet. Detection uses the original image, pixel colors come from img.
//
// Before passing each sprite to rb, the detected background is replaced with white.
// This gives AI models clear contrast between character and background regardless of
// the original background color (which is often dark and similar to character outlines).
// Pixel colors in the output are still taken from the original image.
func NormalizeWithRembg(img image.Image, rb RembgBatch, padding int) (*NormalizedSheet, error) {
	if padding < 0 {
		return nil, fmt.Errorf("spritesheet: padding must be non-negative")
	}

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

	tmpDir, err := os.MkdirTemp("", "spritegen-rembg-*")
	if err != nil {
		return nil, err
	}
	defer os.RemoveAll(tmpDir)

	type entry struct {
		row, col int
		origRect image.Rectangle
		maskPath string
	}

	var entries []entry
	var items []RembgBatchItem

	for i, row := range rows {
		if !row.Label.Empty() {
			inPath := filepath.Join(tmpDir, fmt.Sprintf("in_%04d_label.png", i))
			outPath := filepath.Join(tmpDir, fmt.Sprintf("out_%04d_label.png", i))
			if err := saveWhiteBgPNG(sub.SubImage(row.Label), bg, inPath); err != nil {
				return nil, err
			}
			entries = append(entries, entry{i, -1, row.Label, outPath})
			items = append(items, RembgBatchItem{In: inPath, Out: outPath})
		}
		for j, sprite := range row.Sprites {
			inPath := filepath.Join(tmpDir, fmt.Sprintf("in_%04d_%04d.png", i, j))
			outPath := filepath.Join(tmpDir, fmt.Sprintf("out_%04d_%04d.png", i, j))
			if err := saveWhiteBgPNG(sub.SubImage(sprite), bg, inPath); err != nil {
				return nil, err
			}
			entries = append(entries, entry{i, j, sprite, outPath})
			items = append(items, RembgBatchItem{In: inPath, Out: outPath})
		}
	}

	if err := rb.Process(items); err != nil {
		return nil, fmt.Errorf("spritesheet: rembg: %w", err)
	}

	type maskKey struct{ row, col int }
	masks := make(map[maskKey]image.Image, len(entries))
	for _, e := range entries {
		mf, err := os.Open(e.maskPath)
		if err != nil {
			return nil, err
		}
		mask, _, err := image.Decode(mf)
		mf.Close()
		if err != nil {
			return nil, err
		}
		masks[maskKey{e.row, e.col}] = mask
	}

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

	// Compute the flood-fill background mask for the full image so we can use it
	// as a second gate in copyContent: a pixel is only copied if flood-fill also
	// agrees it is not background. This prevents rembg's boundary overreach from
	// copying original dark-background pixels as a halo around each sprite.
	fullBgMask := identifyBackgroundPixels(img, bg, bgTolerance)
	bounds := img.Bounds()
	bw := bounds.Dx()

	numRows := len(rows)
	outW := padding*(maxCols+2) + labelW + maxCols*spriteW
	outH := padding*(numRows+1) + numRows*cellH
	out := image.NewNRGBA(image.Rect(0, 0, outW, outH))

	// copyContent copies a pixel only when BOTH rembg (mask alpha > 0) and the
	// flood-fill background mask agree the pixel is foreground. The intersection
	// avoids halos: flood-fill removes boundary pixels rembg misclassifies as
	// foreground; rembg removes interior pockets flood-fill cannot reach.
	copyContent := func(origRect image.Rectangle, mask image.Image, dstX, dstY int) {
		for sy := origRect.Min.Y; sy < origRect.Max.Y; sy++ {
			for sx := origRect.Min.X; sx < origRect.Max.X; sx++ {
				if fullBgMask[(sy-bounds.Min.Y)*bw+(sx-bounds.Min.X)] == 1 {
					continue // flood-fill says background — trust it over rembg
				}
				mx, my := sx-origRect.Min.X, sy-origRect.Min.Y
				_, _, _, a := mask.At(mx, my).RGBA()
				if a > 0 {
					r, g, b, _ := img.At(sx, sy).RGBA()
					out.SetNRGBA(dstX+(sx-origRect.Min.X), dstY+(sy-origRect.Min.Y),
						color.NRGBA{uint8(r >> 8), uint8(g >> 8), uint8(b >> 8), 255})
				}
			}
		}
	}

	for i, row := range rows {
		cellY := padding + i*(cellH+padding)
		if !row.Label.Empty() {
			w, h := row.Label.Dx(), row.Label.Dy()
			copyContent(row.Label, masks[maskKey{i, -1}], padding+(labelW-w)/2, cellY+cellH-h)
		}
		for j, sprite := range row.Sprites {
			w, h := sprite.Dx(), sprite.Dy()
			dx := 2*padding + labelW + j*(spriteW+padding) + (spriteW-w)/2
			copyContent(sprite, masks[maskKey{i, j}], dx, cellY+cellH-h)
		}
	}

	return &NormalizedSheet{
		Image:       out,
		LabelWidth:  labelW,
		SpriteWidth: spriteW,
		CellHeight:  cellH,
		Padding:     padding,
	}, nil
}

// ManifestJSON serializes items to JSON. Useful for RembgBatch implementations
// that invoke an external tool via a manifest file.
func ManifestJSON(items []RembgBatchItem) ([]byte, error) {
	return json.Marshal(items)
}

// saveWhiteBgPNG saves img to path with background pixels replaced by white.
// This gives AI background-removal models clear contrast regardless of the
// original background color.
func saveWhiteBgPNG(img image.Image, bg color.RGBA, path string) error {
	bgMask := identifyBackgroundPixels(img, bg, bgTolerance)
	b := img.Bounds()
	w, h := b.Dx(), b.Dy()
	out := image.NewNRGBA(image.Rect(0, 0, w, h))
	for y := b.Min.Y; y < b.Max.Y; y++ {
		for x := b.Min.X; x < b.Max.X; x++ {
			if bgMask[(y-b.Min.Y)*w+(x-b.Min.X)] == 1 {
				out.SetNRGBA(x-b.Min.X, y-b.Min.Y, color.NRGBA{255, 255, 255, 255})
			} else {
				r, g, bv, a := img.At(x, y).RGBA()
				out.SetNRGBA(x-b.Min.X, y-b.Min.Y, color.NRGBA{uint8(r >> 8), uint8(g >> 8), uint8(bv >> 8), uint8(a >> 8)})
			}
		}
	}
	return savePNG(out, path)
}

func savePNG(img image.Image, path string) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	return png.Encode(f, img)
}
