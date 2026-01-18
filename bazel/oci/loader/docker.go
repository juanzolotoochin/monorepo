// Docker implementation of the image loader.
package main

import (
	"context"
	"fmt"
	"io/ioutil"
	"log"
	"os"
	"time"

 	"github.com/docker/docker/api/types"
 	"github.com/docker/docker/client"
 	"github.com/juanique/monorepo/salsa/go/json"
)

// areConfigsEqual compares the OCI config map with the Docker image config.
func areConfigsEqual(ociConfig map[string]interface{}, dockerImage types.ImageInspect) bool {
	// Compare Architecture and OS
	if ociConfig["architecture"] != dockerImage.Architecture {
		return false
	}
	if ociConfig["os"] != dockerImage.Os {
		return false
	}

	// Extract the nested 'config' from OCI
	ociContainerConfig, ok := ociConfig["config"].(map[string]interface{})
	if !ok {
		return false
	}

	// Compare specific fields like Env, Cmd, Entrypoint, Labels
	// We construct a temporary container.Config from OCI map to let usage of reflect or manual comparison
	// But since we have a map, let's check key fields.

	// Check Env
	if !slicesEqual(getStringSlice(ociContainerConfig, "Env"), dockerImage.Config.Env) {
		return false
	}
	// Check Entrypoint
	if !slicesEqual(getStringSlice(ociContainerConfig, "Entrypoint"), dockerImage.Config.Entrypoint) {
		return false
	}
	// Check Cmd
	if !slicesEqual(getStringSlice(ociContainerConfig, "Cmd"), dockerImage.Config.Cmd) {
		return false
	}
	// Check WorkingDir
	if getString(ociContainerConfig, "WorkingDir") != dockerImage.Config.WorkingDir {
		return false
	}
	// Check User
	if getString(ociContainerConfig, "User") != dockerImage.Config.User {
		return false
	}

	// Check Labels
	ociLabels := getMapStringString(ociContainerConfig, "Labels")
	if len(ociLabels) != len(dockerImage.Config.Labels) {
		return false
	}
	for k, v := range ociLabels {
		if dockerImage.Config.Labels[k] != v {
			return false
		}
	}

	return true
}

func getStringSlice(m map[string]interface{}, key string) []string {
	val, ok := m[key]
	if !ok || val == nil {
		return nil
	}
	// Handle []interface{} decoding from JSON
	if slice, ok := val.([]interface{}); ok {
		res := make([]string, len(slice))
		for i, v := range slice {
			res[i] = fmt.Sprint(v)
		}
		return res
	}
	// Handle []string
	if slice, ok := val.([]string); ok {
		return slice
	}
	return nil
}

func getString(m map[string]interface{}, key string) string {
	val, ok := m[key]
	if !ok {
		return ""
	}
	return fmt.Sprint(val)
}

func getMapStringString(m map[string]interface{}, key string) map[string]string {
	val, ok := m[key]
	if !ok {
		return nil
	}
	if mp, ok := val.(map[string]interface{}); ok {
		res := make(map[string]string)
		for k, v := range mp {
			res[k] = fmt.Sprint(v)
		}
		return res
	}
	if mp, ok := val.(map[string]string); ok {
		return mp
	}
	return nil
}

func slicesEqual(a, b []string) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

// DockerLoadAction contains information of the action that was actually
// performed when requesting to load the image.  Since the image may have
// already been loaded, or may some of the tags were already set, this struct
// summarizes what needed to be done.
type DockerLoadAction struct {
	Digest             string   `json:"digest"`
	AlreadyLoaded      bool     `json:"alreadyLoaded"`
	TagsAdded          []string `json:"tagsAdded"`
	TagsAlreadyPresent []string `json:"tagsAlreadyPresent"`
	LoadTime           string   `json:"loadTime"`
}

// JSON returns the JSON representation of the DockerLoadAction
func (d DockerLoadAction) JSON() string {
	return json.MustToJSON(d)
}

// DockerLoader holds a Docker client and provides methods to interact with Docker.
type DockerLoader struct {
	cli *client.Client
}

// NewDockerLoader creates a new DockerLoader using sensible defaults.
func NewDockerLoader() (*DockerLoader, error) {
	cli, err := client.NewClientWithOpts(client.FromEnv, client.WithAPIVersionNegotiation())
	if err != nil {
		return nil, fmt.Errorf("error creating Docker client: %w", err)
	}
	return &DockerLoader{cli: cli}, nil
}

// TagImage tags a Docker image with a new tag
func (d *DockerLoader) TagImage(ctx context.Context, imageID, tag string) error {
	err := d.cli.ImageTag(ctx, imageID, tag)
	if err != nil {
		return fmt.Errorf("error tagging image: %w", err)
	}
	return nil
}

