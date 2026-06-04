import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { MODEL_CANDIDATES, getModelCandidate, getModelCandidates } from './model-candidates.js';

describe('MODEL_CANDIDATES', () => {
  it('uses unique candidate ids', () => {
    const ids = MODEL_CANDIDATES.map((candidate) => candidate.id);
    assert.equal(new Set(ids).size, ids.length);
  });

  it('keeps deferred models out of the default candidate list', () => {
    const candidates = getModelCandidates();
    assert.ok(candidates.length > 0);
    assert.ok(candidates.every((candidate) => candidate.recommendation !== 'defer'));
  });

  it('includes the recommended CoreML Parakeet spike', () => {
    const candidate = getModelCandidate('parakeet-tdt-v3-coreml');
    assert.equal(candidate?.recommendation, 'recommended');
    assert.ok(candidate?.platforms.includes('darwin-arm64'));
  });
});
