#!/usr/bin/env node

/**
 * Run external ASR candidate commands and emit an asr-eval compatible manifest.
 *
 * This runner is intentionally generic: experimental CoreML, MLX, Transformers,
 * or vLLM runtimes can be benchmarked without making them production dybur
 * model IDs first.
 */

import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { performance } from 'node:perf_hooks';

function usage() {
  console.log(`Usage: node scripts/asr-candidate-runner.js [manifest.json] --commands <commands.json> --output <runs.json> [options]

Options:
  --model <id>         Run only a candidate model; repeat to run an exact model set
  --dry-run            Print commands without executing them
  --preflight          Run command setup checks without audio samples
  --timeout-ms <ms>    Per-sample timeout (default: 180000)

Input manifest:
  Same samples[] shape used by scripts/asr-eval.js. Each sample must include audio.

Commands file:
{
  "commands": [
    {
      "model": "parakeet-tdt-v3-mlx",
      "command": "parakeet-mlx \\"{audio}\\" --model mlx-community/parakeet-tdt-0.6b-v3",
      "batchCommand": "parakeet-mlx-batch \\"{manifest}\\" --output \\"{output}\\"",
      "checkCommand": "parakeet-mlx --help",
      "disabled": false
    }
  ]
}`);
}

function parseArgs(argv) {
  const args = [...argv];
  let manifestPath = null;
  const options = {
    commandsPath: null,
    outputPath: null,
    models: [],
    dryRun: false,
    preflight: false,
    timeoutMs: 180_000,
  };

  while (args.length > 0) {
    const arg = args.shift();
    switch (arg) {
      case '--commands':
        options.commandsPath = args.shift() ?? null;
        break;
      case '--output':
        options.outputPath = args.shift() ?? null;
        break;
      case '--model':
        options.models.push(args.shift() ?? '');
        break;
      case '--dry-run':
        options.dryRun = true;
        break;
      case '--preflight':
        options.preflight = true;
        break;
      case '--timeout-ms':
        options.timeoutMs = Number(args.shift());
        break;
      case '--help':
      case '-h':
        usage();
        process.exit(0);
        break;
      default:
        if (arg.startsWith('-')) {
          throw new Error(`Unknown argument: ${arg}`);
        }
        if (manifestPath) {
          throw new Error(`Unexpected positional argument: ${arg}`);
        }
        manifestPath = arg;
    }
  }

  if (options.dryRun && options.preflight) {
    throw new Error('--dry-run and --preflight cannot be combined');
  }

  if (!options.commandsPath || (!options.preflight && (!manifestPath || !options.outputPath))) {
    usage();
    process.exit(1);
  }

  if (!Number.isFinite(options.timeoutMs) || options.timeoutMs <= 0) {
    throw new Error('--timeout-ms must be a positive number');
  }

  options.models = [...new Set(options.models.filter(Boolean))];

  return { manifestPath, options };
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(path.resolve(filePath), 'utf8'));
}

function validateManifest(manifest) {
  if (!Array.isArray(manifest.samples)) {
    throw new Error('Manifest must include samples[]');
  }
  for (const sample of manifest.samples) {
    if (!sample.id || !sample.reference || !sample.audio) {
      throw new Error('Each sample must include id, reference, and audio');
    }
  }
}

function validateCommands(commandsFile) {
  if (!Array.isArray(commandsFile.commands)) {
    throw new Error('Commands file must include commands[]');
  }
  for (const command of commandsFile.commands) {
    if (!command.model || (!command.command && !command.batchCommand)) {
      throw new Error('Each command must include model and command or batchCommand');
    }
    if (command.command != null && typeof command.command !== 'string') {
      throw new Error(`command must be a string for model ${command.model}`);
    }
    if (command.batchCommand != null && typeof command.batchCommand !== 'string') {
      throw new Error(`batchCommand must be a string for model ${command.model}`);
    }
    if (command.checkCommand != null && typeof command.checkCommand !== 'string') {
      throw new Error(`checkCommand must be a string for model ${command.model}`);
    }
  }
}

