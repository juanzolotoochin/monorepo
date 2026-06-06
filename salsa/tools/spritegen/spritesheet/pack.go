package spritesheet

import (
	"fmt"
	"image"
	"image/color"
	"image/draw"
)

// FrameRow is one animation's frames in display order.
type FrameRow struct {
	Label  string
	Frames []image.Image
}

// Pack assembles rows of animation frames into a single sprite sheet.
// Each row corresponds to one animation; columns are frames left-to-right.
// All frames are composited onto a transparent NRGBA canvas with the given
// padding (pixels) between cells and around the sheet edges.
// Frames of varying sizes are bottom-center aligned within their cells.
func Pack(rows []FrameRow, padding int) (*image.NRGBA, error) {
	if padding < 0 {
		return nil, fmt.Errorf("spritesheet: padding must be non-negative")
	}
	if len(rows) == 0 {
		return image.NewNRGBA(image.Rect(0, 0, 0, 0)), nil
	}

	// Compute cell dimensions: max frame width/height across all rows.
	cellW, cellH, maxCols := 0, 0, 0
	for _, row := range rows {
		if len(row.Frames) > maxCols {
			maxCols = len(row.Frames)
		}
		for _, f := range row.Frames {
			b := f.Bounds()
			if b.Dx() > cellW {
				cellW = b.Dx()
			}
			if b.Dy() > cellH {
				cellH = b.Dy()
			}
		}
	}

	numRows := len(rows)
	outW := padding*(maxCols+1) + maxCols*cellW
	outH := padding*(numRows+1) + numRows*cellH
	out := image.NewNRGBA(image.Rect(0, 0, outW, outH))

	// Transparent background (zero-value NRGBA is transparent).
	draw.Draw(out, out.Bounds(), image.NewUniform(color.NRGBA{}), image.Point{}, draw.Src)

	for r, row := range rows {
		cellY := padding + r*(cellH+padding)
		for c, frame := range row.Frames {
			b := frame.Bounds()
			fw, fh := b.Dx(), b.Dy()
			// Bottom-center alignment within cell.
			cellX := padding + c*(cellW+padding)
			dstX := cellX + (cellW-fw)/2
			dstY := cellY + (cellH - fh)
			dst := image.Rect(dstX, dstY, dstX+fw, dstY+fh)
			draw.Draw(out, dst, frame, b.Min, draw.Over)
		}
	}

	return out, nil
}
