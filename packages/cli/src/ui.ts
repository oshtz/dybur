/**
 * CLI UI utilities for dybur
 * Provides styled, branded output for a polished experience
 */

import * as readline from 'readline';

// ANSI color codes
const colors = {
  reset: '\x1b[0m',
  bold: '\x1b[1m',
  dim: '\x1b[2m',
  italic: '\x1b[3m',
  underline: '\x1b[4m',

  // Foreground
  black: '\x1b[30m',
  red: '\x1b[31m',
  green: '\x1b[32m',
  yellow: '\x1b[33m',
  blue: '\x1b[34m',
  magenta: '\x1b[35m',
  cyan: '\x1b[36m',
  white: '\x1b[37m',
  gray: '\x1b[90m',

  // Bright
  brightRed: '\x1b[91m',
  brightGreen: '\x1b[92m',
  brightYellow: '\x1b[93m',
  brightBlue: '\x1b[94m',
  brightMagenta: '\x1b[95m',
  brightCyan: '\x1b[96m',
  brightWhite: '\x1b[97m',

  // Background
  bgBlue: '\x1b[44m',
  bgMagenta: '\x1b[45m',
  bgCyan: '\x1b[46m',

  // dybur brand colors (RGB true color)
  accent: '\x1b[38;2;94;255;192m', // #5effc0 - mint green accent
  textPrimary: '\x1b[38;2;201;255;232m', // #c9ffe8 - light mint text
};

// Check if colors are supported
const supportsColor = process.stdout.isTTY && !process.env['NO_COLOR'];

function c(color: keyof typeof colors, text: string): string {
  if (!supportsColor) return text;
  return `${colors[color]}${text}${colors.reset}`;
}

function bold(text: string): string {
  return c('bold', text);
}

function dim(text: string): string {
  return c('dim', text);
}

function green(text: string): string {
  // Use brand accent color instead of green for consistent branding
  return c('accent', text);
}

function red(text: string): string {
  return c('red', text);
}

function yellow(text: string): string {
  return c('yellow', text);
}

function cyan(text: string): string {
  // Use brand accent color instead of cyan for consistent branding
  return c('accent', text);
}

function magenta(text: string): string {
  return c('magenta', text);
}

function gray(text: string): string {
  return c('gray', text);
}

function blue(text: string): string {
  // Use brand accent color instead of blue for consistent branding
  return c('accent', text);
}

/**
 * dybur brand colors
 * Primary: #c9ffe8 (light mint - text)
 * Accent: #5effc0 (mint green - highlights, CTAs)
 */
export const brand = {
  primary: (text: string) => c('textPrimary', text),
  accent: (text: string) => c('accent', text),
  success: (text: string) => c('accent', text), // Use brand accent for success
  error: red,
  warning: yellow,
  info: (text: string) => c('textPrimary', text),
  muted: gray,
  highlight: bold,
};

/**
 * ASCII art logo
 */
export const LOGO = `
${brand.primary('┌─────────────────────────────────────┐')}
${brand.primary('│')}  ${brand.accent('dybur')} ${dim('- local voice dictation')}     ${brand.primary('│')}
${brand.primary('└─────────────────────────────────────┘')}
`;

/**
 * Compact logo for inline use
 */
export const LOGO_INLINE = `${brand.accent('dybur')}`;

/**
 * Status icons - using dybur brand colors and Unicode symbols
 */
export const icons = {
  success: c('accent', '✓'), // U+2713 Check Mark
  error: red('✗'), // U+2717 Ballot X
  warning: yellow('⚠'), // U+26A0 Warning Sign
  info: c('textPrimary', '✳'), // U+2733 Eight Spoked Asterisk
  arrow: c('accent', '→'), // U+2192 Rightwards Arrow
  bullet: dim('•'), // U+2022 Bullet
  recording: red('●'), // U+25CF Black Circle
  idle: dim('○'), // U+25CB White Circle
  spinner: ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'], // Braille pattern dots
};

/**
 * Box drawing utilities
 */
