import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const runnerPath = path.join(repoRoot, 'scripts', 'asr-candidate-runner.js');

function makeTempFixture() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-asr-candidate-runner-'));
  const audioPath = path.join(dir, 'sample.wav');
  const manifestPath = path.join(dir, 'manifest.json');
  const commandsPath = path.join(dir, 'commands.json');
  const outputPath = path.join(dir, 'runs.json');

  fs.writeFileSync(audioPath, 'not real audio; command fixture only');
  fs.writeFileSync(
    manifestPath,
    `${JSON.stringify(
      {
        samples: [
          {
            id: 'sample-1',
            reference: 'hello world',
            audio: 'sample.wav',
            durationMs: 1234,
          },
        ],
      },
      null,
      2
    )}\n`
  );

  return {
    dir,
    audioPath,
    manifestPath,
    commandsPath,
    outputPath,
  };
}

function writeCommands(filePath, commands) {
  fs.writeFileSync(filePath, `${JSON.stringify({ commands }, null, 2)}\n`);
}

function runRunner(args) {
  return spawnSync(process.execPath, [runnerPath, ...args], {
    cwd: repoRoot,
    encoding: 'utf8',
    windowsHide: true,
  });
}

describe('asr-candidate-runner', () => {
  it('runs command preflight checks without a manifest or output path', (t) => {
    const fixture = makeTempFixture();
    t.after(() => fs.rmSync(fixture.dir, { recursive: true, force: true }));

    writeCommands(fixture.commandsPath, [
      {
        model: 'checked-model',
        command: 'unused "{audio}"',
        checkCommand: `${JSON.stringify(process.execPath)} -e "console.log('ready')"`,
      },
      {
        model: 'unchecked-model',
        command: 'unused "{audio}"',
      },
      {
        model: 'disabled-model',
        command: 'unused "{audio}"',
        disabled: true,
        disabledReason: 'Install the disabled runtime first.',
      },
    ]);

    const result = runRunner(['--commands', fixture.commandsPath, '--preflight']);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /\[ok\] checked-model:/);
    assert.match(result.stdout, /\[unchecked\] unchecked-model: no checkCommand configured/);
    assert.match(
      result.stdout,
      /\[disabled\] disabled-model: Install the disabled runtime first\./
    );
    assert.ok(!fs.existsSync(fixture.outputPath));
  });

  it('fails preflight when an enabled check command fails', (t) => {
    const fixture = makeTempFixture();
    t.after(() => fs.rmSync(fixture.dir, { recursive: true, force: true }));

    writeCommands(fixture.commandsPath, [
      {
        model: 'broken-model',
        command: 'unused "{audio}"',
        checkCommand: `${JSON.stringify(process.execPath)} -e "console.error('missing runtime'); process.exit(7)"`,
      },
    ]);

    const result = runRunner(['--commands', fixture.commandsPath, '--preflight']);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /\[failed\] broken-model:/);
    assert.match(result.stderr, /missing runtime/);
    assert.match(result.stderr, /Preflight failed for 1 command\(s\)/);
    assert.ok(!fs.existsSync(fixture.outputPath));
  });

  it('prints disabled commands and setup reasons in dry-run mode', (t) => {
    const fixture = makeTempFixture();
    t.after(() => fs.rmSync(fixture.dir, { recursive: true, force: true }));

    writeCommands(fixture.commandsPath, [
      {
        model: 'enabled-model',
        command: `${JSON.stringify(process.execPath)} -e "console.log('enabled')"`,
      },
      {
        model: 'disabled-model',
        command: 'missing-runtime "{audio}"',
        disabled: true,
        disabledReason: 'Install the missing runtime first.',
      },
    ]);

    const result = runRunner([
      fixture.manifestPath,
      '--commands',
      fixture.commandsPath,
      '--output',
      fixture.outputPath,
      '--dry-run',
    ]);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /enabled-model sample-1:/);
    assert.match(result.stdout, /\[disabled\] disabled-model sample-1:/);
    assert.match(result.stdout, /Install the missing runtime first\./);
    assert.ok(!fs.existsSync(fixture.outputPath));
  });

  it('skips disabled commands during execution and writes ASR eval output', (t) => {
    const fixture = makeTempFixture();
    t.after(() => fs.rmSync(fixture.dir, { recursive: true, force: true }));

    writeCommands(fixture.commandsPath, [
      {
        model: 'json-model',
        command: `${JSON.stringify(process.execPath)} -e "console.log(JSON.stringify({ text: 'hello world' }))"`,
        outputJsonPath: 'text',
      },
      {
        model: 'disabled-model',
        command: 'missing-runtime "{audio}"',
        disabled: true,
        disabledReason: 'Not installed.',
      },
    ]);

    const result = runRunner([
      fixture.manifestPath,
      '--commands',
      fixture.commandsPath,
      '--output',
      fixture.outputPath,
    ]);

    assert.equal(result.status, 0, result.stderr);
    const output = JSON.parse(fs.readFileSync(fixture.outputPath, 'utf8'));
    assert.equal(output.metadata.runner, 'scripts/asr-candidate-runner.js');
    assert.equal(output.metadata.platform, process.platform);
    assert.equal(output.metadata.arch, process.arch);
    assert.equal(output.metadata.commandCount, 1);
    assert.equal(output.metadata.commands[0].model, 'json-model');
    assert.equal(output.metadata.selectedModel, null);
    assert.equal(output.samples.length, 1);
    assert.deepEqual(
      output.runs.map((run) => run.model),
      ['json-model']
    );
    assert.equal(output.runs[0].sampleId, 'sample-1');
    assert.equal(output.runs[0].hypothesis, 'hello world');
    assert.equal(typeof output.runs[0].latencyMs, 'number');
    assert.match(output.runs[0].command, /JSON\.stringify/);
    assert.equal(typeof output.runs[0].startedAt, 'string');
    assert.equal(typeof output.runs[0].completedAt, 'string');
  });

  it('runs exactly the repeated --model selections', (t) => {
    const fixture = makeTempFixture();
    t.after(() => fs.rmSync(fixture.dir, { recursive: true, force: true }));

    writeCommands(fixture.commandsPath, [
      {
        model: 'baseline-model',
        command: `${JSON.stringify(process.execPath)} -e "console.log('hello world')"`,
      },
      {
        model: 'candidate-model',
        command: `${JSON.stringify(process.execPath)} -e "console.log('hello brave world')"`,
      },
      {
        model: 'unselected-model',
        command: `${JSON.stringify(process.execPath)} -e "process.exit(9)"`,
      },
    ]);

    const result = runRunner([
      fixture.manifestPath,
      '--commands',
      fixture.commandsPath,
      '--output',
      fixture.outputPath,
      '--model',
      'baseline-model',
      '--model',
      'candidate-model',
    ]);

    assert.equal(result.status, 0, result.stderr);
    const output = JSON.parse(fs.readFileSync(fixture.outputPath, 'utf8'));
    assert.equal(output.metadata.selectedModel, null);
    assert.deepEqual(output.metadata.selectedModels, ['baseline-model', 'candidate-model']);
    assert.deepEqual(
      output.runs.map((run) => `${run.model}:${run.hypothesis}`),
      ['baseline-model:hello world', 'candidate-model:hello brave world']
    );
  });

  it('runs batch commands once per model and uses reported per-sample latencies', (t) => {
    const fixture = makeTempFixture();
    t.after(() => fs.rmSync(fixture.dir, { recursive: true, force: true }));

    const secondAudioPath = path.join(fixture.dir, 'sample-2.wav');
    fs.writeFileSync(secondAudioPath, 'second fixture audio');
    fs.writeFileSync(
      fixture.manifestPath,
      `${JSON.stringify(
        {
          samples: [
            {
              id: 'sample-1',
              reference: 'hello world',
              audio: 'sample.wav',
              durationMs: 1234,
            },
            {
              id: 'sample-2',
              reference: 'batch mode',
              audio: 'sample-2.wav',
              durationMs: 2345,
            },
          ],
        },
        null,
        2
      )}\n`
    );

    const batchScript = path.join(fixture.dir, 'batch-runner.mjs');
    fs.writeFileSync(
      batchScript,
      `
import fs from 'node:fs';
const [manifestPath, outputPath] = process.argv.slice(2);
const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
fs.writeFileSync(
  outputPath,
  JSON.stringify({
    runs: manifest.samples.map((sample, index) => ({
      sampleId: sample.id,
      hypothesis: index === 0 ? 'hello world' : 'batch mode',
      latencyMs: 100 + index,
    })),
  })
);
`
    );

    writeCommands(fixture.commandsPath, [
      {
        model: 'batch-model',
        command: `${JSON.stringify(process.execPath)} -e "process.exit(9)"`,
        batchCommand: `${JSON.stringify(process.execPath)} ${JSON.stringify(
          batchScript
        )} "{manifest}" "{output}"`,
      },
    ]);

    const result = runRunner([
      fixture.manifestPath,
      '--commands',
      fixture.commandsPath,
      '--output',
      fixture.outputPath,
    ]);

    assert.equal(result.status, 0, result.stderr);
    const output = JSON.parse(fs.readFileSync(fixture.outputPath, 'utf8'));
    assert.equal(output.metadata.commands[0].batchCommand != null, true);
    assert.deepEqual(
      output.runs.map((run) => [run.model, run.sampleId, run.hypothesis, run.latencyMs]),
      [
        ['batch-model', 'sample-1', 'hello world', 100],
        ['batch-model', 'sample-2', 'batch mode', 101],
      ]
    );
    assert.match(output.runs[0].command, /batch-runner\.mjs/);
    assert.equal(fs.existsSync(path.join(fixture.dir, 'runs.batch-model.batch.json')), true);
  });

  it('reports disabled-only selections with their setup reason', (t) => {
    const fixture = makeTempFixture();
    t.after(() => fs.rmSync(fixture.dir, { recursive: true, force: true }));

    writeCommands(fixture.commandsPath, [
      {
        model: 'disabled-model',
        command: 'missing-runtime "{audio}"',
        disabled: true,
        disabledReason: 'Install this runtime before enabling.',
      },
    ]);

    const result = runRunner([
      fixture.manifestPath,
      '--commands',
      fixture.commandsPath,
      '--output',
      fixture.outputPath,
      '--model',
      'disabled-model',
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /No enabled commands found/);
    assert.match(result.stderr, /disabled-model: Install this runtime before enabling\./);
    assert.ok(!fs.existsSync(fixture.outputPath));
  });
});
