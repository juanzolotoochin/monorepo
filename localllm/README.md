# Local OpenCode + Strata

```sh
bazel run //localllm/opencode
```

The Rust launcher starts Strata on an unused loopback port, waits for model
readiness, and starts OpenCode with a local OpenAI-compatible provider. Exiting
OpenCode stops the server and its engine. Both the primary and small-model
providers point to Strata. Nothing downloads at application startup.

This configuration targets **Linux x86-64 with AVX2 and an NVIDIA Ada GPU
(sm_89)**. It has been exercised on an RTX 4090 (24 GB VRAM), 64 GB system RAM,
and NVIDIA driver 615.71.09. A host NVIDIA driver compatible with CUDA 13 is
required; CUDA headers, compiler components, and user-space libraries come from
pinned Bazel repositories. Other GPUs need an explicit architecture change in
`private/cuda.bzl` and validation; they are not automatically detected at build
time.

The selected model is Qwen3.8 Flash Next GSQ-RCO IQ3_XXS, with a quantized MTP
draft, speculative decoding, and a 64K context limit by default. OpenCode compacts
at about 44K reported tokens, retaining up to 4K of recent turns, and limits individual
tool previews to 400 lines / 16 KB (larger outputs remain on disk). Strata reduces
the requested output budget to the space left when necessary, without truncating
the prompt. A prompt that fills the entire window still needs compaction or a
new session; fitting the output budget does not enlarge the selected context.
The server caps thinking at 12K tokens, leaving approximately 4K of the 16K
response budget for the answer and the thinking wrap-up. This addresses an
observed review request that used the old 8K budget entirely on thinking.
If Strata clamps the response budget near the context limit, that remaining
answer allowance is not guaranteed; OpenCode still needs to compact first.

The main GGUF shards are
75.8 GB and the draft inputs are 5.2 GB (decimal). Allow around **300 GB free
disk space** for downloads, Bazel's repository cache, extracted repositories,
toolchains, and outputs (including the optional IQ2_XS comparison model). Keep Bazel's output base and repository cache on disk,
not a small tmpfs. First-time fetching requires network access and can take a
while. Large targets are tagged `manual` so ordinary `//...` builds/tests do not
fetch the model; the explicit commands below still build them. Subsequent runs reuse those inputs and outputs.

IQ3_XXS uses higher-precision weights than the original IQ2_XS default. It trades
speed and RAM for expected quality; model-author benchmarks are encouraging,
but do not establish a quality gain on this repository. Both variants use the
same pinned model revision, draft, 64K default context, 16K output budget, INT8 KV
cache, and 3 GiB GPU reserve. Changing precision does not increase context size.
The IQ3_XXS first shard is 47.0 GB; its 28.8 GB second shard has the same SHA-256
as IQ2_XS and can be reused from Bazel's repository cache.

The previous variant remains available for comparison:

```sh
bazel run //localllm/opencode --define=localllm_quant=IQ2_XS
```

Omit that flag to return to IQ3_XXS. Model selection happens in Bazel, so the
launcher never fetches a model and only the selected variant enters its runfiles.
The higher-precision model needs more system RAM; close other inference sessions
before starting it on a 64 GB machine. The context-capacity measurements below
were taken with IQ2_XS and should not be read as IQ3_XXS measurements.

Useful commands:

```sh
# Load the real model, check readiness, then stop.
bazel run //localllm/opencode -- --check

# Pass arguments to OpenCode.
bazel run //localllm/opencode -- run 'Explain this repository'
bazel run //localllm/opencode -- --launcher-help

# Avoid the monorepo's remote cache/build-event services when working offline.
bazel build //localllm/opencode --config=full_local --nofetch \
  --sandbox_default_allow_network=false

# Lightweight launcher tests, without fetching model weights.
bazel test //localllm/opencode:launcher_test --config=full_local

# Explicit, large-input reproducibility test: independently convert both models
# into different output paths and compare every output file byte for byte.
bazel test //localllm/strata:reproducibility_test --config=full_local \
  --nofetch --sandbox_default_allow_network=false
```

`--nofetch` requires the external repositories to have been fetched already.
Bazel itself, its repository cache, and the monorepo's shared toolchain setup
remain prerequisites. The exact `bazel run //localllm/opencode` command uses the
monorepo's ordinary remote-cache settings; add `--config=full_local` to avoid
those services.

