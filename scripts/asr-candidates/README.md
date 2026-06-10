# ASR Candidate Wrappers

These scripts are benchmark helpers for models that are not production dybur
model IDs yet. Each wrapper prints a transcript to stdout so
`scripts/asr-candidate-runner.js` can turn the result into an
`scripts/asr-eval.js` compatible run manifest.

Use isolated Python environments. Do not install these dependencies into the app
workspace unless you intentionally want local benchmark tooling there.

## Parakeet TDT v3 ONNX Baseline

This is the benchmark path for dybur's installed production baseline,
`parakeet-tdt-v3-int8`. It loads the model files from
`~/.dybur/models/parakeet-tdt-v3-int8` by default, so it measures the same local
ONNX artifacts that the desktop app installs.

```bash
python -m venv .venv-asr
. .venv-asr/bin/activate
pip install -U onnxruntime soundfile librosa
python scripts/asr-candidates/parakeet-tdt-onnx.py --preflight
python scripts/asr-candidates/parakeet-tdt-onnx.py samples/example.wav
```

On Windows PowerShell, activate with:

```powershell
.venv-asr\Scripts\Activate.ps1
```

Use this wrapper as the baseline command when running readiness gates that
compare experimental candidates against `parakeet-tdt-v3-int8`.

The same wrapper can target `parakeet-tdt-v2-int8` with `--model-id` for legacy
benchmark control runs. v2 is no longer a normal app/CLI picker option.

## Installed Nemotron Streaming INT8

This is the benchmark path for dybur's installed production
`nemotron-streaming-int8` model. It mirrors the app's sherpa-style streaming
transducer shape: encoder cache tensors, decoder state tensors, joiner logits,
and greedy token emission.

```bash
python scripts/asr-candidates/nemotron-streaming-onnx.py --preflight
python scripts/asr-candidates/nemotron-streaming-onnx.py samples/example.wav
```

Use this to compare the older installed streaming model against Parakeet and
newer streaming candidates on the same fixed corpus.

## Installed Whisper Large v3 Turbo ONNX

This is the benchmark path for dybur's installed
`whisper-large-v3-turbo-int8` and `whisper-large-v3-turbo-fp16` models. The
wrapper uses the local ONNX encoder/decoder with the local tokenizer/config.

```bash
python scripts/asr-candidates/whisper-onnx.py --preflight --model-id whisper-large-v3-turbo-int8
python scripts/asr-candidates/whisper-onnx.py samples/example.wav --model-id whisper-large-v3-turbo-int8
```

The current FP16 export fails ORT CPU graph initialization with default
optimizations, so benchmark it with:

```bash
python scripts/asr-candidates/whisper-onnx.py --preflight --model-id whisper-large-v3-turbo-fp16 --disable-optimizations
python scripts/asr-candidates/whisper-onnx.py samples/example.wav --model-id whisper-large-v3-turbo-fp16 --disable-optimizations
```

## Nemotron 3.5 ASR ONNX INT4

This is the benchmark path for
`onnx-community/nemotron-3.5-asr-streaming-0.6b-onnx-int4`. It is a candidate
for dybur's next live streaming backend, but it is not compatible with the
current production sherpa-style encoder/decoder/joiner runtime without an
adapter.

```bash
python -m venv .venv-nemotron35
. .venv-nemotron35/bin/activate
pip install -U onnxruntime-genai huggingface_hub soundfile librosa
python scripts/asr-candidates/nemotron35-ortgenai.py --preflight
python scripts/asr-candidates/nemotron35-ortgenai.py samples/example.wav
```

On Windows PowerShell, activate with:

```powershell
.venv-nemotron35\Scripts\Activate.ps1
```

Keep this wrapper as benchmark-only until dybur has a native ONNX Runtime GenAI
adapter or a sherpa-compatible export with the same quality and latency.

## Qwen3-ASR 0.6B

Qwen recommends the `qwen-asr` Python package and Python 3.12.

```bash
conda create -n qwen3-asr python=3.12 -y
conda activate qwen3-asr
pip install -U qwen-asr
python scripts/asr-candidates/qwen3-asr.py --preflight
python scripts/asr-candidates/qwen3-asr.py samples/example.wav
```

For GPU/vLLM streaming tests, use the official `qwen-asr[vllm]` path directly
and capture stdout with the candidate runner. The wrapper here uses the
Transformers backend because it is simpler for repeatable offline corpus runs.