export const box = {
  topLeft: '┌',
  topRight: '┐',
  bottomLeft: '└',
  bottomRight: '┘',
  horizontal: '─',
  vertical: '│',
  teeRight: '├',
  teeLeft: '┤',
};

/**
 * Print a styled header
 */
export function header(title: string, subtitle?: string): void {
  console.log('');
  console.log(`  ${brand.accent('▸')} ${bold(title)}`);
  if (subtitle) {
    console.log(`    ${dim(subtitle)}`);
  }
  console.log('');
}

/**
 * Print a section divider
 */
export function divider(char = '─', width = 40): void {
  console.log(dim(char.repeat(width)));
}

/**
 * Print a key-value pair
 */
export function keyValue(key: string, value: string, indent = 2): void {
  const spaces = ' '.repeat(indent);
  console.log(`${spaces}${dim(key + ':')} ${value}`);
}

/**
 * Print a list item
 */
export function listItem(text: string, indent = 2): void {
  const spaces = ' '.repeat(indent);
  console.log(`${spaces}${icons.bullet} ${text}`);
}

/**
 * Print a success message
 */
export function success(message: string): void {
  console.log(`  ${icons.success} ${message}`);
}

/**
 * Print an error message
 */
export function error(message: string): void {
  console.log(`  ${icons.error} ${red(message)}`);
}

/**
 * Print a warning message
 */
export function warning(message: string): void {
  console.log(`  ${icons.warning} ${yellow(message)}`);
}

/**
 * Print an info message
 */
export function info(message: string): void {
  console.log(`  ${icons.info} ${message}`);
}

/**
 * Print a command example
 */
export function command(cmd: string, description?: string): void {
  if (description) {
    console.log(`  ${c('accent', cmd)}  ${dim(description)}`);
  } else {
    console.log(`  ${c('accent', cmd)}`);
  }
}

/**
 * Create a simple table
 */
export function table(rows: [string, string][], indent = 2): void {
  const maxKeyLen = Math.max(...rows.map(([k]) => k.length));
  const spaces = ' '.repeat(indent);

  for (const [key, value] of rows) {
    const paddedKey = key.padEnd(maxKeyLen);
    console.log(`${spaces}${dim(paddedKey)}  ${value}`);
  }
}

/**
 * Progress bar
 */
export function progressBar(current: number, total: number, width = 30): string {
  const rawPercent = total > 0 ? current / total : 0;
  const percent = Math.min(Math.max(rawPercent, 0), 1);
  const filled = Math.round(width * percent);
  const empty = width - filled;

  const bar = brand.primary('█'.repeat(filled)) + dim('░'.repeat(empty));
  const percentStr = `${Math.round(percent * 100)}%`.padStart(4);

  return `${bar} ${percentStr}`;
}

/**
 * Spinner for async operations
 */
export class Spinner {
  private frame = 0;
  private interval: ReturnType<typeof setInterval> | null = null;
  private message: string;

  constructor(message: string) {
    this.message = message;
  }

  start(): void {
    if (!supportsColor) {
      console.log(`  ${this.message}...`);
      return;
    }

    process.stdout.write(`  ${c('accent', icons.spinner[0]!)} ${this.message}`);
    this.interval = setInterval(() => {
      this.frame = (this.frame + 1) % icons.spinner.length;
      process.stdout.write(`\r  ${c('accent', icons.spinner[this.frame]!)} ${this.message}`);
    }, 80);
  }

  stop(finalMessage?: string): void {
    if (this.interval) {
      clearInterval(this.interval);
      this.interval = null;
    }
    if (supportsColor) {
      process.stdout.write('\r' + ' '.repeat(this.message.length + 10) + '\r');
    }
    if (finalMessage) {
      console.log(`  ${finalMessage}`);
    }
  }

  succeed(message?: string): void {
    this.stop(`${icons.success} ${message ?? this.message}`);
  }

  fail(message?: string): void {
    this.stop(`${icons.error} ${red(message ?? this.message)}`);
  }
}

/**
 * Format file size
 */
