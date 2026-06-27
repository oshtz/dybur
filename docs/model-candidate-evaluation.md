# Model Candidate Evaluation

dybur's production model registry should only contain models that the desktop app
can download, load, transcribe with, and verify locally. New runtimes such as
CoreML, MLX, Transformers, or vLLM stay in the experimental candidate list until
they pass the same corpus and smoke checks as the existing ONNX models.

## Candidate Priority

1. `nemotron-35-asr-streaming-onnx-int4`
   - Goal: replace or complement the current January 2026 Nemotron streaming export with the newer Nemotron 3.5 ONNX/int4 path.
   - Why: best fit for local live dictation research right now: cache-aware streaming, ONNX/int4 packaging, punctuation/capitalization, multilingual coverage, and model-card support for 0.56s streaming latency.
   - Required before shipping: benchmark against the current `nemotron-streaming-int8`, decide whether dybur can use ONNX Runtime GenAI natively, verify Windows/macOS packaging, and prove streaming metrics improve on the fixed corpus.

2. `parakeet-tdt-v3-coreml`
   - Goal: macOS-only Apple Silicon acceleration for the current default model family.
   - Why: best product fit for a native Mac build; likely smaller and cleaner than a Python MLX path.
   - Required before shipping: CoreML adapter, signed macOS build smoke, WER/latency comparison against `parakeet-tdt-v3-int8`.

3. `qwen3-asr-0.6b`
   - Goal: broader multilingual and unified offline/streaming coverage.
   - Why: materially expands beyond Parakeet's 25 European-language set.
   - Required before shipping: local runtime decision, memory profile, WER/latency comparison, language coverage checks.

4. `moonshine-streaming-tiny`
   - Goal: lightweight low-latency English dictation.
   - Why: useful if it beats Nemotron streaming on first-token/final latency for short utterances.
   - Required before shipping: hallucination checks, streaming behavior check, runtime path that does not require a heavyweight Python install.

5. `parakeet-tdt-v3-mlx`
   - Goal: benchmark/reference runtime for Parakeet on Apple Silicon.
   - Why: useful for comparison, but the model bundle and Python/MLX dependency chain are less attractive than CoreML for production.
   - Required before shipping: native adapter or a decision that it remains benchmark-only.

Deferred candidates:

- `canary-1b-v2`: revisit only if speech translation becomes a product goal.
- `voxtral-mini-3b`: revisit only if dybur grows into audio understanding, summaries, or voice-command workflows.

Use `packages/core/src/model-candidates.ts` to inspect active and deferred candidates.

## Benchmark Workflow

1. Record a fixed local corpus using the sample categories in
   [asr-evaluation.md](./asr-evaluation.md). Tag samples by language, noise
   condition, length, and domain so the report can show per-tag regressions.
2. Fill `benchmarks/asr/<run>.json` with `samples[]` and references.
3. Copy [candidate-commands.example.json](../benchmarks/asr/candidate-commands.example.json)
   to a local command file and enable only the runtimes installed on that
   machine. The Nemotron 3.5, Qwen3-ASR, Moonshine, and CoreML entries point at local
   benchmark wrappers in [scripts/asr-candidates](../scripts/asr-candidates/README.md);
   keep each entry disabled until its dependencies are installed. Local command
   files named `benchmarks/asr/*.local.json` are ignored by git.
4. Validate the corpus before running heavy model commands:

```bash
pnpm eval:asr:manifest benchmarks/asr/<run>.json \
  --config benchmarks/asr/corpus-policy.example.json
```

The checked-in corpus policy is intentionally stricter than the tiny example
manifest. It is the starting policy for real candidate runs: audio files must
exist, durations and tags must be present, the corpus must have at least 12
samples, and required tags must have at least two samples each. Copy it to a
local `benchmarks/asr/*.local.json` file for threshold experiments; local corpus
policy files are ignored by git. CLI flags such as `--min-samples` and
`--required-tag` can override config values for one-off checks.

5. Preflight local runtime setup:

```bash
pnpm eval:asr:candidates --commands benchmarks/asr/candidate-commands.local.json --preflight
```

Preflight mode runs each enabled command's optional `checkCommand` without
loading audio samples. Disabled commands print their setup reason; commands
without a `checkCommand` are reported as unchecked.

6. Dry-run commands:

```bash
pnpm eval:asr:candidates benchmarks/asr/<run>.json \
  --commands benchmarks/asr/candidate-commands.local.json \
  --output benchmarks/asr/candidate-runs.json \
  --dry-run
```

Dry-run mode prints disabled commands and their setup reason without executing
them. Real runs skip disabled commands.

7. Run candidates:

```bash
pnpm eval:asr:candidates benchmarks/asr/<run>.json \
  --commands benchmarks/asr/candidate-commands.local.json \
  --output benchmarks/asr/candidate-runs.json
```

The generated run manifest records source metadata such as runner version,
platform, git head, command file, timeout, selected model, and the concrete
command used for each model/sample run.

