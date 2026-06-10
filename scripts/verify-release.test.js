import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { describe, it } from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const verifierPath = path.join(repoRoot, 'scripts', 'verify-release.js');

const baseUpdateManifest = {
  version: '1.2.1',
  pub_date: '2026-05-29T18:43:15Z',
  platforms: {
    'windows-x64': {
      url: 'https://github.com/oshtz/dybur/releases/download/v1.2.1/dybur-windows-x64.exe',
      sha256: 'a'.repeat(64),
      size: 42 * 1024 * 1024,
    },
    'darwin-arm64': {
      url: 'https://github.com/oshtz/dybur/releases/download/v1.2.1/dybur-macos-arm64.dmg',
      sha256: 'b'.repeat(64),
      size: 16 * 1024 * 1024,
    },
  },
};

const baseRelease = {
  tag_name: 'v1.2.1',
  published_at: '2026-05-29T18:43:15Z',
  html_url: 'https://github.com/oshtz/dybur/releases/tag/v1.2.1',
  draft: false,
  prerelease: false,
  assets: [
    {
      name: 'dybur-macos-arm64.dmg',
      size: 16 * 1024 * 1024,
      browser_download_url:
        'https://github.com/oshtz/dybur/releases/download/v1.2.1/dybur-macos-arm64.dmg',
    },
    {
      name: 'dybur-windows-x64.exe',
      size: 42 * 1024 * 1024,
      browser_download_url:
        'https://github.com/oshtz/dybur/releases/download/v1.2.1/dybur-windows-x64.exe',
    },
    {
      name: 'dybur-update.json',
      size: 700,
      browser_download_url:
        'https://github.com/oshtz/dybur/releases/download/v1.2.1/dybur-update.json',
      content: baseUpdateManifest,
    },
  ],
};

function makeTempFixture(t, { packageJson = { version: '1.2.1' }, release = baseRelease }) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dybur-release-verify-'));
  t.after(() => fs.rmSync(dir, { recursive: true, force: true }));

  const packageJsonPath = path.join(dir, 'package.json');
  const releaseJsonPath = path.join(dir, 'release.json');
  fs.writeFileSync(packageJsonPath, `${JSON.stringify(packageJson, null, 2)}\n`);
  fs.writeFileSync(releaseJsonPath, `${JSON.stringify(release, null, 2)}\n`);

  return { packageJsonPath, releaseJsonPath };
}

function runVerifier(args) {
  return spawnSync(process.execPath, [verifierPath, ...args], {
    cwd: repoRoot,
    encoding: 'utf8',
    windowsHide: true,
  });
}

function runVerifierAsync(args, { env = {} } = {}) {
  return new Promise((resolve) => {
    const child = spawn(process.execPath, [verifierPath, ...args], {
      cwd: repoRoot,
      env: { ...process.env, ...env },
      windowsHide: true,
    });

    let stdout = '';
    let stderr = '';
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => {
      stdout += chunk;
    });
    child.stderr.on('data', (chunk) => {
      stderr += chunk;
    });
    child.on('close', (status) => {
      resolve({ status, stdout, stderr });
    });
  });
}

function listen(server) {
  return new Promise((resolve) => {
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      assert.equal(typeof address, 'object');
      resolve(address.port);
    });
  });
}

