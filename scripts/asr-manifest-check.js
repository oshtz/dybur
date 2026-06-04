#!/usr/bin/env node

/**
 * Validate ASR corpus manifests before expensive model benchmark runs.
 */

import fs from 'node:fs';
import path from 'node:path';

function usage() {
  console.log(`Usage: node scripts/asr-manifest-check.js <manifest.json> [options]

Options:
  --config <file>          Load reusable corpus policy JSON
  --require-audio          Require every sample audio path to exist on disk
  --require-duration       Require every sample to include positive durationMs
  --require-tags           Require every sample to include at least one tag
  --required-tag <tag>     Require corpus coverage for a tag; repeatable
  --min-samples <n>        Require at least n samples
  --min-samples-per-tag <n>
                           Require at least n samples for every required tag
  --json                   Print machine-readable summary
`);
}

const DEFAULT_OPTIONS = {
  requireAudio: false,
  requireDuration: false,
  requireTags: false,
  requiredTags: [],
  minSamples: 1,
  minSamplesPerTag: null,
  json: false,
};

function parsePositiveInteger(value, optionName) {
  const number = Number(value);
  if (!Number.isInteger(number) || number <= 0) {
    throw new Error(`${optionName} must be a positive integer`);
  }
  return number;
}

function requireOptionValue(args, optionName) {
  const value = args.shift();
  if (value == null || value.startsWith('--')) {
    throw new Error(`${optionName} requires a value`);
  }
  return value;
}

function normalizeRequiredTags(value, sourceName) {
  if (value == null) {
    return [];
  }
  if (!Array.isArray(value)) {
    throw new Error(`${sourceName} requiredTags must be an array`);
  }

  return [
    ...new Set(
      value.map((tag) => {
        if (typeof tag !== 'string' || tag.trim().length === 0) {
          throw new Error(`${sourceName} requiredTags must contain non-empty strings`);
        }
        return tag.trim();
      })
    ),
  ].sort();
}

function parseBooleanConfig(value, fieldName, sourceName) {
  if (value == null) {
    return undefined;
  }
  if (typeof value !== 'boolean') {
    throw new Error(`${sourceName} ${fieldName} must be a boolean`);
  }
  return value;
}

function parsePositiveIntegerConfig(value, fieldName, sourceName) {
  if (value == null) {
    return undefined;
  }
  return parsePositiveInteger(value, `${sourceName} ${fieldName}`);
}

function readConfig(configPath) {
  const resolvedPath = path.resolve(configPath);
  const sourceName = `Config ${resolvedPath}`;
  const config = JSON.parse(fs.readFileSync(resolvedPath, 'utf8'));

  if (config == null || typeof config !== 'object' || Array.isArray(config)) {
    throw new Error(`${sourceName} must be a JSON object`);
  }

  const allowedFields = new Set([
    'requireAudio',
    'requireDuration',
    'requireTags',
    'requiredTags',
    'minSamples',
    'minSamplesPerTag',
  ]);
  for (const field of Object.keys(config)) {
    if (!allowedFields.has(field)) {
      throw new Error(`${sourceName} has unknown field: ${field}`);
    }
  }

  const options = {};
  for (const fieldName of ['requireAudio', 'requireDuration', 'requireTags']) {
    const value = parseBooleanConfig(config[fieldName], fieldName, sourceName);
    if (value != null) {
      options[fieldName] = value;
    }
  }

  if (config.requiredTags != null) {
    options.requiredTags = normalizeRequiredTags(config.requiredTags, sourceName);
  }

  const minSamples = parsePositiveIntegerConfig(config.minSamples, 'minSamples', sourceName);
  if (minSamples != null) {
    options.minSamples = minSamples;
  }

  const minSamplesPerTag = parsePositiveIntegerConfig(
    config.minSamplesPerTag,
    'minSamplesPerTag',
    sourceName
  );
  if (minSamplesPerTag != null) {
    options.minSamplesPerTag = minSamplesPerTag;
  }

  return { options, resolvedPath };
}

