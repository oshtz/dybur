#!/usr/bin/env node

/**
 * Score local ASR evaluation runs.
 *
 * The harness intentionally scores saved hypotheses instead of invoking dybur
 * directly. That keeps it usable for real microphone/app smoke runs while the
 * desktop transcription engine remains embedded in the Tauri binary.
 */

import fs from 'node:fs';
import path from 'node:path';

function usage() {
  console.log(`Usage: node scripts/asr-eval.js <manifest.json> [--format markdown|json] [--output file] [--strict]

Manifest shape:
{
  "samples": [
    { "id": "short-email", "reference": "Please send the notes.", "durationMs": 3200 }
  ],
  "runs": [
    { "model": "parakeet-tdt-v3-int8", "sampleId": "short-email", "hypothesis": "please send the notes", "latencyMs": 840 }
  ]
}`);
}

function parseArgs(argv) {
  const args = [...argv];
  const manifestPath = args.shift();
  const options = {
    format: 'markdown',
    output: null,
    strict: false,
  };

  while (args.length > 0) {
    const arg = args.shift();
    switch (arg) {
      case '--format':
        options.format = args.shift() || '';
        break;
      case '--output':
        options.output = args.shift() || '';
        break;
      case '--strict':
        options.strict = true;
        break;
      case '--help':
      case '-h':
        usage();
        process.exit(0);
        break;
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }
  }

  if (!manifestPath) {
    usage();
    process.exit(1);
  }

  if (!['markdown', 'json'].includes(options.format)) {
    throw new Error('--format must be markdown or json');
  }

  return { manifestPath, options };
}

function readManifest(manifestPath) {
  const resolvedPath = path.resolve(manifestPath);
  const manifest = JSON.parse(fs.readFileSync(resolvedPath, 'utf8'));

  if (!Array.isArray(manifest.samples) || !Array.isArray(manifest.runs)) {
    throw new Error('Manifest must include samples[] and runs[] arrays');
  }

  return { manifest, resolvedPath };
}

function validateManifest(manifest, options = {}) {
  const sampleIds = new Set();
  for (const sample of manifest.samples) {
    if (!sample.id || typeof sample.reference !== 'string') {
      throw new Error('Each sample must include id and reference');
    }
    if (sample.tags !== undefined) {
      if (!Array.isArray(sample.tags)) {
        throw new Error(`Sample ${sample.id} tags must be an array`);
      }
      for (const tag of sample.tags) {
        if (typeof tag !== 'string' || tag.trim().length === 0) {
          throw new Error(`Sample ${sample.id} tags must be non-empty strings`);
        }
      }
    }
    if (sampleIds.has(sample.id)) {
      throw new Error(`Duplicate sample id: ${sample.id}`);
    }
    sampleIds.add(sample.id);
  }

  const runKeys = new Set();
  const modelSamples = new Map();
  for (const run of manifest.runs) {
    if (!run.model || !run.sampleId || typeof run.hypothesis !== 'string') {
      throw new Error('Each run must include model, sampleId, and hypothesis');
    }
    if (!sampleIds.has(run.sampleId)) {
      throw new Error(`Run references unknown sample: ${run.sampleId}`);
    }

    const key = `${run.model}\u0000${run.sampleId}`;
    if (runKeys.has(key)) {
      throw new Error(`Duplicate run for model/sample: ${run.model}/${run.sampleId}`);
    }
    runKeys.add(key);

    if (!modelSamples.has(run.model)) {
      modelSamples.set(run.model, new Set());
    }
    modelSamples.get(run.model).add(run.sampleId);
  }

  if (options.strict) {
    for (const [model, modelSampleIds] of modelSamples.entries()) {
      const missing = [...sampleIds].filter((sampleId) => !modelSampleIds.has(sampleId));
      if (missing.length > 0) {
        throw new Error(`Model ${model} is missing sample(s): ${missing.join(', ')}`);
      }
    }
  }
}

function normalizeText(text) {
  return String(text)
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s']/gu, ' ')
    .replace(/\s+/g, ' ')
    .trim();
}