## FluidAudio CoreML Parakeet

This is the macOS Apple Silicon benchmark path for
`FluidInference/parakeet-tdt-0.6b-v3-coreml`.

```bash
git clone https://github.com/FluidInference/FluidAudio.git
export FLUIDAUDIO_PACKAGE_PATH=/path/to/FluidAudio
node scripts/asr-candidates/fluidaudio-coreml.js --preflight
node scripts/asr-candidates/fluidaudio-coreml.js samples/example.wav
```

The wrapper invokes `swift run fluidaudiocli transcribe`. It requires macOS,
Swift, and FluidAudio's CoreML model download/cache path. Keep this as a
benchmark adapter until a native dybur runtime path has matching quality,
latency, install, and signing behavior.

## Moonshine Streaming Tiny

Moonshine Streaming support currently requires a recent Transformers build.

```bash
python -m venv .venv-moonshine
. .venv-moonshine/bin/activate
pip install -U git+https://github.com/huggingface/transformers.git datasets[audio] torch
python scripts/asr-candidates/moonshine-transformers.py --preflight
python scripts/asr-candidates/moonshine-transformers.py samples/example.wav
```

On Windows PowerShell, activate with:

```powershell
.venv-moonshine\Scripts\Activate.ps1
```

## Corpus Run

Copy `benchmarks/asr/candidate-commands.example.json` to a local file, enable
the wrappers you have installed, check setup, then run:

```bash
pnpm eval:asr:manifest benchmarks/asr/<run>.json \
  --require-audio \
  --require-duration \
  --require-tags
```

```bash
pnpm eval:asr:candidates --commands benchmarks/asr/candidate-commands.local.json --preflight
```

Preflight mode runs enabled commands' optional `checkCommand` values without
audio. Disabled commands print their setup reason instead of failing.

Command entries may include both `command` and `batchCommand`. The per-sample
`command` path is useful for simple wrapper smoke tests. The `batchCommand` path
is preferred for promotion readiness because it loads the model once for the
manifest and reports warmed per-sample `latencyMs` values through the generated
batch JSON output.

```bash
pnpm eval:asr:candidates benchmarks/asr/<run>.json \
  --commands benchmarks/asr/candidate-commands.local.json \
  --output benchmarks/asr/candidate-runs.json

pnpm eval:asr benchmarks/asr/candidate-runs.json \
  --output benchmarks/asr/candidate-report.md \
  --strict

pnpm eval:asr benchmarks/asr/candidate-runs.json \
  --format json \
  --output benchmarks/asr/candidate-report.json \
  --strict

pnpm eval:asr:gate benchmarks/asr/candidate-report.json \
  --config benchmarks/asr/gates/candidate-promotion.example.json
```

For promotion-readiness checks, prefer the orchestration script. It runs the
command-file readiness check, manifest policy check, command preflight,
candidate run, strict JSON scoring, and optional gate in sequence, then writes a
`readiness-summary.json` next to the generated ASR outputs:

```bash
pnpm eval:asr:runtime-ready benchmarks/asr/<run>.json \
  --commands benchmarks/asr/candidate-commands.local.json \
  --model nemotron-35-asr-streaming-onnx-int4 \
  --output-dir benchmarks/asr/runtime-readiness/nemotron35
```

Use `--dry-run` to inspect the plan without loading models. For regression
checks, use `--baseline <model>` with one or more `--candidate <model>` flags
and a gate config so preflight, corpus execution, and gating all use the same
explicit model pair:

```bash
pnpm eval:asr:runtime-ready benchmarks/asr/<run>.json \
  --commands benchmarks/asr/candidate-commands.local.json \
  --baseline parakeet-tdt-v3-int8 \
  --candidate nemotron-35-asr-streaming-onnx-int4 \
  --gate-config benchmarks/asr/gates/candidate-promotion.example.json \
  --output-dir benchmarks/asr/runtime-readiness/promotion
```

Non-dry readiness runs require every selected command to be enabled in a local
command file; the checked-in example commands stay disabled until the matching
benchmark runtime is installed.

## Wrapper Checks

```bash
pnpm test:scripts
```

This covers the candidate runner's dry-run output, preflight behavior,
disabled-command handling, JSON transcript extraction, generated ASR run
manifests, ASR manifest validation, strict ASR scoring validation, ASR gate
checks, runtime-readiness orchestration, and CLI candidate catalog smoke checks.
