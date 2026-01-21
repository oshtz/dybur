/**
 * @dybur/cli
 * CLI interface for dybur
 */

// Re-export commands for programmatic use
export { startCommand } from './commands/start.js';
export { stopCommand } from './commands/stop.js';
export { statusCommand } from './commands/status.js';
export { settingsCommand } from './commands/settings.js';
export { doctorCommand } from './commands/doctor.js';
export { modelsCommand } from './commands/models.js';

// Re-export UI utilities for extensions
export * from './ui.js';
