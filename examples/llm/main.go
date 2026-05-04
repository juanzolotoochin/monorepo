package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"strings"

	"github.com/juanique/monorepo/salsa/llm/claude"
)

type TextAnalysis struct {
	Sentiment string   `json:"sentiment" desc:"Overall sentiment: positive, negative, or neutral"`
	Topics    []string `json:"topics"    desc:"Main topics or themes in the text"`
	Summary   string   `json:"summary"   desc:"One-sentence summary of the text"`
}

const sampleText = `The Go programming language was designed at Google in 2007.
It emphasizes simplicity, reliability, and efficiency.
Its concurrency model, based on goroutines and channels,
makes it well-suited for building scalable network services.`

func main() {
	apiKey := os.Getenv("ANTHROPIC_API_KEY")
	if apiKey == "" {
		log.Fatal("ANTHROPIC_API_KEY environment variable not set")
	}

	client := claude.New(apiKey)

	var analysis TextAnalysis
	err := client.Query(
		context.Background(),
		fmt.Sprintf("Analyze this text:\n\n%s", sampleText),
		&analysis,
	)
	if err != nil {
		log.Fatalf("query failed: %v", err)
	}

	fmt.Printf("Sentiment: %s\n", analysis.Sentiment)
	fmt.Printf("Topics:    %s\n", strings.Join(analysis.Topics, ", "))
	fmt.Printf("Summary:   %s\n", analysis.Summary)
}