function words(text) {
  const normalized = normalizeText(text);
  return normalized ? normalized.split(' ') : [];
}

function chars(text) {
  return Array.from(normalizeText(text).replace(/\s+/g, ''));
}

function editDistance(left, right) {
  const previous = Array.from({ length: right.length + 1 }, (_, index) => index);
  const current = Array(right.length + 1).fill(0);

  for (let i = 1; i <= left.length; i += 1) {
    current[0] = i;
    for (let j = 1; j <= right.length; j += 1) {
      const substitutionCost = left[i - 1] === right[j - 1] ? 0 : 1;
      current[j] = Math.min(
        previous[j] + 1,
        current[j - 1] + 1,
        previous[j - 1] + substitutionCost
      );
    }
    for (let j = 0; j < current.length; j += 1) {
      previous[j] = current[j];
    }
  }

  return previous[right.length];
}

function rate(referenceUnits, hypothesisUnits) {
  if (referenceUnits.length === 0) {
    return hypothesisUnits.length === 0 ? 0 : 1;
  }

  return editDistance(referenceUnits, hypothesisUnits) / referenceUnits.length;
}

function mean(values) {
  if (values.length === 0) return null;
  return values.reduce((sum, value) => sum + value, 0) / values.length;
}

function median(values) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const midpoint = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0 ? (sorted[midpoint - 1] + sorted[midpoint]) / 2 : sorted[midpoint];
}

function percent(value) {
  return value == null ? '-' : `${(value * 100).toFixed(1)}%`;
}

function milliseconds(value) {
  return value == null ? '-' : `${Math.round(value)}ms`;
}

function metadataValue(value) {
  return value == null || value === '' ? '-' : String(value);
}

function sampleTags(sample) {
  if (!Array.isArray(sample.tags)) {
    return [];
  }
  return [...new Set(sample.tags.map((tag) => tag.trim()))].sort();
}

function summarizeRuns(runs) {
  return {
    samples: runs.length,
    wer: mean(runs.map((run) => run.wer)),
    cer: mean(runs.map((run) => run.cer)),
    medianLatencyMs: median(runs.map((run) => run.latencyMs).filter((value) => value != null)),
    medianRealtimeFactor: median(
      runs.map((run) => run.realtimeFactor).filter((value) => value != null)
    ),
  };
}

function scoreManifest(manifest) {
  const samples = new Map(manifest.samples.map((sample) => [sample.id, sample]));
  const scoredRuns = [];

  for (const run of manifest.runs) {
    const sample = samples.get(run.sampleId);

    const referenceWords = words(sample.reference);
    const hypothesisWords = words(run.hypothesis);
    const referenceChars = chars(sample.reference);
    const hypothesisChars = chars(run.hypothesis);
    const latencyMs = Number.isFinite(run.latencyMs) ? run.latencyMs : null;
    const durationMs = Number.isFinite(sample.durationMs) ? sample.durationMs : null;

    scoredRuns.push({
      model: run.model,
      sampleId: run.sampleId,
      wer: rate(referenceWords, hypothesisWords),
      cer: rate(referenceChars, hypothesisChars),
      latencyMs,
      durationMs,
      realtimeFactor: latencyMs != null && durationMs ? latencyMs / durationMs : null,
      reference: sample.reference,
      hypothesis: run.hypothesis,
      tags: sampleTags(sample),
    });
  }

  const byModel = new Map();
  const byTagAndModel = new Map();
  for (const run of scoredRuns) {
    if (!byModel.has(run.model)) {
      byModel.set(run.model, []);
    }
    byModel.get(run.model).push(run);

    for (const tag of run.tags) {
      const key = `${tag}\u0000${run.model}`;
      if (!byTagAndModel.has(key)) {
        byTagAndModel.set(key, { tag, model: run.model, runs: [] });
      }
      byTagAndModel.get(key).runs.push(run);
    }
  }

  const models = [...byModel.entries()].map(([model, runs]) => ({
    model,
    ...summarizeRuns(runs),
  }));

  const tagSummaries = [...byTagAndModel.values()]
    .map(({ tag, model, runs }) => ({
      tag,
      model,
      ...summarizeRuns(runs),
    }))
    .sort(
      (left, right) => left.tag.localeCompare(right.tag) || left.model.localeCompare(right.model)
    );

  return {
    generatedAt: new Date().toISOString(),
    sampleCount: samples.size,
    runCount: scoredRuns.length,
    sourceMetadata: manifest.metadata ?? null,
    models,
    tagSummaries,
    runs: scoredRuns,
  };
}

