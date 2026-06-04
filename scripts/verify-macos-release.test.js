import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const verifierPath = path.join(repoRoot, 'scripts', 'verify-macos-release.js');

function makeTempFixture(t, bytes = Buffer.from('fixture dmg bytes')) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-macos-verify-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  const dmgPath = path.join(dir, 'dybur-macos-arm64.dmg');
  fs.writeFileSync(dmgPath, bytes);
  return { dmgPath, sha256: createHash('sha256').update(bytes).digest('hex') };
}

function runVerifier(args) {
  return spawnSync(process.execPath, [verifierPath, ...args], {
    cwd: repoRoot,
    encoding: 'utf8',
    windowsHide: true,
  });
}

describe('verify-macos-release', () => {
  it('accepts a local input file and skips platform checks when requested', (t) => {
    const fixture = makeTempFixture(t);

    const result = runVerifier([
      '--input-file',
      fixture.dmgPath,
      '--expected-sha256',
      fixture.sha256,
      '--min-size-mb',
      '0.00001',
      '--skip-macos-checks',
      '--json',
    ]);

    assert.equal(result.status, 0, result.stderr);
    const summary = JSON.parse(result.stdout);
    assert.equal(summary.ok, true);
    assert.equal(summary.source, 'input-file');
    assert.equal(summary.downloadUrl, null);
    assert.equal(summary.downloadPath, fixture.dmgPath);
    assert.equal(summary.sha256, fixture.sha256);
    assert.deepEqual(summary.issues, []);
    assert.deepEqual(summary.warnings, ['macOS mount/codesign/Gatekeeper checks skipped by flag']);
  });

  it('fails when a local input file hash does not match the expected hash', (t) => {
    const fixture = makeTempFixture(t);

    const result = runVerifier([
      '--input-file',
      fixture.dmgPath,
      '--expected-sha256',
      '0'.repeat(64),
      '--min-size-mb',
      '0.00001',
      '--skip-macos-checks',
      '--json',
    ]);

    assert.equal(result.status, 1);
    const summary = JSON.parse(result.stdout);
    assert.equal(summary.ok, false);
    assert.match(summary.issues.join('\n'), /SHA-256 mismatch/);
  });

  it('fails when a local input file is below the configured minimum size', (t) => {
    const fixture = makeTempFixture(t);

    const result = runVerifier([
      '--input-file',
      fixture.dmgPath,
      '--min-size-mb',
      '1',
      '--skip-macos-checks',
      '--json',
    ]);

    assert.equal(result.status, 1);
    const summary = JSON.parse(result.stdout);
    assert.equal(summary.ok, false);
    assert.match(summary.issues.join('\n'), /dybur-macos-arm64\.dmg is unexpectedly small/);
  });

  it('rejects mutually exclusive macOS check flags', (t) => {
    const fixture = makeTempFixture(t);

    const result = runVerifier([
      '--input-file',
      fixture.dmgPath,
      '--skip-macos-checks',
      '--require-macos-checks',
    ]);

    assert.equal(result.status, 1);
    assert.match(
      result.stderr,
      /--skip-macos-checks cannot be combined with --require-macos-checks/
    );
  });
});
