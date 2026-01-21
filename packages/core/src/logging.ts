/**
 * Logging utilities for dybur
 * Local-only logging with no speech content
 */

import { existsSync, mkdirSync, appendFileSync } from 'fs';
import { getLogsDir, getLogFilePath } from '@dybur/config';

/**
 * Log levels
 */
export type LogLevel = 'debug' | 'info' | 'warn' | 'error';

/**
 * Log entry structure
 */
export interface LogEntry {
  timestamp: string;
  level: LogLevel;
  category: string;
  message: string;
  data?: Record<string, unknown>;
}

/**
 * Logger configuration
 */
export interface LoggerConfig {
  /** Minimum level to log */
  minLevel: LogLevel;
  /** Whether to output to console */
  console: boolean;
  /** Whether to write to file */
  file: boolean;
}

const LOG_LEVEL_ORDER: Record<LogLevel, number> = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3,
};

/**
 * Default logger configuration
 */
const DEFAULT_LOGGER_CONFIG: LoggerConfig = {
  minLevel: 'info',
  console: true,
  file: true,
};

let globalConfig: LoggerConfig = { ...DEFAULT_LOGGER_CONFIG };

/**
 * Configure the global logger
 */
export function configureLogger(config: Partial<LoggerConfig>): void {
  globalConfig = { ...globalConfig, ...config };
}

/**
 * Ensure logs directory exists
 */
function ensureLogsDir(): string {
  const dir = getLogsDir();
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
  return dir;
}

/**
 * Format a log entry for output
 */
function formatLogEntry(entry: LogEntry): string {
  const { timestamp, level, category, message, data } = entry;
  const levelStr = level.toUpperCase().padEnd(5);
  const categoryStr = category ? `[${category}]` : '';

  let line = `${timestamp} ${levelStr} ${categoryStr} ${message}`;

  if (data && Object.keys(data).length > 0) {
    line += ` ${JSON.stringify(data)}`;
  }

  return line;
}

/**
 * Write a log entry
 */
function writeLog(entry: LogEntry): void {
  const levelOrder = LOG_LEVEL_ORDER[entry.level];
  const minLevelOrder = LOG_LEVEL_ORDER[globalConfig.minLevel];

  if (levelOrder < minLevelOrder) {
    return;
  }

  const formatted = formatLogEntry(entry);

  // Console output
  if (globalConfig.console) {
    switch (entry.level) {
      case 'debug':
        console.debug(formatted);
        break;
      case 'info':
        console.info(formatted);
        break;
      case 'warn':
        console.warn(formatted);
        break;
      case 'error':
        console.error(formatted);
        break;
    }
  }

  // File output
  if (globalConfig.file) {
    try {
      ensureLogsDir();
      const logFile = getLogFilePath();
      appendFileSync(logFile, formatted + '\n');
    } catch {
      // Silently fail file logging to avoid cascading errors
    }
  }
}

/**
 * Create a log entry
 */
function createLogEntry(
  level: LogLevel,
  category: string,
  message: string,
  data?: Record<string, unknown>
): LogEntry {
  return {
    timestamp: new Date().toISOString(),
    level,
    category,
    message,
    data,
  };
}

/**
 * Create a logger for a specific category
 */
export function createLogger(category: string) {
  return {
    debug: (message: string, data?: Record<string, unknown>) => {
      writeLog(createLogEntry('debug', category, message, data));
    },
    info: (message: string, data?: Record<string, unknown>) => {
      writeLog(createLogEntry('info', category, message, data));
    },
    warn: (message: string, data?: Record<string, unknown>) => {
      writeLog(createLogEntry('warn', category, message, data));
    },
    error: (message: string, data?: Record<string, unknown>) => {
      writeLog(createLogEntry('error', category, message, data));
    },
  };
}

/**
 * Pre-defined loggers for common categories
 */
export const loggers = {
  service: createLogger('service'),
  model: createLogger('model'),
  hotkey: createLogger('hotkey'),
  audio: createLogger('audio'),
  injection: createLogger('injection'),
  config: createLogger('config'),
};

/**
 * Get the current log file path
 */
export { getLogFilePath, getLogsDir };
