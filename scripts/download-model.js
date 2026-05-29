#!/usr/bin/env node
/**
 * Model download utility for dybur.
 *
 * This script intentionally delegates to @dybur/core so model URLs, file lists,
 * metadata, and cleanup behavior stay in one place.
 */

async function loadCore() {
  try {
    return await import('../packages/core/dist/index.js');
  } catch (error) {
    throw new Error(
      "Unable to load packages/core/dist. Run 'pnpm build' before using scripts/download-model.js."
    );
  }
}

let lastProgressLineLength = 0;

function writeProgress(downloaded, total, status) {
  const parts = [];

  if (status) {
    parts.push(status);
  }

  if (total > 0) {
    const percent = Math.round((downloaded / total) * 100);
    const downloadedMb = (downloaded / 1024 / 1024).toFixed(1);
    const totalMb = (total / 1024 / 1024).toFixed(1);
    parts.push(`${percent}% (${downloadedMb} / ${totalMb} MB)`);
  } else if (downloaded > 0) {
    parts.push(`${(downloaded / 1024 / 1024).toFixed(1)} MB`);
  }

  const line = parts.join(' - ');
  process.stdout.write(`\r${line.padEnd(lastProgressLineLength, ' ')}`);
  lastProgressLineLength = line.length;
}

function finishProgressLine() {
  if (lastProgressLineLength > 0) {
    process.stdout.write('\n');
    lastProgressLineLength = 0;
  }
}

async function main() {
  const {
    DEFAULT_MODEL,
    downloadModel,
    formatBytes,
    getModelDefinition,
    isModelInstalled,
    normalizeModelName,
  } = await loadCore();

  const modelId = normalizeModelName(process.argv[2] ?? DEFAULT_MODEL);
  const model = getModelDefinition(modelId);

  if (!model) {
    throw new Error(`Unknown model: ${modelId}`);
  }

  if (isModelInstalled(modelId)) {
    console.log(`Model already installed: ${model.displayName} (${modelId})`);
    return;
  }

  console.log(`Downloading ${model.displayName} (${modelId})`);
  console.log(`Expected size: ${formatBytes(model.sizeBytes)}`);

  const modelDir = await downloadModel(modelId, writeProgress);
  finishProgressLine();

  console.log(`Model installed at: ${modelDir}`);
}

main().catch((error) => {
  finishProgressLine();
  console.error('Error:', error.message);
  process.exit(1);
});