When a command entry includes `batchCommand`, the candidate runner uses that
path for non-dry corpus execution. Batch commands receive `{manifest}` and
`{output}` placeholders, load the model once, and write `runs[]` with
per-sample `hypothesis` and warmed `latencyMs` values. Prefer this path for
promotion readiness; keep per-sample `command` entries for quick wrapper smoke
tests and runtimes that do not yet support batch execution.

8. Score the generated runs:

```bash
pnpm eval:asr benchmarks/asr/candidate-runs.json \
  --output benchmarks/asr/candidate-report.md \
  --strict

pnpm eval:asr benchmarks/asr/candidate-runs.json \
  --format json \
  --output benchmarks/asr/candidate-report.json \
  --strict
```

9. Gate the result against the current production baseline:

```bash
pnpm eval:asr:gate benchmarks/asr/candidate-report.json \
  --config benchmarks/asr/gates/candidate-promotion.example.json
```

The example config is a starting gate, not a final product policy. Copy it to a
local `benchmarks/asr/gates/*.local.json` file for threshold experiments; local
gate configs are ignored by git.

## Runtime Readiness Runner

Use the runtime-readiness runner when the question is whether a candidate is
ready to move toward production. It chains corpus validation, command preflight,
candidate execution, strict JSON scoring, and an optional gate into one evidence
bundle:

```bash
pnpm eval:asr:runtime-ready benchmarks/asr/<run>.json \
  --commands benchmarks/asr/candidate-commands.local.json \
  --model nemotron-35-asr-streaming-onnx-int4 \
  --output-dir benchmarks/asr/runtime-readiness/nemotron35
```

Unlike the lower-level candidate runner, runtime readiness requires the selected
candidate command to be enabled. Copy
[candidate-commands.example.json](../benchmarks/asr/candidate-commands.example.json)
to a local command file, install the runtime dependencies, verify preflight, and
set `disabled` to `false` before running readiness. A non-dry readiness run
fails before corpus work if the selected command is missing or disabled.

The runner writes:

- `<model>-runs.json`: raw ASR candidate-runner output.
- `<model>-report.json`: strict JSON ASR scoring output.
- `readiness-summary.json`: command plan, step status, output paths, platform
  context, and timestamps.

It uses [corpus-policy.example.json](../benchmarks/asr/corpus-policy.example.json)
by default, so real promotion runs must use a fixed corpus with existing audio,
durations, tags, and minimum coverage. Use `--manifest-config` only for local
policy experiments.

Add a gate when you want the readiness command to fail on quality or regression
thresholds:

```bash
pnpm eval:asr:runtime-ready benchmarks/asr/<run>.json \
  --commands benchmarks/asr/candidate-commands.local.json \
  --baseline parakeet-tdt-v3-int8 \
  --candidate nemotron-35-asr-streaming-onnx-int4 \
  --gate-config benchmarks/asr/gates/candidate-promotion.example.json \
  --output-dir benchmarks/asr/runtime-readiness/promotion
```

For baseline regression gates, include enabled commands for exactly the baseline
and candidate models being compared, then pass `--baseline` plus one or more
`--candidate` flags. The readiness runner filters command preflight and corpus
execution to those model IDs and passes the same IDs through to the gate, so the
report cannot accidentally include unrelated enabled commands.
For candidate-only absolute gates, pass a local gate config with explicit
`candidates`, `maxWer`, `maxCer`, and latency/RTF thresholds.

Preview the plan without running model commands or writing outputs:

```bash
pnpm eval:asr:runtime-ready benchmarks/asr/<run>.json \
  --commands benchmarks/asr/candidate-commands.local.json \
  --model nemotron-35-asr-streaming-onnx-int4 \
  --output-dir benchmarks/asr/runtime-readiness/nemotron35 \
  --dry-run
```

## Installed Model Benchmark - 2026-06-10

The local Windows benchmark now covers every downloaded transcriber with a
runnable audio pipeline. Command file:
`benchmarks/asr/candidate-commands.local.json`; manifest:
`benchmarks/asr/nemotron35-smoke.local.json`; raw/scored outputs:
`.cache/asr-runtime-readiness/all-installed-models/all-installed-runs.json` and
`.cache/asr-runtime-readiness/all-installed-models/all-installed-report.json`.

| Model | WER | CER | Median Latency | Median RTF | Notes |
| --- | ---: | ---: | ---: | ---: | --- |
| `parakeet-tdt-v3-int8` | 0.0000 | 0.0000 | 713.5 ms | 0.1999 | Fastest accurate model on this corpus. |
| `parakeet-tdt-v2-int8` | 0.0000 | 0.0000 | 726 ms | 0.1862 | Legacy benchmark control; v3 remains the normal default due multilingual coverage. |
| `nemotron-35-asr-streaming-onnx-int4` | 0.0000 | 0.0000 | 2685.5 ms | 0.5382 | Accurate, but slower than Parakeet on CPU even in warmed batch mode. |
| `whisper-large-v3-turbo-int8` | 0.0000 | 0.0000 | 2938.5 ms | 0.7577 | Accurate; CPU latency is too high for live dictation default. |
| `whisper-large-v3-turbo-fp16` | 0.0000 | 0.0000 | 7674 ms | 1.7644 | Requires ORT graph optimizations disabled on CPU and runs slower than real time. |
| `nemotron-streaming-int8` | 0.0457 | 0.0311 | 2362.5 ms | 0.4194 | Streaming-capable but drops some leading words/articles on this corpus. |

