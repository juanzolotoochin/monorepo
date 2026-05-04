package main

import (
	"fmt"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"
	"os"
	"path/filepath"
	"regexp"
	"strings"

	"github.com/juanique/monorepo/salsa/llm/vision"
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

var sliceCmd = &cobra.Command{
	Use:   "slice <file>",
	Short: "Slice a sprite sheet into individual subimages",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		outputDir, _ := cmd.Flags().GetString("output")
		readLabels, _ := cmd.Flags().GetBool("read-labels")
		transparentBg, _ := cmd.Flags().GetBool("transparent-bg")
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

		sub, ok := img.(interface {
			SubImage(image.Rectangle) image.Image
		})
		if !ok {
			return fmt.Errorf("image does not support SubImage")
		}

		if err := os.MkdirAll(outputDir, 0755); err != nil {
			return err
		}

		// labeledRows and err are declared here so both branches below can assign with =.
		var labeledRows []spritesheet.LabeledRow
		if readLabels {
			apiKey := os.Getenv("ANTHROPIC_API_KEY")
			if apiKey == "" {
				return fmt.Errorf("ANTHROPIC_API_KEY environment variable not set")
			}
			slicer := spritesheet.Slicer{LabelReader: vision.New(apiKey)}
			labeledRows, err = slicer.Slice(cmd.Context(), img)
		} else {
			var rows []spritesheet.Row
			rows, err = spritesheet.Slice(img)
			if err == nil {
				labeledRows = make([]spritesheet.LabeledRow, len(rows))
				for i, row := range rows {
					labeledRows[i] = spritesheet.LabeledRow{Row: row}
				}
			}
		}
		if err != nil {
			return err
		}

		var info *spritesheet.Info
		if transparentBg {
			info, err = spritesheet.Analyze(img)
			if err != nil {
				return err
			}
		}

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

		fmt.Printf("Sliced %d rows to %s\n", len(labeledRows), outputDir)
		return nil
	},
}

var normalizeCmd = &cobra.Command{
	Use:   "normalize <file>",
	Short: "Normalize a sprite sheet into a uniform grid with transparent background",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		outputPath, _ := cmd.Flags().GetString("output")
		padding, _ := cmd.Flags().GetInt("padding")
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

		result, err := spritesheet.Normalize(img, padding)
		if err != nil {
			return err
		}

		if err := os.MkdirAll(filepath.Dir(outputPath), 0755); err != nil {
			return err
		}

		out, err := os.Create(outputPath)
		if err != nil {
			return err
		}

		if err := png.Encode(out, result.Image); err != nil {
			out.Close()
			os.Remove(outputPath)
			return err
		}
		if err := out.Close(); err != nil {
			os.Remove(outputPath)
			return fmt.Errorf("closing output file: %w", err)
		}

		fmt.Printf("Sprite offset: %d\n", 2*result.Padding+result.LabelWidth)
		fmt.Printf("Sprite size:   %d x %d\n", result.SpriteWidth+result.Padding, result.CellHeight+result.Padding)
		fmt.Printf("Written to:    %s\n", outputPath)
		return nil
	},
}

var labelSanitizeRe = regexp.MustCompile(`[^a-z0-9]+`)

func sanitizeLabel(s string) string {
	s = strings.ToLower(s)
	s = labelSanitizeRe.ReplaceAllString(s, "_")
	return strings.Trim(s, "_")
}

func writeImage(img image.Image, path string) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	return png.Encode(f, img)
}

func main() {
	sliceCmd.Flags().String("output", "", "directory to write subimages into (required)")
	_ = sliceCmd.MarkFlagRequired("output")
	sliceCmd.Flags().Bool("read-labels", false, "use LLM OCR to read label text and include it in filenames (requires ANTHROPIC_API_KEY)")
	sliceCmd.Flags().Bool("transparent-bg", false, "replace background-colored pixels with transparency in output PNGs")
	normalizeCmd.Flags().String("output", "", "path to write the normalized PNG (required)")
	_ = normalizeCmd.MarkFlagRequired("output")
	normalizeCmd.Flags().Int("padding", 8, "gap in pixels between cells and around the sheet edges")
	rootCmd.AddCommand(normalizeCmd)
	rootCmd.AddCommand(infoCmd)
	rootCmd.AddCommand(sliceCmd)
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
