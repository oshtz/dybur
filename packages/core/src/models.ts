/**
 * Model management for dybur
 * Handles downloading, verifying, and managing speech recognition models
 */

import {
  createWriteStream,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
  readdirSync,
  rmSync,
  statSync,
} from 'fs';
import { createHash } from 'crypto';
import { join } from 'path';
import { getModelsDir, getModelPath } from '@dybur/config';

/**
 * Model metadata stored alongside each model
 */
export interface ModelMetadata {
  name: string;
  version: string;
  checksum: string;
  downloadedAt: string;
  size?: number;
  source?: string;
  variant?: 'full' | 'int8';
  files?: string[];
}

/**
 * Information about an installed model
 */
export interface InstalledModel {
  name: string;
  path: string;
  metadata: ModelMetadata | null;
  size: number;
  isDefault: boolean;
}

/**
 * Progress callback for downloads
 */
export type DownloadProgress = (downloaded: number, total: number, file?: string) => void;

/**
 * Default model configuration
 */
export const DEFAULT_MODEL = 'parakeet-tdt-0.6b-v3-onnx';

/**
 * HuggingFace model source (ONNX version for portability)
 */
export const MODEL_REPO = 'istupakov/parakeet-tdt-0.6b-v3-onnx';
export const MODEL_BASE_URL = `https://huggingface.co/${MODEL_REPO}/resolve/main`;

/**
 * Model files to download
 * Using INT8 quantized version for smaller size (~670MB vs ~2.5GB)
 */
export const MODEL_FILES = {
  full: [
    'encoder-model.onnx',
    'encoder-model.onnx.data',
    'decoder_joint-model.onnx',
    'nemo128.onnx',
    'vocab.txt',
    'config.json',
  ],
  int8: [
    'encoder-model.int8.onnx',
    'decoder_joint-model.int8.onnx',
    'nemo128.onnx',
    'vocab.txt',
    'config.json',
  ],
};

/**
 * Get the models directory, creating it if necessary
 */
export function ensureModelsDir(): string {
  const dir = getModelsDir();
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
  return dir;
}

/**
 * List all installed models
 */
export function listModels(): InstalledModel[] {
  const modelsDir = getModelsDir();

  if (!existsSync(modelsDir)) {
    return [];
  }

  const entries = readdirSync(modelsDir, { withFileTypes: true });
  const models: InstalledModel[] = [];

  for (const entry of entries) {
    if (!entry.isDirectory()) continue;

    const modelPath = join(modelsDir, entry.name);
    const metadataPath = join(modelPath, 'metadata.json');

    let metadata: ModelMetadata | null = null;
    if (existsSync(metadataPath)) {
      try {
        metadata = JSON.parse(readFileSync(metadataPath, 'utf-8')) as ModelMetadata;
      } catch {
        // Ignore invalid metadata
      }
    }

    // Calculate directory size
    const size = getDirectorySize(modelPath);

    models.push({
      name: entry.name,
      path: modelPath,
      metadata,
      size,
      isDefault: entry.name === DEFAULT_MODEL,
    });
  }

  return models.sort((a, b) => {
    // Default model first, then alphabetically
    if (a.isDefault) return -1;
    if (b.isDefault) return 1;
    return a.name.localeCompare(b.name);
  });
}

/**
 * Get directory size recursively
 */
function getDirectorySize(dirPath: string): number {
  let size = 0;

  try {
    const entries = readdirSync(dirPath, { withFileTypes: true });
    for (const entry of entries) {
      const entryPath = join(dirPath, entry.name);
      if (entry.isDirectory()) {
        size += getDirectorySize(entryPath);
      } else {
        size += statSync(entryPath).size;
      }
    }
  } catch {
    // Ignore errors
  }

  return size;
}

/**
 * Check if a model is installed (has all required files)
 */
export function isModelInstalled(modelName: string): boolean {
  const modelPath = getModelPath(modelName);
  const metadataPath = join(modelPath, 'metadata.json');

  if (!existsSync(modelPath) || !existsSync(metadataPath)) {
    return false;
  }

  // Check for essential model files
  const hasEncoder =
    existsSync(join(modelPath, 'encoder-model.int8.onnx')) ||
    existsSync(join(modelPath, 'encoder-model.onnx'));
  const hasDecoder =
    existsSync(join(modelPath, 'decoder_joint-model.int8.onnx')) ||
    existsSync(join(modelPath, 'decoder_joint-model.onnx'));
  const hasVocab = existsSync(join(modelPath, 'vocab.txt'));

  return hasEncoder && hasDecoder && hasVocab;
}

/**
 * Check if the default model is installed
 */
export function isDefaultModelInstalled(): boolean {
  return isModelInstalled(DEFAULT_MODEL);
}

/**
 * Get model metadata
 */
export function getModelMetadata(modelName: string): ModelMetadata | null {
  const metadataPath = join(getModelPath(modelName), 'metadata.json');

  if (!existsSync(metadataPath)) {
    return null;
  }

  try {
    return JSON.parse(readFileSync(metadataPath, 'utf-8')) as ModelMetadata;
  } catch {
    return null;
  }
}