function resolveSampleAudio(sample, manifestDir) {
  return path.resolve(manifestDir, sample.audio);
}

function buildCommand(template, sample, audioPath) {
  return template
    .replaceAll('{audio}', audioPath)
    .replaceAll('{sampleId}', String(sample.id))
    .replaceAll('{reference}', String(sample.reference))
    .replaceAll('{durationMs}', String(sample.durationMs ?? ''));
}

function buildBatchCommand(template, manifestPath, outputPath) {
  return template.replaceAll('{manifest}', manifestPath).replaceAll('{output}', outputPath);
}

function sanitizeFileStem(value) {
  return value.replace(/[^A-Za-z0-9._-]+/g, '_').replace(/^_+|_+$/g, '') || 'asr-model';
}

function buildBatchOutputPath(outputPath, model) {
  const ext = path.extname(outputPath);
  const stem = path.basename(outputPath, ext);
  return path.join(path.dirname(outputPath), `${stem}.${sanitizeFileStem(model)}.batch.json`);
}

function getJsonPathValue(value, jsonPath) {
  return jsonPath.split('.').reduce((current, segment) => {
    if (current == null || typeof current !== 'object') return undefined;
    return current[segment];
  }, value);
}

function extractHypothesis(stdout, commandConfig) {
  const output = stdout.trim();
  if (!commandConfig.outputJsonPath) {
    return output;
  }

  const parsed = JSON.parse(output);
  const value = getJsonPathValue(parsed, commandConfig.outputJsonPath);
  if (typeof value !== 'string') {
    throw new Error(
      `Command ${commandConfig.model} did not produce string JSON path ${commandConfig.outputJsonPath}`
    );
  }
  return value.trim();
}

function runCommand(command, timeoutMs) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, {
      shell: true,
      windowsHide: true,
      stdio: ['ignore', 'pipe', 'pipe'],
    });

    let stdout = '';
    let stderr = '';
    let timedOut = false;

    const timeout = setTimeout(() => {
      timedOut = true;
      child.kill();
    }, timeoutMs);

    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      stdout += chunk;
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk;
    });
    child.on('error', (error) => {
      clearTimeout(timeout);
      reject(error);
    });
    child.on('close', (code) => {
      clearTimeout(timeout);
      if (timedOut) {
        reject(new Error(`Command timed out after ${timeoutMs}ms: ${command}`));
        return;
      }
      if (code !== 0) {
        reject(new Error(`Command failed with exit ${code}: ${stderr.trim() || command}`));
        return;
      }
      resolve({ stdout, stderr });
    });
  });
}

function readGitHead() {
  const result = spawnSync('git', ['rev-parse', 'HEAD'], {
    encoding: 'utf8',
    windowsHide: true,
  });

  if (result.status !== 0) {
    return null;
  }

  return result.stdout.trim() || null;
}

function parseBatchOutput(filePath) {
  const parsed = readJson(filePath);
  if (Array.isArray(parsed)) {
    return parsed;
  }
  if (Array.isArray(parsed.runs)) {
    return parsed.runs;
  }
  throw new Error(`Batch output must be an array or include runs[]: ${filePath}`);
}

