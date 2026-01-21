#!/usr/bin/env node
/**
 * Build script for dybur
 * Coordinates building all packages and apps
 */

import { execSync } from 'child_process';
import { existsSync } from 'fs';
import { join } from 'path';

const ROOT_DIR = join(import.meta.dirname, '..');
const IS_WINDOWS = process.platform === 'win32';

function run(command, cwd = ROOT_DIR) {
  console.log(`\n> ${command}`);
  execSync(command, { cwd, stdio: 'inherit' });
}

function buildPackage(name) {
  const pkgDir = join(ROOT_DIR, 'packages', name);
  if (!existsSync(pkgDir)) {
    console.log(`Skipping ${name} (not found)`);
    return;
  }

  console.log(`\nBuilding packages/${name}...`);
  run('pnpm build', pkgDir);
}

function buildSidecar() {
  console.log('\nBuilding CLI sidecar...');
  const scriptsDir = join(ROOT_DIR, 'scripts');

  if (IS_WINDOWS) {
    const ps1Script = join(scriptsDir, 'build-sidecar.ps1');
    if (existsSync(ps1Script)) {
      run(`powershell -ExecutionPolicy Bypass -File "${ps1Script}"`, scriptsDir);
    } else {
      console.log('Warning: build-sidecar.ps1 not found, skipping sidecar build');
    }
  } else {
    const shScript = join(scriptsDir, 'build-sidecar.sh');
    if (existsSync(shScript)) {
      run(`bash "${shScript}"`, scriptsDir);
    } else {
      console.log('Warning: build-sidecar.sh not found, skipping sidecar build');
    }
  }
}

function buildTrayApp() {
  const trayDir = join(ROOT_DIR, 'apps', 'tray');
  if (!existsSync(join(trayDir, 'package.json'))) {
    console.log('Skipping tray app (not initialized)');
    return;
  }

  // Build CLI sidecar first
  buildSidecar();

  console.log('\nBuilding apps/tray...');
  run('pnpm build', trayDir);
}

async function main() {
  const args = process.argv.slice(2);
  const target = args[0] || 'all';

  console.log('=== dybur Build ===');
  console.log(`Target: ${target}`);

  switch (target) {
    case 'all':
      buildPackage('config');
      buildPackage('core');
      buildPackage('cli');
      buildTrayApp();
      break;

    case 'packages':
      buildPackage('config');
      buildPackage('core');
      buildPackage('cli');
      break;

    case 'tray':
      buildTrayApp();
      break;

    default:
      buildPackage(target);
  }

  console.log('\n=== Build Complete ===');
}

main().catch((error) => {
  console.error('Build failed:', error.message);
  process.exit(1);
});