Set `LOCALLLM_CONTEXT` to `32768`, `65536` (default), or `131072`. The same
value configures the engine capacity and OpenCode's context/input limits; an
16K response budget plus a 4K safety buffer is reserved before compaction. For
example:

```sh
LOCALLLM_CONTEXT=131072 bazel run //localllm/opencode
```

Set `LOCALLLM_PORT` to select a port, or `LOCALLLM_STARTUP_TIMEOUT` to change the
600-second startup timeout. An occupied port causes an error; the launcher
never takes over an existing service. Logs live in a temporary directory named
at startup and are removed on shutdown. Startup failures include the server
log. OpenCode sessions persist under `${XDG_DATA_HOME:-$HOME/.local/share}/localllm`.
The working directory is the directory from which `bazel run` was invoked.

## Hermeticity and reproducibility

Repository rules fetch immutable, checksum-verified artifacts. They do not run
compilers, package installers, model conversion, or host Python. Native C++ and
CUDA compile through the monorepo's LLVM toolchain using explicit source lists
and CPU/GPU options. This replaces upstream CMake's dependency fetching and
host CPU detection. Python uses the registered Python 3.11 runtime and a
hash-locked wheel dependency set. OpenCode builds from pinned source with
pinned Bun and an SRI-locked npm closure, without npm lifecycle scripts or
`bun install`. CommonJS build paths are normalized before bundling.

Main-model preparation and draft quantization are ordinary sandboxed Bazel
actions with declared inputs. Draft conversion excludes upstream timing reports,
uses one numeric-library thread, and disables optional NumPy CPU dispatch paths.
Model repositories include tokenizer and draft vocabulary inputs; there are no
floating Hugging Face revisions or runtime model lookups.

The launcher supplies a private configuration, a random local API key, a pinned
ripgrep, and local model definitions. Automatic updates, models.dev refreshes,
project/global plugin discovery, package installation, LSP downloads, sharing,
and formatters are disabled. The web UI is not embedded. This is the terminal
coding client; dynamically installing extensions is intentionally unavailable.

The hermetic boundary covers application builds and inference assets. It does
not include the host kernel/NVIDIA driver, user files, OpenCode session state,
or commands the coding agent executes. Coding tools retain the user's shell
and PATH. The launcher does not itself impose a network sandbox: a coding
request can still invoke network tools. Reproducible binaries and model packs
also do not promise identical model responses across runs or GPU drivers.

Verification includes separately built, byte-identical Strata and OpenCode
executables, model-conversion comparison tests, and local inference/tool use
with only loopback networking available. The checked-in tests make model
conversion reproducibility repeatable; comparing native/application builds
requires separate Bazel output bases to avoid an action-cache hit.

## Quality trial

`quality-comparison.json` records the exact user-requested `midi_keys` code-quality
prompt, settings, final model responses, and source spot-checks. IQ2_XS completed
in about 197 seconds of CLI events; IQ3_XXS with the final budget completed in
about 142 seconds. These are different tool trajectories, not comparable decode
benchmarks: the IQ2 run overlapped a model download, and the initial IQ3 run
also overlapped a conversion test. Do not infer a speed or quality improvement
from those durations.

The initial IQ3_XXS run used the old 8K output limit and exhausted it twice,
including one request with no answer because all output was thinking. That run
was stopped. The final 16K output / 12K thinking configuration completed the
review; captured logs contained no tokenizer exception or empty-answer warning,
and sampled total GPU use peaked at 21,786 MiB. Sampling was every five seconds,
so this is not a guaranteed memory bound. The launcher, upstream context and
thinking-budget tests, and independent IQ3 model-conversion comparison passed.

Neither model demonstrated a clear quality advantage on this single broad
prompt. Both emphasized structural metrics, and source spot-checks found
incorrect or overstated claims. The higher-precision default is a candidate for
further task-specific evaluation, not a demonstrated cure for weak code reviews.

## Context window evaluation (IQ2_XS)

On the RTX 4090, all three tested capacities loaded and answered successfully.
The GGUF metadata advertises a 262,144-token model context; 256K has not been
validated here. The runtime default is 65,536 tokens, with 131,072 available
through `LOCALLLM_CONTEXT`.

