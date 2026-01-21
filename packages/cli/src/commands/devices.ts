/**
 * Devices command - manage input devices (microphones)
 */

import { exec } from 'child_process';
import { promisify } from 'util';
import { loadConfig, updateConfig, isMacOS, isWindows } from '@dybur/config';
import {
  header,
  success,
  info,
  error,
  warning,
  command,
  divider,
  brand,
  cyan,
  dim,
  green,
  yellow,
  icons,
  select,
} from '../ui.js';

const execAsync = promisify(exec);

interface AudioDevice {
  name: string;
  isDefault: boolean;
}

/**
 * List audio input devices using platform-specific commands
 */
async function listAudioDevices(): Promise<AudioDevice[]> {
  const devices: AudioDevice[] = [];

  try {
    if (isWindows()) {
      // Use PowerShell to query audio devices
      const { stdout } = await execAsync(
        `powershell -Command "Get-CimInstance Win32_SoundDevice | Where-Object { $_.Status -eq 'OK' } | Select-Object -ExpandProperty Name"`,
        { timeout: 10000 }
      );

      const lines = stdout
        .split('\n')
        .map((l) => l.trim())
        .filter(Boolean);

      // Also try to get capture devices specifically
      try {
        const { stdout: captureOutput } = await execAsync(
          `powershell -Command "[System.Reflection.Assembly]::LoadWithPartialName('System.Speech') | Out-Null; $recognizer = New-Object System.Speech.Recognition.SpeechRecognizer; $recognizer.AudioDeviceNames | ForEach-Object { Write-Output $_ }; $recognizer.Dispose()"`,
          { timeout: 10000 }
        );
        const captureLines = captureOutput
          .split('\n')
          .map((l) => l.trim())
          .filter(Boolean);
        if (captureLines.length > 0) {
          lines.length = 0;
          lines.push(...captureLines);
        }
      } catch {
        // Fall back to Win32_SoundDevice results
      }

      // Try another method using Windows.Devices.Enumeration through PowerShell
      try {
        const { stdout: inputDevices } = await execAsync(
          `powershell -Command "$audioDevices = Get-WmiObject Win32_PnPEntity | Where-Object { $_.Caption -match 'microphone|audio|input' -and $_.Status -eq 'OK' }; $audioDevices | Select-Object -ExpandProperty Caption"`,
          { timeout: 10000 }
        );
        const inputLines = inputDevices
          .split('\n')
          .map((l) => l.trim())
          .filter(Boolean);
        if (inputLines.length > 0) {
          // Use these instead if found
          lines.length = 0;
          lines.push(...inputLines);
        }
      } catch {
        // Fall back to previous results
      }

      // Deduplicate and mark first as default
      const seen = new Set<string>();
      for (let i = 0; i < lines.length; i++) {
        const name = lines[i]!;
        if (!seen.has(name)) {
          seen.add(name);
          devices.push({
            name,
            isDefault: i === 0 && devices.length === 0,
          });
        }
      }
    } else if (isMacOS()) {
      // Use system_profiler to get audio devices on macOS
      const { stdout } = await execAsync('system_profiler SPAudioDataType -json 2>/dev/null', {
        timeout: 10000,
      });

      try {
        const data = JSON.parse(stdout);
        const audioData = data?.SPAudioDataType?.[0]?._items;
        if (Array.isArray(audioData)) {
          for (let i = 0; i < audioData.length; i++) {
            const device = audioData[i];
            if (device?._name && device?.coreaudio_input_source) {
              devices.push({
                name: device._name,
                isDefault: i === 0,
              });
            }
          }
        }
      } catch {
        // Try alternative method
        const { stdout: altOutput } = await execAsync(
          'system_profiler SPAudioDataType 2>/dev/null | grep "Input Source:"',
          { timeout: 10000 }
        );
        const matches = altOutput.match(/Input Source:\s*(.+)/g);
        if (matches) {
          for (let i = 0; i < matches.length; i++) {
            const name = matches[i]!.replace('Input Source:', '').trim();
            if (name) {
              devices.push({
                name,
                isDefault: i === 0,
              });
            }
          }
        }
      }
    }
  } catch (err) {
    // Unable to query devices
  }

  return devices;
}

/**
 * Show devices help
 */
function showDevicesHelp(): void {
  header('Input Device Management');

  console.log(`  ${dim('Configure which microphone to use for voice dictation.')}`);
  console.log(`  ${dim('Set to null/default to use system default microphone.')}`);
  console.log('');

  divider();
  console.log('');

  console.log(`  ${brand.accent('Commands')}`);
  command('d, d l, d list', 'Select input device interactively');
  command('d set <name>', 'Select a specific microphone');
  command('d reset', 'Reset to system default');
  console.log('');

  console.log(`  ${brand.accent('Examples')}`);
  console.log(`  ${cyan('dybur d')}           ${dim('Interactive device selection')}`);
  console.log(`  ${cyan('dybur d l')}         ${dim('Same as above')}`);
  console.log(`  ${cyan('dybur d set "Mic"')} ${dim('Set device by name')}`);
  console.log(`  ${cyan('dybur d reset')}     ${dim('Use system default')}`);
  console.log('');
}

