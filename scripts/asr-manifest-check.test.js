import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const checkerPath = path.join(repoRoot, 'scripts', 'asr-manifest-check.js');
const corpusPolicyPath = path.join(repoRoot, 'benchmarks', 'asr', 'corpus-policy.example.json');

function makeTempFixture(t, manifest) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-asr-manifest-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  const manifestPath = path.join(dir, 'manifest.json');
  fs.writeFileSync(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  return { dir, manifestPath };
}

function writeJson(filePath, data) {
  fs.writeFileSync(filePath, `${JSON.stringify(data, null, 2)}\n`);
}

function runChecker(args) {
  return spawnSync(process.execPath, [checkerPath, ...args], {
    cwd: repoRoot,
    encoding: 'utf8',
    windowsHide: true,
  });
}

const validManifest = {
  samples: [
    {
      id: 'short-email',
      audio: 'short-email.wav',
      reference: 'Please send the notes.',
      durationMs: 3000,
      tags: ['english', 'email'],
    },
    {
      id: 'noisy-note',
      audio: 'noisy-note.wav',
      reference: 'The room is loud.',
      durationMs: 2500,
      tags: ['english', 'noisy'],
    },
  ],
};

describe('asr-manifest-check', () => {
  it('summarizes a valid manifest without requiring audio files', (t) => {
    const fixture = makeTempFixture(t, validManifest);

    const result = runChecker([
      fixture.manifestPath,
      '--require-duration',
      '--require-tags',
      '--required-tag',
      'noisy',
      '--json',
    ]);

    assert.equal(result.status, 0, result.stderr);
    const summary = JSON.parse(result.stdout);
    assert.equal(summary.sampleCount, 2);
    assert.deepEqual(
      summary.tagSummary.map((tag) => `${tag.tag}:${tag.samples}`),
      ['email:1', 'english:2', 'noisy:1']
    );
    assert.deepEqual(summary.issues, []);
  });

  it('requires audio files when requested', (t) => {
    const fixture = makeTempFixture(t, validManifest);

    const result = runChecker([fixture.manifestPath, '--require-audio']);

    assert.equal(result.status, 1);
    assert.match(result.stdout, /Audio file not found for sample short-email/);
  });

  it('passes audio checks when files exist', (t) => {
    const fixture = makeTempFixture(t, validManifest);
    fs.writeFileSync(path.join(fixture.dir, 'short-email.wav'), 'fixture');
    fs.writeFileSync(path.join(fixture.dir, 'noisy-note.wav'), 'fixture');

    const result = runChecker([fixture.manifestPath, '--require-audio']);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Samples: 2/);
  });

  it('reports duplicate ids, missing tags, and missing required tags', (t) => {
    const manifest = {
      samples: [
        {
          id: 'dupe',
          audio: 'one.wav',
          reference: 'One.',
        },
        {
          id: 'dupe',
          audio: 'two.wav',
          reference: 'Two.',
          tags: ['english'],
        },
      ],
    };
    const fixture = makeTempFixture(t, manifest);

    const result = runChecker([fixture.manifestPath, '--require-tags', '--required-tag', 'noisy']);

    assert.equal(result.status, 1);
    assert.match(result.stdout, /Duplicate sample id: dupe/);
    assert.match(result.stdout, /Sample dupe must include at least one tag/);
    assert.match(result.stdout, /Required tag missing from corpus: noisy/);
  });

  it('loads reusable corpus policy config', (t) => {
    const fixture = makeTempFixture(t, validManifest);
    const configPath = path.join(fixture.dir, 'policy.json');
    writeJson(configPath, {
      minSamples: 2,
      minSamplesPerTag: 1,
      requireDuration: true,
      requireTags: true,
      requiredTags: ['english', 'noisy'],
    });

    const result = runChecker([fixture.manifestPath, '--config', configPath, '--json']);

    assert.equal(result.status, 0, result.stderr);
    const summary = JSON.parse(result.stdout);
    assert.equal(summary.configPath, configPath);
    assert.deepEqual(summary.issues, []);
  });

  it('lets CLI values override corpus policy config values', (t) => {
    const fixture = makeTempFixture(t, validManifest);
    const configPath = path.join(fixture.dir, 'policy.json');
    writeJson(configPath, {
      minSamples: 3,
      requiredTags: ['missing'],
    });

    const result = runChecker([
      fixture.manifestPath,
      '--config',
      configPath,
      '--min-samples',
      '2',
      '--required-tag',
      'english',
    ]);

    assert.equal(result.status, 0, result.stderr);
    assert.doesNotMatch(result.stdout, /Required tag missing/);
  });

  it('enforces minimum sample coverage per required tag', (t) => {
    const fixture = makeTempFixture(t, validManifest);

    const result = runChecker([
      fixture.manifestPath,
      '--required-tag',
      'noisy',
      '--min-samples-per-tag',
      '2',
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stdout, /Required tag noisy must include at least 2 sample\(s\); found 1/);
  });

  it('accepts the checked-in corpus policy config for a complete corpus', (t) => {
    const categoryTags = [
      'short',
      'short',
      'long',
      'long',
      'quiet',
      'quiet',
      'noisy',
      'noisy',
      'domain',
      'domain',
      'punctuation',
      'hebrew',
    ];
    const manifest = {
      samples: categoryTags.map((tag, index) => ({
        id: `sample-${index + 1}`,
        audio: `sample-${index + 1}.wav`,
        reference: `Reference ${index + 1}.`,
        durationMs: 2000 + index,
        tags: ['english', tag],
      })),
    };
    const fixture = makeTempFixture(t, manifest);
    for (const sample of manifest.samples) {
      fs.writeFileSync(path.join(fixture.dir, sample.audio), 'fixture');
    }

    const result = runChecker([fixture.manifestPath, '--config', corpusPolicyPath]);

    assert.equal(result.status, 0, result.stderr);
    assert.match(result.stdout, /Policy:/);
    assert.match(result.stdout, /Samples: 12/);
  });
});
