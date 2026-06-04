#!/usr/bin/env node

/**
 * Check ASR evaluation JSON reports against quality and regression thresholds.
 *
 * This is intended for candidate promotion decisions after `scripts/asr-eval.js`
 * has produced a strict JSON report.
 */

import fs from 'node:fs';
import path from 'node:path';

function usage() {
  console.log(`Usage: node scripts/asr-gate.js <report.json> [options]

Options:
  --config <file>                 JSON gate config. CLI flags override config values
  --baseline <model>              Baseline model for regression checks
  --candidate <model>             Candidate model to check; repeatable. Defaults to all non-baseline models
  --max-wer <rate>                Maximum allowed WER, e.g. 0.08 for 8%
  --max-cer <rate>                Maximum allowed CER, e.g. 0.03 for 3%
  --max-rtf <factor>              Maximum allowed median realtime factor
  --max-latency-ms <ms>           Maximum allowed median latency
  --max-wer-regression <rate>     Maximum WER increase versus baseline
  --max-cer-regression <rate>     Maximum CER increase versus baseline
  --max-rtf-regression <factor>   Maximum realtime factor increase versus baseline
  --max-latency-regression-ms <ms> Maximum latency increase versus baseline
  --skip-tags                     Do not apply thresholds to tag summaries
`);
}

function parseNumber(value, optionName) {
  const number = Number(value);
  if (!Number.isFinite(number) || number < 0) {
    throw new Error(`${optionName} must be a non-negative number`);
  }
  return number;
}

