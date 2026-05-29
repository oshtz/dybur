/**
 * VAD command - toggle Voice Activity Detection
 */

import { loadConfig, saveConfig } from '@dybur/config';
import { header, info, keyValue, brand, dim, success, error } from '../ui.js';

function parseNumber(value: string | undefined, label: string): number | null {
  if (value === undefined) {
    error(`${label} value is required`);
    return null;
  }

  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    error(`${label} must be a number`);
    return null;
  }

  return parsed;
}

function setThreshold(config: ReturnType<typeof loadConfig>, value: string | undefined): void {
  const threshold = parseNumber(value, 'Threshold');
  if (threshold === null) return;

  if (threshold < 0 || threshold > 1) {
    error('Threshold must be between 0.0 and 1.0');
    return;
  }

  config.vadThreshold = threshold;
  saveConfig(config);
  success(`VAD threshold set to ${threshold}`);
  info('Higher values are stricter and may ignore quieter speech');
}

function setMinSpeech(config: ReturnType<typeof loadConfig>, value: string | undefined): void {
  const duration = parseNumber(value, 'Minimum speech duration');
  if (duration === null) return;

  if (duration < 0 || duration > 5000) {
    error('Minimum speech duration must be between 0 and 5000ms');
    return;
  }

  config.vadMinSpeechMs = duration;
  saveConfig(config);
  success(`VAD minimum speech duration set to ${duration}ms`);
}

function setSilenceTimeout(config: ReturnType<typeof loadConfig>, value: string | undefined): void {
  const timeout = parseNumber(value, 'Silence timeout');
  if (timeout === null) return;

  if (timeout < 0 || timeout > 30000) {
    error('Silence timeout must be between 0 and 30000ms');
    return;
  }

  config.silenceTimeoutMs = timeout;
  saveConfig(config);
  success(`Silence timeout set to ${timeout}ms`);
  info('This controls how long a pause can split speech segments');
}

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

  if (subcommand === 'threshold') {
    setThreshold(config, args[1]);
    return;
  }

  if (subcommand === 'min-speech' || subcommand === 'min') {
    setMinSpeech(config, args[1]);
    return;
  }

  if (subcommand === 'silence' || subcommand === 'silence-timeout') {
    setSilenceTimeout(config, args[1]);
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
  keyValue('Silence timeout', `${config.silenceTimeoutMs}ms`);

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
  console.log(`  ${brand.accent('dybur vad threshold 0.6')}      Set speech threshold`);
  console.log(`  ${brand.accent('dybur vad min-speech 250')}     Set minimum speech duration`);
  console.log(`  ${brand.accent('dybur vad silence 1000')}       Set silence timeout`);
  console.log('');

  info('VAD (Voice Activity Detection) filters silence before transcription');
  console.log('');
}
