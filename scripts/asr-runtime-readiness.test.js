import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const readinessPath = path.join(repoRoot, 'scripts', 'asr-runtime-readiness.js');

function makeTempFixture(t) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-asr-runtime-readiness-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  const audioPath = path.join(dir, 'sample.wav');
  const manifestPath = path.join(dir, 'manifest.json');
  const commandsPath = path.join(dir, 'commands.json');
  const manifestConfigPath = path.join(dir, 'manifest-policy.json');
  const gateConfigPath = path.join(dir, 'gate.json');
  const outputDir = path.join(dir, 'readiness');

  fs.writeFileSync(audioPath, 'fixture audio placeholder');
  fs.writeFileSync(
    manifestPath,
    `${JSON.stringify(
      {
        samples: [
          {
            id: 'sample-1',
            audio: 'sample.wav',
            reference: 'hello world',
            durationMs: 1200,
            tags: ['english', 'short'],
          },
        ],
      },
      null,
      2
    )}\n`
  );
  fs.writeFileSync(
    manifestConfigPath,
    `${JSON.stringify(
      {
        minSamples: 1,
        requireAudio: true,
        requireDuration: true,
        requireTags: true,
        requiredTags: ['english'],
      },
      null,
      2
    )}\n`
  );
  fs.writeFileSync(
    gateConfigPath,
    `${JSON.stringify(
      {
        candidates: ['fixture-model'],
        maxWer: 0.01,
        maxCer: 0.01,
        maxRtf: 10,
        checkTags: true,
      },
      null,
      2
    )}\n`
  );
  fs.writeFileSync(
    commandsPath,
    `${JSON.stringify(
      {
        commands: [
          {
            model: 'fixture-model',
            command: `${JSON.stringify(process.execPath)} -e "console.log('hello world')"`,
            checkCommand: `${JSON.stringify(process.execPath)} -e "console.log('ready')"`,
          },
        ],
      },
      null,
      2
    )}\n`
  );

  return {
    commandsPath,
    dir,
    gateConfigPath,
    manifestConfigPath,
    manifestPath,
    outputDir,
  };
}

function writeCommands(filePath, commands) {
  fs.writeFileSync(filePath, `${JSON.stringify({ commands }, null, 2)}\n`);
}

function runReadiness(args) {
  return spawnSync(process.execPath, [readinessPath, ...args], {
    cwd: repoRoot,
    encoding: 'utf8',
    windowsHide: true,
  });
}

