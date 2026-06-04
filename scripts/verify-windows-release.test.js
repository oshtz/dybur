import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const verifierPath = path.join(repoRoot, 'scripts', 'verify-windows-release.ps1');
const powershellCommand = process.platform === 'win32' ? 'powershell' : 'pwsh';
const unsignedOrInvalidSignature = /Authenticode status is (NotSigned|UnknownError)/;
const hasPowerShell =
  spawnSync(powershellCommand, ['-NoProfile', '-Command', '$PSVersionTable.PSVersion'], {
    encoding: 'utf8',
    windowsHide: true,
  }).status === 0;

function makeTempFixture(t, bytes = Buffer.from('fixture exe bytes')) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-windows-verify-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  const exePath = path.join(dir, 'dybur-windows-x64.exe');
  fs.writeFileSync(exePath, bytes);
  return { exePath, sha256: createHash('sha256').update(bytes).digest('hex') };
}

function runVerifier(args) {
  return spawnSync(
    powershellCommand,
    ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', verifierPath, ...args],
    {
      cwd: repoRoot,
      encoding: 'utf8',
      windowsHide: true,
    }
  );
}

describe('verify-windows-release', () => {
  it(
    'accepts a local input file when signature is informational',
    { skip: !hasPowerShell },
    (t) => {
      const fixture = makeTempFixture(t);

      const result = runVerifier([
        '-InputFile',
        fixture.exePath,
        '-ExpectedSha256',
        fixture.sha256,
        '-MinSizeMB',
        '0',
        '-Json',
      ]);

      assert.equal(result.status, 0, result.stderr);
      const summary = JSON.parse(result.stdout);
      assert.equal(summary.ok, true);
      assert.equal(summary.source, 'input-file');
      assert.equal(summary.downloadUrl, null);
      assert.equal(summary.downloadPath, fixture.exePath);
      assert.equal(summary.sha256, fixture.sha256);
      assert.match(summary.signatureStatus, /^(NotSigned|UnknownError)$/);
      assert.deepEqual(summary.issues, []);
      assert.match(summary.warnings.join('\n'), unsignedOrInvalidSignature);
    }
  );

  it('fails a local input file when a signature is required', { skip: !hasPowerShell }, (t) => {
    const fixture = makeTempFixture(t);

    const result = runVerifier([
      '-InputFile',
      fixture.exePath,
      '-MinSizeMB',
      '0',
      '-RequireSignature',
      '-Json',
    ]);

    assert.equal(result.status, 1);
    const summary = JSON.parse(result.stdout);
    assert.equal(summary.ok, false);
    assert.match(summary.issues.join('\n'), unsignedOrInvalidSignature);
  });

  it(
    'fails when a local input file hash does not match the expected hash',
    { skip: !hasPowerShell },
    (t) => {
      const fixture = makeTempFixture(t);

      const result = runVerifier([
        '-InputFile',
        fixture.exePath,
        '-ExpectedSha256',
        '0'.repeat(64),
        '-MinSizeMB',
        '0',
        '-Json',
      ]);

      assert.equal(result.status, 1);
      const summary = JSON.parse(result.stdout);
      assert.equal(summary.ok, false);
      assert.match(summary.issues.join('\n'), /SHA-256 mismatch/);
    }
  );
});