function normalizeBatchRuns({
  batchCommand,
  batchOutputPath,
  commandConfig,
  completedAt,
  manifest,
  startedAt,
  stderr,
}) {
  const sampleIds = new Set(manifest.samples.map((sample) => sample.id));
  const seenSampleIds = new Set();
  const batchRuns = parseBatchOutput(batchOutputPath);
  const runs = [];

  for (const run of batchRuns) {
    if (!run || typeof run !== 'object') {
      throw new Error(`Batch command ${commandConfig.model} produced a malformed run`);
    }
    if (!run.sampleId || typeof run.hypothesis !== 'string') {
      throw new Error(
        `Batch command ${commandConfig.model} must produce sampleId and hypothesis for each run`
      );
    }
    if (!sampleIds.has(run.sampleId)) {
      throw new Error(
        `Batch command ${commandConfig.model} produced unknown sample id: ${run.sampleId}`
      );
    }
    if (seenSampleIds.has(run.sampleId)) {
      throw new Error(
        `Batch command ${commandConfig.model} produced duplicate sample id: ${run.sampleId}`
      );
    }
    if (!Number.isFinite(run.latencyMs)) {
      throw new Error(
        `Batch command ${commandConfig.model} must produce numeric latencyMs for ${run.sampleId}`
      );
    }
    seenSampleIds.add(run.sampleId);
    runs.push({
      model: commandConfig.model,
      sampleId: run.sampleId,
      hypothesis: run.hypothesis.trim(),
      latencyMs: run.latencyMs,
      command: batchCommand,
      batchOutputPath,
      startedAt,
      completedAt,
      stderr: stderr.trim() || undefined,
    });
  }

  const missingSampleIds = [...sampleIds].filter((sampleId) => !seenSampleIds.has(sampleId));
  if (missingSampleIds.length > 0) {
    throw new Error(
      `Batch command ${commandConfig.model} did not produce runs for sample(s): ${missingSampleIds.join(
        ', '
      )}`
    );
  }

  return runs;
}

function buildMetadata({
  commands,
  commandsPath,
  manifestPath,
  models,
  outputPath,
  startedAt,
  timeoutMs,
}) {
  return {
    generatedAt: new Date().toISOString(),
    startedAt,
    runner: 'scripts/asr-candidate-runner.js',
    gitHead: readGitHead(),
    cwd: process.cwd(),
    nodeVersion: process.version,
    platform: process.platform,
    arch: process.arch,
    osRelease: os.release(),
    manifestPath,
    commandsPath,
    outputPath,
    selectedModel: models.length === 1 ? models[0] : null,
    selectedModels: models,
    timeoutMs,
    commandCount: commands.length,
    commands: commands.map((command) => ({
      model: command.model,
      batchCommand: command.batchCommand ?? null,
      checkCommand: command.checkCommand ?? null,
      outputJsonPath: command.outputJsonPath ?? null,
      disabled: Boolean(command.disabled),
    })),
  };
}

async function preflightCommands(commands, timeoutMs) {
  let failures = 0;

  for (const commandConfig of commands) {
    if (commandConfig.disabled) {
      const reason = commandConfig.disabledReason ? `: ${commandConfig.disabledReason}` : '';
      console.log(`[disabled] ${commandConfig.model}${reason}`);
      continue;
    }

    if (!commandConfig.checkCommand) {
      console.log(`[unchecked] ${commandConfig.model}: no checkCommand configured`);
      continue;
    }

    try {
      await runCommand(commandConfig.checkCommand, timeoutMs);
      console.log(`[ok] ${commandConfig.model}: ${commandConfig.checkCommand}`);
    } catch (error) {
      failures += 1;
      const message = error instanceof Error ? error.message : String(error);
      console.error(`[failed] ${commandConfig.model}: ${message}`);
    }
  }

  if (failures > 0) {
    throw new Error(`Preflight failed for ${failures} command(s)`);
  }
}

