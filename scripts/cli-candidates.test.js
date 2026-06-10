import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const cliPath = path.join(repoRoot, 'packages', 'cli', 'dist', 'cli.js');
const candidateCommandsPath = path.join(
  repoRoot,
  'benchmarks',
  'asr',
  'candidate-commands.example.json'
);

function stripAnsi(value) {
  return value.replace(/\x1B\[[0-?]*[ -/]*[@-~]/g, '');
}

function makeCliEnv(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-cli-candidates-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  return {
    ...process.env,
    APPDATA: path.join(dir, 'appdata'),
    HOME: path.join(dir, 'home'),
    USERPROFILE: path.join(dir, 'home'),
    NO_COLOR: '1',
    FORCE_COLOR: '0',
  };
}

function runCli(t, args) {
  const result = spawnSync(process.execPath, [cliPath, ...args], {
    cwd: repoRoot,
    env: makeCliEnv(t),
    encoding: 'utf8',
    windowsHide: true,
  });

  return {
    ...result,
    stdout: stripAnsi(result.stdout),
    stderr: stripAnsi(result.stderr),
  };
}

describe('dybur CLI model candidates', () => {
  it('shows active experimental candidates by default', (t) => {
    const result = runCli(t, ['models', 'candidates']);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Experimental Model Candidates/);
    assert.match(result.stdout, /parakeet-tdt-v3-coreml/);
    assert.match(result.stdout, /nemotron-35-asr-streaming-onnx-int4/);
    assert.match(result.stdout, /qwen3-asr-0\.6b/);
    assert.match(result.stdout, /moonshine-streaming-tiny/);
    assert.doesNotMatch(result.stdout, /canary-1b-v2/);
    assert.match(result.stdout, /Candidates are not production model IDs/);
    assert.match(result.stdout, /dybur models candidates --all/);
  });

  it('includes deferred candidates when requested', (t) => {
    const result = runCli(t, ['models', 'candidates', '--all']);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /canary-1b-v2/);
    assert.match(result.stdout, /voxtral-mini-3b/);
    assert.doesNotMatch(result.stdout, /dybur models candidates --all/);
  });

  it('keeps candidates out of the production model registry', (t) => {
    const result = runCli(t, ['models', 'list', '--available']);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Available Models/);
    assert.match(result.stdout, /parakeet-tdt-v3-int8/);
    assert.doesNotMatch(result.stdout, /parakeet-tdt-v2-int8/);
    assert.match(result.stdout, /whisper-large-v3-turbo-int8/);
    assert.doesNotMatch(result.stdout, /parakeet-tdt-v3-coreml/);
    assert.doesNotMatch(result.stdout, /nemotron-35-asr-streaming-onnx-int4/);
    assert.doesNotMatch(result.stdout, /qwen3-asr-0\.6b/);
    assert.doesNotMatch(result.stdout, /moonshine-streaming-tiny/);
  });

  it('keeps the Nemotron 3.5 benchmark command on the ORT GenAI wrapper', () => {
    const commandsFile = JSON.parse(fs.readFileSync(candidateCommandsPath, 'utf8'));
    const nemotron = commandsFile.commands.find(
      (command) => command.model === 'nemotron-35-asr-streaming-onnx-int4'
    );

    assert.ok(nemotron);
    assert.match(nemotron.command, /scripts\/asr-candidates\/nemotron35-ortgenai\.py/);
    assert.match(nemotron.batchCommand, /scripts\/asr-candidates\/nemotron35-ortgenai\.py/);
    assert.match(nemotron.checkCommand, /scripts\/asr-candidates\/nemotron35-ortgenai\.py/);
    assert.ok(
      fs.existsSync(path.join(repoRoot, 'scripts', 'asr-candidates', 'nemotron35-ortgenai.py'))
    );
  });

  it('includes the production Parakeet v3 baseline command', () => {
    const commandsFile = JSON.parse(fs.readFileSync(candidateCommandsPath, 'utf8'));
    const baseline = commandsFile.commands.find(
      (command) => command.model === 'parakeet-tdt-v3-int8'
    );

    assert.ok(baseline);
    assert.match(baseline.command, /scripts\/asr-candidates\/parakeet-tdt-onnx\.py/);
    assert.match(baseline.batchCommand, /scripts\/asr-candidates\/parakeet-tdt-onnx\.py/);
    assert.match(baseline.checkCommand, /scripts\/asr-candidates\/parakeet-tdt-onnx\.py/);
    assert.ok(
      fs.existsSync(path.join(repoRoot, 'scripts', 'asr-candidates', 'parakeet-tdt-onnx.py'))
    );
  });

  it('keeps the Parakeet v2 command as a disabled legacy benchmark control', () => {
    const commandsFile = JSON.parse(fs.readFileSync(candidateCommandsPath, 'utf8'));
    const legacy = commandsFile.commands.find(
      (command) => command.model === 'parakeet-tdt-v2-int8'
    );

    assert.ok(legacy);
    assert.equal(legacy.disabled, true);
    assert.match(legacy.disabledReason, /Legacy benchmark control/);
    assert.match(legacy.command, /scripts\/asr-candidates\/parakeet-tdt-onnx\.py/);
  });

  it('includes installed production model benchmark adapters', () => {
    const commandsFile = JSON.parse(fs.readFileSync(candidateCommandsPath, 'utf8'));
    const nemotron = commandsFile.commands.find(
      (command) => command.model === 'nemotron-streaming-int8'
    );
    const whisperInt8 = commandsFile.commands.find(
      (command) => command.model === 'whisper-large-v3-turbo-int8'
    );
    const whisperFp16 = commandsFile.commands.find(
      (command) => command.model === 'whisper-large-v3-turbo-fp16'
    );

    assert.ok(nemotron);
    assert.match(nemotron.command, /scripts\/asr-candidates\/nemotron-streaming-onnx\.py/);
    assert.match(nemotron.batchCommand, /scripts\/asr-candidates\/nemotron-streaming-onnx\.py/);
    assert.ok(
      fs.existsSync(path.join(repoRoot, 'scripts', 'asr-candidates', 'nemotron-streaming-onnx.py'))
    );

    assert.ok(whisperInt8);
    assert.match(whisperInt8.command, /scripts\/asr-candidates\/whisper-onnx\.py/);
    assert.match(whisperInt8.batchCommand, /scripts\/asr-candidates\/whisper-onnx\.py/);

    assert.ok(whisperFp16);
    assert.match(whisperFp16.command, /scripts\/asr-candidates\/whisper-onnx\.py/);
    assert.match(whisperFp16.batchCommand, /--disable-optimizations/);
    assert.ok(fs.existsSync(path.join(repoRoot, 'scripts', 'asr-candidates', 'whisper-onnx.py')));
  });
});
