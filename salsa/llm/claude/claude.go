// Package claude provides a simple client for querying Claude with structured outputs.
//
// Usage:
//
//	type Response struct {
//	    Summary string   `json:"summary"`
//	    Tags    []string `json:"tags"`
//	}
//
//	llm := claude.New(apiKey)
//	var res Response
//	err := llm.Query(ctx, "Summarize this text...", &res)
package claude

import (
	"context"
	"encoding/json"
	"fmt"
	"reflect"
	"strings"

	"github.com/anthropics/anthropic-sdk-go"
	"github.com/anthropics/anthropic-sdk-go/option"
)

const defaultModel = anthropic.ModelClaudeSonnet4_6
const defaultMaxTokens int64 = 4096

// Client is a Claude API client that supports structured outputs.
type Client struct {
	inner anthropic.Client
	model anthropic.Model
}

// Option configures the Client.
type Option func(*Client)

// WithModel overrides the default model (claude-sonnet-4-6).
func WithModel(model anthropic.Model) Option {
	return func(c *Client) {
		c.model = model
	}
}

// New creates a new Claude client.
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

// Query sends a prompt to Claude and unmarshals the structured response into result.
// result must be a non-nil pointer to a struct.
func (c *Client) Query(ctx context.Context, prompt string, result any) error {
	rv := reflect.ValueOf(result)
	if rv.Kind() != reflect.Ptr || rv.IsNil() {
		return fmt.Errorf("claude: result must be a non-nil pointer, got %T", result)
	}

	schema, err := schemaFromType(rv.Elem().Type())
	if err != nil {
		return fmt.Errorf("claude: failed to generate schema: %w", err)
	}

	message, err := c.inner.Messages.New(ctx, anthropic.MessageNewParams{
		Model:     c.model,
		MaxTokens: defaultMaxTokens,
		Messages: []anthropic.MessageParam{
			anthropic.NewUserMessage(anthropic.NewTextBlock(prompt)),
		},
		OutputConfig: anthropic.OutputConfigParam{
			Format: anthropic.JSONOutputFormatParam{
				Schema: schema,
			},
		},
	})
	if err != nil {
		return fmt.Errorf("claude: API request failed: %w", err)
	}

	if len(message.Content) == 0 {
		return fmt.Errorf("claude: empty response")
	}

	if err := json.Unmarshal([]byte(message.Content[0].Text), result); err != nil {
		return fmt.Errorf("claude: failed to parse response: %w", err)
	}

	return nil
}

// schemaFromType generates a JSON schema from a Go type using reflection.
func schemaFromType(t reflect.Type) (map[string]any, error) {
	return typeToSchema(t)
}

func typeToSchema(t reflect.Type) (map[string]any, error) {
	// Dereference pointers.
	for t.Kind() == reflect.Ptr {
		t = t.Elem()
	}

	switch t.Kind() {
	case reflect.String:
		return map[string]any{"type": "string"}, nil

	case reflect.Bool:
		return map[string]any{"type": "boolean"}, nil

	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64,
		reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		return map[string]any{"type": "integer"}, nil

	case reflect.Float32, reflect.Float64:
		return map[string]any{"type": "number"}, nil

	case reflect.Slice, reflect.Array:
		items, err := typeToSchema(t.Elem())
		if err != nil {
			return nil, err
		}
		return map[string]any{"type": "array", "items": items}, nil

	case reflect.Map:
		if t.Key().Kind() != reflect.String {
			return nil, fmt.Errorf("unsupported map key type: %s (only string keys supported)", t.Key())
		}
		vals, err := typeToSchema(t.Elem())
		if err != nil {
			return nil, err
		}
		return map[string]any{"type": "object", "additionalProperties": vals}, nil

	case reflect.Struct:
		return structToSchema(t)

	default:
		return nil, fmt.Errorf("unsupported type: %s", t.Kind())
	}
}

func structToSchema(t reflect.Type) (map[string]any, error) {
	properties := make(map[string]any)
	var required []string

	for i := range t.NumField() {
		field := t.Field(i)
		if !field.IsExported() {
			continue
		}

		tag := field.Tag.Get("json")
		if tag == "-" {
			continue
		}

		name, opts := parseJSONTag(tag)
		if name == "" {
			name = field.Name
		}

		fieldSchema, err := typeToSchema(field.Type)
		if err != nil {
			return nil, fmt.Errorf("field %s: %w", field.Name, err)
		}

		// Use the "desc" struct tag for field descriptions.
		if desc := field.Tag.Get("desc"); desc != "" {
			fieldSchema["description"] = desc
		}

		properties[name] = fieldSchema

		if !opts.contains("omitempty") && field.Type.Kind() != reflect.Ptr {
			required = append(required, name)
		}
	}

	schema := map[string]any{
		"type":                 "object",
		"properties":           properties,
		"additionalProperties": false,
	}
	if len(required) > 0 {
		schema["required"] = required
	}

	return schema, nil
}

type tagOpts string

func (o tagOpts) contains(opt string) bool {
	for o != "" {
		var name string
		if i := strings.IndexByte(string(o), ','); i >= 0 {
			name, o = string(o[:i]), o[i+1:]
		} else {
			name, o = string(o), ""
		}
		if name == opt {
			return true
		}
	}
	return false
}

func parseJSONTag(tag string) (string, tagOpts) {
	if i := strings.IndexByte(tag, ','); i >= 0 {
		return tag[:i], tagOpts(tag[i+1:])
	}
	return tag, ""
}
