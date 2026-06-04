#!/usr/bin/env node

/**
 * Download and smoke-check the public macOS DMG release artifact.
 */

import { createHash } from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const execFileAsync = promisify(execFile);

const DEFAULT_REPO = 'oshtz/dybur';
const DEFAULT_ASSET_NAME = 'dybur-macos-arm64.dmg';
const DEFAULT_MIN_SIZE_MB = 5;

function usage() {
  console.log(`Usage: node scripts/verify-macos-release.js [options]

Options:
  --repo <owner/repo>          GitHub repository (default: ${DEFAULT_REPO})
  --asset <name>               Release asset name (default: ${DEFAULT_ASSET_NAME})
  --input-file <file>          Verify an existing local DMG instead of downloading
  --output-dir <dir>           Download directory (default: OS temp/dybur-release-smoke)
  --expected-sha256 <hash>     Fail if downloaded file hash differs
  --min-size-mb <n>            Minimum expected size in MB (default: ${DEFAULT_MIN_SIZE_MB})
  --skip-macos-checks          Skip hdiutil/codesign/spctl checks
  --require-macos-checks       Fail unless hdiutil/codesign/spctl checks run and pass
  --json                       Print machine-readable summary
`);
}

function requireValue(args, optionName) {
  const value = args.shift();
  if (!value || value.startsWith('--')) {
    throw new Error(`${optionName} requires a value`);
  }
  return value;
}

function parsePositiveNumber(value, optionName) {
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) {
    throw new Error(`${optionName} must be a positive number`);
  }
  return parsed;
}

