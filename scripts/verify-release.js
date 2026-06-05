#!/usr/bin/env node

/**
 * Verify that the public GitHub release matches dybur's stable asset contract.
 */

import fs from 'node:fs';
import path from 'node:path';

const DEFAULT_REPO = 'oshtz/dybur';

const EXPECTED_ASSETS = [
  {
    name: 'dybur-macos-arm64.dmg',
    label: 'macOS Apple Silicon DMG',
    minSizeBytes: 5 * 1024 * 1024,
  },
  {
    name: 'dybur-windows-x64.exe',
    label: 'Windows x64 portable EXE',
    minSizeBytes: 15 * 1024 * 1024,
  },
];

const LEGACY_ASSET_NAMES = new Set([
  'dybur-macos.zip',
  'dybur-portable.exe',
  'dybur_1.0.0_aarch64.dmg',
]);

function usage() {
  console.log(`Usage: node scripts/verify-release.js [options]

Options:
  --repo <owner/repo>        GitHub repository (default: ${DEFAULT_REPO})
  --package-json <file>     Package JSON used for expected version (default: package.json)
  --release-json <file>     Verify a release fixture instead of live GitHub
  --api-base-url <url>      GitHub API base URL (default: https://api.github.com)
  --tag <tag>               Expected tag override (default: v<package.version>)
  --skip-download-urls      Skip HEAD checks for /latest/download asset URLs
  --json                    Print machine-readable summary
`);
}

function requireValue(args, optionName) {
  const value = args.shift();
  if (!value || value.startsWith('--')) {
    throw new Error(`${optionName} requires a value`);
  }
  return value;
}

function parseArgs(argv) {
  const options = {
    repo: DEFAULT_REPO,
    packageJsonPath: 'package.json',
    releaseJsonPath: null,
    apiBaseUrl: 'https://api.github.com',
    expectedTag: null,
    skipDownloadUrls: false,
    json: false,
  };
  const args = [...argv];

  while (args.length > 0) {
    const arg = args.shift();
    switch (arg) {
      case '--repo':
        options.repo = requireValue(args, '--repo');
        break;
      case '--package-json':
        options.packageJsonPath = requireValue(args, '--package-json');
        break;
      case '--release-json':
        options.releaseJsonPath = requireValue(args, '--release-json');
        break;
      case '--api-base-url':
        options.apiBaseUrl = requireValue(args, '--api-base-url').replace(/\/+$/, '');
        break;
      case '--tag':
        options.expectedTag = requireValue(args, '--tag');
        break;
      case '--skip-download-urls':
        options.skipDownloadUrls = true;
        break;
      case '--json':
        options.json = true;
        break;
      case '--help':
      case '-h':
        usage();
        process.exit(0);
        break;
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }
  }

  if (!/^[^/]+\/[^/]+$/.test(options.repo)) {
    throw new Error('--repo must use owner/repo format');
  }

  return options;
}

