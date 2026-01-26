/**
 * VAD command - toggle Voice Activity Detection
 */

import { loadConfig, saveConfig } from '@dybur/config';
import { header, info, keyValue, brand, dim, success, error } from '../ui.js';

export async function vadCommand(args: string[]): Promise<void> {
  const config = loadConfig();

  // Handle subcommands
  const subcommand = args[0]?.toLowerCase();

  if (subcommand === 'on' || subcommand === 'enable') {
    config.vadEnabled = true;
    saveConfig(config);
    success('VAD enabled');
    info('Silence will be filtered before transcription');
    return;
  }

  if (subcommand === 'off' || subcommand === 'disable') {
    config.vadEnabled = false;
    saveConfig(config);
    success('VAD disabled');
    info('All audio will be sent to transcription');
    return;
  }

  if (subcommand === 'status') {
    showStatus(config);
    return;
  }

  // No subcommand - toggle
  if (!subcommand) {
    config.vadEnabled = !config.vadEnabled;
    saveConfig(config);
    const status = config.vadEnabled ? 'enabled' : 'disabled';
    success(`VAD ${status}`);
    return;
  }

  // Unknown subcommand - show help
  showHelp();
}

function showStatus(config: ReturnType<typeof loadConfig>): void {
  header('Voice Activity Detection');

  keyValue('Status', config.vadEnabled ? brand.accent('enabled') : dim('disabled'));
  keyValue('Threshold', `${config.vadThreshold}`);
  keyValue('Min speech duration', `${config.vadMinSpeechMs}ms`);

  console.log('');
  console.log(`  ${dim('VAD filters silence and noise before transcription.')}`);
  console.log(`  ${dim('This improves accuracy and reduces processing time.')}`);
  console.log('');
}

function showHelp(): void {
  header('VAD Commands');

  console.log(`  ${brand.accent('dybur vad')}          Toggle VAD on/off`);
  console.log(`  ${brand.accent('dybur vad on')}       Enable VAD`);
  console.log(`  ${brand.accent('dybur vad off')}      Disable VAD`);
  console.log(`  ${brand.accent('dybur vad status')}   Show VAD settings`);
  console.log('');

  info('VAD (Voice Activity Detection) filters silence before transcription');
  console.log('');
}