export function formatSize(bytes: number): string {
  if (bytes === 0) return dim('0 B');
  const units = ['B', 'KB', 'MB', 'GB'];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const value = (bytes / Math.pow(k, i)).toFixed(1);
  return `${value} ${dim(units[i] ?? 'GB')}`;
}

/**
 * Format a path for display (truncate if too long)
 */
export function formatPath(path: string, maxLen = 50): string {
  if (path.length <= maxLen) return dim(path);
  return dim('...' + path.slice(-(maxLen - 3)));
}

/**
 * Print the welcome banner
 */
export function banner(): void {
  console.log(LOGO);
}

/**
 * Print a boxed message
 */
export function boxMessage(lines: string[], title?: string): void {
  const maxLen = Math.max(...lines.map((l) => l.length), title?.length ?? 0);
  const width = maxLen + 4;

  console.log('');
  console.log(`  ${brand.primary(box.topLeft + box.horizontal.repeat(width) + box.topRight)}`);

  if (title) {
    console.log(
      `  ${brand.primary(box.vertical)} ${bold(title.padEnd(maxLen + 2))} ${brand.primary(box.vertical)}`
    );
    console.log(`  ${brand.primary(box.teeRight + box.horizontal.repeat(width) + box.teeLeft)}`);
  }

  for (const line of lines) {
    console.log(
      `  ${brand.primary(box.vertical)} ${line.padEnd(maxLen + 2)} ${brand.primary(box.vertical)}`
    );
  }

  console.log(
    `  ${brand.primary(box.bottomLeft + box.horizontal.repeat(width) + box.bottomRight)}`
  );
  console.log('');
}

/**
 * Strip ANSI escape codes from a string for accurate length calculation
 */
