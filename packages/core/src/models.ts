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
import { getModelsDir, getModelPath, loadConfig } from '@dybur/config';

// ============================================================================
// Model Architecture Types
// ============================================================================

/**
 * Speech recognition model architecture type
 */
export type ModelArchitecture =
  | 'tdt_transducer' // Parakeet v2/v3
  | 'streaming_transducer' // Nemotron
  | 'encoder_decoder'; // Whisper

/**
 * Vocabulary/tokenization type
 */
export type VocabType = 'text_file' | 'bpe';

/**
 * Registry visibility for model picker and normal download flows.
 */
export type ModelVisibility = 'normal' | 'legacy';

/**
 * Role of a model file
 */
export type FileRole =
  | 'encoder'
  | 'decoder'
  | 'decoder_with_past'
  | 'joiner'
  | 'vocab'
  | 'preprocessor'
  | 'embeddings'
  | 'config'
  | 'encoder_data'
  | 'decoder_data'
  | 'embeddings_data';

/**
 * A file that is part of a model
 */
export interface ModelFile {
  name: string;
  role: FileRole;
  required: boolean;
}

/**
 * Model-specific configuration
 */
export interface ModelConfig {
  vocabType: VocabType;
  sampleRate: number;
  nMels: number;
  supportsStreaming: boolean;
  maxDurationS: number;
}

/**
 * Definition of a speech recognition model
 */
export interface ModelDefinition {
  id: string;
  displayName: string;
  description: string;
  architecture: ModelArchitecture;
  repo: string;
  files: ModelFile[];
  sizeBytes: number;
  languages: string[];
  isDefault: boolean;
  visibility: ModelVisibility;
  config: ModelConfig;
}

// ============================================================================
// Model Registry - All Supported Models
// ============================================================================

/**
 * All available models
 */
