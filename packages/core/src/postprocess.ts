/**
 * Post-processing pipeline for transcribed text
 * Handles sentence casing, punctuation, and normalization
 */

import type { DyburConfig } from '@dybur/config';

/**
 * Options for post-processing
 */
export interface PostProcessOptions {
  /** Capitalize first letter of sentences */
  sentenceCase: boolean;
  /** Add basic punctuation */
  autoPunctuation: boolean;
}

/**
 * Extract post-process options from config
 */
export function getPostProcessOptions(config: DyburConfig): PostProcessOptions {
  return {
    sentenceCase: config.sentenceCase,
    autoPunctuation: config.autoPunctuation,
  };
}

/**
 * Trim leading and trailing whitespace
 */
export function trimWhitespace(text: string): string {
  return text.trim();
}

/**
 * Normalize internal whitespace (collapse multiple spaces, normalize line endings)
 */
export function normalizeWhitespace(text: string): string {
  return text
    .replace(/\r\n/g, '\n') // Normalize line endings
    .replace(/\r/g, '\n')
    .replace(/[ \t]+/g, ' ') // Collapse horizontal whitespace
    .replace(/\n{3,}/g, '\n\n'); // Collapse excessive newlines
}

/**
 * Capitalize the first letter of a sentence
 */
export function capitalizeSentence(text: string): string {
  if (text.length === 0) return text;

  // Capitalize first character
  return text.charAt(0).toUpperCase() + text.slice(1);
}

/**
 * Apply sentence casing (capitalize after sentence-ending punctuation)
 */
export function applySentenceCase(text: string): string {
  if (text.length === 0) return text;

  // Pattern: sentence-ending punctuation followed by space and letter
  const sentenceEndPattern = /([.!?])\s+([a-z])/g;

  let result = capitalizeSentence(text);
  result = result.replace(sentenceEndPattern, (_, punct: string, letter: string) => {
    return `${punct} ${letter.toUpperCase()}`;
  });

  return result;
}

/**
 * Add basic punctuation (period at end if missing)
 */
export function addBasicPunctuation(text: string): string {
  if (text.length === 0) return text;

  const trimmed = text.trimEnd();

  // Check if already ends with punctuation
  const lastChar = trimmed.charAt(trimmed.length - 1);
  if (['.', '!', '?', ',', ';', ':'].includes(lastChar)) {
    return text;
  }

  // Add period
  return trimmed + '.';
}

/**
 * Full post-processing pipeline
 */
export function postProcess(text: string, options: PostProcessOptions): string {
  let result = text;

  // Step 1: Trim whitespace
  result = trimWhitespace(result);

  if (result.length === 0) {
    return result;
  }

  // Step 2: Normalize whitespace
  result = normalizeWhitespace(result);

  // Step 3: Sentence casing (if enabled)
  if (options.sentenceCase) {
    result = applySentenceCase(result);
  }

  // Step 4: Basic punctuation (if enabled)
  if (options.autoPunctuation) {
    result = addBasicPunctuation(result);
  }

  return result;
}

/**
 * Post-process with config
 */
export function postProcessWithConfig(text: string, config: DyburConfig): string {
  return postProcess(text, getPostProcessOptions(config));
}
