// Package labels resolves raw animation names (e.g. "JumpAttack") to
// human-readable display labels (e.g. "Jump Attack") using an LLM, with a
// cache to avoid redundant API calls across invocations.
package labels

import (
	"context"
	"fmt"
)

// LLMClient produces human-readable display labels for a batch of raw
// animation names.
type LLMClient interface {
	ExtractLabels(ctx context.Context, names []string) (map[string]string, error)
}

// Cache persists raw→display label mappings across invocations.
// Load returns an empty map (not an error) if no data has been saved yet.
// Save receives the full updated map and overwrites the previous state.
type Cache interface {
	Load() (map[string]string, error)
	Save(cache map[string]string) error
}

// Extractor resolves raw animation names to display labels. It checks the
// cache first and only calls the LLM for names that are not already cached.
type Extractor struct {
	llm   LLMClient
	cache Cache
}

// New returns an Extractor with the given LLM client and cache.
func New(llm LLMClient, cache Cache) *Extractor {
	return &Extractor{llm: llm, cache: cache}
}

// Extract returns a display label for each name. Names already in the cache
// are returned without an LLM call. New names are resolved via the LLM and
// persisted to the cache. If the LLM does not return a label for a name, the
// raw name is used as the fallback.
func (e *Extractor) Extract(ctx context.Context, names []string) (map[string]string, error) {
	cache, err := e.cache.Load()
	if err != nil {
		return nil, fmt.Errorf("labels: load cache: %w", err)
	}

	var missing []string
	for _, name := range names {
		if _, ok := cache[name]; !ok {
			missing = append(missing, name)
		}
	}

	if len(missing) > 0 {
		fresh, err := e.llm.ExtractLabels(ctx, missing)
		if err != nil {
			return nil, fmt.Errorf("labels: llm: %w", err)
		}
		for k, v := range fresh {
			cache[k] = v
		}
		if err := e.cache.Save(cache); err != nil {
			return nil, fmt.Errorf("labels: save cache: %w", err)
		}
	}

	result := make(map[string]string, len(names))
	for _, name := range names {
		if display, ok := cache[name]; ok {
			result[name] = display
		} else {
			result[name] = name // LLM didn't return this name — use raw
		}
	}
	return result, nil
}
