// Package vision provides a Claude-based client for extracting text from images.
package vision

import (
	"bytes"
	"context"
	"encoding/base64"
	"fmt"
	"image"
	"image/png"
	"strings"

	"github.com/anthropics/anthropic-sdk-go"
	"github.com/anthropics/anthropic-sdk-go/option"
)

const defaultModel = anthropic.ModelClaudeSonnet4_6
const defaultMaxTokens int64 = 256

// Client extracts text from images using Claude's vision capability.
type Client struct {
	inner anthropic.Client
	model anthropic.Model
}

// Option configures the Client.
type Option func(*Client)

// WithModel overrides the default model.
func WithModel(model anthropic.Model) Option {
	return func(c *Client) {
		c.model = model
	}
}

// New creates a new vision Client.
func New(apiKey string, opts ...Option) *Client {
	c := &Client{
		inner: anthropic.NewClient(option.WithAPIKey(apiKey)),
		model: defaultModel,
	}
	for _, opt := range opts {
		opt(c)
	}
	return c
}

// ReadText encodes img as PNG, sends it to Claude's vision API, and returns the
// text visible in the image.
func (c *Client) ReadText(ctx context.Context, img image.Image) (string, error) {
	var buf bytes.Buffer
	if err := png.Encode(&buf, img); err != nil {
		return "", fmt.Errorf("vision: encode image: %w", err)
	}
	b64 := base64.StdEncoding.EncodeToString(buf.Bytes())

	message, err := c.inner.Messages.New(ctx, anthropic.MessageNewParams{
		Model:     c.model,
		MaxTokens: defaultMaxTokens,
		Messages: []anthropic.MessageParam{
			anthropic.NewUserMessage(
				anthropic.NewImageBlockBase64("image/png", b64),
				anthropic.NewTextBlock("What text is visible in this image? Reply with only the text, nothing else."),
			),
		},
	})
	if err != nil {
		return "", fmt.Errorf("vision: API request failed: %w", err)
	}

	if len(message.Content) == 0 {
		return "", fmt.Errorf("vision: empty response")
	}

	return strings.TrimSpace(message.Content[0].Text), nil
}

// ReadLabel returns the text visible in img.
func (c *Client) ReadLabel(ctx context.Context, img image.Image) (string, error) {
	return c.ReadText(ctx, img)
}
