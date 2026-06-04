# ASR Evaluation Harness

The current harness scores recorded evaluation runs. It does not invoke the Tauri transcription engine directly because dybur's STT runtime is still embedded in the desktop app.

## Manifest

Create a JSON manifest with:

- `samples`: reference utterances and optional audio metadata.
- `runs`: one hypothesis per model/sample pair, plus latency in milliseconds when measured.

See [benchmarks/asr/example.json](../benchmarks/asr/example.json).

Add `tags` to each sample for slices that matter to product decisions, such as
`english`, `hebrew`, `noisy`, `quiet`, `domain`, `short`, `long`, or `punctuation`.

## Run

```bash
node scripts/asr-eval.js benchmarks/asr/example.json
node scripts/asr-eval.js benchmarks/asr/example.json --format json
node scripts/asr-eval.js benchmarks/asr/example.json --output benchmarks/asr/report.md
node scripts/asr-eval.js benchmarks/asr/example.json --strict
node scripts/asr-manifest-check.js benchmarks/asr/example.json --require-duration --require-tags
node scripts/asr-manifest-check.js benchmarks/asr/<run>.json --config benchmarks/asr/corpus-policy.example.json
node scripts/asr-eval.js benchmarks/asr/example.json --format json --output benchmarks/asr/report.json --strict
node scripts/asr-gate.js benchmarks/asr/candidate-report.json --config benchmarks/asr/gates/candidate-promotion.example.json
```

To run experimental external model candidates such as CoreML/MLX/Qwen/Moonshine
wrappers, use [model-candidate-evaluation.md](./model-candidate-evaluation.md).

## Recommended Sample Set

Capture short, repeatable clips before comparing models:

- Short email sentence.
- Long note with natural pauses.
- Punctuation-heavy task list.
- Quiet speech.
- Noisy room.
- Domain-specific vocabulary.
- Non-English sentence for each language you care about.

Keep the raw audio out of git unless it is intentionally public and license-safe. Store only the manifest, references, hypotheses, and aggregate report in the repository.

## Metrics

- WER: word error rate after lowercase/punctuation normalization.
- CER: character error rate after lowercase/punctuation/space normalization.
- Median latency: median reported inference latency per model.
- Median realtime factor: latency divided by sample duration.
- Tag summary: the same metrics grouped by sample tag, useful for language,
  noise, and domain regressions that an overall average can hide.
- Source metadata: candidate-run reports preserve runner, platform, git head,
  command file, timeout, and command count when that metadata is present in the
  manifest.

Lower WER/CER and lower realtime factor are better.

Use `--strict` for release or candidate-model comparisons. Strict mode rejects
duplicate sample ids, duplicate model/sample runs, and model runs that do not
cover every sample in the manifest.

Use `scripts/asr-manifest-check.js` before expensive model runs. It validates
sample ids, references, audio paths, durations, and tag coverage; add
`--require-audio` once the local audio files have been recorded. For real
candidate comparisons, use
`--config benchmarks/asr/corpus-policy.example.json` so sample count, required
tags, per-tag coverage, audio, duration, and tag requirements come from one
reviewable policy file.

Use `scripts/asr-gate.js` on a strict JSON report when a run needs an explicit
pass/fail decision. It can enforce absolute WER/CER/latency/realtime thresholds
and regression thresholds versus a baseline model, including per-tag summaries.
Use `--config benchmarks/asr/gates/candidate-promotion.example.json` for the
checked-in starting policy, and pass CLI threshold flags only for one-off
experiments.