function renderMarkdown(report, sourcePath) {
  const lines = [
    '# ASR Evaluation Report',
    '',
    `Source: \`${sourcePath}\``,
    `Generated: ${report.generatedAt}`,
    '',
    '## Summary',
    '',
    '| Model | Samples | WER | CER | Median latency | Median realtime |',
    '| --- | ---: | ---: | ---: | ---: | ---: |',
  ];

  for (const model of report.models) {
    lines.push(
      `| ${model.model} | ${model.samples} | ${percent(model.wer)} | ${percent(model.cer)} | ${milliseconds(model.medianLatencyMs)} | ${
        model.medianRealtimeFactor == null ? '-' : `${model.medianRealtimeFactor.toFixed(2)}x`
      } |`
    );
  }

  if (report.sourceMetadata) {
    lines.push('', '## Source Metadata', '');
    lines.push('| Field | Value |');
    lines.push('| --- | --- |');

    const metadataFields = [
      ['Runner', report.sourceMetadata.runner],
      ['Run generated', report.sourceMetadata.generatedAt],
      ['Git head', report.sourceMetadata.gitHead],
      ['Platform', report.sourceMetadata.platform],
      ['Architecture', report.sourceMetadata.arch],
      ['Node', report.sourceMetadata.nodeVersion],
      ['Selected model', report.sourceMetadata.selectedModel],
      ['Timeout', report.sourceMetadata.timeoutMs],
      ['Commands', report.sourceMetadata.commandCount],
      ['Manifest', report.sourceMetadata.manifestPath],
      ['Command file', report.sourceMetadata.commandsPath],
    ];

    for (const [field, value] of metadataFields) {
      lines.push(`| ${field} | ${metadataValue(value).replace(/\|/g, '\\|')} |`);
    }
  }

  if (report.tagSummaries.length > 0) {
    lines.push('', '## Tag Summary', '');
    lines.push('| Tag | Model | Samples | WER | CER | Median latency | Median realtime |');
    lines.push('| --- | --- | ---: | ---: | ---: | ---: | ---: |');

    for (const tag of report.tagSummaries) {
      lines.push(
        `| ${tag.tag} | ${tag.model} | ${tag.samples} | ${percent(tag.wer)} | ${percent(tag.cer)} | ${milliseconds(tag.medianLatencyMs)} | ${
          tag.medianRealtimeFactor == null ? '-' : `${tag.medianRealtimeFactor.toFixed(2)}x`
        } |`
      );
    }
  }

  lines.push('', '## Runs', '');
  lines.push('| Model | Sample | Tags | WER | CER | Latency | Hypothesis |');
  lines.push('| --- | --- | --- | ---: | ---: | ---: | --- |');

  for (const run of report.runs) {
    lines.push(
      `| ${run.model} | ${run.sampleId} | ${run.tags.join(', ') || '-'} | ${percent(run.wer)} | ${percent(run.cer)} | ${milliseconds(run.latencyMs)} | ${run.hypothesis.replace(/\|/g, '\\|')} |`
    );
  }

  return `${lines.join('\n')}\n`;
}

function main() {
  const { manifestPath, options } = parseArgs(process.argv.slice(2));
  const { manifest, resolvedPath } = readManifest(manifestPath);
  validateManifest(manifest, { strict: options.strict });
  const report = scoreManifest(manifest);
  const output =
    options.format === 'json'
      ? `${JSON.stringify(report, null, 2)}\n`
      : renderMarkdown(report, resolvedPath);

  if (options.output) {
    fs.writeFileSync(path.resolve(options.output), output);
  } else {
    process.stdout.write(output);
  }
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