describe('asr-runtime-readiness', () => {
  it('requires a manifest and command file', () => {
    const result = runReadiness([]);

    assert.equal(result.status, 1);
    assert.match(result.stdout, /Usage: node scripts\/asr-runtime-readiness\.js/);
  });

  it('prints the readiness plan without creating outputs in dry-run mode', (t) => {
    const fixture = makeTempFixture(t);

    const result = runReadiness([
      fixture.manifestPath,
      '--commands',
      fixture.commandsPath,
      '--model',
      'fixture-model',
      '--manifest-config',
      fixture.manifestConfigPath,
      '--output-dir',
      fixture.outputDir,
      '--dry-run',
    ]);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /ASR runtime readiness dry-run/);
    assert.match(result.stdout, /\[plan\] command-config:/);
    assert.match(result.stdout, /\[plan\] manifest-check:/);
    assert.match(result.stdout, /\[plan\] preflight:/);
    assert.match(result.stdout, /\[plan\] candidate-run:/);
    assert.match(result.stdout, /\[plan\] score:/);
    assert.doesNotMatch(result.stdout, /\[plan\] gate:/);
    assert.match(result.stdout, /asr-candidate-runner\.js/);
    assert.match(result.stdout, /--preflight/);
    assert.match(result.stdout, /fixture-model-runs\.json/);
    assert.match(result.stdout, /command file contains enabled model fixture-model/);
    assert.ok(!fs.existsSync(fixture.outputDir));
  });

  it('fails readiness before corpus work when the selected command is disabled', (t) => {
    const fixture = makeTempFixture(t);
    writeCommands(fixture.commandsPath, [
      {
        model: 'fixture-model',
        command: 'missing-runtime "{audio}"',
        disabled: true,
        disabledReason: 'Install the runtime first.',
      },
    ]);

    const result = runReadiness([
      fixture.manifestPath,
      '--commands',
      fixture.commandsPath,
      '--model',
      'fixture-model',
      '--manifest-config',
      fixture.manifestConfigPath,
      '--output-dir',
      fixture.outputDir,
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /Selected command is disabled for fixture-model/);
    assert.doesNotMatch(result.stdout, /\[run\] manifest-check/);

    const summaryPath = path.join(fixture.outputDir, 'readiness-summary.json');
    assert.ok(fs.existsSync(summaryPath));
    const summary = JSON.parse(fs.readFileSync(summaryPath, 'utf8'));
    assert.equal(summary.status, 'failed');
    assert.deepEqual(
      summary.steps.map((step) => `${step.name}:${step.status}`),
      [
        'command-config:failed',
        'manifest-check:pending',
        'preflight:pending',
        'candidate-run:pending',
        'score:pending',
      ]
    );
  });

  it('runs the readiness workflow, applies an optional gate, and writes a summary', (t) => {
    const fixture = makeTempFixture(t);

    const result = runReadiness([
      fixture.manifestPath,
      '--commands',
      fixture.commandsPath,
      '--model',
      'fixture-model',
      '--manifest-config',
      fixture.manifestConfigPath,
      '--gate-config',
      fixture.gateConfigPath,
      '--output-dir',
      fixture.outputDir,
    ]);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /\[ok\] manifest-check/);
    assert.match(result.stdout, /\[ok\] preflight/);
    assert.match(result.stdout, /\[ok\] candidate-run/);
    assert.match(result.stdout, /\[ok\] score/);
    assert.match(result.stdout, /\[ok\] gate/);

    const runsPath = path.join(fixture.outputDir, 'fixture-model-runs.json');
    const reportPath = path.join(fixture.outputDir, 'fixture-model-report.json');
    const summaryPath = path.join(fixture.outputDir, 'readiness-summary.json');
    assert.ok(fs.existsSync(runsPath));
    assert.ok(fs.existsSync(reportPath));
    assert.ok(fs.existsSync(summaryPath));

    const summary = JSON.parse(fs.readFileSync(summaryPath, 'utf8'));
    assert.equal(summary.runner, 'scripts/asr-runtime-readiness.js');
    assert.equal(summary.model, 'fixture-model');
    assert.equal(summary.runsPath, runsPath);
    assert.equal(summary.reportPath, reportPath);
    assert.equal(summary.status, 'passed');
    assert.deepEqual(
      summary.steps.map((step) => `${step.name}:${step.status}`),
      [
        'command-config:passed',
        'manifest-check:passed',
        'preflight:passed',
        'candidate-run:passed',
        'score:passed',
        'gate:passed',
      ]
    );
  });

  it('runs an exact baseline comparison and gates the selected pair', (t) => {
    const fixture = makeTempFixture(t);
    writeCommands(fixture.commandsPath, [
      {
        model: 'baseline-model',
        command: `${JSON.stringify(process.execPath)} -e "console.log('hello world')"`,
        checkCommand: `${JSON.stringify(process.execPath)} -e "console.log('baseline ready')"`,
      },
      {
        model: 'candidate-model',
        command: `${JSON.stringify(process.execPath)} -e "console.log('hello brave world')"`,
        checkCommand: `${JSON.stringify(process.execPath)} -e "console.log('candidate ready')"`,
      },
      {
        model: 'unselected-model',
        command: `${JSON.stringify(process.execPath)} -e "process.exit(9)"`,
        checkCommand: `${JSON.stringify(process.execPath)} -e "process.exit(9)"`,
      },
    ]);
    fs.writeFileSync(
      fixture.gateConfigPath,
      `${JSON.stringify(
        {
          baseline: 'wrong-baseline',
          candidates: ['wrong-candidate'],
          maxWerRegression: 0.5,
          checkTags: true,
        },
        null,
        2
      )}\n`
    );

    const result = runReadiness([
      fixture.manifestPath,
      '--commands',
      fixture.commandsPath,
      '--baseline',
      'baseline-model',
      '--candidate',
      'candidate-model',
      '--manifest-config',
      fixture.manifestConfigPath,
      '--gate-config',
      fixture.gateConfigPath,
      '--output-dir',
      fixture.outputDir,
    ]);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /\[ok\] gate/);
    assert.match(result.stdout, /--model baseline-model --model candidate-model/);
    assert.match(result.stdout, /--baseline baseline-model --candidate candidate-model/);

    const summaryPath = path.join(fixture.outputDir, 'readiness-summary.json');
    const summary = JSON.parse(fs.readFileSync(summaryPath, 'utf8'));
    assert.equal(summary.baselineModel, 'baseline-model');
    assert.deepEqual(summary.candidateModels, ['candidate-model']);
    assert.deepEqual(summary.selectedModels, ['baseline-model', 'candidate-model']);
    assert.equal(summary.model, null);

    const report = JSON.parse(fs.readFileSync(summary.reportPath, 'utf8'));
    assert.deepEqual(
      report.models.map((model) => model.model),
      ['baseline-model', 'candidate-model']
    );
  });
});
