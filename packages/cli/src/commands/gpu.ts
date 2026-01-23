/**
 * GPU command - control GPU acceleration mode
 */

import { loadConfig, saveConfig } from '@dybur/config';
import { header, info, keyValue, brand, dim, success } from '../ui.js';

export async function gpuCommand(args: string[]): Promise<void> {
  const config = loadConfig();

  // Handle subcommands
  const subcommand = args[0]?.toLowerCase();

  if (subcommand === 'on' || subcommand === 'auto' || subcommand === 'enable') {
    config.gpuMode = 'auto';
    saveConfig(config);
    success('GPU acceleration enabled (auto-detect)');
    info('Will use DirectML (Windows) or CoreML (macOS) if available');
    return;
  }

  if (subcommand === 'off' || subcommand === 'cpu' || subcommand === 'disable') {
    config.gpuMode = 'cpu';
    saveConfig(config);
    success('GPU acceleration disabled (CPU-only mode)');
    info('All inference will run on CPU');
    return;
  }

  if (subcommand === 'status') {
    showStatus(config);
    return;
  }

  // No subcommand - toggle
  if (!subcommand) {
    config.gpuMode = config.gpuMode === 'auto' ? 'cpu' : 'auto';
    saveConfig(config);
    const status = config.gpuMode === 'auto' ? 'enabled (auto)' : 'disabled (CPU-only)';
    success(`GPU acceleration ${status}`);
    return;
  }

  // Unknown subcommand - show help
  showHelp();
}

function showStatus(config: ReturnType<typeof loadConfig>): void {
  header('GPU Acceleration');

  const isAuto = config.gpuMode === 'auto';
  keyValue('Mode', isAuto ? brand.accent('auto (GPU if available)') : dim('cpu (GPU disabled)'));

  console.log('');
  console.log(`  ${dim('Platform-specific GPU providers:')}`);
  console.log(`  ${dim('  Windows: DirectML (works with AMD, Intel, NVIDIA)')}`);
  console.log(`  ${dim('  macOS:   CoreML (Apple Silicon / Intel)')}`);
  console.log('');
  console.log(`  ${dim('GPU acceleration speeds up speech recognition.')}`);
  console.log(`  ${dim('If GPU fails, the app will automatically fall back to CPU.')}`);
  console.log('');
  info('Note: Restart the app for GPU mode changes to take effect');
  console.log('');
}

function showHelp(): void {
  header('GPU Commands');

  console.log(`  ${brand.accent('dybur gpu')}          Toggle GPU mode`);
  console.log(`  ${brand.accent('dybur gpu on')}       Enable GPU (auto-detect)`);
  console.log(`  ${brand.accent('dybur gpu off')}      Disable GPU (CPU-only)`);
  console.log(`  ${brand.accent('dybur gpu status')}   Show GPU settings`);
  console.log('');

  info('GPU acceleration uses DirectML (Windows) or CoreML (macOS)');
  console.log('');
}