/**
 * List and select input devices interactively
 */
async function listCommand(): Promise<void> {
  header('Input Devices');

  const config = loadConfig();
  const currentDevice = config.inputDevice;

  console.log(
    `  ${dim('Current:')} ${currentDevice ? cyan(currentDevice) : dim('System default')}`
  );
  console.log('');

  const devices = await listAudioDevices();

  if (devices.length === 0) {
    warning('Could not enumerate audio devices');
    console.log('');
    console.log(`  ${dim('To set a device manually, use:')}`);
    console.log(`  ${cyan('dybur d set "Device Name"')}`);
    console.log('');
    console.log(`  ${dim('Note: The exact device name must match what the system sees.')}`);
    console.log(`  ${dim('You can find device names in your system sound settings.')}`);
    console.log('');
    return;
  }

  // Build choices for interactive selection
  const choices = [
    {
      label: 'System default',
      value: null as string | null,
      hint: 'use OS default microphone',
    },
    ...devices.map((device) => ({
      label: device.name,
      value: device.name as string | null,
      hint: device.isDefault ? 'system default' : undefined,
    })),
  ];

  // Find current selection index
  const currentIndex = currentDevice
    ? choices.findIndex((c) => c.value === currentDevice)
    : 0;

  const selected = await select({
    message: 'Select input device',
    choices,
    initial: currentIndex >= 0 ? currentIndex : 0,
  });

  // User cancelled
  if (selected === undefined) {
    info('Selection cancelled');
    console.log('');
    return;
  }

  // Apply selection
  if (selected === null) {
    // Reset to system default
    updateConfig({ inputDevice: null });
    success('Input device reset to system default');
  } else {
    updateConfig({ inputDevice: selected });
    success(`Input device set to: ${cyan(selected)}`);
  }

  console.log('');
  info('Changes will take effect on the next recording');
  console.log('');

  console.log(`  ${yellow(icons.warning)} ${dim('If the service is running, restart it:')}`);
  console.log(`    ${cyan('dybur stop && dybur start')}`);
  console.log('');
}

/**
 * Set the input device
 */
async function setCommand(deviceName: string): Promise<void> {
  header('Set Input Device');

  if (!deviceName || deviceName.trim().length === 0) {
    error('Device name is required');
    console.log('');
    console.log(`  ${dim('Usage:')} ${cyan('dybur devices set "<device name>"')}`);
    console.log('');
    console.log(`  ${dim('Example:')} ${cyan('dybur devices set "Microphone (Realtek)"')}`);
    console.log('');
    process.exit(1);
  }

  // Clean up the device name (remove surrounding quotes if present)
  const cleanName = deviceName.replace(/^["']|["']$/g, '').trim();

  try {
    updateConfig({ inputDevice: cleanName });

    success(`Input device set to: ${cyan(cleanName)}`);
    console.log('');
    info('Changes will take effect on the next recording');
    console.log('');

    // Warn if service is running
    console.log(`  ${yellow(icons.warning)} ${dim('If the service is running, restart it:')}`);
    console.log(`    ${cyan('dybur stop && dybur start')}`);
    console.log('');
  } catch (err) {
    error(`Failed to update configuration: ${err}`);
    process.exit(1);
  }
}

/**
 * Reset to system default
 */
async function resetCommand(): Promise<void> {
  header('Reset Input Device');

  try {
    updateConfig({ inputDevice: null });

    success('Input device reset to system default');
    console.log('');
    info('Changes will take effect on the next recording');
    console.log('');
  } catch (err) {
    error(`Failed to update configuration: ${err}`);
    process.exit(1);
  }
}

export async function devicesCommand(args: string[]): Promise<void> {
  const subcommand = args[0];

  switch (subcommand) {
    case 'list':
    case 'l':
    case undefined:
      await listCommand();
      break;

    case 'set':
    case 's':
      // Join remaining args in case device name has spaces and wasn't quoted
      const deviceName = args.slice(1).join(' ');
      await setCommand(deviceName);
      break;

    case 'reset':
    case 'default':
    case 'r':
      await resetCommand();
      break;

    case '--help':
    case '-h':
    case 'help':
    case 'h':
      showDevicesHelp();
      break;

    default:
      error(`Unknown subcommand: ${subcommand}`);
      console.log('');
      showDevicesHelp();
      process.exit(1);
  }
}
