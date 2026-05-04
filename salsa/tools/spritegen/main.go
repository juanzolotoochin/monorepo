package main

import (
	"fmt"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"
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

var sliceCmd = &cobra.Command{
	Use:   "slice <file>",
	Short: "Slice a sprite sheet into individual subimages",
	Args:  cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		outputDir, _ := cmd.Flags().GetString("output")
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

		rows, err := spritesheet.Slice(img)
		if err != nil {
			return err
		}

		if err := os.MkdirAll(outputDir, 0755); err != nil {
			return err
		}

		sub, ok := img.(interface {
			SubImage(image.Rectangle) image.Image
		})
		if !ok {
			return fmt.Errorf("image does not support SubImage")
		}

		for i, row := range rows {
			labelPath := filepath.Join(outputDir, fmt.Sprintf("%02d_label.png", i))
			if err := writeSubImage(sub, row.Label, labelPath); err != nil {
				return err
			}
			for j, sprite := range row.Sprites {
				spritePath := filepath.Join(outputDir, fmt.Sprintf("%02d_%02d.png", i, j))
				if err := writeSubImage(sub, sprite, spritePath); err != nil {
					return err
				}
			}
		}

		fmt.Printf("Sliced %d rows to %s\n", len(rows), outputDir)
		return nil
	},
}

func writeSubImage(img interface {
	SubImage(image.Rectangle) image.Image
}, rect image.Rectangle, path string) error {
	f, err := os.Create(path)
	if err != nil {
		return err
	}
	defer f.Close()
	return png.Encode(f, img.SubImage(rect))
}

func main() {
	sliceCmd.Flags().String("output", "", "directory to write subimages into (required)")
	_ = sliceCmd.MarkFlagRequired("output")
	rootCmd.AddCommand(infoCmd)
	rootCmd.AddCommand(sliceCmd)
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