describe('verify-release', () => {
  it('accepts a release matching the package version and stable asset contract', (t) => {
    const fixture = makeTempFixture(t, {});

    const result = runVerifier([
      '--package-json',
      fixture.packageJsonPath,
      '--release-json',
      fixture.releaseJsonPath,
      '--skip-download-urls',
      '--json',
    ]);

    assert.equal(result.status, 0, result.stderr);
    const summary = JSON.parse(result.stdout);
    assert.equal(summary.ok, true);
    assert.equal(summary.expectedTag, 'v1.2.1');
    assert.equal(summary.actualTag, 'v1.2.1');
    assert.deepEqual(summary.issues, []);
    assert.equal(summary.manifestCheck.ok, true);
  });

  it('fails when the latest release tag does not match the package version', (t) => {
    const fixture = makeTempFixture(t, {
      packageJson: { version: '1.2.2' },
    });

    const result = runVerifier([
      '--package-json',
      fixture.packageJsonPath,
      '--release-json',
      fixture.releaseJsonPath,
      '--skip-download-urls',
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stdout, /Latest release tag v1\.2\.1 does not match v1\.2\.2/);
  });

  it('fails when a stable asset is missing', (t) => {
    const fixture = makeTempFixture(t, {
      release: {
        ...baseRelease,
        assets: baseRelease.assets.filter((asset) => asset.name !== 'dybur-windows-x64.exe'),
      },
    });

    const result = runVerifier([
      '--package-json',
      fixture.packageJsonPath,
      '--release-json',
      fixture.releaseJsonPath,
      '--skip-download-urls',
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stdout, /Missing Windows x64 portable EXE: dybur-windows-x64\.exe/);
  });

  it('fails when the update manifest asset is missing', (t) => {
    const fixture = makeTempFixture(t, {
      release: {
        ...baseRelease,
        assets: baseRelease.assets.filter((asset) => asset.name !== 'dybur-update.json'),
      },
    });

    const result = runVerifier([
      '--package-json',
      fixture.packageJsonPath,
      '--release-json',
      fixture.releaseJsonPath,
      '--skip-download-urls',
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stdout, /Missing update manifest: dybur-update\.json/);
  });

  it('fails when the update manifest omits a required platform', (t) => {
    const fixture = makeTempFixture(t, {
      release: {
        ...baseRelease,
        assets: baseRelease.assets.map((asset) =>
          asset.name === 'dybur-update.json'
            ? {
                ...asset,
                content: {
                  ...baseUpdateManifest,
                  platforms: {
                    'windows-x64': baseUpdateManifest.platforms['windows-x64'],
                  },
                },
              }
            : asset
        ),
      },
    });

    const result = runVerifier([
      '--package-json',
      fixture.packageJsonPath,
      '--release-json',
      fixture.releaseJsonPath,
      '--skip-download-urls',
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stdout, /Update manifest missing platform darwin-arm64/);
  });

  it('fails when a stable asset is too small', (t) => {
    const fixture = makeTempFixture(t, {
      release: {
        ...baseRelease,
        assets: baseRelease.assets.map((asset) =>
          asset.name === 'dybur-macos-arm64.dmg' ? { ...asset, size: 1024 } : asset
        ),
      },
    });

    const result = runVerifier([
      '--package-json',
      fixture.packageJsonPath,
      '--release-json',
      fixture.releaseJsonPath,
      '--skip-download-urls',
    ]);

    assert.equal(result.status, 1);
    assert.match(result.stdout, /dybur-macos-arm64\.dmg is unexpectedly small: 1024 bytes/);
  });

  it('rejects known legacy public asset names', (t) => {
    const fixture = makeTempFixture(t, {
      release: {
        ...baseRelease,
        assets: [
          ...baseRelease.assets,
          {
            name: 'dybur-portable.exe',
            size: 42 * 1024 * 1024,
            browser_download_url:
              'https://github.com/oshtz/dybur/releases/download/v1.2.1/dybur-portable.exe',
          },
        ],
      },
    });

    const result = runVerifier([
      '--package-json',
      fixture.packageJsonPath,
      '--release-json',
      fixture.releaseJsonPath,
      '--skip-download-urls',
    ]);

    assert.equal(result.status, 1);
    assert.match(
      result.stdout,
      /Legacy release asset should not be published: dybur-portable\.exe/
    );
  });

  it('uses GitHub token for live API requests', async (t) => {
    let authorization = null;
    const server = http.createServer((request, response) => {
      authorization = request.headers.authorization ?? null;
      assert.equal(request.url, '/repos/oshtz/dybur/releases/latest');

      response.writeHead(200, { 'Content-Type': 'application/json' });
      response.end(JSON.stringify(baseRelease));
    });
    t.after(() => server.close());
    const port = await listen(server);
    const fixture = makeTempFixture(t, {});

    const result = await runVerifierAsync(
      [
        '--package-json',
        fixture.packageJsonPath,
        '--api-base-url',
        `http://127.0.0.1:${port}`,
        '--skip-download-urls',
        '--json',
      ],
      {
        env: {
          GITHUB_TOKEN: '',
          GH_TOKEN: 'test-token',
        },
      }
    );

    assert.equal(result.status, 0, result.stderr);
    assert.equal(authorization, 'Bearer test-token');
    const summary = JSON.parse(result.stdout);
    assert.equal(summary.ok, true);
  });

  it('suggests a GitHub token when unauthenticated API requests are forbidden', async (t) => {
    const server = http.createServer((_request, response) => {
      response.writeHead(403, { 'Content-Type': 'application/json' });
      response.end(JSON.stringify({ message: 'rate limit exceeded' }));
    });
    t.after(() => server.close());
    const port = await listen(server);
    const fixture = makeTempFixture(t, {});

    const result = await runVerifierAsync(
      [
        '--package-json',
        fixture.packageJsonPath,
        '--api-base-url',
        `http://127.0.0.1:${port}`,
        '--skip-download-urls',
      ],
      {
        env: {
          GITHUB_TOKEN: '',
          GH_TOKEN: '',
        },
      }
    );

    assert.equal(result.status, 1);
    assert.match(result.stderr, /set GITHUB_TOKEN or GH_TOKEN/);
  });
});
