/**
 * Doctor command - runs diagnostics
 */

import { existsSync } from 'fs';
import { exec } from 'child_process';
import { promisify } from 'util';
import {
  loadConfig,
  validateConfig,
  getConfigPath,
  getAllPaths,
  isMacOS,
  isWindows,
} from '@dybur/config';
import {
  isDefaultModelInstalled,
  getModelMetadata,
  DEFAULT_MODEL,
  getLogFilePath,
} from '@dybur/core';
import {
  header,
  success,
  warning,
  error,
  divider,
  dim,
  green,
  red,
  yellow,
  Spinner,
} from '../ui.js';

const execAsync = promisify(exec);

interface DiagnosticResult {
  name: string;
  status: 'pass' | 'warn' | 'fail';
  message: string;
  details?: string;
}

/**
 * Check configuration validity
 */
function checkConfig(): DiagnosticResult {
  const configPath = getConfigPath();

  if (!existsSync(configPath)) {
    return {
      name: 'Configuration',
      status: 'warn',
      message: 'Config file not found',
      details: `Will be created at: ${configPath}`,
    };
  }

  try {
    const config = loadConfig();
    const validation = validateConfig(config);

    if (!validation.valid) {
      const errors = validation.errors.map((e) => `${e.field}: ${e.message}`).join(', ');
      return {
        name: 'Configuration',
        status: 'warn',
        message: 'Config has validation warnings',
        details: errors,
      };
    }

    return {
      name: 'Configuration',
      status: 'pass',
      message: 'Valid configuration',
      details: `Hotkey: ${config.hotkey}`,
    };
  } catch (err) {
    return {
      name: 'Configuration',
      status: 'fail',
      message: 'Failed to load config',
      details: String(err),
    };
  }
}

/**
 * Check model installation
 */
function checkModel(): DiagnosticResult {
  if (!isDefaultModelInstalled()) {
    return {
      name: 'Speech Model',
      status: 'fail',
      message: `Model not installed`,
      details: `Run: dybur models prefetch`,
    };
  }

  const metadata = getModelMetadata(DEFAULT_MODEL);

  if (!metadata) {
    return {
      name: 'Speech Model',
      status: 'warn',
      message: 'Model installed but metadata missing',
    };
  }

  return {
    name: 'Speech Model',
    status: 'pass',
    message: DEFAULT_MODEL,
    details: `${metadata.variant ?? 'full'} variant, downloaded ${metadata.downloadedAt.split('T')[0]}`,
  };
}

/**
 * Check audio device availability
 */
async function checkAudioDevice(): Promise<DiagnosticResult> {
  try {
    if (isWindows()) {
      const { stdout } = await execAsync(
        'powershell -Command "Get-WmiObject Win32_SoundDevice | Select-Object Name"'
      );
      if (stdout.toLowerCase().includes('microphone') || stdout.toLowerCase().includes('audio')) {
        return {
          name: 'Audio Device',
          status: 'pass',
          message: 'Audio device detected',
        };
      }
    } else if (isMacOS()) {
      const { stdout } = await execAsync('system_profiler SPAudioDataType 2>/dev/null | head -20');
      if (stdout.length > 0) {
        return {
          name: 'Audio Device',
          status: 'pass',
          message: 'Audio device detected',
        };
      }
    }

    return {
      name: 'Audio Device',
      status: 'warn',
      message: 'Unable to verify audio device',
      details: 'Manual verification required',
    };
  } catch {
    return {
      name: 'Audio Device',
      status: 'warn',
      message: 'Unable to check audio devices',
    };
  }
}

/**
 * Check hotkey configuration
 */
function checkHotkey(): DiagnosticResult {
  const config = loadConfig();
  const validation = validateConfig({ hotkey: config.hotkey });

  if (!validation.valid) {
    return {
      name: 'Hotkey',
      status: 'fail',
      message: 'Invalid hotkey configuration',
      details: validation.errors[0]?.message,
    };
  }

  return {
    name: 'Hotkey',
    status: 'pass',
    message: config.hotkey,
    details: 'Full test requires running service',
  };
}

/**
 * Check input device configuration
 */
function checkInputDevice(): DiagnosticResult {
  const config = loadConfig();
  const inputDevice = config.inputDevice;

  if (!inputDevice) {
    return {
      name: 'Input Device',
      status: 'pass',
      message: 'Using system default',
      details: 'Run "dybur devices list" to see available devices',
    };
  }

  return {
    name: 'Input Device',
    status: 'pass',
    message: inputDevice,
    details: 'Device availability verified at recording time',
  };
}

/**
 * Check directories and permissions
 */
function checkDirectories(): DiagnosticResult {
  const paths = getAllPaths();
  const issues: string[] = [];

  if (!existsSync(paths.configDir)) {
    issues.push('Config directory missing');
  }

  if (!existsSync(paths.dataDir)) {
    issues.push('Data directory missing');
  }

  if (issues.length > 0) {
    return {
      name: 'Directories',
      status: 'warn',
      message: 'Some directories missing',
      details: 'Will be created on first use',
    };
  }

  return {
    name: 'Directories',
    status: 'pass',
    message: 'All directories accessible',
  };
}

/**
 * Format diagnostic result for output
 */
function formatResult(result: DiagnosticResult): void {
  const statusIcons = {
    pass: green('●'),
    warn: yellow('●'),
    fail: red('●'),
  };

  const statusColors = {
    pass: green,
    warn: yellow,
    fail: red,
  };

  const icon = statusIcons[result.status];
  const colorFn = statusColors[result.status];

  console.log(`  ${icon} ${dim(result.name)}`);
  console.log(`    ${colorFn(result.message)}`);

  if (result.details) {
    console.log(`    ${dim(result.details)}`);
  }
}

export async function doctorCommand(_args: string[]): Promise<void> {
  header('dybur Diagnostics');

  const spinner = new Spinner('Running checks');
  spinner.start();

  const results: DiagnosticResult[] = [];

  // Run all checks
  results.push(checkConfig());
  results.push(checkModel());
  results.push(await checkAudioDevice());
  results.push(checkHotkey());
  results.push(checkInputDevice());
  results.push(checkDirectories());

  spinner.stop();

  // Print results
  for (const result of results) {
    formatResult(result);
    console.log('');
  }

  divider();
  console.log('');

  // Summary
  const passed = results.filter((r) => r.status === 'pass').length;
  const warnings = results.filter((r) => r.status === 'warn').length;
  const failed = results.filter((r) => r.status === 'fail').length;

  console.log(
    `  ${green('●')} ${passed} passed  ` +
      `${yellow('●')} ${warnings} warnings  ` +
      `${red('●')} ${failed} failed`
  );
  console.log('');

  if (failed > 0) {
    error('Some checks failed - see details above');
    process.exit(1);
  } else if (warnings > 0) {
    warning('All critical checks passed with warnings');
  } else {
    success('All checks passed - dybur is ready');
  }

  console.log('');
  console.log(`  ${dim('Log file:')} ${getLogFilePath()}`);
  console.log('');
}
