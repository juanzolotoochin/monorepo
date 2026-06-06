package main

import (
	"context"
	"fmt"
	"image"
	_ "image/gif"
	_ "image/jpeg"
	"image/png"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"

	"github.com/bazelbuild/rules_go/go/tools/bazel"
	"github.com/juanique/monorepo/salsa/llm/claude"
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
		useRembg, _ := cmd.Flags().GetBool("rembg")
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

		if useRembg {
			toolPath, err := bazel.Runfile("salsa/tools/spritegen/rembg_tool")
			if err != nil {
				return fmt.Errorf("rembg_tool not found in runfiles (build with Bazel): %w", err)
			}
			rb := &rembgTool{toolPath: toolPath}

			tmpDir, err := os.MkdirTemp("", "spritegen-rembg-*")
			if err != nil {
				return err
			}
			defer os.RemoveAll(tmpDir)

			var items []spritesheet.RembgBatchItem
			for i, row := range labeledRows {
				labelPart := ""
				if row.LabelText != "" {
					labelPart = sanitizeLabel(row.LabelText) + "_"
				}
				if !row.Label.Empty() {
					tmpIn := filepath.Join(tmpDir, fmt.Sprintf("%04d_label.png", i))
					finalPath := filepath.Join(outputDir, fmt.Sprintf("%02d_%slabel.png", i, labelPart))
					if err := writeImage(sub.SubImage(row.Label), tmpIn); err != nil {
						return err
					}
					items = append(items, spritesheet.RembgBatchItem{In: tmpIn, Out: finalPath})
				}
				for j, sprite := range row.Sprites {
					tmpIn := filepath.Join(tmpDir, fmt.Sprintf("%04d_%04d.png", i, j))
					finalPath := filepath.Join(outputDir, fmt.Sprintf("%02d_%s%02d.png", i, labelPart, j))
					if err := writeImage(sub.SubImage(sprite), tmpIn); err != nil {
						return err
					}
					items = append(items, spritesheet.RembgBatchItem{In: tmpIn, Out: finalPath})
				}
			}
			if err := rb.Process(items); err != nil {
				return err
			}
			fmt.Printf("Sliced %d rows to %s\n", len(labeledRows), outputDir)
			return nil
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
		useRembg, _ := cmd.Flags().GetBool("rembg")
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

		var result *spritesheet.NormalizedSheet
		if useRembg {
			toolPath, err := bazel.Runfile("salsa/tools/spritegen/rembg_tool")
			if err != nil {
				return fmt.Errorf("rembg_tool not found in runfiles (build with Bazel): %w", err)
			}
			result, err = spritesheet.NormalizeWithRembg(img, &rembgTool{toolPath: toolPath}, padding)
			if err != nil {
				return err
			}
		} else {
			result, err = spritesheet.Normalize(img, padding)
			if err != nil {
				return err
			}
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

// rembgTool implements spritesheet.RembgBatch by invoking the rembg Python tool.
// Items are passed via a manifest JSON file so the model loads only once.
type rembgTool struct {
	toolPath string
}

func (r *rembgTool) Process(items []spritesheet.RembgBatchItem) error {
	manifestFile, err := os.CreateTemp("", "spritegen-rembg-manifest-*.json")
	if err != nil {
		return err
	}
	manifestPath := manifestFile.Name()
	manifestFile.Close()
	defer os.Remove(manifestPath)

	data, err := spritesheet.ManifestJSON(items)
	if err != nil {
		return err
	}
	if err := os.WriteFile(manifestPath, data, 0644); err != nil {
		return err
	}

	c := exec.Command(r.toolPath, manifestPath)
	c.Stdout = os.Stderr
	c.Stderr = os.Stderr
	if err := c.Run(); err != nil {
		return fmt.Errorf("rembg: %w", err)
	}
	return nil
}

// frameNameRe matches filenames like "Attack (3).png" → groups: name, number.
var frameNameRe = regexp.MustCompile(`^(.+?)\s+\((\d+)\)\.png$`)

// animLabel is a single entry in the LLM's structured label response.
type animLabel struct {
	Raw     string `json:"raw"     desc:"The raw animation name as found in the filename"`
	Display string `json:"display" desc:"Human-readable display label for this animation, suitable for printing on a sprite sheet"`
}

// animLabelsResponse is the structured output from the LLM label extraction call.
type animLabelsResponse struct {
	Labels []animLabel `json:"labels"`
}

// extractAnimLabels calls Claude to produce a display label for each raw animation name.
// Names not returned by the LLM fall back to their raw value.
func extractAnimLabels(ctx context.Context, apiKey string, names []string) (map[string]string, error) {
	prompt := "I have a sprite sheet with animations named by these filename prefixes:\n"
	for _, n := range names {
		prompt += "  - " + n + "\n"
	}
	prompt += "\nFor each name, return a clean, human-readable label suitable for display on a sprite sheet (e.g. split CamelCase into words, capitalize correctly). Return them in the same order as given."

	llm := claude.New(apiKey)
	var resp animLabelsResponse
	if err := llm.Query(ctx, prompt, &resp); err != nil {
		return nil, fmt.Errorf("LLM label extraction: %w", err)
	}

	result := make(map[string]string, len(names))
	for _, l := range resp.Labels {
		result[l.Raw] = l.Display
	}
	return result, nil
}

var packCmd = &cobra.Command{
	Use:   "pack <dir>",
	Short: "Assemble individual frame PNGs from a directory into a sprite sheet",
	Long: `Reads PNG files named "<Animation> (<N>).png" from dir,
groups them by animation name (one row each), and writes a sprite sheet.
Each row is prefixed with a label column showing the animation name.`,
	Args: cobra.ExactArgs(1),
	RunE: func(cmd *cobra.Command, args []string) error {
		dir := args[0]
		outputPath, _ := cmd.Flags().GetString("output")
		padding, _ := cmd.Flags().GetInt("padding")
		readLabels, _ := cmd.Flags().GetBool("read-labels")

		entries, err := os.ReadDir(dir)
		if err != nil {
			return err
		}

		type frameEntry struct {
			name  string
			index int
			path  string
		}
		grouped := map[string][]frameEntry{}
		for _, e := range entries {
			if e.IsDir() {
				continue
			}
			m := frameNameRe.FindStringSubmatch(e.Name())
			if m == nil {
				continue
			}
			animName := m[1]
			idx, _ := strconv.Atoi(m[2])
			grouped[animName] = append(grouped[animName], frameEntry{animName, idx, filepath.Join(dir, e.Name())})
		}
		if len(grouped) == 0 {
			return fmt.Errorf("no matching PNG files found in %s (expected \"Name (N).png\" pattern)", dir)
		}

		animNames := make([]string, 0, len(grouped))
		for name := range grouped {
			animNames = append(animNames, name)
		}
		sort.Strings(animNames)

		// displayLabel maps raw animation name → label to render in the sheet.
		displayLabel := make(map[string]string, len(animNames))
		for _, name := range animNames {
			displayLabel[name] = name // default: use raw name
		}
		if readLabels {
			apiKey := os.Getenv("ANTHROPIC_API_KEY")
			if apiKey == "" {
				return fmt.Errorf("ANTHROPIC_API_KEY environment variable not set")
			}
			extracted, err := extractAnimLabels(cmd.Context(), apiKey, animNames)
			if err != nil {
				return err
			}
			for raw, display := range extracted {
				displayLabel[raw] = display
			}
		}

		rows := make([]spritesheet.FrameRow, 0, len(animNames))
		for _, name := range animNames {
			frames := grouped[name]
			sort.Slice(frames, func(i, j int) bool { return frames[i].index < frames[j].index })
			imgs := make([]image.Image, 0, len(frames))
			for _, fe := range frames {
				f, err := os.Open(fe.path)
				if err != nil {
					return err
				}
				img, _, err := image.Decode(f)
				f.Close()
				if err != nil {
					return fmt.Errorf("decoding %s: %w", fe.path, err)
				}
				imgs = append(imgs, img)
			}
			rows = append(rows, spritesheet.FrameRow{Label: displayLabel[name], Frames: imgs})
		}

		sheet, err := spritesheet.Pack(rows, padding)
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
		if err := png.Encode(out, sheet); err != nil {
			out.Close()
			os.Remove(outputPath)
			return err
		}
		if err := out.Close(); err != nil {
			os.Remove(outputPath)
			return fmt.Errorf("closing output: %w", err)
		}

		labels := make([]string, len(animNames))
		for i, name := range animNames {
			labels[i] = displayLabel[name]
		}
		fmt.Printf("Animations: %s\n", strings.Join(labels, ", "))
		fmt.Printf("Sheet size: %d x %d\n", sheet.Bounds().Dx(), sheet.Bounds().Dy())
		fmt.Printf("Written to: %s\n", outputPath)
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
	sliceCmd.Flags().Bool("rembg", false, "use rembg AI model for background removal (requires Bazel runfiles)")
	normalizeCmd.Flags().String("output", "", "path to write the normalized PNG (required)")
	_ = normalizeCmd.MarkFlagRequired("output")
	normalizeCmd.Flags().Int("padding", 8, "gap in pixels between cells and around the sheet edges")
	normalizeCmd.Flags().Bool("rembg", false, "use rembg AI model for background removal (requires Bazel runfiles)")
	packCmd.Flags().String("output", "", "path to write the output PNG (required)")
	_ = packCmd.MarkFlagRequired("output")
	packCmd.Flags().Int("padding", 4, "gap in pixels between cells and around sheet edges")
	packCmd.Flags().Bool("read-labels", false, "use LLM to extract human-readable labels from animation names (requires ANTHROPIC_API_KEY)")
	rootCmd.AddCommand(packCmd)
	rootCmd.AddCommand(normalizeCmd)
	rootCmd.AddCommand(infoCmd)
	rootCmd.AddCommand(sliceCmd)
	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
}