export const MODEL_REGISTRY: ModelDefinition[] = [
  // Parakeet TDT v2 - English only
  {
    id: 'parakeet-tdt-v2-int8',
    displayName: 'Parakeet TDT v2 (English)',
    description: 'Fast, English-optimized transducer model',
    architecture: 'tdt_transducer',
    repo: 'istupakov/parakeet-tdt-0.6b-v2-onnx',
    files: [
      { name: 'encoder-model.int8.onnx', role: 'encoder', required: true },
      { name: 'decoder_joint-model.int8.onnx', role: 'decoder', required: true },
      { name: 'nemo128.onnx', role: 'preprocessor', required: false },
      { name: 'vocab.txt', role: 'vocab', required: true },
      { name: 'config.json', role: 'config', required: false },
    ],
    sizeBytes: 661_000_000,
    languages: ['en'],
    isDefault: false,
    visibility: 'legacy',
    config: {
      vocabType: 'text_file',
      sampleRate: 16000,
      nMels: 128,
      supportsStreaming: false,
      maxDurationS: 1440,
    },
  },
  // Parakeet TDT v3 - Multilingual (DEFAULT)
  {
    id: 'parakeet-tdt-v3-int8',
    displayName: 'Parakeet TDT v3 (Multilingual)',
    description: 'Balanced accuracy, 25 languages',
    architecture: 'tdt_transducer',
    repo: 'istupakov/parakeet-tdt-0.6b-v3-onnx',
    files: [
      { name: 'encoder-model.int8.onnx', role: 'encoder', required: true },
      { name: 'decoder_joint-model.int8.onnx', role: 'decoder', required: true },
      { name: 'nemo128.onnx', role: 'preprocessor', required: false },
      { name: 'vocab.txt', role: 'vocab', required: true },
      { name: 'config.json', role: 'config', required: false },
    ],
    sizeBytes: 670_000_000,
    languages: ['en', 'de', 'es', 'fr', 'it', 'pt', 'nl', 'pl', 'ru', 'uk', 'ja', 'ko', 'zh'],
    isDefault: true,
    visibility: 'normal',
    config: {
      vocabType: 'text_file',
      sampleRate: 16000,
      nMels: 128,
      supportsStreaming: false,
      maxDurationS: 1440,
    },
  },
  // Nemotron Streaming - English
  {
    id: 'nemotron-streaming-int8',
    displayName: 'Nemotron Streaming (English)',
    description: 'Low-latency streaming transducer',
    architecture: 'streaming_transducer',
    repo: 'csukuangfj/sherpa-onnx-nemotron-speech-streaming-en-0.6b-int8-2026-01-14',
    files: [
      { name: 'encoder.int8.onnx', role: 'encoder', required: true },
      { name: 'decoder.int8.onnx', role: 'decoder', required: true },
      { name: 'joiner.int8.onnx', role: 'joiner', required: true },
      { name: 'tokens.txt', role: 'vocab', required: true },
    ],
    sizeBytes: 663_000_000,
    languages: ['en'],
    isDefault: false,
    visibility: 'normal',
    config: {
      vocabType: 'text_file',
      sampleRate: 16000,
      nMels: 80,
      supportsStreaming: true,
      maxDurationS: 1440,
    },
  },
  // Whisper Large v3 Turbo - INT8
  {
    id: 'whisper-large-v3-turbo-int8',
    displayName: 'Whisper Large v3 Turbo (INT8)',
    description: 'Popular model, 99 languages, balanced',
    architecture: 'encoder_decoder',
    repo: 'onnx-community/whisper-large-v3-turbo',
    files: [
      { name: 'onnx/encoder_model_int8.onnx', role: 'encoder', required: true },
      { name: 'onnx/decoder_model_int8.onnx', role: 'decoder', required: true },
      { name: 'tokenizer.json', role: 'vocab', required: true },
      { name: 'config.json', role: 'config', required: false },
      { name: 'generation_config.json', role: 'config', required: false },
    ],
    sizeBytes: 1_100_000_000,
    languages: [], // All languages
    isDefault: false,
    visibility: 'normal',
    config: {
      vocabType: 'bpe',
      sampleRate: 16000,
      nMels: 128,
      supportsStreaming: false,
      maxDurationS: 30,
    },
  },
  // Whisper Large v3 Turbo - FP16
  {
    id: 'whisper-large-v3-turbo-fp16',
    displayName: 'Whisper Large v3 Turbo (FP16)',
    description: 'High accuracy, 99 languages',
    architecture: 'encoder_decoder',
    repo: 'onnx-community/whisper-large-v3-turbo',
    files: [
      { name: 'onnx/encoder_model_fp16.onnx', role: 'encoder', required: true },
      { name: 'onnx/decoder_model_fp16.onnx', role: 'decoder', required: true },
      { name: 'tokenizer.json', role: 'vocab', required: true },
      { name: 'config.json', role: 'config', required: false },
      { name: 'generation_config.json', role: 'config', required: false },
    ],
    sizeBytes: 1_600_000_000,
    languages: [],
    isDefault: false,
    visibility: 'normal',
    config: {
      vocabType: 'bpe',
      sampleRate: 16000,
      nMels: 128,
      supportsStreaming: false,
      maxDurationS: 30,
    },
  },
];

/**
 * Get a model definition by ID
 */
export function getModelDefinition(modelId: string): ModelDefinition | undefined {
  return MODEL_REGISTRY.find((m) => m.id === modelId);
}

/**
 * Get the default model definition
 */
export function getDefaultModelDefinition(): ModelDefinition {
  const defaultModel = MODEL_REGISTRY.find((m) => m.isDefault);
  if (!defaultModel) {
    throw new Error('No default model defined');
  }
  return defaultModel;
}

/**
 * Get all available model definitions
 */
export function getAvailableModels(): ModelDefinition[] {
  return MODEL_REGISTRY.filter((model) => model.visibility === 'normal');
}

/**
 * Normalize legacy model names to new IDs
 */
export function normalizeModelName(name: string): string {
  const legacyMap: Record<string, string> = {
    'parakeet-tdt-0.6b-v3-onnx': 'parakeet-tdt-v3-int8',
    'parakeet-tdt-0.6b-v2-onnx': 'parakeet-tdt-v2-int8',
  };
  return legacyMap[name] ?? name;
}

// ============================================================================
// Legacy Constants (for backward compatibility)
// ============================================================================

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
  variant?: string;
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
 * Default model ID
 */
export const DEFAULT_MODEL = 'parakeet-tdt-v3-int8';

/**
 * Default model repository (for backward compatibility)
 */
export const MODEL_REPO = 'istupakov/parakeet-tdt-0.6b-v3-onnx';

/**
 * Build HuggingFace download URL
 */
