# ASR Evaluation Harness

The current harness scores recorded evaluation runs. It does not invoke the Tauri transcription engine directly because dybur's STT runtime is still embedded in the desktop app.

## Manifest

Create a JSON manifest with:

- `samples`: reference utterances and optional audio metadata.
- `runs`: one hypothesis per model/sample pair, plus latency in milliseconds when measured.

See [benchmarks/asr/example.json](../benchmarks/asr/example.json).

## Run

```bash
node scripts/asr-eval.js benchmarks/asr/example.json
node scripts/asr-eval.js benchmarks/asr/example.json --format json
node scripts/asr-eval.js benchmarks/asr/example.json --output benchmarks/asr/report.md
```

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

Lower WER/CER and lower realtime factor are better.