| Capacity | Largest prompt tested | Marker retrieval | Expert cache | Generation speed* |
| --- | ---: | --- | ---: | ---: |
| 32,768 | 24,508 tokens | Passed | 15.82 GiB | 112.9 tokens/s |
| 65,536 | 57,273 tokens | Passed | 15.34 GiB | 110.7 tokens/s |
| 131,072 | 122,804 tokens | Passed | 14.27 GiB | 107.6 tokens/s |

*Generation used the same 3,840-token code-review prompt and a 512-token reply
budget at each capacity, after the build completed. These rates do not measure
512-token replies over a full 128K prompt. The long-prompt retrieval test used
synthetic source text with markers at the beginning, middle, and end. Processing
the 57K and 123K prompts took about 19 and 42 seconds respectively.

These are single-run smoke measurements with varying desktop GPU activity;
background build work also overlapped part of the 128K capacity test. They are
not averaged performance benchmarks or broad code-review quality evaluations.
Raw measurements and the protocol are in `context-window-results.json`.

The measurements above used the original 700 MiB VRAM reserve. A subsequent
long-session report failed at `verify: instantiate: out of memory`, where CUDA
allocates a speculative-verification graph. The launcher now explicitly reserves
3,072 MiB from the automatic expert-cache budget for these lazy allocations and
desktop GPU usage. This reduces the expert cache; the earlier speed/cache numbers
are historical, not measurements of the new reserve. The reserve is allocation
headroom, not a guarantee that 3 GiB will remain free throughout a session.
OpenCode compaction is driven by token counts; it does not monitor free VRAM or
release the engine's cached CUDA graphs. The configured KV capacity also remains
allocated when the conversation is compacted. With the new reserve, a 64K
chat-only stress run completed six sequential requests (about 6K–48K prompt
tokens, 512–517 response tokens) with stable memory at request boundaries and
2,825 MiB free at startup. This does not reproduce every detail of the reported
session. An initial diagnostic run also exposed a separate tokenizer exception
in `/v1/messages/count_tokens`; that endpoint issue is not fixed by this change. The same tokenizer exception also
occurred during an IQ3_XXS chat-completion request; OpenCode retried it.
Both attempts are recorded in `context-window-results.json`.

Strata's automatic expert cache consumes most of the remaining VRAM at every capacity.
Increasing context reduces that cache rather than simply adding all the extra
allocation on top. 64K is a conservative default; 128K provides more room for
large reviews at the cost of a smaller expert cache and longer processing when
that context is actually used. The maximum practical window has not been
established.

## Pins and updates

- `MODULE.bazel`: Strata, llama.cpp/ggml, Bun, ripgrep, Python dependency hub.
- `private/repositories.bzl`: OpenCode source commit and archive digest.
- `private/cuda.lock.json`: NVIDIA redistribution components and SHA-256 hashes.
- `private/model.lock.json` and `private/model-iq3-xxs.lock.json`: GGUF revision,
  immutable URLs, sizes, hashes for IQ2_XS and IQ3_XXS respectively.
- `private/mtp.lock.json`: immutable safetensors revision, tensor ranges, shapes,
  dtypes, and per-range SHA-256 hashes. Range downloads fail closed on a digest
  mismatch, including servers that return the wrong range.
- `private/opencode-npm.lock.json`: resolved Linux x64 dependency closure from
  the pinned upstream Bun lock, including workspace mappings.
- `strata/requirements.lock.txt`: pinned Python packages and artifact hashes.

For a source update, review upstream changes and update the archive digest,
explicit native source lists/build overlays, CUDA aliases, and patches together.
Do not restore upstream network fetches or `-march=native`. Regenerate the npm
closure from the new source's `bun.lock` with the pinned Bun executable:

```sh
/path/to/pinned/bun localllm/private/lock_opencode.ts /path/to/opencode-source
```

Update the OpenCode version in `private/build_opencode.ts` with its source pin.
The lock generator is TypeScript because it uses Bun's own lockfile parser; the
launcher and output-comparison test are Rust. The small draft adapter is Python
because the upstream quantizers are Python.

For model updates, use immutable Hugging Face commit URLs and independently
verify the full shards or tensor bytes against the recorded hashes. Review
licenses and upstream model/engine compatibility for the chosen replacement.
Re-run launcher tests, model reproducibility, independent binary builds, and
the GPU-backed end-to-end check after changing any pin. Pinned vendor tools and
binary npm/wheel dependencies are bootstrap inputs; this is not a claim that
every toolchain and transitive dependency is rebuilt from source.