async function main() {
  const startedAt = new Date().toISOString();
  const { manifestPath, options } = parseArgs(process.argv.slice(2));
  const resolvedCommands = path.resolve(options.commandsPath);
  const commandsFile = readJson(resolvedCommands);

  validateCommands(commandsFile);

  const selectedModels = new Set(options.models);
  const matchingCommands = commandsFile.commands.filter(
    (command) => selectedModels.size === 0 || selectedModels.has(command.model)
  );
  if (matchingCommands.length === 0) {
    throw new Error(
      options.models.length === 1
        ? `No command found for model: ${options.models[0]}`
        : options.models.length > 1
          ? `No command found for model(s): ${options.models.join(', ')}`
          : 'No commands found'
    );
  }
  const foundModels = new Set(matchingCommands.map((command) => command.model));
  const missingModels = options.models.filter((model) => !foundModels.has(model));
  if (missingModels.length > 0) {
    throw new Error(
      missingModels.length === 1
        ? `No command found for model: ${missingModels[0]}`
        : `No command found for model(s): ${missingModels.join(', ')}`
    );
  }

  if (options.preflight) {
    await preflightCommands(matchingCommands, options.timeoutMs);
    return;
  }

  const resolvedManifest = path.resolve(manifestPath);
  const manifestDir = path.dirname(resolvedManifest);
  const manifest = readJson(resolvedManifest);

  validateManifest(manifest);

  const commands = options.dryRun
    ? matchingCommands
    : matchingCommands.filter((command) => !command.disabled);
  if (commands.length === 0) {
    const disabledReasons = matchingCommands
      .filter((command) => command.disabled)
      .map(
        (command) =>
          `${command.model}${command.disabledReason ? `: ${command.disabledReason}` : ''}`
      )
      .join('\n');
    throw new Error(`No enabled commands found${disabledReasons ? `\n${disabledReasons}` : ''}`);
  }

  const resolvedOutput = path.resolve(options.outputPath);
  const planned = [];
  const runs = [];

  for (const commandConfig of commands) {
    if (commandConfig.batchCommand) {
      const batchOutputPath = buildBatchOutputPath(resolvedOutput, commandConfig.model);
      const command = buildBatchCommand(commandConfig.batchCommand, resolvedManifest, batchOutputPath);
      planned.push({
        model: commandConfig.model,
        command,
        disabled: Boolean(commandConfig.disabled),
        disabledReason: commandConfig.disabledReason,
        batch: true,
      });

      if (options.dryRun) {
        continue;
      }

      fs.mkdirSync(path.dirname(batchOutputPath), { recursive: true });
      const runStartedAt = new Date().toISOString();
      const { stderr } = await runCommand(command, options.timeoutMs);
      if (!fs.existsSync(batchOutputPath)) {
        throw new Error(`Batch command did not create output file: ${batchOutputPath}`);
      }
      runs.push(
        ...normalizeBatchRuns({
          batchCommand: command,
          batchOutputPath,
          commandConfig,
          completedAt: new Date().toISOString(),
          manifest,
          startedAt: runStartedAt,
          stderr,
        })
      );
      continue;
    }

    for (const sample of manifest.samples) {
      const audioPath = resolveSampleAudio(sample, manifestDir);
      const command = buildCommand(commandConfig.command, sample, audioPath);
      const runStartedAt = new Date().toISOString();
      planned.push({
        model: commandConfig.model,
        sampleId: sample.id,
        command,
        disabled: Boolean(commandConfig.disabled),
        disabledReason: commandConfig.disabledReason,
      });

      if (options.dryRun) {
        continue;
      }

      if (!fs.existsSync(audioPath)) {
        throw new Error(`Audio file not found for sample ${sample.id}: ${audioPath}`);
      }

      const started = performance.now();
      const { stdout, stderr } = await runCommand(command, options.timeoutMs);
      const latencyMs = Math.round(performance.now() - started);
      const hypothesis = extractHypothesis(stdout, commandConfig);

      runs.push({
        model: commandConfig.model,
        sampleId: sample.id,
        hypothesis,
        latencyMs,
        command,
        startedAt: runStartedAt,
        completedAt: new Date().toISOString(),
        stderr: stderr.trim() || undefined,
      });
    }
  }

  if (options.dryRun) {
    for (const item of planned) {
      const prefix = item.disabled ? '[disabled] ' : '';
      const suffix = item.disabledReason ? ` (${item.disabledReason})` : '';
      const target = item.batch ? 'batch' : item.sampleId;
      console.log(`${prefix}${item.model} ${target}: ${item.command}${suffix}`);
    }
    return;
  }

  const output = {
    metadata: buildMetadata({
      commands,
      commandsPath: resolvedCommands,
      manifestPath: resolvedManifest,
      models: options.models,
      outputPath: resolvedOutput,
      startedAt,
      timeoutMs: options.timeoutMs,
    }),
    samples: manifest.samples,
    runs,
  };
  fs.mkdirSync(path.dirname(resolvedOutput), { recursive: true });
  fs.writeFileSync(resolvedOutput, `${JSON.stringify(output, null, 2)}\n`);
}

try {
  await main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
