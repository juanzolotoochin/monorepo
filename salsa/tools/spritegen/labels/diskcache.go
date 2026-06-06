package labels

import (
	"encoding/json"
	"os"
	"path/filepath"
)

// DiskCache implements Cache using a JSON file on disk.
type DiskCache struct {
	path string
}

// NewDiskCache returns a Cache backed by the given file path.
func NewDiskCache(path string) *DiskCache {
	return &DiskCache{path: path}
}

// DefaultDiskCache returns a DiskCache stored at the platform cache directory
// (~/.cache/spritegen/labels.json on Linux).
func DefaultDiskCache() (*DiskCache, error) {
	dir, err := os.UserCacheDir()
	if err != nil {
		return nil, err
	}
	return NewDiskCache(filepath.Join(dir, "spritegen", "labels.json")), nil
}

// Load reads the cache file and returns its contents. Returns an empty map if
// the file does not exist.
func (d *DiskCache) Load() (map[string]string, error) {
	data, err := os.ReadFile(d.path)
	if os.IsNotExist(err) {
		return map[string]string{}, nil
	}
	if err != nil {
		return nil, err
	}
	var m map[string]string
	if err := json.Unmarshal(data, &m); err != nil {
		return nil, err
	}
	return m, nil
}

// Save writes cache to disk, creating parent directories as needed.
func (d *DiskCache) Save(cache map[string]string) error {
	if err := os.MkdirAll(filepath.Dir(d.path), 0755); err != nil {
		return err
	}
	data, err := json.MarshalIndent(cache, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(d.path, data, 0644)
}
