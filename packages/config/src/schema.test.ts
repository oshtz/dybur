/**
 * Tests for config schema validation
 */

import { describe, it } from 'node:test';
import assert from 'node:assert';
import { DEFAULT_CONFIG, validateConfig, mergeWithDefaults, type DyburConfig } from './schema.js';

describe('DEFAULT_CONFIG', () => {
  it('has all required fields', () => {
    assert.strictEqual(typeof DEFAULT_CONFIG.hotkey, 'string');
    assert.strictEqual(typeof DEFAULT_CONFIG.autoPunctuation, 'boolean');
    assert.strictEqual(typeof DEFAULT_CONFIG.sentenceCase, 'boolean');
    assert.strictEqual(typeof DEFAULT_CONFIG.silenceTimeoutMs, 'number');
    assert.strictEqual(typeof DEFAULT_CONFIG.model, 'string');
    assert.strictEqual(typeof DEFAULT_CONFIG.clipboardCleanup, 'boolean');
    assert.strictEqual(typeof DEFAULT_CONFIG.vadEnabled, 'boolean');
    assert.strictEqual(typeof DEFAULT_CONFIG.vadThreshold, 'number');
    assert.strictEqual(typeof DEFAULT_CONFIG.vadMinSpeechMs, 'number');
    assert.strictEqual(typeof DEFAULT_CONFIG.gpuMode, 'string');
    assert.strictEqual(typeof DEFAULT_CONFIG.streamingEnabled, 'boolean');
  });

  it('has sensible default values', () => {
    assert.strictEqual(DEFAULT_CONFIG.hotkey, 'Ctrl+Shift+Space');
    assert.strictEqual(DEFAULT_CONFIG.autoPunctuation, true);
    assert.strictEqual(DEFAULT_CONFIG.sentenceCase, true);
    assert.strictEqual(DEFAULT_CONFIG.silenceTimeoutMs, 1000);
    assert.strictEqual(DEFAULT_CONFIG.model, 'parakeet-tdt-v3-int8');
    assert.strictEqual(DEFAULT_CONFIG.clipboardCleanup, true);
    assert.strictEqual(DEFAULT_CONFIG.vadEnabled, true);
    assert.strictEqual(DEFAULT_CONFIG.vadThreshold, 0.5);
    assert.strictEqual(DEFAULT_CONFIG.vadMinSpeechMs, 250);
    assert.strictEqual(DEFAULT_CONFIG.gpuMode, 'auto');
    assert.strictEqual(DEFAULT_CONFIG.streamingEnabled, true);
  });
});

