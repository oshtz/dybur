/**
 * Configuration loading, saving, and management for dybur
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'fs';
import { dirname } from 'path';
import { type DyburConfig, DEFAULT_CONFIG, validateConfig, mergeWithDefaults } from './schema.js';
import { getConfigPath, getConfigDir } from './paths.js';

/**
 * Logger interface for config operations
 */
export interface ConfigLogger {
  warn: (message: string) => void;
  error: (message: string) => void;
  debug: (message: string) => void;
}

/**
 * Default console logger
 */
const defaultLogger: ConfigLogger = {
  warn: (msg) => console.warn(`[config] ${msg}`),
  error: (msg) => console.error(`[config] ${msg}`),
  debug: () => {}, // No-op by default
};

/**
 * Options for loading config
 */
export interface LoadConfigOptions {
  /** Custom path to config file */
  path?: string;
  /** Logger for warnings/errors */
  logger?: ConfigLogger;
  /** Create config file if it doesn't exist */
  createIfMissing?: boolean;
}

/**
 * Load configuration from disk, merging with defaults
 */
export function loadConfig(options: LoadConfigOptions = {}): DyburConfig {
  const { path = getConfigPath(), logger = defaultLogger, createIfMissing = true } = options;

  // If config doesn't exist, return defaults (and optionally create file)
  if (!existsSync(path)) {
    logger.debug(`Config file not found at ${path}`);

    if (createIfMissing) {
      try {
        saveConfig(DEFAULT_CONFIG, { path, logger });
        logger.debug(`Created default config at ${path}`);
      } catch (error) {
        logger.warn(`Failed to create default config: ${error}`);
      }
    }

    return { ...DEFAULT_CONFIG };
  }

  // Read and parse config file
  let userConfig: Partial<DyburConfig>;
  try {
    const content = readFileSync(path, 'utf-8');
    userConfig = JSON.parse(content) as Partial<DyburConfig>;
  } catch (error) {
    if (error instanceof SyntaxError) {
      logger.error(`Invalid JSON in config file: ${error.message}`);
    } else {
      logger.error(`Failed to read config file: ${error}`);
    }
    logger.warn('Using default configuration');
    return { ...DEFAULT_CONFIG };
  }

  // Validate and merge with defaults
  const config = mergeWithDefaults(userConfig, (field, message) => {
    logger.warn(`Config validation: ${field} - ${message}`);
  });

  return config;
}

/**
 * Options for saving config
 */
export interface SaveConfigOptions {
  /** Custom path to config file */
  path?: string;
  /** Logger for warnings/errors */
  logger?: ConfigLogger;
}

/**
 * Save configuration to disk
 */
export function saveConfig(config: DyburConfig, options: SaveConfigOptions = {}): void {
  const { path = getConfigPath(), logger = defaultLogger } = options;

  // Validate before saving
  const validation = validateConfig(config);
  if (!validation.valid) {
    const errorMessages = validation.errors.map((e) => `${e.field}: ${e.message}`).join(', ');
    throw new Error(`Cannot save invalid config: ${errorMessages}`);
  }

  // Ensure directory exists
  const dir = dirname(path);
  if (!existsSync(dir)) {
    try {
      mkdirSync(dir, { recursive: true });
      logger.debug(`Created config directory: ${dir}`);
    } catch (error) {
      throw new Error(`Failed to create config directory: ${error}`);
    }
  }

  // Write config file
  try {
    const content = JSON.stringify(config, null, 2);
    writeFileSync(path, content, 'utf-8');
    logger.debug(`Saved config to ${path}`);
  } catch (error) {
    throw new Error(`Failed to write config file: ${error}`);
  }
}

/**
 * Update specific config values, merging with existing config
 */
export function updateConfig(
  updates: Partial<DyburConfig>,
  options: LoadConfigOptions & SaveConfigOptions = {}
): DyburConfig {
  const { logger = defaultLogger } = options;

  // Load existing config
  const currentConfig = loadConfig(options);

  // Validate updates
  const validation = validateConfig(updates);
  if (!validation.valid) {
    for (const error of validation.errors) {
      logger.warn(`Ignoring invalid update for ${error.field}: ${error.message}`);
    }
  }

  // Merge updates with current config (only valid fields)
  const invalidFields = new Set(validation.errors.map((e) => e.field));
  const validUpdates: Partial<DyburConfig> = {};

  for (const key of Object.keys(updates) as (keyof DyburConfig)[]) {
    if (!invalidFields.has(key)) {
      (validUpdates as Record<string, unknown>)[key] = updates[key];
    }
  }

  const newConfig: DyburConfig = { ...currentConfig, ...validUpdates };

  // Save updated config
  saveConfig(newConfig, options);

  return newConfig;
}

/**
 * Reset config to defaults
 */
export function resetConfig(options: SaveConfigOptions = {}): DyburConfig {
  const config = { ...DEFAULT_CONFIG };
  saveConfig(config, options);
  return config;
}

/**
 * Check if config file exists
 */
export function configExists(path?: string): boolean {
  return existsSync(path ?? getConfigPath());
}

/**
 * Get the config directory path
 */
export { getConfigDir, getConfigPath };
