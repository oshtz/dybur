/**
 * dybur CLI entry point
 */

import { parseArgs } from 'node:util';
import { startCommand } from './commands/start.js';
import { stopCommand } from './commands/stop.js';
import { statusCommand } from './commands/status.js';
import { settingsCommand } from './commands/settings.js';
import { doctorCommand } from './commands/doctor.js';
import { modelsCommand } from './commands/models.js';
import { devicesCommand } from './commands/devices.js';
import { vadCommand } from './commands/vad.js';
import { gpuCommand } from './commands/gpu.js';
import { banner, brand, header, command, info, error, dim, cyan, boxMessage } from './ui.js';

const VERSION = '1.0.0';

function showHelp(): void {
  banner();

  console.log(`  ${dim('Local voice dictation for macOS & Windows')}`);
  console.log(`  ${dim('Powered by')} ${cyan('NVIDIA Parakeet')} ${dim('- 100% offline')}`);
  console.log('');

  header('Commands');
  command('start', 'Start the background service');
  command('stop', 'Stop the background service');
  command('status, s', 'Show service status & health');
  command('settings, config', 'Open configuration file');
  command('doctor, diag', 'Run diagnostics');
  command('models, m', 'Manage speech models');
  command('devices, d', 'Manage input devices');
  command('vad', 'Toggle Voice Activity Detection');
  command('gpu', 'Toggle GPU acceleration');
  console.log('');

  header('Model Commands');
  command('models list', 'List installed models');
  command('models prefetch', 'Download default model');
  command('models clean', 'Remove unused models');
  console.log('');

  header('Device Commands');
  command('d, d l', 'List & select microphone interactively');
  command('d set <name>', 'Select a specific microphone');
  command('d reset', 'Reset to system default');
  console.log('');

  header('Options');
  command('-h, --help', 'Show this help message');
  command('-v, --version', 'Show version number');
  console.log('');

  header('Quick Start');
  info(`Run ${cyan('dybur start')} to begin`);
  info(`Press ${brand.accent('Ctrl+Shift+Space')} to dictate`);
  console.log('');

  console.log(`  ${dim('Docs:')} ${cyan('https://github.com/oshtz/dybur')}`);
  console.log('');
}

function showVersion(): void {
  boxMessage(
    [
      `Version: ${brand.accent(VERSION)}`,
      `Platform: ${process.platform}`,
      `Node: ${process.version}`,
    ],
    'dybur'
  );
}

async function main() {
  const args = process.argv.slice(2);

  // Parse global options
  const { values, positionals } = parseArgs({
    args,
    options: {
      help: { type: 'boolean', short: 'h' },
      version: { type: 'boolean', short: 'v' },
    },
    allowPositionals: true,
    strict: false,
  });

  // Handle global flags
  if (values.version) {
    showVersion();
    return;
  }

  if (values.help || positionals.length === 0) {
    showHelp();
    return;
  }

  // Route to command
  const cmd = positionals[0];
  const commandArgs = positionals.slice(1);

  try {
    switch (cmd) {
      case 'start':
        await startCommand(commandArgs);
        break;

      case 'stop':
        await stopCommand(commandArgs);
        break;

      case 'status':
      case 's':
        await statusCommand(commandArgs);
        break;

      case 'settings':
      case 'config':
        await settingsCommand(commandArgs);
        break;

      case 'doctor':
      case 'diag':
        await doctorCommand(commandArgs);
        break;

      case 'models':
      case 'm':
        await modelsCommand(commandArgs);
        break;

      case 'devices':
      case 'd':
        await devicesCommand(commandArgs);
        break;

      case 'vad':
        await vadCommand(commandArgs);
        break;

      case 'gpu':
        await gpuCommand(commandArgs);
        break;

      default:
        error(`Unknown command: ${cmd}`);
        console.log('');
        info(`Run ${cyan('dybur --help')} for usage information`);
        process.exit(1);
    }
  } catch (err) {
    console.log('');
    if (err instanceof Error) {
      error(err.message);
    } else {
      error('An unexpected error occurred');
    }
    process.exit(1);
  }
}

main();
