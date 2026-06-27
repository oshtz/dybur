#!/usr/bin/env node
/**
 * Build script for dybur
 * Coordinates building all packages and apps
 */

import { execSync } from 'child_process';
import { existsSync } from 'fs';
import { join } from 'path';

const ROOT_DIR = join(import.meta.dirname, '..');

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

function buildPackages() {
  buildPackage('config');
  buildPackage('core');
  buildPackage('cli');
}

function buildTrayApp() {
  const trayDir = join(ROOT_DIR, 'apps', 'tray');
  if (!existsSync(join(trayDir, 'package.json'))) {
    console.log('Skipping tray app (not initialized)');
    return;
  }

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
      buildPackages();
      buildTrayApp();
      break;

    case 'packages':
      buildPackages();
      break;

    case 'tray':
      buildPackages();
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
