# ASR Candidate Wrappers

These scripts are benchmark helpers for models that are not production dybur
model IDs yet. Each wrapper prints a transcript to stdout so
`scripts/asr-candidate-runner.js` can turn the result into an
`scripts/asr-eval.js` compatible run manifest.

Use isolated Python environments. Do not install these dependencies into the app
workspace unless you intentionally want local benchmark tooling there.

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

## Wrapper Checks

```bash
pnpm test:scripts
```

This covers the candidate runner's dry-run output, preflight behavior,
disabled-command handling, JSON transcript extraction, generated ASR run
manifests, ASR manifest validation, strict ASR scoring validation, ASR gate
checks, and CLI candidate catalog smoke checks.