function parseArgs(argv) {
  const args = [...argv];
  const manifestPath = args.shift();
  const cliOptions = { requiredTags: [] };
  const cliOverrides = new Set();
  let configPath = null;

  while (args.length > 0) {
    const arg = args.shift();
    switch (arg) {
      case '--config':
        configPath = requireOptionValue(args, '--config');
        break;
      case '--require-audio':
        cliOptions.requireAudio = true;
        cliOverrides.add('requireAudio');
        break;
      case '--require-duration':
        cliOptions.requireDuration = true;
        cliOverrides.add('requireDuration');
        break;
      case '--require-tags':
        cliOptions.requireTags = true;
        cliOverrides.add('requireTags');
        break;
      case '--required-tag':
        cliOptions.requiredTags.push(requireOptionValue(args, '--required-tag'));
        cliOverrides.add('requiredTags');
        break;
      case '--min-samples':
        cliOptions.minSamples = parsePositiveInteger(
          requireOptionValue(args, '--min-samples'),
          '--min-samples'
        );
        cliOverrides.add('minSamples');
        break;
      case '--min-samples-per-tag':
        cliOptions.minSamplesPerTag = parsePositiveInteger(
          requireOptionValue(args, '--min-samples-per-tag'),
          '--min-samples-per-tag'
        );
        cliOverrides.add('minSamplesPerTag');
        break;
      case '--json':
        cliOptions.json = true;
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

  const config = configPath ? readConfig(configPath) : null;
  const options = {
    ...DEFAULT_OPTIONS,
    ...(config?.options ?? {}),
    json: Boolean(cliOptions.json),
  };

  for (const field of cliOverrides) {
    options[field] = cliOptions[field];
  }
  options.requiredTags = normalizeRequiredTags(options.requiredTags, 'Options');

  return { manifestPath, options, configPath: config?.resolvedPath ?? null };
}

function readManifest(manifestPath) {
  const resolvedPath = path.resolve(manifestPath);
  const manifest = JSON.parse(fs.readFileSync(resolvedPath, 'utf8'));

  if (!Array.isArray(manifest.samples)) {
    throw new Error('Manifest must include samples[]');
  }

  return {
    manifest,
    manifestDir: path.dirname(resolvedPath),
    resolvedPath,
  };
}

function normalizeTags(sample) {
  if (sample.tags == null) {
    return [];
  }
  if (!Array.isArray(sample.tags)) {
    throw new Error(`Sample ${sample.id ?? '<unknown>'} tags must be an array`);
  }

  const tags = sample.tags.map((tag) => {
    if (typeof tag !== 'string' || tag.trim().length === 0) {
      throw new Error(`Sample ${sample.id ?? '<unknown>'} tags must be non-empty strings`);
    }
    return tag.trim();
  });

  return [...new Set(tags)].sort();
}

function validateSample({ issues, manifestDir, options, sample, sampleIds, tagCounts }) {
  if (!sample.id || typeof sample.id !== 'string') {
    issues.push('Each sample must include string id');
  } else if (sampleIds.has(sample.id)) {
    issues.push(`Duplicate sample id: ${sample.id}`);
  } else {
    sampleIds.add(sample.id);
  }

  if (typeof sample.reference !== 'string' || sample.reference.trim().length === 0) {
    issues.push(`Sample ${sample.id ?? '<unknown>'} must include non-empty reference`);
  }

  if (
    sample.audio == null ||
    typeof sample.audio !== 'string' ||
    sample.audio.trim().length === 0
  ) {
    issues.push(`Sample ${sample.id ?? '<unknown>'} must include audio`);
  } else if (options.requireAudio) {
    const audioPath = path.resolve(manifestDir, sample.audio);
    if (!fs.existsSync(audioPath)) {
      issues.push(`Audio file not found for sample ${sample.id}: ${audioPath}`);
    }
  }

  if (
    sample.durationMs != null &&
    (!Number.isFinite(sample.durationMs) || sample.durationMs <= 0)
  ) {
    issues.push(`Sample ${sample.id ?? '<unknown>'} durationMs must be positive`);
  }
  if (options.requireDuration && !Number.isFinite(sample.durationMs)) {
    issues.push(`Sample ${sample.id ?? '<unknown>'} must include durationMs`);
  }

  let tags = [];
  try {
    tags = normalizeTags(sample);
  } catch (error) {
    issues.push(error instanceof Error ? error.message : String(error));
  }

  if (options.requireTags && tags.length === 0) {
    issues.push(`Sample ${sample.id ?? '<unknown>'} must include at least one tag`);
  }

  for (const tag of tags) {
    tagCounts.set(tag, (tagCounts.get(tag) ?? 0) + 1);
  }
}

function checkManifest({ manifest, manifestDir, options, resolvedPath }) {
  const issues = [];
  const sampleIds = new Set();
  const tagCounts = new Map();

  if (manifest.samples.length < options.minSamples) {
    issues.push(`Manifest must include at least ${options.minSamples} sample(s)`);
  }

  for (const sample of manifest.samples) {
    validateSample({
      issues,
      manifestDir,
      options,
      sample,
      sampleIds,
      tagCounts,
    });
  }

  for (const tag of options.requiredTags) {
    const samples = tagCounts.get(tag) ?? 0;
    if (samples === 0) {
      issues.push(`Required tag missing from corpus: ${tag}`);
    } else if (options.minSamplesPerTag != null && samples < options.minSamplesPerTag) {
      issues.push(
        `Required tag ${tag} must include at least ${options.minSamplesPerTag} sample(s); found ${samples}`
      );
    }
  }

  const tagSummary = [...tagCounts.entries()]
    .map(([tag, samples]) => ({ tag, samples }))
    .sort((left, right) => left.tag.localeCompare(right.tag));

  return {
    manifestPath: resolvedPath,
    sampleCount: manifest.samples.length,
    tagSummary,
    issues,
  };
}

function renderText(summary) {
  const lines = [
    `ASR manifest: ${summary.manifestPath}`,
    summary.configPath ? `Policy: ${summary.configPath}` : null,
    `Samples: ${summary.sampleCount}`,
    'Tags:',
  ].filter(Boolean);

  if (summary.tagSummary.length === 0) {
    lines.push('  - none');
  } else {
    for (const tag of summary.tagSummary) {
      lines.push(`  - ${tag.tag}: ${tag.samples}`);
    }
  }

  if (summary.issues.length > 0) {
    lines.push('', 'Issues:');
    for (const issue of summary.issues) {
      lines.push(`  - ${issue}`);
    }
  }

  return `${lines.join('\n')}\n`;
}

function main() {
  const { manifestPath, options, configPath } = parseArgs(process.argv.slice(2));
  const input = readManifest(manifestPath);
  const summary = checkManifest({ ...input, options });
  summary.configPath = configPath;

  if (options.json) {
    process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
  } else {
    process.stdout.write(renderText(summary));
  }

  if (summary.issues.length > 0) {
    process.exit(1);
  }
}

try {
  main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