Excluded downloaded folders:

- `silero-vad` is voice activity detection only, not an ASR transcriber.
- `canary-qwen-2.5b-fp16` currently contains decoder/embedding/tokenizer ONNX
  artifacts but no audio encoder/preprocessor in the installed folder, so it
  cannot transcribe the fixed audio corpus as installed.

The legacy `nemotron-streaming-int8` adapter exposed a runtime bug while adding
coverage: seeding the decoder with token `0` and skipping chunk 0 collapsed the
short smoke transcript to `"n"`. Seeding with `<blk>` and decoding the first
chunk produces the expected transcript and is now mirrored in the Rust
offline/streaming paths.

`parakeet-tdt-v2-int8` stays in the registry for explicit lookup, already
installed models, and benchmark control runs, but normal model availability
helpers filter it out. New install and picker flows should offer
`parakeet-tdt-v3-int8`, not v2.

## Automated Checks

Use these before changing candidate metadata, wrappers, or runner behavior:

```bash
pnpm test:scripts
pnpm test
```

`pnpm test:scripts` covers candidate-runner dry-runs, preflight behavior,
disabled-command reporting, JSON transcript extraction, run-manifest output,
manifest validation, strict ASR scoring validation, ASR gate checks, and CLI
candidate catalog smoke checks. It also covers the runtime-readiness runner's
dry-run plan, evidence summary, and optional gate orchestration. `pnpm test`
runs the workspace package tests and then the script tests.

## Shipping Gate

A candidate can move from candidate metadata into the production model registry
only after these checks are true:

- The runtime works without cloud calls during dictation.
- The model files have a clear license and downloadable source.
- The app can install or locate the model predictably.
- The model passes the fixed corpus with acceptable WER/CER and latency.
- The runtime has a platform-specific smoke test for the target OS.
- The model is exposed in FTUE/CLI only with accurate size, language, and streaming labels.

## Backend Notes

CoreML Parakeet is the preferred macOS spike. It should be treated as a separate
runtime adapter, not as an ONNX model entry. Keep the current ONNX Parakeet v3
default until the CoreML adapter has equivalent or better quality and a clean
Mac install path.

Qwen3-ASR and Moonshine should start as external benchmark commands. If either
wins on the corpus, choose the smallest maintainable runtime path before adding
it to the product model picker.

## Candidate Wrappers

The wrapper scripts under [scripts/asr-candidates](../scripts/asr-candidates/README.md)
are deliberately benchmark-only:

- `parakeet-tdt-onnx.py` loads dybur's installed `parakeet-tdt-v3-int8`
  production baseline from `~/.dybur/models` through ONNX Runtime. Use it as the
  baseline command for readiness gates that compare experimental candidates
  against the current default model. It can still target
  `parakeet-tdt-v2-int8` for legacy benchmark control runs with `--model-id`.
- `nemotron-streaming-onnx.py` loads dybur's installed
  `nemotron-streaming-int8` encoder/decoder/joiner export. It uses the model's
  cache metadata, seeds the decoder with `<blk>`, and decodes the first chunk.
- `whisper-onnx.py` loads dybur's installed Whisper Large v3 Turbo INT8 or FP16
  ONNX encoder/decoder with the local tokenizer. Use `--disable-optimizations`
  for the current FP16 export on ORT CPU.
- `qwen3-asr.py` uses the official `qwen-asr` package's Transformers backend
  for repeatable offline corpus runs. Qwen's vLLM backend remains the right
  path for high-throughput or streaming experiments, but it is too heavy to
  treat as a dybur desktop runtime until benchmark results justify it.
- `nemotron35-ortgenai.py` loads
  `onnx-community/nemotron-3.5-asr-streaming-0.6b-onnx-int4` through ONNX
  Runtime GenAI for a benchmark-only corpus run. The production question is
  separate: dybur needs a native ONNX Runtime GenAI adapter or a sherpa-compatible
  export before this can become a selectable model ID.
- `fluidaudio-coreml.js` invokes FluidAudio's Swift CLI on macOS for the
  CoreML Parakeet spike. It is the preferred Apple Silicon benchmark path before
  deciding whether dybur needs a native Swift sidecar or deeper Tauri
  integration.
- `moonshine-transformers.py` uses the Hugging Face ASR pipeline for
  `UsefulSensors/moonshine-streaming-tiny`. The model card notes that the
  Transformers path is not fully efficient streaming yet, so benchmark both
  latency and hallucination behavior before considering product exposure.
