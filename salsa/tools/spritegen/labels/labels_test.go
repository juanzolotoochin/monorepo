package labels_test

import (
	"context"
	"errors"
	"testing"

	"github.com/juanique/monorepo/salsa/tools/spritegen/labels"
	"github.com/stretchr/testify/assert"
	"github.com/stretchr/testify/require"
)

// fakeCache is an in-memory Cache for tests.
type fakeCache struct {
	data    map[string]string
	saveErr error
	saved   map[string]string // last value passed to Save
}

func newFakeCache(initial map[string]string) *fakeCache {
	if initial == nil {
		initial = map[string]string{}
	}
	return &fakeCache{data: initial}
}

func (f *fakeCache) Load() (map[string]string, error) {
	out := make(map[string]string, len(f.data))
	for k, v := range f.data {
		out[k] = v
	}
	return out, nil
}

func (f *fakeCache) Save(cache map[string]string) error {
	if f.saveErr != nil {
		return f.saveErr
	}
	f.saved = cache
	f.data = cache
	return nil
}

// fakeLLM is a stub LLMClient for tests.
type fakeLLM struct {
	responses map[string]string // raw → display
	calls     [][]string        // recorded call arguments
	err       error
}

func (f *fakeLLM) ExtractLabels(_ context.Context, names []string) (map[string]string, error) {
	f.calls = append(f.calls, names)
	if f.err != nil {
		return nil, f.err
	}
	result := make(map[string]string, len(names))
	for _, n := range names {
		if display, ok := f.responses[n]; ok {
			result[n] = display
		}
	}
	return result, nil
}

func TestExtract_AllCached(t *testing.T) {
	cache := newFakeCache(map[string]string{"Idle": "Idle", "Walk": "Walk"})
	llm := &fakeLLM{}
	ex := labels.New(llm, cache)

	result, err := ex.Extract(context.Background(), []string{"Idle", "Walk"})
	require.NoError(t, err)
	assert.Equal(t, map[string]string{"Idle": "Idle", "Walk": "Walk"}, result)
	assert.Empty(t, llm.calls, "LLM should not be called when all names are cached")
}

func TestExtract_Nonecached(t *testing.T) {
	cache := newFakeCache(nil)
	llm := &fakeLLM{responses: map[string]string{
		"JumpAttack": "Jump Attack",
		"Run":        "Run",
	}}
	ex := labels.New(llm, cache)

	result, err := ex.Extract(context.Background(), []string{"JumpAttack", "Run"})
	require.NoError(t, err)
	assert.Equal(t, "Jump Attack", result["JumpAttack"])
	assert.Equal(t, "Run", result["Run"])
	require.Len(t, llm.calls, 1)
	assert.ElementsMatch(t, []string{"JumpAttack", "Run"}, llm.calls[0])
}

func TestExtract_PartialCache(t *testing.T) {
	cache := newFakeCache(map[string]string{"Idle": "Idle"})
	llm := &fakeLLM{responses: map[string]string{"JumpAttack": "Jump Attack"}}
	ex := labels.New(llm, cache)

	result, err := ex.Extract(context.Background(), []string{"Idle", "JumpAttack"})
	require.NoError(t, err)
	assert.Equal(t, "Idle", result["Idle"])
	assert.Equal(t, "Jump Attack", result["JumpAttack"])
	// LLM called only for the missing name.
	require.Len(t, llm.calls, 1)
	assert.Equal(t, []string{"JumpAttack"}, llm.calls[0])
}

func TestExtract_NewEntriesSavedToCache(t *testing.T) {
	cache := newFakeCache(map[string]string{"Idle": "Idle"})
	llm := &fakeLLM{responses: map[string]string{"Walk": "Walk"}}
	ex := labels.New(llm, cache)

	_, err := ex.Extract(context.Background(), []string{"Idle", "Walk"})
	require.NoError(t, err)
	// Both old and new entries should be in the saved cache.
	assert.Equal(t, "Idle", cache.saved["Idle"])
	assert.Equal(t, "Walk", cache.saved["Walk"])
}

func TestExtract_LLMError(t *testing.T) {
	cache := newFakeCache(nil)
	llm := &fakeLLM{err: errors.New("API timeout")}
	ex := labels.New(llm, cache)

	_, err := ex.Extract(context.Background(), []string{"Attack"})
	assert.ErrorContains(t, err, "API timeout")
}

func TestExtract_CacheSaveError(t *testing.T) {
	cache := newFakeCache(nil)
	cache.saveErr = errors.New("disk full")
	llm := &fakeLLM{responses: map[string]string{"Walk": "Walk"}}
	ex := labels.New(llm, cache)

	_, err := ex.Extract(context.Background(), []string{"Walk"})
	assert.ErrorContains(t, err, "disk full")
}

func TestExtract_LLMPartialResponse_FallsBackToRawName(t *testing.T) {
	cache := newFakeCache(nil)
	// LLM returns a label for "Walk" but not for "Attack".
	llm := &fakeLLM{responses: map[string]string{"Walk": "Walk"}}
	ex := labels.New(llm, cache)

	result, err := ex.Extract(context.Background(), []string{"Walk", "Attack"})
	require.NoError(t, err)
	assert.Equal(t, "Walk", result["Walk"])
	assert.Equal(t, "Attack", result["Attack"], "missing LLM response should fall back to raw name")
}
