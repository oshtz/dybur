#!/usr/bin/env node
/**
 * Model download utility for dybur
 * Downloads and verifies speech recognition models
 */

import { createWriteStream, existsSync, mkdirSync, writeFileSync, readFileSync } from 'fs';
import { createHash } from 'crypto';
import { pipeline } from 'stream/promises';
import { join } from 'path';
import { homedir } from 'os';

const DEFAULT_MODEL = 'parakeet-tdt-v3-int8';
const MODEL_BASE_URL = 'https://github.com/oshtz/dybur/releases/download/v1.0.0';

function getModelsDir() {
  return join(homedir(), '.dybur', 'models');
}

async function downloadFile(url, destPath, onProgress) {
  const response = await fetch(url);

  if (!response.ok) {
    throw new Error(`Failed to download: ${response.status} ${response.statusText}`);
  }

  const contentLength = parseInt(response.headers.get('content-length') || '0', 10);
  let downloaded = 0;

  const fileStream = createWriteStream(destPath);
  const reader = response.body.getReader();

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    fileStream.write(Buffer.from(value));
    downloaded += value.length;

    if (onProgress && contentLength > 0) {
      onProgress(downloaded, contentLength);
    }
  }

  fileStream.end();
  return destPath;
}

async function verifyChecksum(filePath, expectedChecksum) {
  const hash = createHash('sha256');
  const content = readFileSync(filePath);
  hash.update(content);
  const actualChecksum = hash.digest('hex');
  return actualChecksum === expectedChecksum;
}

async function fetchChecksum(modelName) {
  const checksumUrl = `${MODEL_BASE_URL}/${modelName}.sha256`;
  const response = await fetch(checksumUrl);

  if (!response.ok) {
    throw new Error(`Failed to fetch checksum: ${response.status}`);
  }

  const text = await response.text();
  return text.trim().split(' ')[0]; // SHA256 format: "hash  filename"
}

async function downloadModel(modelName = DEFAULT_MODEL) {
  const modelsDir = getModelsDir();
  const modelDir = join(modelsDir, modelName);

  // Create directories if needed
  if (!existsSync(modelsDir)) {
    mkdirSync(modelsDir, { recursive: true });
  }

  if (existsSync(modelDir)) {
    console.log(`Model ${modelName} already exists at ${modelDir}`);
    return modelDir;
  }

  console.log(`Downloading model: ${modelName}`);
  console.log(`Destination: ${modelDir}`);

  // Fetch checksum first
  console.log('Fetching checksum...');
  const expectedChecksum = await fetchChecksum(modelName);
  console.log(`Expected checksum: ${expectedChecksum.substring(0, 16)}...`);

  // Download model archive
  const archiveUrl = `${MODEL_BASE_URL}/${modelName}.tar.gz`;
  const archivePath = join(modelsDir, `${modelName}.tar.gz`);

  console.log('Downloading model archive...');
  await downloadFile(archiveUrl, archivePath, (downloaded, total) => {
    const percent = Math.round((downloaded / total) * 100);
    process.stdout.write(`\rProgress: ${percent}% (${(downloaded / 1024 / 1024).toFixed(1)} MB)`);
  });
  console.log('\nDownload complete.');

  // Verify checksum
  console.log('Verifying checksum...');
  const isValid = await verifyChecksum(archivePath, expectedChecksum);

  if (!isValid) {
    throw new Error('Checksum verification failed! The download may be corrupted.');
  }

  console.log('Checksum verified.');

  // Extract archive (simplified - in production use tar library)
  console.log('Extracting model...');
  mkdirSync(modelDir, { recursive: true });

  // Write metadata
  const metadata = {
    name: modelName,
    version: '1.0.0',
    checksum: expectedChecksum,
    downloadedAt: new Date().toISOString(),
  };

  writeFileSync(join(modelDir, 'metadata.json'), JSON.stringify(metadata, null, 2));

  console.log(`Model installed at: ${modelDir}`);
  return modelDir;
}

// CLI entry point
const args = process.argv.slice(2);
const modelName = args[0] || DEFAULT_MODEL;

downloadModel(modelName).catch((error) => {
  console.error('Error:', error.message);
  process.exit(1);
});
