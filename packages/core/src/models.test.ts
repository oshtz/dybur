import assert from 'node:assert/strict';
import { describe, it } from 'node:test';

import { getAvailableModels, getModelDefinition } from './models.js';

describe('model registry visibility', () => {
  it('keeps Parakeet v2 as an explicit legacy model but out of normal availability', () => {
    const legacy = getModelDefinition('parakeet-tdt-v2-int8');
    const availableIds = getAvailableModels().map((model) => model.id);

    assert.equal(legacy?.visibility, 'legacy');
    assert.ok(!availableIds.includes('parakeet-tdt-v2-int8'));
    assert.ok(availableIds.includes('parakeet-tdt-v3-int8'));
  });
});