export function buildDownloadUrl(repo: string, file: string): string {
  return `https://huggingface.co/${repo}/resolve/main/${file}`;
}

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
  // Normalize legacy model names
  const modelId = normalizeModelName(modelName);
  const modelPath = getModelPath(modelId);
  const metadataPath = join(modelPath, 'metadata.json');

  if (!existsSync(modelPath) || !existsSync(metadataPath)) {
    return false;
  }

  // Get model definition to check required files
  const modelDef = getModelDefinition(modelId);
  if (modelDef) {
    // Check all required files exist
    for (const file of modelDef.files) {
      if (file.required) {
        const filePath = join(modelPath, file.name);
        if (!existsSync(filePath)) {
          return false;
        }
      }
    }
    return true;
  }

  // Fallback for unknown models: check for basic files
  const hasEncoder =
    existsSync(join(modelPath, 'encoder-model.int8.onnx')) ||
    existsSync(join(modelPath, 'encoder-model.onnx')) ||
    existsSync(join(modelPath, 'encoder.int8.onnx')) ||
    existsSync(join(modelPath, 'onnx/encoder_model_int8.onnx'));
  const hasDecoder =
    existsSync(join(modelPath, 'decoder_joint-model.int8.onnx')) ||
    existsSync(join(modelPath, 'decoder_joint-model.onnx')) ||
    existsSync(join(modelPath, 'decoder.int8.onnx')) ||
    existsSync(join(modelPath, 'onnx/decoder_model_int8.onnx')) ||
    existsSync(join(modelPath, 'onnx/decoder_with_past_model_int8.onnx'));
  const hasVocab =
    existsSync(join(modelPath, 'vocab.txt')) ||
    existsSync(join(modelPath, 'tokens.txt')) ||
    existsSync(join(modelPath, 'tokenizer.json'));

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
    for (;;) {
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
 * @param modelId Model ID from registry (e.g., 'parakeet-tdt-v3-int8')
 * @param onProgress Progress callback
 */
export async function downloadModel(
  modelId: string = DEFAULT_MODEL,
  onProgress?: DownloadProgress
): Promise<string> {
  // Normalize legacy model names
  const normalizedId = normalizeModelName(modelId);
  const modelDir = getModelPath(normalizedId);

  // Check if already installed
  if (isModelInstalled(normalizedId)) {
    return modelDir;
  }

  // Get model definition from registry
  const modelDef = getModelDefinition(normalizedId);
  if (!modelDef) {
    throw new Error(`Unknown model: ${normalizedId}`);
  }

  // Create model directory
  mkdirSync(modelDir, { recursive: true });

  let totalDownloaded = 0;
  const downloadedFiles: string[] = [];
  const totalFiles = modelDef.files.length;

  try {
    for (let i = 0; i < modelDef.files.length; i++) {
      const file = modelDef.files[i]!;
      const url = buildDownloadUrl(modelDef.repo, file.name);
      const destPath = join(modelDir, file.name);

      // Create subdirectories if needed (e.g., for "onnx/encoder.onnx")
      const pathParts = file.name.split('/').slice(0, -1);
      if (pathParts.length > 0) {
        const destDir = join(modelDir, ...pathParts);
        if (!existsSync(destDir)) {
          mkdirSync(destDir, { recursive: true });
        }
      }

      if (onProgress) {
        onProgress(0, 0, `[${i + 1}/${totalFiles}] ${file.name}`);
      }

      const fileSize = await downloadFile(url, destPath, (downloaded, total) => {
        if (onProgress) {
          onProgress(downloaded, total, `[${i + 1}/${totalFiles}] ${file.name}`);
        }
      });

      totalDownloaded += fileSize;
      downloadedFiles.push(file.name);
    }

    // Determine version from model ID
    const version = normalizedId.includes('v2') ? 'v2' : normalizedId.includes('v3') ? 'v3' : 'v1';

    // Write metadata
    const metadata: ModelMetadata = {
      name: normalizedId,
      version,
      checksum: '',
      downloadedAt: new Date().toISOString(),
      size: totalDownloaded,
      source: modelDef.repo,
      variant: normalizedId,
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
  const activeModel = normalizeModelName(
    loadConfig({ createIfMissing: false }).model ?? DEFAULT_MODEL
  );

  for (const model of models) {
    if (!model.isDefault && normalizeModelName(model.name) !== activeModel) {
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
