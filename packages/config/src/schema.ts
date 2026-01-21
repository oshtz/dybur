/**
 * Configuration schema for dybur
 * Single source of truth for all config options
 */

/**
 * dybur configuration schema
 */
export interface DyburConfig {
  /**
   * Global hotkey to trigger dictation
   * Format: "Modifier+Key" (e.g., "Ctrl+Shift+Space")
   * @default "Ctrl+Shift+Space"
   */
  hotkey: string;

  /**
   * Automatically insert punctuation (periods at pauses)
   * @default true
   */
  autoPunctuation: boolean;

  /**
   * Capitalize first letter of sentences
   * @default true
   */
  sentenceCase: boolean;

  /**
   * Silence duration in milliseconds before auto-stopping recording
   * Set to 0 to disable auto-stop
   * @default 1000
   */
  silenceTimeoutMs: number;

  /**
   * Speech recognition model to use
   * @default "parakeet-tdt-0.6b-v3-onnx"
   */
  model: string;

  /**
   * Restore original clipboard content after text injection
   * @default true
   */
  clipboardCleanup: boolean;

  /**
   * Input device (microphone) name to use for recording
   * Set to null or undefined to use system default
   * @default null
   */
  inputDevice: string | null;
}

/**
 * Default configuration values
 */
export const DEFAULT_CONFIG: DyburConfig = {
  hotkey: 'Ctrl+Shift+Space',
  autoPunctuation: true,
  sentenceCase: true,
  silenceTimeoutMs: 1000,
  model: 'parakeet-tdt-0.6b-v3-onnx',
  clipboardCleanup: true,
  inputDevice: null,
};

/**
 * Validation result type
 */
export interface ValidationResult {
  valid: boolean;
  errors: ValidationError[];
}

export interface ValidationError {
  field: string;
  message: string;
  value?: unknown;
}

/**
 * Valid hotkey modifiers
 */
const VALID_MODIFIERS = ['Ctrl', 'Alt', 'Shift', 'Meta', 'Cmd', 'Win'] as const;

/**
 * Validate hotkey format
 */
function validateHotkey(hotkey: string): ValidationError[] {
  const errors: ValidationError[] = [];

  if (typeof hotkey !== 'string' || hotkey.trim().length === 0) {
    errors.push({
      field: 'hotkey',
      message: 'Hotkey must be a non-empty string',
      value: hotkey,
    });
    return errors;
  }

  const parts = hotkey.split('+').map((p) => p.trim());

  if (parts.length < 2) {
    errors.push({
      field: 'hotkey',
      message: 'Hotkey must include at least one modifier and a key (e.g., "Ctrl+Space")',
      value: hotkey,
    });
    return errors;
  }

  // Check that all but the last part are valid modifiers
  const modifiers = parts.slice(0, -1);
  const key = parts[parts.length - 1];

  for (const mod of modifiers) {
    if (!VALID_MODIFIERS.includes(mod as (typeof VALID_MODIFIERS)[number])) {
      errors.push({
        field: 'hotkey',
        message: `Invalid modifier "${mod}". Valid modifiers: ${VALID_MODIFIERS.join(', ')}`,
        value: hotkey,
      });
    }
  }

  if (!key || key.length === 0) {
    errors.push({
      field: 'hotkey',
      message: 'Hotkey must end with a key (e.g., "Space", "A", "F1")',
      value: hotkey,
    });
  }

  return errors;
}

/**
 * Validate a partial config object
 */
export function validateConfig(config: Partial<DyburConfig>): ValidationResult {
  const errors: ValidationError[] = [];

  // Validate hotkey if present
  if (config.hotkey !== undefined) {
    errors.push(...validateHotkey(config.hotkey));
  }

  // Validate autoPunctuation
  if (config.autoPunctuation !== undefined && typeof config.autoPunctuation !== 'boolean') {
    errors.push({
      field: 'autoPunctuation',
      message: 'autoPunctuation must be a boolean',
      value: config.autoPunctuation,
    });
  }

  // Validate sentenceCase
  if (config.sentenceCase !== undefined && typeof config.sentenceCase !== 'boolean') {
    errors.push({
      field: 'sentenceCase',
      message: 'sentenceCase must be a boolean',
      value: config.sentenceCase,
    });
  }

  // Validate silenceTimeoutMs
  if (config.silenceTimeoutMs !== undefined) {
    if (typeof config.silenceTimeoutMs !== 'number') {
      errors.push({
        field: 'silenceTimeoutMs',
        message: 'silenceTimeoutMs must be a number',
        value: config.silenceTimeoutMs,
      });
    } else if (config.silenceTimeoutMs < 0) {
      errors.push({
        field: 'silenceTimeoutMs',
        message: 'silenceTimeoutMs must be >= 0',
        value: config.silenceTimeoutMs,
      });
    } else if (config.silenceTimeoutMs > 30000) {
      errors.push({
        field: 'silenceTimeoutMs',
        message: 'silenceTimeoutMs must be <= 30000 (30 seconds)',
        value: config.silenceTimeoutMs,
      });
    }
  }

  // Validate model
  if (config.model !== undefined) {
    if (typeof config.model !== 'string' || config.model.trim().length === 0) {
      errors.push({
        field: 'model',
        message: 'model must be a non-empty string',
        value: config.model,
      });
    }
  }

  // Validate clipboardCleanup
  if (config.clipboardCleanup !== undefined && typeof config.clipboardCleanup !== 'boolean') {
    errors.push({
      field: 'clipboardCleanup',
      message: 'clipboardCleanup must be a boolean',
      value: config.clipboardCleanup,
    });
  }

  // Validate inputDevice
  if (config.inputDevice !== undefined && config.inputDevice !== null) {
    if (typeof config.inputDevice !== 'string') {
      errors.push({
        field: 'inputDevice',
        message: 'inputDevice must be a string or null',
        value: config.inputDevice,
      });
    } else if (config.inputDevice.trim().length === 0) {
      errors.push({
        field: 'inputDevice',
        message: 'inputDevice must be a non-empty string or null',
        value: config.inputDevice,
      });
    }
  }

  return {
    valid: errors.length === 0,
    errors,
  };
}

/**
 * Merge user config with defaults, validating and falling back as needed
 */
export function mergeWithDefaults(
  userConfig: Partial<DyburConfig>,
  onWarning?: (field: string, message: string) => void
): DyburConfig {
  const result = { ...DEFAULT_CONFIG };
  const validation = validateConfig(userConfig);

  // Create a set of invalid fields for quick lookup
  const invalidFields = new Set(validation.errors.map((e) => e.field));

  // Merge valid fields only
  for (const key of Object.keys(userConfig) as (keyof DyburConfig)[]) {
    if (!invalidFields.has(key) && userConfig[key] !== undefined) {
      // TypeScript needs help here due to the generic nature
      (result as Record<string, unknown>)[key] = userConfig[key];
    }
  }

  // Report warnings for invalid fields
  if (onWarning) {
    for (const error of validation.errors) {
      onWarning(error.field, `${error.message}. Using default value.`);
    }
  }

  return result;
}