function parseArgs(argv) {
  const options = {
    repo: DEFAULT_REPO,
    assetName: DEFAULT_ASSET_NAME,
    inputFile: null,
    outputDir: path.join(os.tmpdir(), 'dybur-release-smoke'),
    expectedSha256: null,
    minSizeMb: DEFAULT_MIN_SIZE_MB,
    skipMacosChecks: false,
    requireMacosChecks: false,
    json: false,
  };
  const args = [...argv];

  while (args.length > 0) {
    const arg = args.shift();
    switch (arg) {
      case '--repo':
        options.repo = requireValue(args, '--repo');
        break;
      case '--asset':
        options.assetName = requireValue(args, '--asset');
        break;
      case '--input-file':
        options.inputFile = requireValue(args, '--input-file');
        break;
      case '--output-dir':
        options.outputDir = requireValue(args, '--output-dir');
        break;
      case '--expected-sha256':
        options.expectedSha256 = requireValue(args, '--expected-sha256').toLowerCase();
        break;
      case '--min-size-mb':
        options.minSizeMb = parsePositiveNumber(
          requireValue(args, '--min-size-mb'),
          '--min-size-mb'
        );
        break;
      case '--skip-macos-checks':
        options.skipMacosChecks = true;
        break;
      case '--require-macos-checks':
        options.requireMacosChecks = true;
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

  if (options.skipMacosChecks && options.requireMacosChecks) {
    throw new Error('--skip-macos-checks cannot be combined with --require-macos-checks');
  }

  return options;
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

async function downloadFile({ assetName, outputDir, repo }) {
  await fs.promises.mkdir(outputDir, { recursive: true });

  const downloadUrl = `https://github.com/${repo}/releases/latest/download/${assetName}`;
  const downloadPath = path.join(outputDir, assetName);
  await fs.promises.rm(downloadPath, { force: true });

  const response = await fetchWithRetry(downloadUrl, {
    headers: {
      'User-Agent': 'dybur-macos-release-verifier',
    },
    redirect: 'follow',
  });

  if (!response.ok || !response.body) {
    throw new Error(`${downloadUrl} returned ${response.status}`);
  }

  const bytes = Buffer.from(await response.arrayBuffer());
  await fs.promises.writeFile(downloadPath, bytes);

  return { downloadPath, downloadUrl };
}

async function resolveArtifact(options) {
  if (options.inputFile) {
    const downloadPath = path.resolve(options.inputFile);
    await fs.promises.access(downloadPath, fs.constants.R_OK);
    return {
      source: 'input-file',
      downloadPath,
      downloadUrl: null,
    };
  }

  return {
    source: 'github-release',
    ...(await downloadFile(options)),
  };
}

async function sha256File(filePath) {
  const hash = createHash('sha256');
  const stream = fs.createReadStream(filePath);

  for await (const chunk of stream) {
    hash.update(chunk);
  }

  return hash.digest('hex');
}

async function commandExists(command) {
  const probe = process.platform === 'win32' ? 'where' : 'command';
  const args = process.platform === 'win32' ? [command] : ['-v', command];

  try {
    await execFileAsync(probe, args);
    return true;
  } catch {
    return false;
  }
}

async function runCommand(command, args, options = {}) {
  const result = {
    command: `${command} ${args.join(' ')}`,
    ok: false,
    stdout: '',
    stderr: '',
  };

  try {
    const output = await execFileAsync(command, args, {
      ...options,
      timeout: 120000,
    });
    result.ok = true;
    result.stdout = output.stdout.trim();
    result.stderr = output.stderr.trim();
  } catch (error) {
    result.stdout = error.stdout?.trim?.() ?? '';
    result.stderr = error.stderr?.trim?.() ?? error.message;
  }

  return result;
}

async function findAppBundle(mountPoint) {
  const entries = await fs.promises.readdir(mountPoint, { withFileTypes: true });
  const app = entries.find((entry) => entry.isDirectory() && entry.name.endsWith('.app'));
  return app ? path.join(mountPoint, app.name) : null;
}

async function runMacosChecks(downloadPath, { requireMacosChecks, skipMacosChecks }) {
  const checks = [];
  const warnings = [];
  const issues = [];

  if (skipMacosChecks) {
    warnings.push('macOS mount/codesign/Gatekeeper checks skipped by flag');
    return { checks, warnings, issues };
  }

  if (process.platform !== 'darwin') {
    const message = 'macOS mount/codesign/Gatekeeper checks require macOS';
    if (requireMacosChecks) {
      issues.push(message);
    } else {
      warnings.push(message);
    }
    return { checks, warnings, issues };
  }

  for (const command of ['hdiutil', 'codesign', 'spctl']) {
    if (!(await commandExists(command))) {
      issues.push(`${command} is required for macOS release checks`);
      return { checks, warnings, issues };
    }
  }

  const mountRoot = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'dybur-dmg-'));
  let mounted = false;

  try {
    const attach = await runCommand('hdiutil', [
      'attach',
      '-nobrowse',
      '-readonly',
      '-mountpoint',
      mountRoot,
      downloadPath,
    ]);
    checks.push({ name: 'hdiutil attach', ...attach });
    if (!attach.ok) {
      issues.push(`hdiutil attach failed: ${attach.stderr}`);
      return { checks, warnings, issues };
    }
    mounted = true;

    const appPath = await findAppBundle(mountRoot);
    if (!appPath) {
      issues.push('No .app bundle found in mounted DMG');
      return { checks, warnings, issues };
    }

    const codesign = await runCommand('codesign', [
      '--verify',
      '--deep',
      '--strict',
      '--verbose=2',
      appPath,
    ]);
    checks.push({ name: 'codesign verify', ...codesign });
    if (!codesign.ok) {
      issues.push(`codesign verification failed: ${codesign.stderr}`);
    }

    const spctl = await runCommand('spctl', [
      '--assess',
      '--type',
      'execute',
      '--verbose',
      appPath,
    ]);
    checks.push({ name: 'spctl assess', ...spctl });
    if (!spctl.ok) {
      issues.push(`Gatekeeper assessment failed: ${spctl.stderr}`);
    }
  } finally {
    if (mounted) {
      checks.push({
        name: 'hdiutil detach',
        ...(await runCommand('hdiutil', ['detach', mountRoot])),
      });
    }
    await fs.promises.rm(mountRoot, { recursive: true, force: true });
  }

  return { checks, warnings, issues };
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const { source, downloadPath, downloadUrl } = await resolveArtifact(options);
  const file = await fs.promises.stat(downloadPath);
  const hash = await sha256File(downloadPath);
  const issues = [];
  const warnings = [];

  if (file.size < options.minSizeMb * 1024 * 1024) {
    issues.push(`${options.assetName} is unexpectedly small: ${file.size} bytes`);
  }

  if (options.expectedSha256 && hash !== options.expectedSha256) {
    issues.push(
      `${options.assetName} SHA-256 mismatch: expected ${options.expectedSha256}, got ${hash}`
    );
  }

  const macos = await runMacosChecks(downloadPath, options);
  issues.push(...macos.issues);
  warnings.push(...macos.warnings);

  const summary = {
    ok: issues.length === 0,
    source,
    repo: options.repo,
    assetName: options.assetName,
    downloadUrl,
    downloadPath,
    sizeBytes: file.size,
    sizeMB: Math.round((file.size / (1024 * 1024)) * 10) / 10,
    sha256: hash,
    platform: process.platform,
    checks: macos.checks,
    warnings,
    issues,
  };

  if (options.json) {
    process.stdout.write(`${JSON.stringify(summary, null, 2)}\n`);
  } else {
    process.stdout.write(`macOS release asset: ${summary.assetName}\n`);
    process.stdout.write(
      `${source === 'input-file' ? 'File' : 'Downloaded'}: ${summary.downloadPath}\n`
    );
    process.stdout.write(`Size: ${summary.sizeMB} MB\n`);
    process.stdout.write(`SHA-256: ${summary.sha256}\n`);

    if (summary.checks.length > 0) {
      process.stdout.write('\nChecks:\n');
      for (const check of summary.checks) {
        process.stdout.write(`  - ${check.name}: ${check.ok ? 'OK' : 'FAIL'}\n`);
      }
    }

    if (warnings.length > 0) {
      process.stdout.write('\nWarnings:\n');
      for (const warning of warnings) {
        process.stdout.write(`  - ${warning}\n`);
      }
    }

    if (issues.length > 0) {
      process.stdout.write('\nIssues:\n');
      for (const issue of issues) {
        process.stdout.write(`  - ${issue}\n`);
      }
    }
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
