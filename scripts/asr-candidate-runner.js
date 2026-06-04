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
  --model <id>         Run only one candidate model
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
    model: null,
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
        options.model = args.shift() ?? null;
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
    if (!command.model || !command.command) {
      throw new Error('Each command must include model and command');
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

function buildMetadata({
  commands,
  commandsPath,
  manifestPath,
  model,
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
    selectedModel: model,
    timeoutMs,
    commandCount: commands.length,
    commands: commands.map((command) => ({
      model: command.model,
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

  const matchingCommands = commandsFile.commands.filter(
    (command) => !options.model || command.model === options.model
  );
  if (matchingCommands.length === 0) {
    throw new Error(
      options.model ? `No command found for model: ${options.model}` : 'No commands found'
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

  const planned = [];
  const runs = [];

  for (const commandConfig of commands) {
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
      console.log(`${prefix}${item.model} ${item.sampleId}: ${item.command}${suffix}`);
    }
    return;
  }

  const resolvedOutput = path.resolve(options.outputPath);
  const output = {
    metadata: buildMetadata({
      commands,
      commandsPath: resolvedCommands,
      manifestPath: resolvedManifest,
      model: options.model,
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