describe('validateConfig', () => {
  describe('hotkey validation', () => {
    it('accepts valid hotkeys', () => {
      const validHotkeys = [
        'Ctrl+Space',
        'Ctrl+Shift+Space',
        'Alt+A',
        'Cmd+Shift+D',
        'Meta+F1',
        'Ctrl+Alt+Shift+X',
      ];

      for (const hotkey of validHotkeys) {
        const result = validateConfig({ hotkey });
        assert.strictEqual(result.valid, true, `Expected "${hotkey}" to be valid`);
      }
    });

    it('rejects hotkeys without modifiers', () => {
      const result = validateConfig({ hotkey: 'Space' });
      assert.strictEqual(result.valid, false);
      assert.ok(result.errors.some((e) => e.field === 'hotkey'));
    });

    it('rejects empty hotkeys', () => {
      const result = validateConfig({ hotkey: '' });
      assert.strictEqual(result.valid, false);
    });

    it('rejects invalid modifiers', () => {
      const result = validateConfig({ hotkey: 'Invalid+Space' });
      assert.strictEqual(result.valid, false);
      assert.ok(result.errors[0]?.message.includes('Invalid modifier'));
    });
  });

  describe('silenceTimeoutMs validation', () => {
    it('accepts valid timeout values', () => {
      const validValues = [0, 500, 1000, 5000, 30000];

      for (const value of validValues) {
        const result = validateConfig({ silenceTimeoutMs: value });
        assert.strictEqual(result.valid, true, `Expected ${value} to be valid`);
      }
    });

    it('rejects negative values', () => {
      const result = validateConfig({ silenceTimeoutMs: -100 });
      assert.strictEqual(result.valid, false);
    });

    it('rejects values over 30 seconds', () => {
      const result = validateConfig({ silenceTimeoutMs: 60000 });
      assert.strictEqual(result.valid, false);
    });

    it('rejects non-number values', () => {
      const result = validateConfig({ silenceTimeoutMs: '1000' as unknown as number });
      assert.strictEqual(result.valid, false);
    });
  });

  describe('boolean field validation', () => {
    it('accepts boolean values', () => {
      const result = validateConfig({
        autoPunctuation: false,
        sentenceCase: true,
        clipboardCleanup: false,
      });
      assert.strictEqual(result.valid, true);
    });

    it('rejects non-boolean values', () => {
      const result = validateConfig({
        autoPunctuation: 'true' as unknown as boolean,
      });
      assert.strictEqual(result.valid, false);
    });
  });

  describe('model validation', () => {
    it('accepts valid model names', () => {
      const result = validateConfig({ model: 'parakeet-tdt-1.0b-v4' });
      assert.strictEqual(result.valid, true);
    });

    it('rejects empty model names', () => {
      const result = validateConfig({ model: '' });
      assert.strictEqual(result.valid, false);
    });
  });

  describe('VAD validation', () => {
    it('accepts boundary VAD values', () => {
      for (const vadThreshold of [0, 0.5, 1]) {
        const result = validateConfig({ vadThreshold });
        assert.strictEqual(result.valid, true, `Expected threshold ${vadThreshold} to be valid`);
      }

      for (const vadMinSpeechMs of [0, 250, 5000]) {
        const result = validateConfig({ vadMinSpeechMs });
        assert.strictEqual(
          result.valid,
          true,
          `Expected min speech ${vadMinSpeechMs}ms to be valid`
        );
      }
    });

    it('rejects out-of-range VAD values', () => {
      assert.strictEqual(validateConfig({ vadThreshold: -0.1 }).valid, false);
      assert.strictEqual(validateConfig({ vadThreshold: 1.1 }).valid, false);
      assert.strictEqual(validateConfig({ vadMinSpeechMs: -1 }).valid, false);
      assert.strictEqual(validateConfig({ vadMinSpeechMs: 5001 }).valid, false);
    });
  });
});

describe('mergeWithDefaults', () => {
  it('returns defaults for empty config', () => {
    const result = mergeWithDefaults({});
    assert.deepStrictEqual(result, DEFAULT_CONFIG);
  });

  it('merges valid user values', () => {
    const userConfig: Partial<DyburConfig> = {
      hotkey: 'Alt+D',
      silenceTimeoutMs: 2000,
    };

    const result = mergeWithDefaults(userConfig);

    assert.strictEqual(result.hotkey, 'Alt+D');
    assert.strictEqual(result.silenceTimeoutMs, 2000);
    // Defaults preserved
    assert.strictEqual(result.autoPunctuation, DEFAULT_CONFIG.autoPunctuation);
    assert.strictEqual(result.model, DEFAULT_CONFIG.model);
  });

  it('falls back to defaults for invalid values', () => {
    const warnings: string[] = [];
    const userConfig = {
      hotkey: 'InvalidKey',
      silenceTimeoutMs: -100,
    };

    const result = mergeWithDefaults(userConfig, (field, msg) => {
      warnings.push(`${field}: ${msg}`);
    });

    // Invalid values should fall back to defaults
    assert.strictEqual(result.hotkey, DEFAULT_CONFIG.hotkey);
    assert.strictEqual(result.silenceTimeoutMs, DEFAULT_CONFIG.silenceTimeoutMs);

    // Warnings should be called
    assert.ok(warnings.length > 0);
  });

  it('preserves valid values while rejecting invalid ones', () => {
    const userConfig = {
      hotkey: 'Ctrl+D', // valid
      silenceTimeoutMs: -100, // invalid
      autoPunctuation: false, // valid
    };

    const result = mergeWithDefaults(userConfig);

    assert.strictEqual(result.hotkey, 'Ctrl+D');
    assert.strictEqual(result.silenceTimeoutMs, DEFAULT_CONFIG.silenceTimeoutMs);
    assert.strictEqual(result.autoPunctuation, false);
  });
});
