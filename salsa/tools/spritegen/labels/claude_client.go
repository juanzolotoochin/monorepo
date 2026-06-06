package labels

import (
	"context"
	"fmt"
	"strings"

	"github.com/juanique/monorepo/salsa/llm/claude"
)

// ClaudeClient implements LLMClient using the Claude API.
type ClaudeClient struct {
	client *claude.Client
}

// NewClaudeClient returns an LLMClient backed by the Claude API.
func NewClaudeClient(apiKey string) *ClaudeClient {
	return &ClaudeClient{client: claude.New(apiKey)}
}

type animLabel struct {
	Raw     string `json:"raw"     desc:"The raw animation name as found in the filename"`
	Display string `json:"display" desc:"Human-readable display label for this animation, suitable for printing on a sprite sheet"`
}

type animLabelsResponse struct {
	Labels []animLabel `json:"labels"`
}

// ExtractLabels calls Claude to produce display labels for the given raw names.
func (c *ClaudeClient) ExtractLabels(ctx context.Context, names []string) (map[string]string, error) {
	var sb strings.Builder
	sb.WriteString("I have a sprite sheet with animations named by these filename prefixes:\n")
	for _, n := range names {
		fmt.Fprintf(&sb, "  - %s\n", n)
	}
	sb.WriteString("\nFor each name, return a clean, human-readable label suitable for display on a sprite sheet (e.g. split CamelCase into words, capitalize correctly). Return them in the same order as given.")

	var resp animLabelsResponse
	if err := c.client.Query(ctx, sb.String(), &resp); err != nil {
		return nil, err
	}

	result := make(map[string]string, len(names))
	for _, l := range resp.Labels {
		result[l.Raw] = l.Display
	}
	return result, nil
}