function stripAnsi(str: string): string {
  // eslint-disable-next-line no-control-regex
  return str.replace(/\x1b\[[0-9;]*m/g, '');
}

/**
 * Get the visual width of a character (accounting for wide characters)
 */
function getCharWidth(char: string): number {
  const code = char.codePointAt(0);
  if (code === undefined) return 0;

  // Control characters have zero width
  if (code < 32 || (code >= 0x7f && code < 0xa0)) return 0;

  // Common wide character ranges (CJK, emoji, etc.)
  if (
    (code >= 0x1100 && code <= 0x115f) || // Hangul Jamo
    (code >= 0x2e80 && code <= 0xa4cf && code !== 0x303f) || // CJK
    (code >= 0xac00 && code <= 0xd7a3) || // Hangul syllables
    (code >= 0xf900 && code <= 0xfaff) || // CJK compatibility
    (code >= 0xfe10 && code <= 0xfe1f) || // Vertical forms
    (code >= 0xfe30 && code <= 0xfe6f) || // CJK compatibility forms
    (code >= 0xff00 && code <= 0xff60) || // Fullwidth forms
    (code >= 0xffe0 && code <= 0xffe6) || // Fullwidth forms
    (code >= 0x1f300 && code <= 0x1f9ff) || // Emojis
    (code >= 0x20000 && code <= 0x2fffd) || // CJK extension
    (code >= 0x30000 && code <= 0x3fffd) // CJK extension
  ) {
    return 2;
  }

  return 1;
}

/**
 * Get the visual width of a string (accounting for wide characters and ANSI codes)
 */
function getStringWidth(str: string): number {
  const plain = stripAnsi(str);
  let width = 0;
  for (const char of plain) {
    width += getCharWidth(char);
  }
  return width;
}

/**
 * Calculate how many visual lines a string takes in the terminal
 */
function getVisualLineCount(str: string, columns: number): number {
  if (columns <= 0) return 1;
  const width = getStringWidth(str);
  return Math.max(1, Math.ceil(width / columns));
}

/**
 * Interactive select menu with arrow key navigation
 */
export async function select<T>(options: {
  message: string;
  choices: Array<{ label: string; value: T; hint?: string }>;
  initial?: number;
}): Promise<T | null> {
  const { message, choices, initial = 0 } = options;

  // Check if we can use interactive mode
  // On Windows, isTTY may be false even in interactive terminals, so we try to enable raw mode
  const canUseRawMode = process.stdin.setRawMode !== undefined;
  if (!process.stdin.isTTY && !canUseRawMode) {
    // Fallback for non-TTY: just return first choice
    return choices[0]?.value ?? null;
  }

  return new Promise((resolve) => {
    let selectedIndex = initial;
    let rendered = false;
    let cleanedUp = false;
    let lastVisualLines = 0;

    // Enable raw mode for keypress detection
    readline.emitKeypressEvents(process.stdin);
    if (process.stdin.setRawMode) {
      process.stdin.setRawMode(true);
    }
    process.stdin.resume();

    // Track if we should ignore input (for flushing buffered keypresses)
    let ignoreInput = true;

    const render = () => {
      const columns = process.stdout.columns || 80;

      // Move cursor up to redraw (except first render)
      if (rendered && lastVisualLines > 0) {
        // Move up by actual visual lines rendered last time
        process.stdout.write(`\x1b[${lastVisualLines}A\x1b[0J`);
      }
      rendered = true;

      // Build all lines first to calculate visual line count
      const lines: string[] = [];
      lines.push(`  ${brand.accent('?')} ${bold(message)}`);

      for (let i = 0; i < choices.length; i++) {
        const choice = choices[i]!;
        const isSelected = i === selectedIndex;
        const cursor = isSelected ? brand.accent('❯') : ' ';
        const label = isSelected ? brand.accent(choice.label) : choice.label;
        const hint = choice.hint ? dim(` (${choice.hint})`) : '';
        lines.push(`  ${cursor} ${label}${hint}`);
      }

      // Calculate total visual lines for next clear
      lastVisualLines = lines.reduce((sum, line) => sum + getVisualLineCount(line, columns), 0);

      // Output all lines
      for (const line of lines) {
        console.log(line);
      }
    };

    const cleanup = () => {
      if (cleanedUp) return;
      cleanedUp = true;
      process.stdin.removeListener('keypress', onKeypress);
      if (process.stdin.setRawMode) {
        process.stdin.setRawMode(false);
      }
      process.stdin.pause();
      console.log('');
    };

    const onKeypress = (_str: string, key: { name: string; ctrl?: boolean }) => {
      // Always allow Ctrl+C
      if (key.ctrl && key.name === 'c') {
        cleanup();
        process.exit(0);
      }

      // Ignore buffered input during initial flush period
      if (ignoreInput) {
        return;
      }

      if (key.name === 'up' || key.name === 'k') {
        selectedIndex = selectedIndex > 0 ? selectedIndex - 1 : choices.length - 1;
        render();
      } else if (key.name === 'down' || key.name === 'j') {
        selectedIndex = selectedIndex < choices.length - 1 ? selectedIndex + 1 : 0;
        render();
      } else if (key.name === 'return') {
        cleanup();
        resolve(choices[selectedIndex]?.value ?? null);
      } else if (key.name === 'escape' || key.name === 'q') {
        cleanup();
        resolve(null);
      }
    };

    // Attach keypress listener first to catch any buffered events
    process.stdin.on('keypress', onKeypress);

    // Initial render
    render();

    // After a short delay, start accepting input
    // This flushes any buffered keypresses (like Enter from running the command)
    setTimeout(() => {
      ignoreInput = false;
    }, 50);
  });
}

/**
 * Simple confirmation prompt
 */
export async function confirm(message: string, defaultValue = false): Promise<boolean> {
  if (!process.stdin.isTTY) {
    return defaultValue;
  }

  return new Promise((resolve) => {
    const rl = readline.createInterface({
      input: process.stdin,
      output: process.stdout,
    });

    const hint = defaultValue ? 'Y/n' : 'y/N';
    rl.question(
      `  ${brand.accent('?')} ${bold(message)} ${dim(`(${hint})`)} `,
      (answer: string) => {
        rl.close();
        const normalized = answer.toLowerCase().trim();
        if (normalized === '') {
          resolve(defaultValue);
        } else {
          resolve(normalized === 'y' || normalized === 'yes');
        }
      }
    );
  });
}

export { bold, dim, green, red, yellow, cyan, magenta, gray, blue };