function readPackageVersion(packageJsonPath) {
  const resolvedPath = path.resolve(packageJsonPath);
  const packageJson = JSON.parse(fs.readFileSync(resolvedPath, 'utf8'));

  if (typeof packageJson.version !== 'string' || packageJson.version.trim().length === 0) {
    throw new Error(`${resolvedPath} must include a string version`);
  }

  return packageJson.version.trim();
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function fetchWithRetry(url, init, { attempts = 3 } = {}) {
  let lastError = null;

  for (let attempt = 1; attempt <= attempts; attempt += 1) {
    try {
      const response = await fetch(url, init);
      if (response.ok || response.status < 500 || attempt === attempts) {
        return response;
      }
      lastError = new Error(`${url} returned ${response.status} ${response.statusText}`);
    } catch (error) {
      lastError = error;
      if (attempt === attempts) {
        break;
      }
    }

    await sleep(250 * attempt);
  }

  throw lastError ?? new Error(`Failed to fetch ${url}`);
}

function getGitHubApiHeaders() {
  const headers = {
    Accept: 'application/vnd.github+json',
    'User-Agent': 'dybur-release-verifier',
  };
  const token = process.env.GITHUB_TOKEN || process.env.GH_TOKEN;

  if (token) {
    headers.Authorization = `Bearer ${token}`;
  }

  return headers;
}

async function fetchJson(url) {
  const response = await fetchWithRetry(url, {
    headers: getGitHubApiHeaders(),
  });

  if (!response.ok) {
    const rateLimitHint =
      response.status === 403 && !(process.env.GITHUB_TOKEN || process.env.GH_TOKEN)
        ? '; set GITHUB_TOKEN or GH_TOKEN to use an authenticated API request'
        : '';
    throw new Error(
      `GitHub API returned ${response.status} ${response.statusText}${rateLimitHint}`
    );
  }

  return response.json();
}

async function readRelease(options) {
  if (options.releaseJsonPath) {
    const resolvedPath = path.resolve(options.releaseJsonPath);
    return JSON.parse(fs.readFileSync(resolvedPath, 'utf8'));
  }

  const [owner, repo] = options.repo.split('/');
  const encodedOwner = encodeURIComponent(owner);
  const encodedRepo = encodeURIComponent(repo);
  return fetchJson(`${options.apiBaseUrl}/repos/${encodedOwner}/${encodedRepo}/releases/latest`);
}

async function verifyLatestDownloadUrl({ assetName, repo }) {
  const response = await fetchWithRetry(
    `https://github.com/${repo}/releases/latest/download/${assetName}`,
    {
      method: 'HEAD',
      headers: {
        'User-Agent': 'dybur-release-verifier',
      },
      redirect: 'follow',
    }
  );

  if (!response.ok) {
    return {
      ok: false,
      status: response.status,
      url: response.url,
    };
  }

  return {
    ok: true,
    status: response.status,
    url: response.url,
    host: new URL(response.url).hostname,
  };
}

function summarizeAssets(release) {
  return (release.assets ?? []).map((asset) => ({
    name: asset.name,
    size: asset.size,
    browserDownloadUrl: asset.browser_download_url,
  }));
}

async function verifyRelease({ expectedTag, options, release }) {
  const issues = [];
  const warnings = [];
  const assets = summarizeAssets(release);
  const assetByName = new Map(assets.map((asset) => [asset.name, asset]));

  if (release.tag_name !== expectedTag) {
    issues.push(
      `Latest release tag ${release.tag_name ?? '<missing>'} does not match ${expectedTag}`
    );
  }

  if (release.draft) {
    issues.push(`${release.tag_name ?? '<unknown>'} is still a draft release`);
  }
  if (release.prerelease) {
    issues.push(`${release.tag_name ?? '<unknown>'} is marked prerelease`);
  }

  for (const expected of EXPECTED_ASSETS) {
    const asset = assetByName.get(expected.name);
    if (!asset) {
      issues.push(`Missing ${expected.label}: ${expected.name}`);
      continue;
    }

    if (!asset.browserDownloadUrl) {
      issues.push(`${expected.name} is missing browser_download_url`);
    }

    if (!Number.isFinite(asset.size) || asset.size < expected.minSizeBytes) {
      issues.push(`${expected.name} is unexpectedly small: ${asset.size ?? '<unknown>'} bytes`);
    }
  }

  for (const asset of assets) {
    if (LEGACY_ASSET_NAMES.has(asset.name)) {
      issues.push(`Legacy release asset should not be published: ${asset.name}`);
    } else if (!EXPECTED_ASSETS.some((expected) => expected.name === asset.name)) {
      warnings.push(`Unexpected extra release asset: ${asset.name}`);
    }
  }

  const downloadChecks = [];
  if (!options.skipDownloadUrls && !options.releaseJsonPath) {
    for (const expected of EXPECTED_ASSETS) {
      if (!assetByName.has(expected.name)) {
        continue;
      }

      const result = await verifyLatestDownloadUrl({
        assetName: expected.name,
        repo: options.repo,
      });
      downloadChecks.push({ assetName: expected.name, ...result });
      if (!result.ok) {
        issues.push(
          `${expected.name} /latest/download URL returned ${result.status} (${result.url})`
        );
      }
    }
  }

  return {
    ok: issues.length === 0,
    expectedTag,
    actualTag: release.tag_name ?? null,
    publishedAt: release.published_at ?? null,
    htmlUrl: release.html_url ?? null,
    assets,
    downloadChecks,
    warnings,
    issues,
  };
}

function renderText(summary) {
  const lines = [
    `Expected tag: ${summary.expectedTag}`,
    `Latest release: ${summary.actualTag ?? '<missing>'}`,
    summary.publishedAt ? `Published: ${summary.publishedAt}` : null,
    summary.htmlUrl ? `Release URL: ${summary.htmlUrl}` : null,
    '',
    'Assets:',
  ].filter((line) => line !== null);

  for (const expected of EXPECTED_ASSETS) {
    const asset = summary.assets.find((candidate) => candidate.name === expected.name);
    if (!asset) {
      lines.push(`  - ${expected.name}: missing`);
      continue;
    }
    lines.push(`  - ${asset.name}: ${(asset.size / (1024 * 1024)).toFixed(1)} MB`);
  }

  if (summary.downloadChecks.length > 0) {
    lines.push('', 'Download URLs:');
    for (const check of summary.downloadChecks) {
      lines.push(
        `  - ${check.assetName}: ${check.ok ? 'OK' : 'FAIL'} ${check.host ?? check.url ?? ''}`.trimEnd()
      );
    }
  }

  if (summary.warnings.length > 0) {
    lines.push('', 'Warnings:');
    for (const warning of summary.warnings) {
      lines.push(`  - ${warning}`);
    }
  }

  if (summary.issues.length > 0) {
    lines.push('', 'Issues:');
    for (const issue of summary.issues) {
      lines.push(`  - ${issue}`);
    }
  }

  return `${lines.join('\n')}\n`;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const packageVersion = readPackageVersion(options.packageJsonPath);
  const expectedTag = options.expectedTag ?? `v${packageVersion}`;
  const release = await readRelease(options);
  const summary = await verifyRelease({ expectedTag, options, release });

  if (options.json) {
    process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
  } else {
    process.stdout.write(renderText(summary));
  }

  if (!summary.ok) {
    process.exit(1);
  }
}

try {
  await main();
} catch (error) {
  console.error(error instanceof Error ? error.message : error);
  process.exit(1);
}
