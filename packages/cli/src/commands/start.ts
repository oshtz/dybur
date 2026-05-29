/**
 * Start command - launches the background service
 */

import { spawn, execSync } from 'child_process';
import { existsSync } from 'fs';
import { join } from 'path';
import { homedir } from 'os';
import {
  isModelInstalled,
  downloadModel,
  DEFAULT_MODEL,
  downloadTrayApp,
  TRAY_APP_VERSION,
} from '@dybur/core';
import { loadConfig, getTrayAppPath, isMacOS } from '@dybur/config';
import {
  header,
  success,
  info,
  warning,
  error,
  keyValue,
  brand,
  cyan,
  dim,
  Spinner,
  progressBar,
} from '../ui.js';

/**
 * Check if the tray app is already running (macOS only)
 */
function isTrayAppRunning(): boolean {
  if (!isMacOS()) {
    return false;
  }

  try {
    const result = execSync('pgrep -x dybur', {
      encoding: 'utf-8',
      stdio: ['pipe', 'pipe', 'pipe'],
    });
    return result.trim().length > 0;
  } catch {
    // pgrep returns non-zero exit code if no process found
    return false;
  }
}

/**
 * Get the path to the tray app binary, checking development paths first
 */
function findTrayAppPath(): string | null {
  // Check development paths first
  const devPaths = [
    join(process.cwd(), 'apps', 'tray', 'src-tauri', 'target', 'release', 'dybur.exe'),
    join(process.cwd(), 'apps', 'tray', 'src-tauri', 'target', 'release', 'dybur'),
    process.env['DYBUR_TRAY_PATH'],
  ].filter(Boolean) as string[];

  for (const p of devPaths) {
    if (existsSync(p)) {
      return p;
    }
  }

  // Check installed location (~/.dybur/bin/)
  const installedPath = getTrayAppPath();
  if (existsSync(installedPath)) {
    return installedPath;
  }

  // Check macOS Applications folders
  if (isMacOS()) {
    const macOSPaths = [
      '/Applications/dybur.app/Contents/MacOS/dybur',
      join(homedir(), 'Applications', 'dybur.app', 'Contents', 'MacOS', 'dybur'),
    ];

    for (const p of macOSPaths) {
      if (existsSync(p)) {
        return p;
      }
    }
  }

  return null;
}

export async function startCommand(_args: string[]): Promise<void> {
  header('Starting dybur');

  // Load config
  const config = loadConfig();

  keyValue('Model', config.model);
  keyValue('Hotkey', brand.accent(config.hotkey));
  console.log('');

  // Check if model is installed
  const modelId = config.model ?? DEFAULT_MODEL;
  if (!isModelInstalled(modelId)) {
    warning(`Model not found: ${modelId}`);
    info('Downloading model from HuggingFace...');
    console.log(`  ${dim('This only needs to happen once')}`);
    console.log('');

    let lastFile = '';

    try {
      await downloadModel(modelId, (downloaded, total, file) => {
        if (file && file !== lastFile) {
          if (lastFile) {
            process.stdout.write('\n');
          }
          lastFile = file;
          console.log(`  ${dim('Downloading:')} ${file}`);
        }

        if (total > 0) {
          const bar = progressBar(downloaded, total);
          process.stdout.write(`\r  ${bar}`);
        }
      });

      console.log('\n');
      success('Model downloaded');
      console.log('');
    } catch (err) {
      console.log('\n');
      error(`Failed to download model: ${err}`);
      info(`Run ${cyan(`dybur models download ${modelId}`)} to try again`);
      process.exit(1);
    }
  }

  // Check if tray app is already running
  if (isTrayAppRunning()) {
    success('dybur is already running');
    console.log('');
    info(`Press ${brand.accent(config.hotkey)} to begin dictating`);
    console.log('');
    return;
  }

  // Find or download tray app
  let trayPath = findTrayAppPath();

  if (!trayPath) {
    warning('Tray application not found');
    info(`Downloading from GitHub releases (${TRAY_APP_VERSION})...`);
    console.log(`  ${dim('This only needs to happen once')}`);
    console.log('');

    try {
      trayPath = await downloadTrayApp(TRAY_APP_VERSION, (downloaded, total, status) => {
        if (status) {
          console.log(`  ${dim(status)}`);
        } else if (total > 0) {
          const bar = progressBar(downloaded, total);
          process.stdout.write(`\r  ${bar}`);
        }
      });

      console.log('\n');
      success('Tray application installed');
      console.log('');
    } catch (err) {
      console.log('\n');
      error(`Failed to download tray application: ${err}`);
      console.log('');
      info('You can try:');
      console.log(`  ${dim('1.')} Check your internet connection`);
      console.log(
        `  ${dim('2.')} Download manually from ${cyan('https://github.com/oshtz/dybur/releases')}`
      );
      console.log(`  ${dim('3.')} Build from source: ${cyan('cd apps/tray && pnpm tauri build')}`);
      process.exit(1);
    }
  }

  // Spawn tray app
  const spinner = new Spinner('Launching tray application');
  spinner.start();

  const child = spawn(trayPath, [], {
    detached: true,
    stdio: 'ignore',
  });

  child.unref();

  // Brief delay to check if process started
  await new Promise((resolve) => setTimeout(resolve, 500));

  spinner.succeed('dybur started');
  console.log('');
  info(`Press ${brand.accent(config.hotkey)} to begin dictating`);

  // Show macOS-specific note about accessibility permissions
  if (isMacOS()) {
    console.log('');
    console.log(`  ${dim('Note: You may need to grant accessibility permissions')}`);
    console.log(`  ${dim('System Settings > Privacy & Security > Accessibility')}`);
  }

  console.log('');
}
