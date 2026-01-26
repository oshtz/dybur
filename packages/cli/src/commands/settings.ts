/**
 * Settings command - opens or shows configuration
 */

import { exec } from 'child_process';
import { existsSync } from 'fs';
import { getConfigPath, loadConfig, isMacOS, isWindows } from '@dybur/config';
import { header, info, keyValue, brand, dim, Spinner } from '../ui.js';

/**
 * Open a file in the default editor
 */
function openInEditor(filePath: string): void {
  if (isWindows()) {
    exec(`start "" "${filePath}"`);
  } else if (isMacOS()) {
    exec(`open "${filePath}"`);
  }
}

export async function settingsCommand(args: string[]): Promise<void> {
  const configPath = getConfigPath();

  // Check for --path flag
  if (args.includes('--path')) {
    console.log(configPath);
    return;
  }

  // Check for --show flag
  if (args.includes('--show')) {
    header('Current Configuration');

    const config = loadConfig();

    keyValue('Hotkey', brand.accent(config.hotkey));
    keyValue('Auto punctuation', config.autoPunctuation ? 'enabled' : 'disabled');
    keyValue('Sentence case', config.sentenceCase ? 'enabled' : 'disabled');
    keyValue('Silence timeout', `${config.silenceTimeoutMs}ms`);
    keyValue('Model', config.model);
    keyValue('Clipboard cleanup', config.clipboardCleanup ? 'enabled' : 'disabled');
    keyValue('Recording mode', config.recordingMode === 'push_to_talk' ? 'push-to-talk' : 'toggle');
    keyValue('VAD (silence filter)', config.vadEnabled ? 'enabled' : 'disabled');
    if (config.vadEnabled) {
      keyValue('  VAD threshold', `${config.vadThreshold}`);
      keyValue('  VAD min speech', `${config.vadMinSpeechMs}ms`);
    }

    console.log('');
    console.log(`  ${dim('Path:')} ${configPath}`);
    console.log('');
    return;
  }

  header('Settings');

  // Ensure config exists
  if (!existsSync(configPath)) {
    loadConfig({ createIfMissing: true });
    info('Created default config');
  }

  // Open in editor
  const spinner = new Spinner('Opening config in editor');
  spinner.start();

  openInEditor(configPath);

  spinner.succeed('Config opened');
  console.log('');
  console.log(`  ${dim('Path:')} ${configPath}`);
  console.log('');
  info('Restart dybur after making changes');
  console.log('');
}
