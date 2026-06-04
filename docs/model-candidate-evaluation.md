# Model Candidate Evaluation

dybur's production model registry should only contain models that the desktop app
can download, load, transcribe with, and verify locally. New runtimes such as
CoreML, MLX, Transformers, or vLLM stay in the experimental candidate list until
they pass the same corpus and smoke checks as the existing ONNX models.

## Candidate Priority

1. `parakeet-tdt-v3-coreml`
   - Goal: macOS-only Apple Silicon acceleration for the current default model family.
   - Why: best product fit for a native Mac build; likely smaller and cleaner than a Python MLX path.
   - Required before shipping: CoreML adapter, signed macOS build smoke, WER/latency comparison against `parakeet-tdt-v3-int8`.

2. `qwen3-asr-0.6b`
   - Goal: broader multilingual and unified offline/streaming coverage.
   - Why: materially expands beyond Parakeet's 25 European-language set.
   - Required before shipping: local runtime decision, memory profile, WER/latency comparison, language coverage checks.

3. `moonshine-streaming-tiny`
   - Goal: lightweight low-latency English dictation.
   - Why: useful if it beats Nemotron streaming on first-token/final latency for short utterances.
   - Required before shipping: hallucination checks, streaming behavior check, runtime path that does not require a heavyweight Python install.

4. `parakeet-tdt-v3-mlx`
   - Goal: benchmark/reference runtime for Parakeet on Apple Silicon.
   - Why: useful for comparison, but the model bundle and Python/MLX dependency chain are less attractive than CoreML for production.
   - Required before shipping: native adapter or a decision that it remains benchmark-only.

Deferred candidates:

- `canary-1b-v2`: revisit only if speech translation becomes a product goal.
- `voxtral-mini-3b`: revisit only if dybur grows into audio understanding, summaries, or voice-command workflows.

Use `dybur models candidates` to list active candidates and `dybur models candidates --all`
to include deferred options.

## Benchmark Workflow

1. Record a fixed local corpus using the sample categories in
   [asr-evaluation.md](./asr-evaluation.md). Tag samples by language, noise
   condition, length, and domain so the report can show per-tag regressions.
2. Fill `benchmarks/asr/<run>.json` with `samples[]` and references.
3. Copy [candidate-commands.example.json](../benchmarks/asr/candidate-commands.example.json)
   to a local command file and enable only the runtimes installed on that
   machine. The Qwen3-ASR, Moonshine, and CoreML entries point at local
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

## Automated Checks

Use these before changing candidate metadata, wrappers, or runner behavior:

```bash
pnpm test:scripts
pnpm test
```

`pnpm test:scripts` covers candidate-runner dry-runs, preflight behavior,
disabled-command reporting, JSON transcript extraction, run-manifest output,
manifest validation, strict ASR scoring validation, ASR gate checks, and CLI
candidate catalog smoke checks. `pnpm test` runs the workspace package tests and
then the script tests.

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

- `qwen3-asr.py` uses the official `qwen-asr` package's Transformers backend
  for repeatable offline corpus runs. Qwen's vLLM backend remains the right
  path for high-throughput or streaming experiments, but it is too heavy to
  treat as a dybur desktop runtime until benchmark results justify it.
- `fluidaudio-coreml.js` invokes FluidAudio's Swift CLI on macOS for the
  CoreML Parakeet spike. It is the preferred Apple Silicon benchmark path before
  deciding whether dybur needs a native Swift sidecar or deeper Tauri
  integration.
- `moonshine-transformers.py` uses the Hugging Face ASR pipeline for
  `UsefulSensors/moonshine-streaming-tiny`. The model card notes that the
  Transformers path is not fully efficient streaming yet, so benchmark both
  latency and hallucination behavior before considering product exposure.
