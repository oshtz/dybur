#!/usr/bin/env node

/**
 * Orchestrate the ASR evidence required before promoting a candidate runtime.
 *
 * This intentionally composes the existing manifest, candidate runner, scorer,
 * and gate scripts so candidate promotion uses the same checks developers run
 * manually during ASR evaluation.
 */

import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { performance } from 'node:perf_hooks';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DEFAULT_MODEL = 'nemotron-35-asr-streaming-onnx-int4';
const DEFAULT_TIMEOUT_MS = 180_000;
const DEFAULT_MANIFEST_CONFIG = path.join(
  repoRoot,
  'benchmarks',
  'asr',
  'corpus-policy.example.json'
);

function usage() {
  console.log(`Usage: node scripts/asr-runtime-readiness.js <manifest.json> --commands <commands.json> [options]

Options:
  --model <id>              Candidate model to run (default: ${DEFAULT_MODEL})
  --baseline <id>           Baseline model command to include and gate against
  --candidate <id>          Candidate model to include; repeatable for comparisons
  --all-models              Run all enabled commands instead of filtering to one model
  --output-dir <dir>        Output directory (default: benchmarks/asr/runtime-readiness/<model>)
  --manifest-config <file>  Corpus policy config (default: benchmarks/asr/corpus-policy.example.json)
  --gate-config <file>      Optional gate config to apply to the JSON report
  --timeout-ms <ms>         Per-sample candidate timeout (default: ${DEFAULT_TIMEOUT_MS})
  --skip-manifest-check     Skip corpus policy validation
  --skip-preflight          Skip command setup checks
  --skip-gate               Ignore --gate-config for this run
  --dry-run                 Print the readiness plan without running commands or writing outputs
`);
}

function requireValue(args, optionName) {
  const value = args.shift();
  if (value == null || value.startsWith('--')) {
    throw new Error(`${optionName} requires a value`);
  }
  return value;
}

