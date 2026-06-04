import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const evalPath = path.join(repoRoot, 'scripts', 'asr-eval.js');

function makeTempFixture(t, manifest) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-asr-eval-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  const manifestPath = path.join(dir, 'manifest.json');
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  return { dir, manifestPath };
}

function runEval(args) {
  return spawnSync(process.execPath, [evalPath, ...args], {
    cwd: repoRoot,
    encoding: 'utf8',
    windowsHide: true,
  });
}

const completeManifest = {
  metadata: {
    runner: 'scripts/asr-candidate-runner.js',
    generatedAt: '2026-06-03T00:00:00.000Z',
    gitHead: 'abc123',
    platform: 'win32',
    arch: 'x64',
    nodeVersion: 'v20.0.0',
    selectedModel: null,
    timeoutMs: 180000,
    commandCount: 1,
    manifestPath: 'benchmarks/asr/example.json',
    commandsPath: 'benchmarks/asr/candidate-commands.local.json',
  },
  samples: [
    {
      id: 'short-email',
      reference: 'Please send the notes.',
      durationMs: 3000,
      tags: ['english', 'email'],
    },
    {
      id: 'quiet-note',
      reference: 'The room is quiet now.',
      durationMs: 2500,
      tags: ['english', 'quiet'],
    },
  ],
  runs: [
    {
      model: 'model-a',
      sampleId: 'short-email',
      hypothesis: 'please send the notes',
      latencyMs: 300,
    },
    {
      model: 'model-a',
      sampleId: 'quiet-note',
      hypothesis: 'the room is quiet now',
      latencyMs: 250,
    },
  ],
};

describe('asr-eval', () => {
  it('scores complete manifests in strict JSON mode', (t) => {
    const fixture = makeTempFixture(t, completeManifest);

    const result = runEval([fixture.manifestPath, '--format', 'json', '--strict']);

    assert.equal(result.status, 0, result.stderr);
    const report = JSON.parse(result.stdout);
    assert.equal(report.sampleCount, 2);
    assert.equal(report.runCount, 2);
    assert.equal(report.sourceMetadata.runner, 'scripts/asr-candidate-runner.js');
    assert.equal(report.sourceMetadata.gitHead, 'abc123');
    assert.equal(report.models[0].model, 'model-a');
    assert.equal(report.models[0].samples, 2);
    assert.equal(report.runs[0].tags.join(','), 'email,english');

    const english = report.tagSummaries.find(
      (summary) => summary.tag === 'english' && summary.model === 'model-a'
    );
    assert.equal(english.samples, 2);

    const email = report.tagSummaries.find(
      (summary) => summary.tag === 'email' && summary.model === 'model-a'
    );
    assert.equal(email.samples, 1);
  });

  it('renders tag summaries in markdown reports', (t) => {
    const fixture = makeTempFixture(t, completeManifest);

    const result = runEval([fixture.manifestPath, '--strict']);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /## Tag Summary/);
    assert.match(result.stdout, /## Source Metadata/);
    assert.match(result.stdout, /\| Runner \| scripts\/asr-candidate-runner\.js \|/);
    assert.match(result.stdout, /\| english \| model-a \| 2 \|/);
    assert.match(result.stdout, /\| Model \| Sample \| Tags \|/);
  });

  it('fails strict mode when a model does not cover every sample', (t) => {
    const manifest = {
      ...completeManifest,
      runs: completeManifest.runs.slice(0, 1),
    };
    const fixture = makeTempFixture(t, manifest);

    const result = runEval([fixture.manifestPath, '--strict']);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /Model model-a is missing sample\(s\): quiet-note/);
  });

  it('rejects duplicate runs for the same model and sample', (t) => {
    const manifest = {
      ...completeManifest,
      runs: [...completeManifest.runs, completeManifest.runs[0]],
    };
    const fixture = makeTempFixture(t, manifest);

    const result = runEval([fixture.manifestPath]);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /Duplicate run for model\/sample: model-a\/short-email/);
  });

  it('rejects duplicate sample ids', (t) => {
    const manifest = {
      ...completeManifest,
      samples: [completeManifest.samples[0], completeManifest.samples[0]],
    };
    const fixture = makeTempFixture(t, manifest);

    const result = runEval([fixture.manifestPath]);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /Duplicate sample id: short-email/);
  });

  it('rejects malformed sample tags', (t) => {
    const manifest = {
      ...completeManifest,
      samples: [{ ...completeManifest.samples[0], tags: ['english', ''] }],
    };
    const fixture = makeTempFixture(t, manifest);

    const result = runEval([fixture.manifestPath]);

    assert.equal(result.status, 1);
    assert.match(result.stderr, /Sample short-email tags must be non-empty strings/);
  });
});
