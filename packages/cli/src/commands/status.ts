/**
 * Status command - shows service health
 */

import { exec } from 'child_process';
import { promisify } from 'util';
import { loadConfig, getAllPaths, isMacOS, isWindows } from '@dybur/config';
import { isModelInstalled, getModelMetadata, DEFAULT_MODEL } from '@dybur/core';
import {
  header,
  success,
  info,
  warning,
  divider,
  brand,
  cyan,
  dim,
  green,
  red,
  formatPath,
} from '../ui.js';

const execAsync = promisify(exec);

/**
 * Check if tray process is running
 */
async function isTrayRunning(): Promise<boolean> {
  try {
    if (isWindows()) {
      const { stdout } = await execAsync('tasklist /FI "IMAGENAME eq dybur.exe"');
      return stdout.includes('dybur.exe');
    } else if (isMacOS()) {
      const { stdout } = await execAsync('pgrep -f dybur');
      return stdout.trim().length > 0;
    }
  } catch {
    return false;
  }

  return false;
}

function statusIcon(ok: boolean): string {
  return ok ? green('●') : red('○');
}

export async function statusCommand(_args: string[]): Promise<void> {
  header('dybur Status');

  const config = loadConfig({ createIfMissing: false });
  const activeModel = config.model ?? DEFAULT_MODEL;
  const running = await isTrayRunning();
  const modelInstalled = isModelInstalled(activeModel);
  const modelMeta = modelInstalled ? getModelMetadata(activeModel) : null;
  const paths = getAllPaths();

  // Service status
  console.log(
    `  ${statusIcon(running)} ${dim('Service:')}     ${running ? green('Running') : red('Stopped')}`
  );
  console.log(
    `  ${statusIcon(modelInstalled)} ${dim('Model:')}       ${modelInstalled ? green(activeModel) : red(`${activeModel} not installed`)}`
  );

  if (modelMeta) {
    console.log(`              ${dim('Downloaded:')} ${modelMeta.downloadedAt.split('T')[0]}`);
    if (modelMeta.variant) {
      console.log(`              ${dim('Variant:')}    ${modelMeta.variant}`);
    }
  }

  console.log('');
  divider();
  console.log('');

  // Configuration
  console.log(`  ${brand.accent('Configuration')}`);
  console.log(`  ${dim('Hotkey:')}      ${brand.accent(config.hotkey)}`);
  console.log(
    `  ${dim('Punctuation:')} ${config.autoPunctuation ? green('enabled') : dim('disabled')}`
  );
  console.log(
    `  ${dim('Sentence case:')} ${config.sentenceCase ? green('enabled') : dim('disabled')}`
  );
  console.log(`  ${dim('Silence timeout:')} ${config.silenceTimeoutMs}ms`);
  console.log(
    `  ${dim('Recording mode:')} ${config.recordingMode === 'push_to_talk' ? 'push-to-talk' : 'toggle'}`
  );

  console.log('');
  divider();
  console.log('');

  // Paths
  console.log(`  ${brand.accent('Paths')}`);
  console.log(`  ${dim('Config:')}  ${formatPath(paths.configPath, 45)}`);
  console.log(`  ${dim('Models:')}  ${formatPath(paths.modelsDir, 45)}`);
  console.log(`  ${dim('Logs:')}    ${formatPath(paths.logsDir, 45)}`);

  console.log('');
  divider();
  console.log('');

  // Overall status
  if (running && modelInstalled) {
    success(`Ready ${dim('- press')} ${brand.accent(config.hotkey)} ${dim('to dictate')}`);
  } else if (!modelInstalled) {
    warning('Model required');
    info(`Run ${cyan(`dybur models download ${activeModel}`)} to download`);
  } else {
    warning('Service not running');
    info(`Run ${cyan('dybur start')} to begin`);
  }

  console.log('');
}