// checkForExistingImage checks if an image with the specified ID exists in
// Docker.  If it does, it checks if all the tags are present.  If not, it tags
// the image with the missing tags.
func (d *DockerLoader) checkForExistingImage(ctx context.Context, imageID string, tags []string) (DockerLoadAction, error) {
	action := DockerLoadAction{}

	images, err := d.cli.ImageList(ctx, types.ImageListOptions{})
	if err != nil {
		return action, fmt.Errorf("error listing Docker images: %w", err)
	}

	tagsPresent := map[string]bool{}
	for _, tag := range tags {
		tagsPresent[tag] = false
	}

	var existingImage types.ImageSummary
	for _, image := range images {
		if image.ID == imageID {
			existingImage = image
			action.AlreadyLoaded = true
			break
		}
	}

	if !action.AlreadyLoaded {
		// We'll add all tags during the load itself
		action.TagsAdded = tags
		return action, nil
	}

	// The image was already there, we need to check if any extra tags are needed
	for _, tag := range existingImage.RepoTags {
		_, expected := tagsPresent[tag]
		if expected {
			tagsPresent[tag] = true
		}
	}

	for tag, alreadyPresent := range tagsPresent {
		if alreadyPresent {
			action.TagsAlreadyPresent = append(action.TagsAlreadyPresent, tag)
			continue
		}

		// Tag not there, we need to tag the image
		d.TagImage(ctx, imageID, tag)
		action.TagsAdded = append(action.TagsAlreadyPresent, tag)
	}

	action.Digest = imageID

	return action, nil
}

type LoadError struct {
	ErrorDetail struct {
		Message string `json:"message"`
	} `json:"errorDetail"`
}



// CheckImageExists checks if the image already exists in Docker using ID or fuzzy config match.
// If valid, returns true and an Action with AlreadyLoaded=true (and ensures tags).
// If invalid, returns false.
func (d *DockerLoader) CheckImageExists(ctx context.Context, imageID string, ociConfig map[string]interface{}, repoTags []string) (bool, DockerLoadAction, error) {
	action := DockerLoadAction{Digest: imageID}

	// 1. Check Strict ID
	_, _, err := d.cli.ImageInspectWithRaw(ctx, imageID)
	if err == nil {
		action.AlreadyLoaded = true
		// Ensure tags
		if err := d.ensureTags(ctx, imageID, repoTags, &action); err != nil {
			return true, action, err
		}
		return true, action, nil
	} else if !client.IsErrNotFound(err) {
		return false, action, fmt.Errorf("error inspecting image ID: %w", err)
	}

	// 2. Check Loose Match via First Tag
	if len(repoTags) == 0 {
		return false, action, nil
	}
	firstTag := repoTags[0]
	inspect, _, err := d.cli.ImageInspectWithRaw(ctx, firstTag)
	if err == nil {
		// Tag exists. Compare Configs.
		if areConfigsEqual(ociConfig, inspect) {
			action.AlreadyLoaded = true
			log.Println("Found existing image with matching config (ID mismatch ignored due to normalization).")
			if err := d.ensureTags(ctx, inspect.ID, repoTags, &action); err != nil {
				return true, action, err
			}
			return true, action, nil
		} else {
			log.Println("Existing image tag found but config does not match.")
		}
	} else if !client.IsErrNotFound(err) {
		log.Println("Error inspecting existing tag:", err)
	}

	return false, action, nil
}

func (d *DockerLoader) ensureTags(ctx context.Context, imageID string, repoTags []string, action *DockerLoadAction) error {
	// We need to know current tags to populate TagsAlreadyPresent
	inspect, _, err := d.cli.ImageInspectWithRaw(ctx, imageID)
	if err != nil {
		return err
	}

	currentTags := map[string]bool{}
	for _, t := range inspect.RepoTags {
		currentTags[t] = true
	}

	for _, tag := range repoTags {
		if currentTags[tag] {
			action.TagsAlreadyPresent = append(action.TagsAlreadyPresent, tag)
		} else {
			if err := d.TagImage(ctx, imageID, tag); err != nil {
				return err
			}
			action.TagsAdded = append(action.TagsAdded, tag)
		}
	}
	return nil
}

// LoadTarIntoDocker ensures that the given tar is loaded and tagged with the given tags.
func (d *DockerLoader) LoadTarIntoDocker(ctx context.Context, tarPath, imageID string, repoTags []string) (DockerLoadAction, error) {
	start := time.Now()
	// Check if the image already exists
	action, err := d.checkForExistingImage(ctx, imageID, repoTags)
	if err != nil {
		return action, err
	}
	if action.AlreadyLoaded {
		action.LoadTime = time.Since(start).String()
		return action, nil
	}

	// Open the tar file
	tar, err := os.Open(tarPath)
	if err != nil {
		return action, fmt.Errorf("error opening tar file (%s): %w", tarPath, err)
	}
	defer tar.Close()

	// Load the tar file into Docker
	response, err := d.cli.ImageLoad(ctx, tar, true)
	if err != nil {
		return action, fmt.Errorf("error loading tar file into Docker: %w", err)
	}
	defer response.Body.Close()

	// Read all data from readCloser
	data, err := ioutil.ReadAll(response.Body)
	if err != nil {
		return action, fmt.Errorf("Error reading data: %W", err)
	}

	// Convert data to a string
	loadErr := LoadError{}
	json.FromJSON(string(data), &loadErr)
	if loadErr.ErrorDetail.Message != "" {
		log.Println("Load error:", loadErr.ErrorDetail.Message)
		return action, fmt.Errorf("Error loading tar file into Docker, error details: %s", loadErr.ErrorDetail.Message)
	}

	action.Digest = imageID
	action.LoadTime = time.Since(start).String()
	return action, nil
}