/**
 * Calculate SHA256 checksum of a file
 */
export function calculateChecksum(filePath: string): string {
  const hash = createHash('sha256');
  const content = readFileSync(filePath);
  hash.update(content);
  return hash.digest('hex');
}

/**
 * Download a single file with progress
 */
async function downloadFile(
  url: string,
  destPath: string,
  onProgress?: (downloaded: number, total: number) => void
): Promise<number> {
  const response = await fetch(url);

  if (!response.ok) {
    throw new Error(`Failed to download: ${response.status} ${response.statusText}`);
  }

  const contentLength = parseInt(response.headers.get('content-length') ?? '0', 10);
  const reader = response.body?.getReader();

  if (!reader) {
    throw new Error('Failed to get response reader');
  }

  const fileStream = createWriteStream(destPath);
  let downloaded = 0;

  try {
    while (true) {
      const { done, value } = await reader.read();
      if (done) break;

      fileStream.write(Buffer.from(value));
      downloaded += value.length;

      if (onProgress && contentLength > 0) {
        onProgress(downloaded, contentLength);
      }
    }
  } finally {
    fileStream.end();
  }

  return downloaded;
}

/**
 * Download a model from HuggingFace
 * @param modelName Model name (default: parakeet-tdt-0.6b-v3-onnx)
 * @param variant 'int8' for quantized (smaller), 'full' for original
 * @param onProgress Progress callback
 */
export async function downloadModel(
  modelName: string = DEFAULT_MODEL,
  onProgress?: DownloadProgress,
  variant: 'full' | 'int8' = 'int8'
): Promise<string> {
  const modelDir = getModelPath(modelName);

  // Check if already installed
  if (isModelInstalled(modelName)) {
    return modelDir;
  }

  // Create model directory
  mkdirSync(modelDir, { recursive: true });

  const files = MODEL_FILES[variant];
  let totalDownloaded = 0;
  const downloadedFiles: string[] = [];

  try {
    for (const file of files) {
      const url = `${MODEL_BASE_URL}/${file}`;
      const destPath = join(modelDir, file);

      if (onProgress) {
        onProgress(0, 0, file);
      }

      const fileSize = await downloadFile(url, destPath, (downloaded, total) => {
        if (onProgress) {
          onProgress(downloaded, total, file);
        }
      });

      totalDownloaded += fileSize;
      downloadedFiles.push(file);
    }

    // Write metadata
    const metadata: ModelMetadata = {
      name: modelName,
      version: 'v3',
      checksum: '', // Would compute combined checksum if needed
      downloadedAt: new Date().toISOString(),
      size: totalDownloaded,
      source: MODEL_REPO,
      variant,
      files: downloadedFiles,
    };

    writeFileSync(join(modelDir, 'metadata.json'), JSON.stringify(metadata, null, 2));

    return modelDir;
  } catch (error) {
    // Clean up partial download on failure
    rmSync(modelDir, { recursive: true, force: true });
    throw error;
  }
}

/**
 * Remove a model
 */
export function removeModel(modelName: string): boolean {
  const modelPath = getModelPath(modelName);

  if (!existsSync(modelPath)) {
    return false;
  }

  rmSync(modelPath, { recursive: true, force: true });
  return true;
}

/**
 * Remove all models except the default
 */
export function cleanModels(): string[] {
  const models = listModels();
  const removed: string[] = [];

  for (const model of models) {
    if (!model.isDefault) {
      if (removeModel(model.name)) {
        removed.push(model.name);
      }
    }
  }

  return removed;
}

/**
 * Format bytes as human-readable string
 */
export function formatBytes(bytes: number): string {
  if (bytes === 0) return '0 B';

  const units = ['B', 'KB', 'MB', 'GB'];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const unit = units[i] ?? 'GB';

  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${unit}`;
}

/**
 * Get model file paths for inference
 */
export function getModelFiles(modelName: string = DEFAULT_MODEL): {
  encoder: string;
  decoder: string;
  vocab: string;
} | null {
  const modelPath = getModelPath(modelName);

  if (!isModelInstalled(modelName)) {
    return null;
  }

  // Prefer INT8 quantized versions
  const encoderInt8 = join(modelPath, 'encoder-model.int8.onnx');
  const decoderInt8 = join(modelPath, 'decoder_joint-model.int8.onnx');
  const encoderFull = join(modelPath, 'encoder-model.onnx');
  const decoderFull = join(modelPath, 'decoder_joint-model.onnx');

  const encoder = existsSync(encoderInt8) ? encoderInt8 : encoderFull;
  const decoder = existsSync(decoderInt8) ? decoderInt8 : decoderFull;
  const vocab = join(modelPath, 'vocab.txt');

  if (!existsSync(encoder) || !existsSync(decoder) || !existsSync(vocab)) {
    return null;
  }

  return { encoder, decoder, vocab };
}
