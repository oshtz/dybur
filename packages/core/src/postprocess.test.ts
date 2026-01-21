/**
 * Tests for post-processing pipeline
 */

import { describe, it } from 'node:test';
import assert from 'node:assert';
import {
  trimWhitespace,
  normalizeWhitespace,
  capitalizeSentence,
  applySentenceCase,
  addBasicPunctuation,
  postProcess,
} from './postprocess.js';

describe('trimWhitespace', () => {
  it('removes leading whitespace', () => {
    assert.strictEqual(trimWhitespace('  hello'), 'hello');
  });

  it('removes trailing whitespace', () => {
    assert.strictEqual(trimWhitespace('hello  '), 'hello');
  });

  it('removes both leading and trailing whitespace', () => {
    assert.strictEqual(trimWhitespace('  hello  '), 'hello');
  });

  it('handles empty strings', () => {
    assert.strictEqual(trimWhitespace(''), '');
  });

  it('handles strings with only whitespace', () => {
    assert.strictEqual(trimWhitespace('   '), '');
  });
});

describe('normalizeWhitespace', () => {
  it('collapses multiple spaces', () => {
    assert.strictEqual(normalizeWhitespace('hello    world'), 'hello world');
  });

  it('normalizes Windows line endings', () => {
    assert.strictEqual(normalizeWhitespace('hello\r\nworld'), 'hello\nworld');
  });

  it('normalizes old Mac line endings', () => {
    assert.strictEqual(normalizeWhitespace('hello\rworld'), 'hello\nworld');
  });

  it('collapses excessive newlines', () => {
    assert.strictEqual(normalizeWhitespace('hello\n\n\n\nworld'), 'hello\n\nworld');
  });

  it('collapses tabs', () => {
    assert.strictEqual(normalizeWhitespace('hello\t\tworld'), 'hello world');
  });
});

describe('capitalizeSentence', () => {
  it('capitalizes first letter', () => {
    assert.strictEqual(capitalizeSentence('hello'), 'Hello');
  });

  it('preserves already capitalized', () => {
    assert.strictEqual(capitalizeSentence('Hello'), 'Hello');
  });

  it('handles empty strings', () => {
    assert.strictEqual(capitalizeSentence(''), '');
  });

  it('handles single character', () => {
    assert.strictEqual(capitalizeSentence('a'), 'A');
  });
});

describe('applySentenceCase', () => {
  it('capitalizes first letter', () => {
    assert.strictEqual(applySentenceCase('hello world'), 'Hello world');
  });

  it('capitalizes after periods', () => {
    assert.strictEqual(applySentenceCase('hello. world'), 'Hello. World');
  });

  it('capitalizes after exclamation marks', () => {
    assert.strictEqual(applySentenceCase('hello! world'), 'Hello! World');
  });

  it('capitalizes after question marks', () => {
    assert.strictEqual(applySentenceCase('hello? world'), 'Hello? World');
  });

  it('handles multiple sentences', () => {
    const input = 'first sentence. second sentence. third one';
    const expected = 'First sentence. Second sentence. Third one';
    assert.strictEqual(applySentenceCase(input), expected);
  });

  it('handles empty strings', () => {
    assert.strictEqual(applySentenceCase(''), '');
  });
});

describe('addBasicPunctuation', () => {
  it('adds period if missing', () => {
    assert.strictEqual(addBasicPunctuation('hello world'), 'hello world.');
  });

  it('preserves existing period', () => {
    assert.strictEqual(addBasicPunctuation('hello world.'), 'hello world.');
  });

  it('preserves exclamation mark', () => {
    assert.strictEqual(addBasicPunctuation('hello world!'), 'hello world!');
  });

  it('preserves question mark', () => {
    assert.strictEqual(addBasicPunctuation('hello world?'), 'hello world?');
  });

  it('preserves comma', () => {
    assert.strictEqual(addBasicPunctuation('hello,'), 'hello,');
  });

  it('handles empty strings', () => {
    assert.strictEqual(addBasicPunctuation(''), '');
  });

  it('handles trailing whitespace', () => {
    assert.strictEqual(addBasicPunctuation('hello world  '), 'hello world.');
  });
});

describe('postProcess', () => {
  it('applies full pipeline with all options enabled', () => {
    const input = '  hello world  ';
    const options = { sentenceCase: true, autoPunctuation: true };
    assert.strictEqual(postProcess(input, options), 'Hello world.');
  });

  it('skips sentence case when disabled', () => {
    const input = 'hello world';
    const options = { sentenceCase: false, autoPunctuation: true };
    assert.strictEqual(postProcess(input, options), 'hello world.');
  });

  it('skips punctuation when disabled', () => {
    const input = 'hello world';
    const options = { sentenceCase: true, autoPunctuation: false };
    assert.strictEqual(postProcess(input, options), 'Hello world');
  });

  it('handles empty input', () => {
    const options = { sentenceCase: true, autoPunctuation: true };
    assert.strictEqual(postProcess('', options), '');
    assert.strictEqual(postProcess('   ', options), '');
  });

  it('processes multiple sentences correctly', () => {
    const input = '  first sentence. second sentence  ';
    const options = { sentenceCase: true, autoPunctuation: true };
    assert.strictEqual(postProcess(input, options), 'First sentence. Second sentence.');
  });

  it('normalizes whitespace in all cases', () => {
    const input = 'hello    world\r\n';
    const options = { sentenceCase: false, autoPunctuation: false };
    assert.strictEqual(postProcess(input, options), 'hello world');
  });
});
