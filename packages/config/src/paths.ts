/**
 * OS-specific path utilities for dybur
 */

import { homedir, platform } from 'os';
import { join } from 'path';

/**
 * Supported platforms
 */
export type Platform = 'darwin' | 'win32';

/**
 * Get the current platform
 * @throws Error if running on unsupported platform
 */
export function getPlatform(): Platform {
  const p = platform();
  if (p !== 'darwin' && p !== 'win32') {
    throw new Error(`Unsupported platform: ${p}. dybur only supports macOS and Windows.`);
  }
  return p;
}

/**
 * Check if running on macOS
 */
export function isMacOS(): boolean {
  return platform() === 'darwin';
}

/**
 * Check if running on Windows
 */
export function isWindows(): boolean {
  return platform() === 'win32';
}

/**
 * Get the configuration directory path
 *
 * macOS: ~/Library/Application Support/dybur/
 * Windows: %APPDATA%\dybur\
 */
export function getConfigDir(): string {
  if (isMacOS()) {
    return join(homedir(), 'Library', 'Application Support', 'dybur');
  }

  // Windows: use APPDATA environment variable
  const appData = process.env['APPDATA'];
  if (!appData) {
    // Fallback if APPDATA is not set (unusual)
    return join(homedir(), 'AppData', 'Roaming', 'dybur');
  }

  return join(appData, 'dybur');
}

/**
 * Get the full path to the config file
 */
export function getConfigPath(): string {
  return join(getConfigDir(), 'config.json');
}

/**
 * Get the data directory path (for models, logs, etc.)
 *
 * Both platforms: ~/.dybur/
 */
export function getDataDir(): string {
  return join(homedir(), '.dybur');
}

/**
 * Get the models directory path
 */
export function getModelsDir(): string {
  return join(getDataDir(), 'models');
}

/**
 * Get the logs directory path
 */
export function getLogsDir(): string {
  return join(getDataDir(), 'logs');
}

/**
 * Get the path to a specific model
 */
export function getModelPath(modelName: string): string {
  return join(getModelsDir(), modelName);
}

/**
 * Get today's log file path
 */
export function getLogFilePath(): string {
  const today = new Date().toISOString().split('T')[0]; // YYYY-MM-DD
  return join(getLogsDir(), `dybur-${today}.log`);
}

/**
 * Get the bin directory path (for downloaded binaries like tray app)
 */
export function getBinDir(): string {
  return join(getDataDir(), 'bin');
}

/**
 * Get the architecture (arm64 or x64)
 */
export function getArch(): 'arm64' | 'x64' {
  return process.arch === 'arm64' ? 'arm64' : 'x64';
}

/**
 * Get the tray app binary path
 *
 * macOS: ~/.dybur/bin/dybur.app/Contents/MacOS/dybur
 * Windows: ~/.dybur/bin/dybur.exe
 */
export function getTrayAppPath(): string {
  if (isMacOS()) {
    return join(getBinDir(), 'dybur.app', 'Contents', 'MacOS', 'dybur');
  }
  return join(getBinDir(), 'dybur.exe');
}

/**
 * Get the tray app bundle/directory path
 *
 * macOS: ~/.dybur/bin/dybur.app
 * Windows: ~/.dybur/bin/dybur.exe
 */
export function getTrayAppBundlePath(): string {
  if (isMacOS()) {
    return join(getBinDir(), 'dybur.app');
  }
  return join(getBinDir(), 'dybur.exe');
}

/**
 * Get all paths as a structured object (useful for diagnostics)
 */
export function getAllPaths() {
  return {
    platform: platform(),
    arch: getArch(),
    configDir: getConfigDir(),
    configPath: getConfigPath(),
    dataDir: getDataDir(),
    modelsDir: getModelsDir(),
    logsDir: getLogsDir(),
    logFile: getLogFilePath(),
    binDir: getBinDir(),
    trayApp: getTrayAppPath(),
  };
}
