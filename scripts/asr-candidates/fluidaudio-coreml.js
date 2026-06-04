#!/usr/bin/env node

/**
 * Benchmark wrapper for FluidAudio CoreML Parakeet on macOS.
 *
 * This is a candidate path, not dybur's production runtime. It invokes the
 * FluidAudio Swift CLI and prints only the transcript by default so the shared
 * ASR harness can score it.
 */

import { spawnSync } from 'node:child_process';
import path from 'node:path';
import process from 'node:process';

function usage() {
  console.log(`Usage: node scripts/asr-candidates/fluidaudio-coreml.js <audio> [options]

Options:
  --package-path <path>  Path to a local FluidAudio checkout.
                         Defaults to FLUIDAUDIO_PACKAGE_PATH.
  --model-version <v>    Optional FluidAudio model version, e.g. v2 or v3.
                         Omit to use FluidAudio CLI defaults.
  --executable <name>    Swift executable name. Default: fluidaudiocli
  --json                 Print {"text": "..."} instead of plain text
  --raw                  Print FluidAudio stdout without transcript extraction
  --preflight            Check platform, package path, and Swift availability
  -h, --help             Show this help

Setup:
  git clone https://github.com/FluidInference/FluidAudio.git
  export FLUIDAUDIO_PACKAGE_PATH=/path/to/FluidAudio
  node scripts/asr-candidates/fluidaudio-coreml.js samples/example.wav
`);
}

function fail(message, exitCode = 1) {
  console.error(message);
  process.exit(exitCode);
}

function parseArgs(argv) {
  if (argv.includes('--help') || argv.includes('-h')) {
    usage();
    process.exit(0);
  }

  const args = [...argv];
  const options = {
    audio: null,
    packagePath: process.env.FLUIDAUDIO_PACKAGE_PATH ?? null,
    modelVersion: null,
    executable: 'fluidaudiocli',
    json: false,
    raw: false,
    preflight: false,
  };

  while (args.length > 0) {
    const arg = args.shift();
    switch (arg) {
      case '--package-path':
        options.packagePath = args.shift() ?? null;
        break;
      case '--model-version':
        options.modelVersion = args.shift() ?? null;
        break;
      case '--executable':
        options.executable = args.shift() ?? null;
        break;
      case '--json':
        options.json = true;
        break;
      case '--raw':
        options.raw = true;
        break;
      case '--preflight':
        options.preflight = true;
        break;
      default:
        if (arg.startsWith('-')) {
          fail(`Unknown argument: ${arg}`);
        }
        if (options.audio) {
          fail(`Unexpected positional argument: ${arg}`);
        }
        options.audio = arg;
    }
  }

  if (!options.preflight && !options.audio) {
    usage();
    process.exit(1);
  }
  if (!options.executable) {
    fail('--executable must not be empty');
  }

  return options;
}

function extractTranscript(stdout) {
  const output = stdout.trim();
  const lines = output
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean);

  for (const line of [...lines].reverse()) {
    const match = line.match(/^Transcription:\s*(.+)$/i);
    if (match) {
      return match[1].trim();
    }
  }

  return lines.at(-1) ?? output;
}

function main() {
  const options = parseArgs(process.argv.slice(2));

  if (process.platform !== 'darwin') {
    fail('FluidAudio CoreML benchmarks require macOS with Swift and CoreML support.', 2);
  }

  if (!options.packagePath) {
    fail('Set FLUIDAUDIO_PACKAGE_PATH or pass --package-path /path/to/FluidAudio.', 2);
  }

  if (options.preflight) {
    const swiftVersion = spawnSync('swift', ['--version'], {
      encoding: 'utf8',
      windowsHide: true,
    });

    if (swiftVersion.error) {
      fail(
        swiftVersion.error.code === 'ENOENT'
          ? 'Missing dependency: swift. Install Xcode command line tools on macOS.'
          : String(swiftVersion.error),
        2
      );
    }

    if (swiftVersion.status !== 0) {
      process.stderr.write(swiftVersion.stderr || swiftVersion.stdout);
      process.exit(swiftVersion.status ?? 1);
    }

    console.log(`ok: FluidAudio CoreML preflight passed for ${path.resolve(options.packagePath)}`);
    return;
  }

  const swiftArgs = [
    'run',
    '--package-path',
    path.resolve(options.packagePath),
    options.executable,
    'transcribe',
    path.resolve(options.audio),
  ];

  if (options.modelVersion) {
    swiftArgs.push('--model-version', options.modelVersion);
  }

  const result = spawnSync('swift', swiftArgs, {
    encoding: 'utf8',
    windowsHide: true,
  });

  if (result.error) {
    fail(
      result.error.code === 'ENOENT'
        ? 'Missing dependency: swift. Install Xcode command line tools on macOS.'
        : String(result.error),
      2
    );
  }

  if (result.status !== 0) {
    process.stderr.write(result.stderr || result.stdout);
    process.exit(result.status ?? 1);
  }

  const text = options.raw ? result.stdout.trim() : extractTranscript(result.stdout);
  if (options.json) {
    console.log(JSON.stringify({ text }));
  } else {
    console.log(text);
  }
}

main();
