package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"time"

	"github.com/juanique/monorepo/salsa/go/json"
	"github.com/juanique/monorepo/salsa/go/must"
	"github.com/spf13/cobra"
)

type Options struct {
	Output                string
	OnlyGetImageID        bool
	LogToFile             string
	NoReuseExistingLayers bool
	NoRun                 bool // backwards compatibilty with rules_dockerk
}

var opts = Options{}

var rootCmd = &cobra.Command{
	Use:   "loader",
	Short: "loader is a tool that loads images into docker incrementally",
	Run: func(cmd *cobra.Command, args []string) {
		imagePath := args[0]
		repoTags := args[1:]

		image := must.Must(NewImage(imagePath))
		must.NoError(buildAndLoadImage(image, repoTags))
	},
}

func buildAndLoadImage(i Image, repoTags []string) error {
	ctx := context.Background()
	originalImage := i

	dockerImageId := i.Manifest.Config.Digest
	log.Println("Computed Image ID:", dockerImageId)
	builder := NewImageBuilder(dockerImageId, repoTags)
	if err := builder.Prepare(&i); err != nil {
		log.Println("Could not prepare image:", err)

		// Undo any attempts to modify the image
		i = originalImage
	}

	if opts.OnlyGetImageID {
		fmt.Println(i.Manifest.Config.Digest)
		return nil
	}

	loader, err := NewDockerLoader()
	if err != nil {
		return err
	}

	if len(repoTags) == 0 {
		return fmt.Errorf("No repo tags specified")
	}

	// 1. Check if Image is already loaded (Strict ID or Loose Config match)
	var configData map[string]interface{}
	if err := json.FromFile(builder.ConfigPath, &configData); err != nil {
		return fmt.Errorf("failed to read config: %w", err)
	}

	found, action, err := loader.CheckImageExists(ctx, dockerImageId, configData, repoTags)
	log.Println("Checking for ID:", dockerImageId)
	if err != nil {
		return err
	}

	if found {
		log.Println("Image already loaded.")
		// We still print the action JSON for bazel consumption if needed?
		// Existing code prints action JSON if opts.Output == "json"
		if opts.Output == "json" {
			fmt.Println(action.JSON())
		}
		// Print legacy logs
		if action.AlreadyLoaded {
			log.Println("Image ID", dockerImageId, "was already loaded.")
			fmt.Println("Image ID", dockerImageId, "was already loaded.")
		}
		for _, tag := range action.TagsAlreadyPresent {
			log.Println("Image was already tagged with", tag)
			fmt.Println("Image was already tagged with", tag)
		}
		for _, tag := range action.TagsAdded {
			log.Println("Tagged image with", tag)
			fmt.Println("Tagged image with", tag)
		}
		return nil
	}

	// 2. If not loaded, we must load.
	// Since containerd might be strict about layers, we should provide ALL layers.
	// We do NOT use SkipLayers optimization here because we've determined the image isn't "the same"
	// or we can't reliably perform a partial load.
	// NOTE: CheckImageExists handles the case where "Content is same, ID differs".
	// If it returned false, it means content (config) is effectively different or strict check failed and loose check failed.
	// So we are treating it as a new image -> Full Load.

	tarPath, err := builder.Build(i, BuildOpts{SkipLayers: nil})
	if err != nil {
		return err
	}

	// LoadTarIntoDocker will check for existing image strictly by ID again,
	// but we already know it's not there by ID (from CheckImageExists strict check).
	// So it should proceed to load.
	action = must.Must(loader.LoadTarIntoDocker(context.Background(), tarPath, i.Manifest.Config.Digest, repoTags))

	if opts.Output == "json" {
		fmt.Println(action.JSON())
		log.Println(action.JSON())
	}

	if action.AlreadyLoaded {
		log.Println("Image ID", dockerImageId, "was already loaded.")
		fmt.Println("Image ID", dockerImageId, "was already loaded.")
	}

	for _, tag := range action.TagsAlreadyPresent {
		log.Println("Image was already tagged with", tag)
		fmt.Println("Image was already tagged with", tag)
	}

	for _, tag := range action.TagsAdded {
		log.Println("Tagged image with", tag)
		fmt.Println("Tagged image with", tag)
	}

	return nil
}

func main() {
	startTime := time.Now()
	rootCmd.Flags().StringVar(&opts.Output, "output", "", "Format for the output")
	rootCmd.Flags().BoolVar(&opts.OnlyGetImageID, "only-get-image-id", false, "Only print the image ID, not build it")
	rootCmd.Flags().BoolVar(&opts.NoRun, "norun", false, "unused - only here for backwards compatibility with rules_docker")
	rootCmd.Flags().BoolVar(&opts.NoReuseExistingLayers, "noreusexistinglayers", false, "do not reuse existing layers")
	rootCmd.Flags().StringVar(&opts.LogToFile, "log-to-file", "", "whether to print logs to a file")

	if err := rootCmd.Execute(); err != nil {
		os.Exit(1)
	}
	log.Println("Total time:", time.Since(startTime))
}