function parseArgs(argv) {
  const args = [...argv];
  let manifestPath = null;
  const options = {
    allModels: false,
    baselineModel: null,
    candidateModels: [],
    commandsPath: null,
    dryRun: false,
    gateConfigPath: null,
    manifestConfigPath: DEFAULT_MANIFEST_CONFIG,
    model: DEFAULT_MODEL,
    outputDir: null,
    skipGate: false,
    skipManifestCheck: false,
    skipPreflight: false,
    timeoutMs: DEFAULT_TIMEOUT_MS,
  };

  while (args.length > 0) {
    const arg = args.shift();
    switch (arg) {
      case '--commands':
        options.commandsPath = requireValue(args, '--commands');
        break;
      case '--model':
        options.model = requireValue(args, '--model');
        options.allModels = false;
        break;
      case '--baseline':
        options.baselineModel = requireValue(args, '--baseline');
        options.allModels = false;
        break;
      case '--candidate':
        options.candidateModels.push(requireValue(args, '--candidate'));
        options.allModels = false;
        break;
      case '--all-models':
        options.allModels = true;
        break;
      case '--output-dir':
        options.outputDir = requireValue(args, '--output-dir');
        break;
      case '--manifest-config':
        options.manifestConfigPath = requireValue(args, '--manifest-config');
        break;
      case '--gate-config':
        options.gateConfigPath = requireValue(args, '--gate-config');
        break;
      case '--timeout-ms':
        options.timeoutMs = Number(requireValue(args, '--timeout-ms'));
        break;
      case '--skip-manifest-check':
        options.skipManifestCheck = true;
        break;
      case '--skip-preflight':
        options.skipPreflight = true;
        break;
      case '--skip-gate':
        options.skipGate = true;
        break;
      case '--dry-run':
        options.dryRun = true;
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

  if (!Number.isFinite(options.timeoutMs) || options.timeoutMs <= 0) {
    throw new Error('--timeout-ms must be a positive number');
  }

  if (!manifestPath || !options.commandsPath) {
    usage();
    process.exit(1);
  }

  options.candidateModels = [...new Set(options.candidateModels.filter(Boolean))];

  return { manifestPath, options };
}

function resolveInput(filePath) {
  return path.resolve(filePath);
}

function sanitizeFileStem(value) {
  return value.replace(/[^A-Za-z0-9._-]+/g, '_').replace(/^_+|_+$/g, '') || 'asr-candidate';
}

function uniqueValues(values) {
  return [...new Set(values.filter(Boolean))];
}

function displayPath(filePath) {
  const relative = path.relative(repoRoot, filePath);
  if (!relative.startsWith('..') && !path.isAbsolute(relative)) {
    return relative || '.';
  }
  return filePath;
}

function quoteArg(value) {
  const text = String(value);
  if (/^[A-Za-z0-9_./:=\\-]+$/.test(text)) {
    return text;
  }
  return JSON.stringify(text);
}

function displayCommand(scriptRelPath, args) {
  return ['node', scriptRelPath, ...args.map(displayPath).map(quoteArg)].join(' ');
}

function buildCommandConfigStep(context) {
  const target = context.allModels
    ? 'at least one enabled command'
    : `enabled model${context.selectedModels.length === 1 ? '' : 's'} ${context.selectedModels.join(
        ', '
      )}`;
  return {
    command: `command file contains ${target} in ${displayPath(context.commandsPath)}`,
    kind: 'local',
    name: 'command-config',
    status: 'pending',
  };
}

function buildScriptStep(name, scriptRelPath, args) {
  const scriptPath = path.join(repoRoot, scriptRelPath);
  return {
    args,
    command: displayCommand(scriptRelPath, args),
    kind: 'script',
    name,
    scriptPath,
    status: 'pending',
  };
}

function addModelFilters(args, context) {
  if (context.allModels) {
    return args;
  }
  return context.selectedModels.reduce(
    (modelArgs, model) => [...modelArgs, '--model', model],
    args
  );
}

function resolveSelectedModels(options) {
  if (options.allModels) {
    return [];
  }
  if (options.baselineModel) {
    const candidates =
      options.candidateModels.length > 0 ? options.candidateModels : [options.model];
    return uniqueValues([options.baselineModel, ...candidates]);
  }
  if (options.candidateModels.length > 0) {
    return uniqueValues(options.candidateModels);
  }
  return [options.model];
}

function resolveModelStem(options, selectedModels) {
  if (options.allModels) {
    return 'all-models';
  }
  if (options.baselineModel) {
    const candidates =
      options.candidateModels.length > 0 ? options.candidateModels : [options.model];
    return `comparison-${sanitizeFileStem(options.baselineModel)}-vs-${sanitizeFileStem(
      candidates.join('-')
    )}`;
  }
  if (selectedModels.length === 1) {
    return sanitizeFileStem(selectedModels[0]);
  }
  return `models-${sanitizeFileStem(selectedModels.join('-'))}`;
}

function buildContext({ manifestPath, options }) {
  const resolvedManifest = resolveInput(manifestPath);
  const resolvedCommands = resolveInput(options.commandsPath);
  const resolvedManifestConfig = options.manifestConfigPath
    ? resolveInput(options.manifestConfigPath)
    : null;
  const resolvedGateConfig = options.gateConfigPath ? resolveInput(options.gateConfigPath) : null;
  const selectedModels = resolveSelectedModels(options);
  const candidateModels =
    options.baselineModel && options.candidateModels.length === 0
      ? [options.model]
      : options.candidateModels;
  const modelStem = resolveModelStem(options, selectedModels);
  const outputDir = resolveInput(
    options.outputDir ?? path.join(repoRoot, 'benchmarks', 'asr', 'runtime-readiness', modelStem)
  );
  const runsPath = path.join(outputDir, `${modelStem}-runs.json`);
  const reportPath = path.join(outputDir, `${modelStem}-report.json`);
  const summaryPath = path.join(outputDir, 'readiness-summary.json');

  const context = {
    allModels: options.allModels,
    baselineModel: options.baselineModel,
    candidateModels,
    commandsPath: resolvedCommands,
    dryRun: options.dryRun,
    gateConfigPath: resolvedGateConfig,
    manifestConfigPath: resolvedManifestConfig,
    manifestPath: resolvedManifest,
    model: selectedModels.length === 1 ? selectedModels[0] : null,
    outputDir,
    reportPath,
    runsPath,
    selectedModels,
    skipGate: options.skipGate,
    skipManifestCheck: options.skipManifestCheck,
    skipPreflight: options.skipPreflight,
    summaryPath,
    timeoutMs: options.timeoutMs,
  };

  context.steps = buildSteps(context);
  return context;
}

function buildSteps(context) {
  const steps = [buildCommandConfigStep(context)];

  if (!context.skipManifestCheck) {
    const args = [context.manifestPath];
    if (context.manifestConfigPath) {
      args.push('--config', context.manifestConfigPath);
    }
    steps.push(buildScriptStep('manifest-check', 'scripts/asr-manifest-check.js', args));
  }

  if (!context.skipPreflight) {
    const args = addModelFilters(
      [
        '--commands',
        context.commandsPath,
        '--preflight',
        '--timeout-ms',
        String(context.timeoutMs),
      ],
      context
    );
    steps.push(buildScriptStep('preflight', 'scripts/asr-candidate-runner.js', args));
  }

  const runArgs = addModelFilters(
    [
      context.manifestPath,
      '--commands',
      context.commandsPath,
      '--output',
      context.runsPath,
      '--timeout-ms',
      String(context.timeoutMs),
    ],
    context
  );
  steps.push(buildScriptStep('candidate-run', 'scripts/asr-candidate-runner.js', runArgs));

  steps.push(
    buildScriptStep('score', 'scripts/asr-eval.js', [
      context.runsPath,
      '--format',
      'json',
      '--output',
      context.reportPath,
      '--strict',
    ])
  );

  if (context.gateConfigPath && !context.skipGate) {
    const gateArgs = [context.reportPath, '--config', context.gateConfigPath];
    if (context.baselineModel) {
      gateArgs.push('--baseline', context.baselineModel);
    }
    for (const candidateModel of context.candidateModels) {
      gateArgs.push('--candidate', candidateModel);
    }
    steps.push(buildScriptStep('gate', 'scripts/asr-gate.js', gateArgs));
  }

  return steps;
}

function readJson(filePath) {
  return JSON.parse(fs.readFileSync(filePath, 'utf8'));
}

function validateCommandConfig(context) {
  const commandsFile = readJson(context.commandsPath);
  if (!Array.isArray(commandsFile.commands)) {
    throw new Error('Commands file must include commands[]');
  }

  const selectedModels = new Set(context.selectedModels);
  const commands = commandsFile.commands.filter(
    (command) => context.allModels || selectedModels.has(command.model)
  );
  if (commands.length === 0) {
    throw new Error(
      context.allModels ? 'No commands found' : `No command found for model: ${context.model}`
    );
  }

  for (const command of commands) {
    if (!command.model || !command.command) {
      throw new Error('Each command must include model and command');
    }
  }

  if (context.allModels) {
    const enabledCommands = commands.filter((command) => !command.disabled);
    if (enabledCommands.length === 0) {
      throw new Error('No enabled commands found in command file');
    }
    return;
  }

  const byModel = new Map(commands.map((command) => [command.model, command]));
  const missingModels = context.selectedModels.filter((model) => !byModel.has(model));
  if (missingModels.length > 0) {
    throw new Error(
      missingModels.length === 1
        ? `No command found for model: ${missingModels[0]}`
        : `No command found for model(s): ${missingModels.join(', ')}`
    );
  }

  for (const model of context.selectedModels) {
    const selectedCommand = byModel.get(model);
    if (selectedCommand.disabled) {
      const reason = selectedCommand.disabledReason ? `: ${selectedCommand.disabledReason}` : '';
      throw new Error(`Selected command is disabled for ${model}${reason}`);
    }
  }
}

function writeOutput(stream, value) {
  if (value) {
    stream.write(value);
  }
}

function completeStep(step, started, status, exitCode) {
  step.completedAt = new Date().toISOString();
  step.durationMs = Math.round(performance.now() - started);
  step.exitCode = exitCode;
  step.status = status;
}

function runStep(step, context) {
  step.startedAt = new Date().toISOString();
  const started = performance.now();
  console.log(`[run] ${step.name}: ${step.command}`);

  if (step.kind === 'local') {
    try {
      validateCommandConfig(context);
      completeStep(step, started, 'passed', 0);
    } catch (error) {
      completeStep(step, started, 'failed', null);
      throw error;
    }
  } else {
    const result = spawnSync(process.execPath, [step.scriptPath, ...step.args], {
      cwd: repoRoot,
      encoding: 'utf8',
      windowsHide: true,
    });

    writeOutput(process.stdout, result.stdout);
    writeOutput(process.stderr, result.stderr);

    if (result.error) {
      completeStep(step, started, 'failed', null);
      throw result.error;
    }
    if (result.status !== 0) {
      completeStep(step, started, 'failed', result.status);
      throw new Error(`${step.name} failed with exit ${result.status}`);
    }

    completeStep(step, started, 'passed', result.status);
  }

  console.log(`[ok] ${step.name}`);
}

function buildSummary(context, status) {
  return {
    runner: 'scripts/asr-runtime-readiness.js',
    generatedAt: new Date().toISOString(),
    status,
    model: context.model,
    allModels: context.allModels,
    baselineModel: context.baselineModel,
    candidateModels: context.candidateModels,
    selectedModels: context.selectedModels,
    manifestPath: context.manifestPath,
    commandsPath: context.commandsPath,
    manifestConfigPath: context.manifestConfigPath,
    gateConfigPath: context.gateConfigPath,
    outputDir: context.outputDir,
    runsPath: context.runsPath,
    reportPath: context.reportPath,
    summaryPath: context.summaryPath,
    timeoutMs: context.timeoutMs,
    steps: context.steps.map((step) => ({
      name: step.name,
      command: step.command,
      status: step.status,
      startedAt: step.startedAt ?? null,
      completedAt: step.completedAt ?? null,
      durationMs: step.durationMs ?? null,
      exitCode: step.exitCode ?? null,
    })),
  };
}

function writeSummary(context, status) {
  fs.mkdirSync(context.outputDir, { recursive: true });
  fs.writeFileSync(context.summaryPath, `${JSON.stringify(buildSummary(context, status), null, 2)}\n`);
}

function printDryRun(context) {
  console.log('ASR runtime readiness dry-run');
  console.log(
    `Model: ${context.allModels ? 'all enabled commands' : context.selectedModels.join(', ')}`
  );
  console.log(`Output directory: ${displayPath(context.outputDir)}`);
  for (const step of context.steps) {
    console.log(`[plan] ${step.name}: ${step.command}`);
  }
}

function execute(context) {
  if (context.dryRun) {
    printDryRun(context);
    return;
  }

  fs.mkdirSync(context.outputDir, { recursive: true });
  for (const step of context.steps) {
    runStep(step, context);
  }
  writeSummary(context, 'passed');
  console.log(`ASR runtime readiness summary: ${displayPath(context.summaryPath)}`);
}

let context = null;

try {
  context = buildContext(parseArgs(process.argv.slice(2)));
  execute(context);
} catch (error) {
  if (context && !context.dryRun) {
    writeSummary(context, 'failed');
  }
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
