/**
 * @dybur/config
 * Configuration management for dybur
 */

// Schema and types
export {
  type DyburConfig,
  DEFAULT_CONFIG,
  validateConfig,
  mergeWithDefaults,
  type ValidationResult,
  type ValidationError,
} from './schema.js';

// Path utilities
export {
  getPlatform,
  isMacOS,
  isWindows,
  getArch,
  getConfigDir,
  getConfigPath,
  getDataDir,
  getModelsDir,
  getLogsDir,
  getBinDir,
  getModelPath,
  getLogFilePath,
  getTrayAppPath,
  getTrayAppBundlePath,
  getAllPaths,
  type Platform,
} from './paths.js';

// Config operations
export {
  loadConfig,
  saveConfig,
  updateConfig,
  resetConfig,
  configExists,
  type LoadConfigOptions,
  type SaveConfigOptions,
  type ConfigLogger,
} from './config.js';