function parseArgs(argv) {
  const args = [...argv];
  const reportPath = args.shift();
  const options = {
    configPath: null,
    baseline: null,
    candidates: [],
    maxWer: null,
    maxCer: null,
    maxRtf: null,
    maxLatencyMs: null,
    maxWerRegression: null,
    maxCerRegression: null,
    maxRtfRegression: null,
    maxLatencyRegressionMs: null,
    checkTags: true,
  };

  while (args.length > 0) {
    const arg = args.shift();
    switch (arg) {
      case '--config':
        options.configPath = args.shift() ?? null;
        break;
      case '--baseline':
        options.baseline = args.shift() ?? null;
        break;
      case '--candidate':
        options.candidates.push(args.shift() ?? '');
        break;
      case '--max-wer':
        options.maxWer = parseNumber(args.shift(), '--max-wer');
        break;
      case '--max-cer':
        options.maxCer = parseNumber(args.shift(), '--max-cer');
        break;
      case '--max-rtf':
        options.maxRtf = parseNumber(args.shift(), '--max-rtf');
        break;
      case '--max-latency-ms':
        options.maxLatencyMs = parseNumber(args.shift(), '--max-latency-ms');
        break;
      case '--max-wer-regression':
        options.maxWerRegression = parseNumber(args.shift(), '--max-wer-regression');
        break;
      case '--max-cer-regression':
        options.maxCerRegression = parseNumber(args.shift(), '--max-cer-regression');
        break;
      case '--max-rtf-regression':
        options.maxRtfRegression = parseNumber(args.shift(), '--max-rtf-regression');
        break;
      case '--max-latency-regression-ms':
        options.maxLatencyRegressionMs = parseNumber(args.shift(), '--max-latency-regression-ms');
        break;
      case '--skip-tags':
        options.checkTags = false;
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

  if (!reportPath) {
    usage();
    process.exit(1);
  }

  options.candidates = options.candidates.filter(Boolean);
  return { reportPath, options: mergeConfigOptions(options) };
}

function readGateConfig(configPath) {
  const resolvedConfig = path.resolve(configPath);
  const config = JSON.parse(fs.readFileSync(resolvedConfig, 'utf8'));

  if (config == null || typeof config !== 'object' || Array.isArray(config)) {
    throw new Error('Gate config must be a JSON object');
  }

  return config;
}

function numberFromConfig(config, key) {
  if (config[key] == null) {
    return null;
  }
  return parseNumber(config[key], key);
}

function mergeConfigOptions(cliOptions) {
  if (!cliOptions.configPath) {
    return cliOptions;
  }

  const config = readGateConfig(cliOptions.configPath);
  const configOptions = {
    configPath: cliOptions.configPath,
    baseline: typeof config.baseline === 'string' ? config.baseline : null,
    candidates: Array.isArray(config.candidates)
      ? config.candidates.filter((candidate) => typeof candidate === 'string' && candidate)
      : [],
    maxWer: numberFromConfig(config, 'maxWer'),
    maxCer: numberFromConfig(config, 'maxCer'),
    maxRtf: numberFromConfig(config, 'maxRtf'),
    maxLatencyMs: numberFromConfig(config, 'maxLatencyMs'),
    maxWerRegression: numberFromConfig(config, 'maxWerRegression'),
    maxCerRegression: numberFromConfig(config, 'maxCerRegression'),
    maxRtfRegression: numberFromConfig(config, 'maxRtfRegression'),
    maxLatencyRegressionMs: numberFromConfig(config, 'maxLatencyRegressionMs'),
    checkTags: typeof config.checkTags === 'boolean' ? config.checkTags : true,
  };

  return {
    configPath: cliOptions.configPath,
    baseline: cliOptions.baseline ?? configOptions.baseline,
    candidates: cliOptions.candidates.length > 0 ? cliOptions.candidates : configOptions.candidates,
    maxWer: cliOptions.maxWer ?? configOptions.maxWer,
    maxCer: cliOptions.maxCer ?? configOptions.maxCer,
    maxRtf: cliOptions.maxRtf ?? configOptions.maxRtf,
    maxLatencyMs: cliOptions.maxLatencyMs ?? configOptions.maxLatencyMs,
    maxWerRegression: cliOptions.maxWerRegression ?? configOptions.maxWerRegression,
    maxCerRegression: cliOptions.maxCerRegression ?? configOptions.maxCerRegression,
    maxRtfRegression: cliOptions.maxRtfRegression ?? configOptions.maxRtfRegression,
    maxLatencyRegressionMs:
      cliOptions.maxLatencyRegressionMs ?? configOptions.maxLatencyRegressionMs,
    checkTags: cliOptions.checkTags && configOptions.checkTags,
  };
}

function readReport(reportPath) {
  const report = JSON.parse(fs.readFileSync(path.resolve(reportPath), 'utf8'));

  if (!Array.isArray(report.models)) {
    throw new Error('Report must include models[]');
  }
  if (report.tagSummaries != null && !Array.isArray(report.tagSummaries)) {
    throw new Error('Report tagSummaries must be an array when present');
  }

  return report;
}

function formatPercent(value) {
  return `${(value * 100).toFixed(1)}%`;
}

function formatMetric(value, kind) {
  if (value == null) return '-';
  if (kind === 'wer' || kind === 'cer') return formatPercent(value);
  if (kind === 'latency') return `${Math.round(value)}ms`;
  return `${value.toFixed(2)}x`;
}

function compareMaximum({ failures, label, metric, metricKind, threshold }) {
  if (threshold == null || metric == null) return;
  if (metric > threshold) {
    failures.push(
      `${label} ${metricKind} ${formatMetric(metric, metricKind)} exceeds ${formatMetric(
        threshold,
        metricKind
      )}`
    );
  }
}

function compareRegression({
  baseline,
  candidate,
  failures,
  label,
  metricName,
  metricKind,
  threshold,
}) {
  if (threshold == null || baseline?.[metricName] == null || candidate?.[metricName] == null) {
    return;
  }

  const delta = candidate[metricName] - baseline[metricName];
  if (delta > threshold) {
    failures.push(
      `${label} ${metricKind} regression ${formatMetric(delta, metricKind)} exceeds ${formatMetric(
        threshold,
        metricKind
      )} versus baseline`
    );
  }
}

function checkSummary({ baseline, failures, options, summary, label }) {
  compareMaximum({
    failures,
    label,
    metric: summary.wer,
    metricKind: 'wer',
    threshold: options.maxWer,
  });
  compareMaximum({
    failures,
    label,
    metric: summary.cer,
    metricKind: 'cer',
    threshold: options.maxCer,
  });
  compareMaximum({
    failures,
    label,
    metric: summary.medianRealtimeFactor,
    metricKind: 'rtf',
    threshold: options.maxRtf,
  });
  compareMaximum({
    failures,
    label,
    metric: summary.medianLatencyMs,
    metricKind: 'latency',
    threshold: options.maxLatencyMs,
  });

  compareRegression({
    baseline,
    candidate: summary,
    failures,
    label,
    metricName: 'wer',
    metricKind: 'wer',
    threshold: options.maxWerRegression,
  });
  compareRegression({
    baseline,
    candidate: summary,
    failures,
    label,
    metricName: 'cer',
    metricKind: 'cer',
    threshold: options.maxCerRegression,
  });
  compareRegression({
    baseline,
    candidate: summary,
    failures,
    label,
    metricName: 'medianRealtimeFactor',
    metricKind: 'rtf',
    threshold: options.maxRtfRegression,
  });
  compareRegression({
    baseline,
    candidate: summary,
    failures,
    label,
    metricName: 'medianLatencyMs',
    metricKind: 'latency',
    threshold: options.maxLatencyRegressionMs,
  });
}

function getCandidateModels(report, options) {
  const modelIds = report.models.map((model) => model.model);

  if (options.candidates.length > 0) {
    return options.candidates;
  }

  return modelIds.filter((model) => model !== options.baseline);
}

function checkReport(report, options) {
  const failures = [];
  const byModel = new Map(report.models.map((model) => [model.model, model]));
  const baseline = options.baseline ? byModel.get(options.baseline) : null;

  if (options.baseline && !baseline) {
    failures.push(`Baseline model not found: ${options.baseline}`);
  }

  const candidateModels = getCandidateModels(report, options);
  for (const candidateModel of candidateModels) {
    const summary = byModel.get(candidateModel);
    if (!summary) {
      failures.push(`Candidate model not found: ${candidateModel}`);
      continue;
    }

    checkSummary({
      baseline,
      failures,
      options,
      summary,
      label: `model ${candidateModel}`,
    });
  }

  if (options.checkTags && Array.isArray(report.tagSummaries)) {
    const byTagAndModel = new Map(
      report.tagSummaries.map((summary) => [`${summary.tag}\u0000${summary.model}`, summary])
    );
    const tags = new Set(report.tagSummaries.map((summary) => summary.tag));

    for (const tag of tags) {
      const tagBaseline = options.baseline
        ? byTagAndModel.get(`${tag}\u0000${options.baseline}`)
        : null;

      for (const candidateModel of candidateModels) {
        const summary = byTagAndModel.get(`${tag}\u0000${candidateModel}`);
        if (!summary) {
          continue;
        }

        checkSummary({
          baseline: tagBaseline,
          failures,
          options,
          summary,
          label: `tag ${tag} model ${candidateModel}`,
        });
      }
    }
  }

  return { candidateModels, failures };
}

function main() {
  const { reportPath, options } = parseArgs(process.argv.slice(2));
  const report = readReport(reportPath);
  const { candidateModels, failures } = checkReport(report, options);

  if (failures.length > 0) {
    console.error(`ASR gate failed for ${candidateModels.length} candidate model(s):`);
    for (const failure of failures) {
      console.error(`- ${failure}`);
    }
    process.exit(1);
  }

  console.log(`ASR gate passed for ${candidateModels.length} candidate model(s).`);
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
