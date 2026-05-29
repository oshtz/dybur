#!/usr/bin/env node

// src/cli.ts
import { parseArgs } from "util";

// src/commands/start.ts
import { spawn, execSync } from "child_process";
import { existsSync as existsSync5 } from "fs";
import { join as join4 } from "path";
import { homedir as homedir2 } from "os";

// ../core/dist/models.js
import { createWriteStream, existsSync as existsSync2, mkdirSync as mkdirSync2, readFileSync as readFileSync2, writeFileSync as writeFileSync2, readdirSync, rmSync, statSync } from "fs";
import { join as join2 } from "path";

// ../config/dist/schema.js
var DEFAULT_CONFIG = {
  hotkey: "Ctrl+Shift+Space",
  autoPunctuation: true,
  sentenceCase: true,
  silenceTimeoutMs: 1e3,
  model: "parakeet-tdt-v3-int8",
  clipboardCleanup: true,
  inputDevice: null,
  recordingMode: "toggle",
  vadEnabled: true,
  vadThreshold: 0.5,
  vadMinSpeechMs: 250,
  gpuMode: "auto",
  streamingEnabled: true
};
var VALID_MODIFIERS = ["Ctrl", "Alt", "Shift", "Meta", "Cmd", "Win"];
function validateHotkey(hotkey) {
  const errors = [];
  if (typeof hotkey !== "string" || hotkey.trim().length === 0) {
    errors.push({
      field: "hotkey",
      message: "Hotkey must be a non-empty string",
      value: hotkey
    });
    return errors;
  }
  const parts = hotkey.split("+").map((p) => p.trim());
  if (parts.length < 2) {
    errors.push({
      field: "hotkey",
      message: 'Hotkey must include at least one modifier and a key (e.g., "Ctrl+Space")',
      value: hotkey
    });
    return errors;
  }
  const modifiers = parts.slice(0, -1);
  const key = parts[parts.length - 1];
  for (const mod of modifiers) {
    if (!VALID_MODIFIERS.includes(mod)) {
      errors.push({
        field: "hotkey",
        message: `Invalid modifier "${mod}". Valid modifiers: ${VALID_MODIFIERS.join(", ")}`,
        value: hotkey
      });
    }
  }
  if (!key || key.length === 0) {
    errors.push({
      field: "hotkey",
      message: 'Hotkey must end with a key (e.g., "Space", "A", "F1")',
      value: hotkey
    });
  }
  return errors;
}
function validateConfig(config) {
  const errors = [];
  if (config.hotkey !== void 0) {
    errors.push(...validateHotkey(config.hotkey));
  }
  if (config.autoPunctuation !== void 0 && typeof config.autoPunctuation !== "boolean") {
    errors.push({
      field: "autoPunctuation",
      message: "autoPunctuation must be a boolean",
      value: config.autoPunctuation
    });
  }
  if (config.sentenceCase !== void 0 && typeof config.sentenceCase !== "boolean") {
    errors.push({
      field: "sentenceCase",
      message: "sentenceCase must be a boolean",
      value: config.sentenceCase
    });
  }
  if (config.silenceTimeoutMs !== void 0) {
    if (typeof config.silenceTimeoutMs !== "number") {
      errors.push({
        field: "silenceTimeoutMs",
        message: "silenceTimeoutMs must be a number",
        value: config.silenceTimeoutMs
      });
    } else if (config.silenceTimeoutMs < 0) {
      errors.push({
        field: "silenceTimeoutMs",
        message: "silenceTimeoutMs must be >= 0",
        value: config.silenceTimeoutMs
      });
    } else if (config.silenceTimeoutMs > 3e4) {
      errors.push({
        field: "silenceTimeoutMs",
        message: "silenceTimeoutMs must be <= 30000 (30 seconds)",
        value: config.silenceTimeoutMs
      });
    }
  }
  if (config.model !== void 0) {
    if (typeof config.model !== "string" || config.model.trim().length === 0) {
      errors.push({
        field: "model",
        message: "model must be a non-empty string",
        value: config.model
      });
    }
  }
  if (config.clipboardCleanup !== void 0 && typeof config.clipboardCleanup !== "boolean") {
    errors.push({
      field: "clipboardCleanup",
      message: "clipboardCleanup must be a boolean",
      value: config.clipboardCleanup
    });
  }
  if (config.inputDevice !== void 0 && config.inputDevice !== null) {
    if (typeof config.inputDevice !== "string") {
      errors.push({
        field: "inputDevice",
        message: "inputDevice must be a string or null",
        value: config.inputDevice
      });
    } else if (config.inputDevice.trim().length === 0) {
      errors.push({
        field: "inputDevice",
        message: "inputDevice must be a non-empty string or null",
        value: config.inputDevice
      });
    }
  }
  if (config.recordingMode !== void 0) {
    if (config.recordingMode !== "toggle" && config.recordingMode !== "push_to_talk") {
      errors.push({
        field: "recordingMode",
        message: 'recordingMode must be "toggle" or "push_to_talk"',
        value: config.recordingMode
      });
    }
  }
  if (config.vadEnabled !== void 0 && typeof config.vadEnabled !== "boolean") {
    errors.push({
      field: "vadEnabled",
      message: "vadEnabled must be a boolean",
      value: config.vadEnabled
    });
  }
  if (config.vadThreshold !== void 0) {
    if (typeof config.vadThreshold !== "number") {
      errors.push({
        field: "vadThreshold",
        message: "vadThreshold must be a number",
        value: config.vadThreshold
      });
    } else if (config.vadThreshold < 0 || config.vadThreshold > 1) {
      errors.push({
        field: "vadThreshold",
        message: "vadThreshold must be between 0.0 and 1.0",
        value: config.vadThreshold
      });
    }
  }
  if (config.vadMinSpeechMs !== void 0) {
    if (typeof config.vadMinSpeechMs !== "number") {
      errors.push({
        field: "vadMinSpeechMs",
        message: "vadMinSpeechMs must be a number",
        value: config.vadMinSpeechMs
      });
    } else if (config.vadMinSpeechMs < 0 || config.vadMinSpeechMs > 5e3) {
      errors.push({
        field: "vadMinSpeechMs",
        message: "vadMinSpeechMs must be between 0 and 5000",
        value: config.vadMinSpeechMs
      });
    }
  }
  if (config.gpuMode !== void 0) {
    if (config.gpuMode !== "auto" && config.gpuMode !== "cpu") {
      errors.push({
        field: "gpuMode",
        message: 'gpuMode must be "auto" or "cpu"',
        value: config.gpuMode
      });
    }
  }
  if (config.streamingEnabled !== void 0 && typeof config.streamingEnabled !== "boolean") {
    errors.push({
      field: "streamingEnabled",
      message: "streamingEnabled must be a boolean",
      value: config.streamingEnabled
    });
  }
  return {
    valid: errors.length === 0,
    errors
  };
}
function mergeWithDefaults(userConfig, onWarning) {
  const result = { ...DEFAULT_CONFIG };
  const validation = validateConfig(userConfig);
  const invalidFields = new Set(validation.errors.map((e) => e.field));
  for (const key of Object.keys(userConfig)) {
    if (!invalidFields.has(key) && userConfig[key] !== void 0) {
      result[key] = userConfig[key];
    }
  }
  if (onWarning) {
    for (const error2 of validation.errors) {
      onWarning(error2.field, `${error2.message}. Using default value.`);
    }
  }
  return result;
}

// ../config/dist/paths.js
import { homedir, platform } from "os";
import { join } from "path";
function getPlatform() {
  const p = platform();
  if (p !== "darwin" && p !== "win32") {
    throw new Error(`Unsupported platform: ${p}. dybur only supports macOS and Windows.`);
  }
  return p;
}
function isMacOS() {
  return platform() === "darwin";
}
function isWindows() {
  return platform() === "win32";
}
function getConfigDir() {
  if (isMacOS()) {
    return join(homedir(), "Library", "Application Support", "dybur");
  }
  const appData = process.env["APPDATA"];
  if (!appData) {
    return join(homedir(), "AppData", "Roaming", "dybur");
  }
  return join(appData, "dybur");
}
function getConfigPath() {
  return join(getConfigDir(), "config.json");
}
function getDataDir() {
  return join(homedir(), ".dybur");
}
function getModelsDir() {
  return join(getDataDir(), "models");
}
function getLogsDir() {
  return join(getDataDir(), "logs");
}
function getModelPath(modelName) {
  return join(getModelsDir(), modelName);
}
function getLogFilePath() {
  const today = (/* @__PURE__ */ new Date()).toISOString().split("T")[0];
  return join(getLogsDir(), `dybur-${today}.log`);
}
function getBinDir() {
  return join(getDataDir(), "bin");
}
function getArch() {
  return process.arch === "arm64" ? "arm64" : "x64";
}
function getTrayAppPath() {
  if (isMacOS()) {
    return join(getBinDir(), "dybur.app", "Contents", "MacOS", "dybur");
  }
  return join(getBinDir(), "dybur.exe");
}
function getTrayAppBundlePath() {
  if (isMacOS()) {
    return join(getBinDir(), "dybur.app");
  }
  return join(getBinDir(), "dybur.exe");
}
function getAllPaths() {
  return {
    platform: platform(),
    arch: getArch(),
    configDir: getConfigDir(),
    configPath: getConfigPath(),
    dataDir: getDataDir(),
    modelsDir: getModelsDir(),
    logsDir: getLogsDir(),
    logFile: getLogFilePath(),
    binDir: getBinDir(),
    trayApp: getTrayAppPath()
  };
}

// ../config/dist/config.js
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "fs";
import { dirname } from "path";
var defaultLogger = {
  warn: (msg) => console.warn(`[config] ${msg}`),
  error: (msg) => console.error(`[config] ${msg}`),
  debug: () => {
  }
  // No-op by default
};
function loadConfig(options = {}) {
  const { path = getConfigPath(), logger = defaultLogger, createIfMissing = true } = options;
  if (!existsSync(path)) {
    logger.debug(`Config file not found at ${path}`);
    if (createIfMissing) {
      try {
        saveConfig(DEFAULT_CONFIG, { path, logger });
        logger.debug(`Created default config at ${path}`);
      } catch (error2) {
        logger.warn(`Failed to create default config: ${error2}`);
      }
    }
    return { ...DEFAULT_CONFIG };
  }
  let userConfig;
  try {
    const content = readFileSync(path, "utf-8");
    userConfig = JSON.parse(content);
  } catch (error2) {
    if (error2 instanceof SyntaxError) {
      logger.error(`Invalid JSON in config file: ${error2.message}`);
    } else {
      logger.error(`Failed to read config file: ${error2}`);
    }
    logger.warn("Using default configuration");
    return { ...DEFAULT_CONFIG };
  }
  const config = mergeWithDefaults(userConfig, (field, message) => {
    logger.warn(`Config validation: ${field} - ${message}`);
  });
  return config;
}
function saveConfig(config, options = {}) {
  const { path = getConfigPath(), logger = defaultLogger } = options;
  const validation = validateConfig(config);
  if (!validation.valid) {
    const errorMessages = validation.errors.map((e) => `${e.field}: ${e.message}`).join(", ");
    throw new Error(`Cannot save invalid config: ${errorMessages}`);
  }
  const dir = dirname(path);
  if (!existsSync(dir)) {
    try {
      mkdirSync(dir, { recursive: true });
      logger.debug(`Created config directory: ${dir}`);
    } catch (error2) {
      throw new Error(`Failed to create config directory: ${error2}`);
    }
  }
  try {
    const content = JSON.stringify(config, null, 2);
    writeFileSync(path, content, "utf-8");
    logger.debug(`Saved config to ${path}`);
  } catch (error2) {
    throw new Error(`Failed to write config file: ${error2}`);
  }
}
function updateConfig(updates, options = {}) {
  const { logger = defaultLogger } = options;
  const currentConfig = loadConfig(options);
  const validation = validateConfig(updates);
  if (!validation.valid) {
    for (const error2 of validation.errors) {
      logger.warn(`Ignoring invalid update for ${error2.field}: ${error2.message}`);
    }
  }
  const invalidFields = new Set(validation.errors.map((e) => e.field));
  const validUpdates = {};
  for (const key of Object.keys(updates)) {
    if (!invalidFields.has(key)) {
      validUpdates[key] = updates[key];
    }
  }
  const newConfig = { ...currentConfig, ...validUpdates };
  saveConfig(newConfig, options);
  return newConfig;
}

// ../core/dist/models.js
var MODEL_REGISTRY = [
  // Parakeet TDT v2 - English only
  {
    id: "parakeet-tdt-v2-int8",
    displayName: "Parakeet TDT v2 (English)",
    description: "Fast, English-optimized transducer model",
    architecture: "tdt_transducer",
    repo: "istupakov/parakeet-tdt-0.6b-v2-onnx",
    files: [
      { name: "encoder-model.int8.onnx", role: "encoder", required: true },
      { name: "decoder_joint-model.int8.onnx", role: "decoder", required: true },
      { name: "nemo128.onnx", role: "preprocessor", required: false },
      { name: "vocab.txt", role: "vocab", required: true },
      { name: "config.json", role: "config", required: false }
    ],
    sizeBytes: 661e6,
    languages: ["en"],
    isDefault: false,
    config: {
      vocabType: "text_file",
      sampleRate: 16e3,
      nMels: 128,
      supportsStreaming: false,
      maxDurationS: 1440
    }
  },
  // Parakeet TDT v3 - Multilingual (DEFAULT)
  {
    id: "parakeet-tdt-v3-int8",
    displayName: "Parakeet TDT v3 (Multilingual)",
    description: "Balanced accuracy, 25 languages",
    architecture: "tdt_transducer",
    repo: "istupakov/parakeet-tdt-0.6b-v3-onnx",
    files: [
      { name: "encoder-model.int8.onnx", role: "encoder", required: true },
      { name: "decoder_joint-model.int8.onnx", role: "decoder", required: true },
      { name: "nemo128.onnx", role: "preprocessor", required: false },
      { name: "vocab.txt", role: "vocab", required: true },
      { name: "config.json", role: "config", required: false }
    ],
    sizeBytes: 67e7,
    languages: ["en", "de", "es", "fr", "it", "pt", "nl", "pl", "ru", "uk", "ja", "ko", "zh"],
    isDefault: true,
    config: {
      vocabType: "text_file",
      sampleRate: 16e3,
      nMels: 128,
      supportsStreaming: false,
      maxDurationS: 1440
    }
  },
  // Nemotron Streaming - English
  {
    id: "nemotron-streaming-int8",
    displayName: "Nemotron Streaming (English)",
    description: "Low-latency streaming transducer",
    architecture: "streaming_transducer",
    repo: "csukuangfj/sherpa-onnx-nemotron-speech-streaming-en-0.6b-int8-2026-01-14",
    files: [
      { name: "encoder.int8.onnx", role: "encoder", required: true },
      { name: "decoder.int8.onnx", role: "decoder", required: true },
      { name: "joiner.int8.onnx", role: "joiner", required: true },
      { name: "tokens.txt", role: "vocab", required: true }
    ],
    sizeBytes: 663e6,
    languages: ["en"],
    isDefault: false,
    config: {
      vocabType: "text_file",
      sampleRate: 16e3,
      nMels: 80,
      supportsStreaming: true,
      maxDurationS: 1440
    }
  },
  // Whisper Large v3 Turbo - INT8
  {
    id: "whisper-large-v3-turbo-int8",
    displayName: "Whisper Large v3 Turbo (INT8)",
    description: "Popular model, 99 languages, balanced",
    architecture: "encoder_decoder",
    repo: "onnx-community/whisper-large-v3-turbo",
    files: [
      { name: "onnx/encoder_model_int8.onnx", role: "encoder", required: true },
      { name: "onnx/decoder_model_int8.onnx", role: "decoder", required: true },
      { name: "tokenizer.json", role: "vocab", required: true },
      { name: "config.json", role: "config", required: false },
      { name: "generation_config.json", role: "config", required: false }
    ],
    sizeBytes: 11e8,
    languages: [],
    // All languages
    isDefault: false,
    config: {
      vocabType: "bpe",
      sampleRate: 16e3,
      nMels: 128,
      supportsStreaming: false,
      maxDurationS: 30
    }
  },
  // Whisper Large v3 Turbo - FP16
  {
    id: "whisper-large-v3-turbo-fp16",
    displayName: "Whisper Large v3 Turbo (FP16)",
    description: "High accuracy, 99 languages",
    architecture: "encoder_decoder",
    repo: "onnx-community/whisper-large-v3-turbo",
    files: [
      { name: "onnx/encoder_model_fp16.onnx", role: "encoder", required: true },
      { name: "onnx/decoder_model_fp16.onnx", role: "decoder", required: true },
      { name: "tokenizer.json", role: "vocab", required: true },
      { name: "config.json", role: "config", required: false },
      { name: "generation_config.json", role: "config", required: false }
    ],
    sizeBytes: 16e8,
    languages: [],
    isDefault: false,
    config: {
      vocabType: "bpe",
      sampleRate: 16e3,
      nMels: 128,
      supportsStreaming: false,
      maxDurationS: 30
    }
  }
];
function getModelDefinition(modelId) {
  return MODEL_REGISTRY.find((m) => m.id === modelId);
}
function getDefaultModelDefinition() {
  const defaultModel = MODEL_REGISTRY.find((m) => m.isDefault);
  if (!defaultModel) {
    throw new Error("No default model defined");
  }
  return defaultModel;
}
function getAvailableModels() {
  return MODEL_REGISTRY;
}
function normalizeModelName(name) {
  const legacyMap = {
    "parakeet-tdt-0.6b-v3-onnx": "parakeet-tdt-v3-int8",
    "parakeet-tdt-0.6b-v2-onnx": "parakeet-tdt-v2-int8"
  };
  return legacyMap[name] ?? name;
}
var DEFAULT_MODEL = "parakeet-tdt-v3-int8";
function buildDownloadUrl(repo, file) {
  return `https://huggingface.co/${repo}/resolve/main/${file}`;
}
function listModels() {
  const modelsDir = getModelsDir();
  if (!existsSync2(modelsDir)) {
    return [];
  }
  const entries = readdirSync(modelsDir, { withFileTypes: true });
  const models = [];
  for (const entry of entries) {
    if (!entry.isDirectory())
      continue;
    const modelPath = join2(modelsDir, entry.name);
    const metadataPath = join2(modelPath, "metadata.json");
    let metadata = null;
    if (existsSync2(metadataPath)) {
      try {
        metadata = JSON.parse(readFileSync2(metadataPath, "utf-8"));
      } catch {
      }
    }
    const size = getDirectorySize(modelPath);
    models.push({
      name: entry.name,
      path: modelPath,
      metadata,
      size,
      isDefault: entry.name === DEFAULT_MODEL
    });
  }
  return models.sort((a, b) => {
    if (a.isDefault)
      return -1;
    if (b.isDefault)
      return 1;
    return a.name.localeCompare(b.name);
  });
}
function getDirectorySize(dirPath) {
  let size = 0;
  try {
    const entries = readdirSync(dirPath, { withFileTypes: true });
    for (const entry of entries) {
      const entryPath = join2(dirPath, entry.name);
      if (entry.isDirectory()) {
        size += getDirectorySize(entryPath);
      } else {
        size += statSync(entryPath).size;
      }
    }
  } catch {
  }
  return size;
}
function isModelInstalled(modelName) {
  const modelId = normalizeModelName(modelName);
  const modelPath = getModelPath(modelId);
  const metadataPath = join2(modelPath, "metadata.json");
  if (!existsSync2(modelPath) || !existsSync2(metadataPath)) {
    return false;
  }
  const modelDef = getModelDefinition(modelId);
  if (modelDef) {
    for (const file of modelDef.files) {
      if (file.required) {
        const filePath = join2(modelPath, file.name);
        if (!existsSync2(filePath)) {
          return false;
        }
      }
    }
    return true;
  }
  const hasEncoder = existsSync2(join2(modelPath, "encoder-model.int8.onnx")) || existsSync2(join2(modelPath, "encoder-model.onnx")) || existsSync2(join2(modelPath, "encoder.int8.onnx")) || existsSync2(join2(modelPath, "onnx/encoder_model_int8.onnx"));
  const hasDecoder = existsSync2(join2(modelPath, "decoder_joint-model.int8.onnx")) || existsSync2(join2(modelPath, "decoder_joint-model.onnx")) || existsSync2(join2(modelPath, "decoder.int8.onnx")) || existsSync2(join2(modelPath, "onnx/decoder_model_int8.onnx")) || existsSync2(join2(modelPath, "onnx/decoder_with_past_model_int8.onnx"));
  const hasVocab = existsSync2(join2(modelPath, "vocab.txt")) || existsSync2(join2(modelPath, "tokens.txt")) || existsSync2(join2(modelPath, "tokenizer.json"));
  return hasEncoder && hasDecoder && hasVocab;
}
function getModelMetadata(modelName) {
  const metadataPath = join2(getModelPath(modelName), "metadata.json");
  if (!existsSync2(metadataPath)) {
    return null;
  }
  try {
    return JSON.parse(readFileSync2(metadataPath, "utf-8"));
  } catch {
    return null;
  }
}
async function downloadFile(url, destPath, onProgress) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`Failed to download: ${response.status} ${response.statusText}`);
  }
  const contentLength = parseInt(response.headers.get("content-length") ?? "0", 10);
  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error("Failed to get response reader");
  }
  const fileStream = createWriteStream(destPath);
  let downloaded = 0;
  try {
    for (; ; ) {
      const { done, value } = await reader.read();
      if (done)
        break;
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
async function downloadModel(modelId = DEFAULT_MODEL, onProgress) {
  const normalizedId = normalizeModelName(modelId);
  const modelDir = getModelPath(normalizedId);
  if (isModelInstalled(normalizedId)) {
    return modelDir;
  }
  const modelDef = getModelDefinition(normalizedId);
  if (!modelDef) {
    throw new Error(`Unknown model: ${normalizedId}`);
  }
  mkdirSync2(modelDir, { recursive: true });
  let totalDownloaded = 0;
  const downloadedFiles = [];
  const totalFiles = modelDef.files.length;
  try {
    for (let i = 0; i < modelDef.files.length; i++) {
      const file = modelDef.files[i];
      const url = buildDownloadUrl(modelDef.repo, file.name);
      const destPath = join2(modelDir, file.name);
      const pathParts = file.name.split("/").slice(0, -1);
      if (pathParts.length > 0) {
        const destDir = join2(modelDir, ...pathParts);
        if (!existsSync2(destDir)) {
          mkdirSync2(destDir, { recursive: true });
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
    const version = normalizedId.includes("v2") ? "v2" : normalizedId.includes("v3") ? "v3" : "v1";
    const metadata = {
      name: normalizedId,
      version,
      checksum: "",
      downloadedAt: (/* @__PURE__ */ new Date()).toISOString(),
      size: totalDownloaded,
      source: modelDef.repo,
      variant: normalizedId,
      files: downloadedFiles
    };
    writeFileSync2(join2(modelDir, "metadata.json"), JSON.stringify(metadata, null, 2));
    return modelDir;
  } catch (error2) {
    rmSync(modelDir, { recursive: true, force: true });
    throw error2;
  }
}
function removeModel(modelName) {
  const modelPath = getModelPath(modelName);
  if (!existsSync2(modelPath)) {
    return false;
  }
  rmSync(modelPath, { recursive: true, force: true });
  return true;
}
function cleanModels() {
  const models = listModels();
  const removed = [];
  const activeModel = normalizeModelName(loadConfig({ createIfMissing: false }).model ?? DEFAULT_MODEL);
  for (const model of models) {
    if (!model.isDefault && normalizeModelName(model.name) !== activeModel) {
      if (removeModel(model.name)) {
        removed.push(model.name);
      }
    }
  }
  return removed;
}
function formatBytes(bytes) {
  if (bytes === 0)
    return "0 B";
  const units = ["B", "KB", "MB", "GB"];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const unit = units[i] ?? "GB";
  return `${(bytes / Math.pow(k, i)).toFixed(1)} ${unit}`;
}

// ../core/dist/logging.js
import { existsSync as existsSync3, mkdirSync as mkdirSync3, appendFileSync } from "fs";
var LOG_LEVEL_ORDER = {
  debug: 0,
  info: 1,
  warn: 2,
  error: 3
};
var DEFAULT_LOGGER_CONFIG = {
  minLevel: "info",
  console: true,
  file: true
};
var globalConfig = { ...DEFAULT_LOGGER_CONFIG };
function ensureLogsDir() {
  const dir = getLogsDir();
  if (!existsSync3(dir)) {
    mkdirSync3(dir, { recursive: true });
  }
  return dir;
}
function formatLogEntry(entry) {
  const { timestamp, level, category, message, data } = entry;
  const levelStr = level.toUpperCase().padEnd(5);
  const categoryStr = category ? `[${category}]` : "";
  let line = `${timestamp} ${levelStr} ${categoryStr} ${message}`;
  if (data && Object.keys(data).length > 0) {
    line += ` ${JSON.stringify(data)}`;
  }
  return line;
}
function writeLog(entry) {
  const levelOrder = LOG_LEVEL_ORDER[entry.level];
  const minLevelOrder = LOG_LEVEL_ORDER[globalConfig.minLevel];
  if (levelOrder < minLevelOrder) {
    return;
  }
  const formatted = formatLogEntry(entry);
  if (globalConfig.console) {
    switch (entry.level) {
      case "debug":
        console.debug(formatted);
        break;
      case "info":
        console.info(formatted);
        break;
      case "warn":
        console.warn(formatted);
        break;
      case "error":
        console.error(formatted);
        break;
    }
  }
  if (globalConfig.file) {
    try {
      ensureLogsDir();
      const logFile = getLogFilePath();
      appendFileSync(logFile, formatted + "\n");
    } catch {
    }
  }
}
function createLogEntry(level, category, message, data) {
  return {
    timestamp: (/* @__PURE__ */ new Date()).toISOString(),
    level,
    category,
    message,
    data
  };
}
function createLogger(category) {
  return {
    debug: (message, data) => {
      writeLog(createLogEntry("debug", category, message, data));
    },
    info: (message, data) => {
      writeLog(createLogEntry("info", category, message, data));
    },
    warn: (message, data) => {
      writeLog(createLogEntry("warn", category, message, data));
    },
    error: (message, data) => {
      writeLog(createLogEntry("error", category, message, data));
    }
  };
}
var loggers = {
  service: createLogger("service"),
  model: createLogger("model"),
  hotkey: createLogger("hotkey"),
  audio: createLogger("audio"),
  injection: createLogger("injection"),
  config: createLogger("config")
};

// ../core/dist/tray.js
import { createWriteStream as createWriteStream2, existsSync as existsSync4, mkdirSync as mkdirSync4, readFileSync as readFileSync3, writeFileSync as writeFileSync3, rmSync as rmSync2, chmodSync, mkdtempSync, readdirSync as readdirSync2, cpSync } from "fs";
import { join as join3 } from "path";
import { tmpdir } from "os";
import { exec } from "child_process";
import { promisify } from "util";
var execAsync = promisify(exec);
var GITHUB_REPO = "oshtz/dybur";
var GITHUB_RELEASES_URL = `https://github.com/${GITHUB_REPO}/releases`;
var TRAY_APP_VERSION = "v1.2.1";
function getTrayAssetName() {
  const platform2 = getPlatform();
  const arch = getArch();
  if (platform2 === "darwin") {
    return `dybur-macos-${arch}.dmg`;
  }
  return `dybur-windows-${arch}.exe`;
}
function getTrayDownloadUrl(version = TRAY_APP_VERSION) {
  const assetName = getTrayAssetName();
  return `${GITHUB_RELEASES_URL}/download/${version}/${assetName}`;
}
function ensureBinDir() {
  const dir = getBinDir();
  if (!existsSync4(dir)) {
    mkdirSync4(dir, { recursive: true });
  }
  return dir;
}
async function downloadFile2(url, destPath, onProgress) {
  const response = await fetch(url, {
    redirect: "follow",
    headers: {
      "User-Agent": "dybur-cli"
    }
  });
  if (!response.ok) {
    throw new Error(`Download failed: ${response.status} ${response.statusText}`);
  }
  const contentLength = parseInt(response.headers.get("content-length") ?? "0", 10);
  const reader = response.body?.getReader();
  if (!reader) {
    throw new Error("Failed to get response reader");
  }
  const fileStream = createWriteStream2(destPath);
  let downloaded = 0;
  try {
    for (; ; ) {
      const { done, value } = await reader.read();
      if (done)
        break;
      fileStream.write(Buffer.from(value));
      downloaded += value.length;
      if (onProgress && contentLength > 0) {
        onProgress(downloaded, contentLength);
      }
    }
  } finally {
    fileStream.end();
    await new Promise((resolve, reject) => {
      fileStream.on("finish", resolve);
      fileStream.on("error", reject);
    });
  }
}
function quotePosixPath(path) {
  return `'${path.replace(/'/g, "'\\''")}'`;
}
async function installDmg(dmgPath, bundlePath) {
  const mountPoint = mkdtempSync(join3(tmpdir(), "dybur-dmg-"));
  let mounted = false;
  try {
    await execAsync(`hdiutil attach ${quotePosixPath(dmgPath)} -mountpoint ${quotePosixPath(mountPoint)} -nobrowse -readonly -quiet`);
    mounted = true;
    const appName = readdirSync2(mountPoint).find((name) => name.endsWith(".app"));
    if (!appName) {
      throw new Error("DMG did not contain a .app bundle");
    }
    cpSync(join3(mountPoint, appName), bundlePath, { recursive: true });
  } finally {
    if (mounted) {
      try {
        await execAsync(`hdiutil detach ${quotePosixPath(mountPoint)} -quiet`);
      } catch {
        await execAsync(`hdiutil detach ${quotePosixPath(mountPoint)} -force -quiet`).catch(() => void 0);
      }
    }
    rmSync2(mountPoint, { recursive: true, force: true });
  }
}
async function downloadTrayApp(version = TRAY_APP_VERSION, onProgress) {
  const platform2 = getPlatform();
  const arch = getArch();
  const binDir = ensureBinDir();
  const bundlePath = getTrayAppBundlePath();
  const trayPath = getTrayAppPath();
  if (existsSync4(bundlePath)) {
    rmSync2(bundlePath, { recursive: true, force: true });
  }
  const downloadUrl = getTrayDownloadUrl(version);
  const assetName = getTrayAssetName();
  const directExecutable = assetName.endsWith(".exe");
  const installerPath = join3(binDir, assetName);
  const downloadPath = directExecutable ? trayPath : installerPath;
  try {
    if (onProgress) {
      onProgress(0, 0, "Downloading tray application...");
    }
    await downloadFile2(downloadUrl, downloadPath, (downloaded, total) => {
      if (onProgress) {
        onProgress(downloaded, total);
      }
    });
    if (!directExecutable) {
      if (onProgress) {
        onProgress(0, 0, "Installing...");
      }
      if (isMacOS()) {
        await installDmg(installerPath, bundlePath);
        if (existsSync4(trayPath)) {
          chmodSync(trayPath, 493);
        }
        try {
          await execAsync(`xattr -rd com.apple.quarantine ${quotePosixPath(bundlePath)}`);
        } catch {
        }
      } else {
        throw new Error(`Unsupported tray app asset type: ${assetName}`);
      }
      rmSync2(installerPath, { force: true });
    }
    if (directExecutable) {
      chmodSync(trayPath, 493);
    }
    if (!existsSync4(trayPath)) {
      throw new Error("Installation failed: tray app binary not found");
    }
    const metadata = {
      version,
      platform: platform2,
      arch,
      downloadedAt: (/* @__PURE__ */ new Date()).toISOString(),
      source: downloadUrl
    };
    writeFileSync3(join3(binDir, "tray-metadata.json"), JSON.stringify(metadata, null, 2));
    return trayPath;
  } catch (error2) {
    if (existsSync4(downloadPath)) {
      rmSync2(downloadPath, { force: true });
    }
    if (existsSync4(bundlePath)) {
      rmSync2(bundlePath, { recursive: true, force: true });
    }
    throw error2;
  }
}

// src/ui.ts
import * as readline from "readline";
var colors = {
  reset: "\x1B[0m",
  bold: "\x1B[1m",
  dim: "\x1B[2m",
  italic: "\x1B[3m",
  underline: "\x1B[4m",
  // Foreground
  black: "\x1B[30m",
  red: "\x1B[31m",
  green: "\x1B[32m",
  yellow: "\x1B[33m",
  blue: "\x1B[34m",
  magenta: "\x1B[35m",
  cyan: "\x1B[36m",
  white: "\x1B[37m",
  gray: "\x1B[90m",
  // Bright
  brightRed: "\x1B[91m",
  brightGreen: "\x1B[92m",
  brightYellow: "\x1B[93m",
  brightBlue: "\x1B[94m",
  brightMagenta: "\x1B[95m",
  brightCyan: "\x1B[96m",
  brightWhite: "\x1B[97m",
  // Background
  bgBlue: "\x1B[44m",
  bgMagenta: "\x1B[45m",
  bgCyan: "\x1B[46m",
  // dybur brand colors (RGB true color)
  accent: "\x1B[38;2;94;255;192m",
  // #5effc0 - mint green accent
  textPrimary: "\x1B[38;2;201;255;232m"
  // #c9ffe8 - light mint text
};
var supportsColor = process.stdout.isTTY && !process.env["NO_COLOR"];
function c(color, text) {
  if (!supportsColor) return text;
  return `${colors[color]}${text}${colors.reset}`;
}
function bold(text) {
  return c("bold", text);
}
function dim(text) {
  return c("dim", text);
}
function green(text) {
  return c("accent", text);
}
function red(text) {
  return c("red", text);
}
function yellow(text) {
  return c("yellow", text);
}
function cyan(text) {
  return c("accent", text);
}
function gray(text) {
  return c("gray", text);
}
var brand = {
  primary: (text) => c("textPrimary", text),
  accent: (text) => c("accent", text),
  success: (text) => c("accent", text),
  // Use brand accent for success
  error: red,
  warning: yellow,
  info: (text) => c("textPrimary", text),
  muted: gray,
  highlight: bold
};
var LOGO = `
${brand.primary("\u250C\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2510")}
${brand.primary("\u2502")}  ${brand.accent("dybur")} ${dim("- local voice dictation")}     ${brand.primary("\u2502")}
${brand.primary("\u2514\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2500\u2518")}
`;
var LOGO_INLINE = `${brand.accent("dybur")}`;
var icons = {
  success: c("accent", "\u2713"),
  // U+2713 Check Mark
  error: red("\u2717"),
  // U+2717 Ballot X
  warning: yellow("\u26A0"),
  // U+26A0 Warning Sign
  info: c("textPrimary", "\u2733"),
  // U+2733 Eight Spoked Asterisk
  arrow: c("accent", "\u2192"),
  // U+2192 Rightwards Arrow
  bullet: dim("\u2022"),
  // U+2022 Bullet
  recording: red("\u25CF"),
  // U+25CF Black Circle
  idle: dim("\u25CB"),
  // U+25CB White Circle
  spinner: ["\u280B", "\u2819", "\u2839", "\u2838", "\u283C", "\u2834", "\u2826", "\u2827", "\u2807", "\u280F"]
  // Braille pattern dots
};
var box = {
  topLeft: "\u250C",
  topRight: "\u2510",
  bottomLeft: "\u2514",
  bottomRight: "\u2518",
  horizontal: "\u2500",
  vertical: "\u2502",
  teeRight: "\u251C",
  teeLeft: "\u2524"
};
function header(title, subtitle) {
  console.log("");
  console.log(`  ${brand.accent("\u25B8")} ${bold(title)}`);
  if (subtitle) {
    console.log(`    ${dim(subtitle)}`);
  }
  console.log("");
}
function divider(char = "\u2500", width = 40) {
  console.log(dim(char.repeat(width)));
}
function keyValue(key, value, indent = 2) {
  const spaces = " ".repeat(indent);
  console.log(`${spaces}${dim(key + ":")} ${value}`);
}
function success(message) {
  console.log(`  ${icons.success} ${message}`);
}
function error(message) {
  console.log(`  ${icons.error} ${red(message)}`);
}
function warning(message) {
  console.log(`  ${icons.warning} ${yellow(message)}`);
}
function info(message) {
  console.log(`  ${icons.info} ${message}`);
}
function command(cmd, description) {
  if (description) {
    console.log(`  ${c("accent", cmd)}  ${dim(description)}`);
  } else {
    console.log(`  ${c("accent", cmd)}`);
  }
}
function progressBar(current, total, width = 30) {
  const rawPercent = total > 0 ? current / total : 0;
  const percent = Math.min(Math.max(rawPercent, 0), 1);
  const filled = Math.round(width * percent);
  const empty = width - filled;
  const bar = brand.primary("\u2588".repeat(filled)) + dim("\u2591".repeat(empty));
  const percentStr = `${Math.round(percent * 100)}%`.padStart(4);
  return `${bar} ${percentStr}`;
}
var Spinner = class {
  frame = 0;
  interval = null;
  message;
  constructor(message) {
    this.message = message;
  }
  start() {
    if (!supportsColor) {
      console.log(`  ${this.message}...`);
      return;
    }
    process.stdout.write(`  ${c("accent", icons.spinner[0])} ${this.message}`);
    this.interval = setInterval(() => {
      this.frame = (this.frame + 1) % icons.spinner.length;
      process.stdout.write(`\r  ${c("accent", icons.spinner[this.frame])} ${this.message}`);
    }, 80);
  }
  stop(finalMessage) {
    if (this.interval) {
      clearInterval(this.interval);
      this.interval = null;
    }
    if (supportsColor) {
      process.stdout.write("\r" + " ".repeat(this.message.length + 10) + "\r");
    }
    if (finalMessage) {
      console.log(`  ${finalMessage}`);
    }
  }
  succeed(message) {
    this.stop(`${icons.success} ${message ?? this.message}`);
  }
  fail(message) {
    this.stop(`${icons.error} ${red(message ?? this.message)}`);
  }
};
function formatSize(bytes) {
  if (bytes === 0) return dim("0 B");
  const units = ["B", "KB", "MB", "GB"];
  const k = 1024;
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  const value = (bytes / Math.pow(k, i)).toFixed(1);
  return `${value} ${dim(units[i] ?? "GB")}`;
}
function formatPath(path, maxLen = 50) {
  if (path.length <= maxLen) return dim(path);
  return dim("..." + path.slice(-(maxLen - 3)));
}
function banner() {
  console.log(LOGO);
}
function boxMessage(lines, title) {
  const maxLen = Math.max(...lines.map((l) => l.length), title?.length ?? 0);
  const width = maxLen + 4;
  console.log("");
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
  console.log("");
}
function stripAnsi(str) {
  return str.replace(/\x1b\[[0-9;]*m/g, "");
}
function getCharWidth(char) {
  const code = char.codePointAt(0);
  if (code === void 0) return 0;
  if (code < 32 || code >= 127 && code < 160) return 0;
  if (code >= 4352 && code <= 4447 || // Hangul Jamo
  code >= 11904 && code <= 42191 && code !== 12351 || // CJK
  code >= 44032 && code <= 55203 || // Hangul syllables
  code >= 63744 && code <= 64255 || // CJK compatibility
  code >= 65040 && code <= 65055 || // Vertical forms
  code >= 65072 && code <= 65135 || // CJK compatibility forms
  code >= 65280 && code <= 65376 || // Fullwidth forms
  code >= 65504 && code <= 65510 || // Fullwidth forms
  code >= 127744 && code <= 129535 || // Emojis
  code >= 131072 && code <= 196605 || // CJK extension
  code >= 196608 && code <= 262141) {
    return 2;
  }
  return 1;
}
function getStringWidth(str) {
  const plain = stripAnsi(str);
  let width = 0;
  for (const char of plain) {
    width += getCharWidth(char);
  }
  return width;
}
function getVisualLineCount(str, columns) {
  if (columns <= 0) return 1;
  const width = getStringWidth(str);
  return Math.max(1, Math.ceil(width / columns));
}
async function select(options) {
  const { message, choices, initial = 0 } = options;
  const canUseRawMode = process.stdin.setRawMode !== void 0;
  if (!process.stdin.isTTY && !canUseRawMode) {
    return choices[0]?.value ?? null;
  }
  return new Promise((resolve) => {
    let selectedIndex = initial;
    let rendered = false;
    let cleanedUp = false;
    let lastVisualLines = 0;
    readline.emitKeypressEvents(process.stdin);
    if (process.stdin.setRawMode) {
      process.stdin.setRawMode(true);
    }
    process.stdin.resume();
    let ignoreInput = true;
    const render = () => {
      const columns = process.stdout.columns || 80;
      if (rendered && lastVisualLines > 0) {
        process.stdout.write(`\x1B[${lastVisualLines}A\x1B[0J`);
      }
      rendered = true;
      const lines = [];
      lines.push(`  ${brand.accent("?")} ${bold(message)}`);
      for (let i = 0; i < choices.length; i++) {
        const choice = choices[i];
        const isSelected = i === selectedIndex;
        const cursor = isSelected ? brand.accent("\u276F") : " ";
        const label = isSelected ? brand.accent(choice.label) : choice.label;
        const hint = choice.hint ? dim(` (${choice.hint})`) : "";
        lines.push(`  ${cursor} ${label}${hint}`);
      }
      lastVisualLines = lines.reduce((sum, line) => sum + getVisualLineCount(line, columns), 0);
      for (const line of lines) {
        console.log(line);
      }
    };
    const cleanup = () => {
      if (cleanedUp) return;
      cleanedUp = true;
      process.stdin.removeListener("keypress", onKeypress);
      if (process.stdin.setRawMode) {
        process.stdin.setRawMode(false);
      }
      process.stdin.pause();
      console.log("");
    };
    const onKeypress = (_str, key) => {
      if (key.ctrl && key.name === "c") {
        cleanup();
        process.exit(0);
      }
      if (ignoreInput) {
        return;
      }
      if (key.name === "up" || key.name === "k") {
        selectedIndex = selectedIndex > 0 ? selectedIndex - 1 : choices.length - 1;
        render();
      } else if (key.name === "down" || key.name === "j") {
        selectedIndex = selectedIndex < choices.length - 1 ? selectedIndex + 1 : 0;
        render();
      } else if (key.name === "return") {
        cleanup();
        resolve(choices[selectedIndex]?.value ?? null);
      } else if (key.name === "escape" || key.name === "q") {
        cleanup();
        resolve(null);
      }
    };
    process.stdin.on("keypress", onKeypress);
    render();
    setTimeout(() => {
      ignoreInput = false;
    }, 50);
  });
}

// src/commands/start.ts
function isTrayAppRunning() {
  if (!isMacOS()) {
    return false;
  }
  try {
    const result = execSync("pgrep -x dybur", {
      encoding: "utf-8",
      stdio: ["pipe", "pipe", "pipe"]
    });
    return result.trim().length > 0;
  } catch {
    return false;
  }
}
function findTrayAppPath() {
  const devPaths = [
    join4(process.cwd(), "apps", "tray", "src-tauri", "target", "release", "dybur.exe"),
    join4(process.cwd(), "apps", "tray", "src-tauri", "target", "release", "dybur"),
    process.env["DYBUR_TRAY_PATH"]
  ].filter(Boolean);
  for (const p of devPaths) {
    if (existsSync5(p)) {
      return p;
    }
  }
  const installedPath = getTrayAppPath();
  if (existsSync5(installedPath)) {
    return installedPath;
  }
  if (isMacOS()) {
    const macOSPaths = [
      "/Applications/dybur.app/Contents/MacOS/dybur",
      join4(homedir2(), "Applications", "dybur.app", "Contents", "MacOS", "dybur")
    ];
    for (const p of macOSPaths) {
      if (existsSync5(p)) {
        return p;
      }
    }
  }
  return null;
}
async function startCommand(_args) {
  header("Starting dybur");
  const config = loadConfig();
  keyValue("Model", config.model);
  keyValue("Hotkey", brand.accent(config.hotkey));
  console.log("");
  const modelId = config.model ?? DEFAULT_MODEL;
  if (!isModelInstalled(modelId)) {
    warning(`Model not found: ${modelId}`);
    info("Downloading model from HuggingFace...");
    console.log(`  ${dim("This only needs to happen once")}`);
    console.log("");
    let lastFile = "";
    try {
      await downloadModel(modelId, (downloaded, total, file) => {
        if (file && file !== lastFile) {
          if (lastFile) {
            process.stdout.write("\n");
          }
          lastFile = file;
          console.log(`  ${dim("Downloading:")} ${file}`);
        }
        if (total > 0) {
          const bar = progressBar(downloaded, total);
          process.stdout.write(`\r  ${bar}`);
        }
      });
      console.log("\n");
      success("Model downloaded");
      console.log("");
    } catch (err) {
      console.log("\n");
      error(`Failed to download model: ${err}`);
      info(`Run ${cyan(`dybur models download ${modelId}`)} to try again`);
      process.exit(1);
    }
  }
  if (isTrayAppRunning()) {
    success("dybur is already running");
    console.log("");
    info(`Press ${brand.accent(config.hotkey)} to begin dictating`);
    console.log("");
    return;
  }
  let trayPath = findTrayAppPath();
  if (!trayPath) {
    warning("Tray application not found");
    info(`Downloading from GitHub releases (${TRAY_APP_VERSION})...`);
    console.log(`  ${dim("This only needs to happen once")}`);
    console.log("");
    try {
      trayPath = await downloadTrayApp(TRAY_APP_VERSION, (downloaded, total, status) => {
        if (status) {
          console.log(`  ${dim(status)}`);
        } else if (total > 0) {
          const bar = progressBar(downloaded, total);
          process.stdout.write(`\r  ${bar}`);
        }
      });
      console.log("\n");
      success("Tray application installed");
      console.log("");
    } catch (err) {
      console.log("\n");
      error(`Failed to download tray application: ${err}`);
      console.log("");
      info("You can try:");
      console.log(`  ${dim("1.")} Check your internet connection`);
      console.log(
        `  ${dim("2.")} Download manually from ${cyan("https://github.com/oshtz/dybur/releases")}`
      );
      console.log(`  ${dim("3.")} Build from source: ${cyan("cd apps/tray && pnpm tauri build")}`);
      process.exit(1);
    }
  }
  const spinner = new Spinner("Launching tray application");
  spinner.start();
  const child = spawn(trayPath, [], {
    detached: true,
    stdio: "ignore"
  });
  child.unref();
  await new Promise((resolve) => setTimeout(resolve, 500));
  spinner.succeed("dybur started");
  console.log("");
  info(`Press ${brand.accent(config.hotkey)} to begin dictating`);
  if (isMacOS()) {
    console.log("");
    console.log(`  ${dim("Note: You may need to grant accessibility permissions")}`);
    console.log(`  ${dim("System Settings > Privacy & Security > Accessibility")}`);
  }
  console.log("");
}

// src/commands/stop.ts
import { exec as exec2 } from "child_process";
import { promisify as promisify2 } from "util";
var execAsync2 = promisify2(exec2);
async function killTrayProcess() {
  try {
    if (isWindows()) {
      await execAsync2("taskkill /IM dybur.exe /F");
      return true;
    } else if (isMacOS()) {
      await execAsync2("pkill -f dybur");
      return true;
    }
  } catch {
    return false;
  }
  return false;
}
async function stopCommand(_args) {
  header("Stopping dybur");
  const spinner = new Spinner("Stopping service");
  spinner.start();
  const killed = await killTrayProcess();
  if (killed) {
    spinner.succeed("dybur stopped");
  } else {
    spinner.stop();
    info("dybur was not running");
  }
  console.log("");
}

// src/commands/status.ts
import { exec as exec3 } from "child_process";
import { promisify as promisify3 } from "util";
var execAsync3 = promisify3(exec3);
async function isTrayRunning() {
  try {
    if (isWindows()) {
      const { stdout } = await execAsync3('tasklist /FI "IMAGENAME eq dybur.exe"');
      return stdout.includes("dybur.exe");
    } else if (isMacOS()) {
      const { stdout } = await execAsync3("pgrep -f dybur");
      return stdout.trim().length > 0;
    }
  } catch {
    return false;
  }
  return false;
}
function statusIcon(ok) {
  return ok ? green("\u25CF") : red("\u25CB");
}
async function statusCommand(_args) {
  header("dybur Status");
  const config = loadConfig({ createIfMissing: false });
  const activeModel = config.model ?? DEFAULT_MODEL;
  const running = await isTrayRunning();
  const modelInstalled = isModelInstalled(activeModel);
  const modelMeta = modelInstalled ? getModelMetadata(activeModel) : null;
  const paths = getAllPaths();
  console.log(
    `  ${statusIcon(running)} ${dim("Service:")}     ${running ? green("Running") : red("Stopped")}`
  );
  console.log(
    `  ${statusIcon(modelInstalled)} ${dim("Model:")}       ${modelInstalled ? green(activeModel) : red(`${activeModel} not installed`)}`
  );
  if (modelMeta) {
    console.log(`              ${dim("Downloaded:")} ${modelMeta.downloadedAt.split("T")[0]}`);
    if (modelMeta.variant) {
      console.log(`              ${dim("Variant:")}    ${modelMeta.variant}`);
    }
  }
  console.log("");
  divider();
  console.log("");
  console.log(`  ${brand.accent("Configuration")}`);
  console.log(`  ${dim("Hotkey:")}      ${brand.accent(config.hotkey)}`);
  console.log(
    `  ${dim("Punctuation:")} ${config.autoPunctuation ? green("enabled") : dim("disabled")}`
  );
  console.log(
    `  ${dim("Sentence case:")} ${config.sentenceCase ? green("enabled") : dim("disabled")}`
  );
  console.log(`  ${dim("Silence timeout:")} ${config.silenceTimeoutMs}ms`);
  console.log(
    `  ${dim("Recording mode:")} ${config.recordingMode === "push_to_talk" ? "push-to-talk" : "toggle"}`
  );
  console.log("");
  divider();
  console.log("");
  console.log(`  ${brand.accent("Paths")}`);
  console.log(`  ${dim("Config:")}  ${formatPath(paths.configPath, 45)}`);
  console.log(`  ${dim("Models:")}  ${formatPath(paths.modelsDir, 45)}`);
  console.log(`  ${dim("Logs:")}    ${formatPath(paths.logsDir, 45)}`);
  console.log("");
  divider();
  console.log("");
  if (running && modelInstalled) {
    success(`Ready ${dim("- press")} ${brand.accent(config.hotkey)} ${dim("to dictate")}`);
  } else if (!modelInstalled) {
    warning("Model required");
    info(`Run ${cyan(`dybur models download ${activeModel}`)} to download`);
  } else {
    warning("Service not running");
    info(`Run ${cyan("dybur start")} to begin`);
  }
  console.log("");
}

// src/commands/settings.ts
import { exec as exec4 } from "child_process";
import { existsSync as existsSync6 } from "fs";
function openInEditor(filePath) {
  if (isWindows()) {
    exec4(`start "" "${filePath}"`);
  } else if (isMacOS()) {
    exec4(`open "${filePath}"`);
  }
}
async function settingsCommand(args) {
  const configPath = getConfigPath();
  if (args.includes("--path")) {
    console.log(configPath);
    return;
  }
  if (args.includes("--show")) {
    header("Current Configuration");
    const config = loadConfig();
    keyValue("Hotkey", brand.accent(config.hotkey));
    keyValue("Auto punctuation", config.autoPunctuation ? "enabled" : "disabled");
    keyValue("Sentence case", config.sentenceCase ? "enabled" : "disabled");
    keyValue("Silence timeout", `${config.silenceTimeoutMs}ms`);
    keyValue("Model", config.model);
    keyValue("Clipboard cleanup", config.clipboardCleanup ? "enabled" : "disabled");
    keyValue("Recording mode", config.recordingMode === "push_to_talk" ? "push-to-talk" : "toggle");
    keyValue("VAD (silence filter)", config.vadEnabled ? "enabled" : "disabled");
    keyValue("Streaming preview", config.streamingEnabled ? "enabled" : "disabled");
    if (config.vadEnabled) {
      keyValue("  VAD threshold", `${config.vadThreshold}`);
      keyValue("  VAD min speech", `${config.vadMinSpeechMs}ms`);
    }
    console.log("");
    console.log(`  ${dim("Path:")} ${configPath}`);
    console.log("");
    return;
  }
  header("Settings");
  if (!existsSync6(configPath)) {
    loadConfig({ createIfMissing: true });
    info("Created default config");
  }
  const spinner = new Spinner("Opening config in editor");
  spinner.start();
  openInEditor(configPath);
  spinner.succeed("Config opened");
  console.log("");
  console.log(`  ${dim("Path:")} ${configPath}`);
  console.log("");
  info("Restart dybur after making changes");
  console.log("");
}

// src/commands/doctor.ts
import { existsSync as existsSync7 } from "fs";
import { exec as exec5 } from "child_process";
import { promisify as promisify4 } from "util";
var execAsync4 = promisify4(exec5);
function checkConfig() {
  const configPath = getConfigPath();
  if (!existsSync7(configPath)) {
    return {
      name: "Configuration",
      status: "warn",
      message: "Config file not found",
      details: `Will be created at: ${configPath}`
    };
  }
  try {
    const config = loadConfig();
    const validation = validateConfig(config);
    if (!validation.valid) {
      const errors = validation.errors.map((e) => `${e.field}: ${e.message}`).join(", ");
      return {
        name: "Configuration",
        status: "warn",
        message: "Config has validation warnings",
        details: errors
      };
    }
    return {
      name: "Configuration",
      status: "pass",
      message: "Valid configuration",
      details: `Hotkey: ${config.hotkey}`
    };
  } catch (err) {
    return {
      name: "Configuration",
      status: "fail",
      message: "Failed to load config",
      details: String(err)
    };
  }
}
function checkModel() {
  const config = loadConfig();
  const activeModel = config.model ?? DEFAULT_MODEL;
  if (!isModelInstalled(activeModel)) {
    return {
      name: "Speech Model",
      status: "fail",
      message: `Model not installed: ${activeModel}`,
      details: `Run: dybur models download ${activeModel}`
    };
  }
  const metadata = getModelMetadata(activeModel);
  if (!metadata) {
    return {
      name: "Speech Model",
      status: "warn",
      message: "Model installed but metadata missing"
    };
  }
  return {
    name: "Speech Model",
    status: "pass",
    message: activeModel,
    details: `${metadata.variant ?? "full"} variant, downloaded ${metadata.downloadedAt.split("T")[0]}`
  };
}
async function checkAudioDevice() {
  try {
    if (isWindows()) {
      const { stdout } = await execAsync4(
        'powershell -Command "Get-WmiObject Win32_SoundDevice | Select-Object Name"'
      );
      if (stdout.toLowerCase().includes("microphone") || stdout.toLowerCase().includes("audio")) {
        return {
          name: "Audio Device",
          status: "pass",
          message: "Audio device detected"
        };
      }
    } else if (isMacOS()) {
      const { stdout } = await execAsync4("system_profiler SPAudioDataType 2>/dev/null | head -20");
      if (stdout.length > 0) {
        return {
          name: "Audio Device",
          status: "pass",
          message: "Audio device detected"
        };
      }
    }
    return {
      name: "Audio Device",
      status: "warn",
      message: "Unable to verify audio device",
      details: "Manual verification required"
    };
  } catch {
    return {
      name: "Audio Device",
      status: "warn",
      message: "Unable to check audio devices"
    };
  }
}
function checkHotkey() {
  const config = loadConfig();
  const validation = validateConfig({ hotkey: config.hotkey });
  if (!validation.valid) {
    return {
      name: "Hotkey",
      status: "fail",
      message: "Invalid hotkey configuration",
      details: validation.errors[0]?.message
    };
  }
  return {
    name: "Hotkey",
    status: "pass",
    message: config.hotkey,
    details: "Full test requires running service"
  };
}
function checkInputDevice() {
  const config = loadConfig();
  const inputDevice = config.inputDevice;
  if (!inputDevice) {
    return {
      name: "Input Device",
      status: "pass",
      message: "Using system default",
      details: 'Run "dybur devices list" to see available devices'
    };
  }
  return {
    name: "Input Device",
    status: "pass",
    message: inputDevice,
    details: "Device availability verified at recording time"
  };
}
function checkDirectories() {
  const paths = getAllPaths();
  const issues = [];
  if (!existsSync7(paths.configDir)) {
    issues.push("Config directory missing");
  }
  if (!existsSync7(paths.dataDir)) {
    issues.push("Data directory missing");
  }
  if (issues.length > 0) {
    return {
      name: "Directories",
      status: "warn",
      message: "Some directories missing",
      details: "Will be created on first use"
    };
  }
  return {
    name: "Directories",
    status: "pass",
    message: "All directories accessible"
  };
}
function formatResult(result) {
  const statusIcons = {
    pass: green("\u25CF"),
    warn: yellow("\u25CF"),
    fail: red("\u25CF")
  };
  const statusColors = {
    pass: green,
    warn: yellow,
    fail: red
  };
  const icon = statusIcons[result.status];
  const colorFn = statusColors[result.status];
  console.log(`  ${icon} ${dim(result.name)}`);
  console.log(`    ${colorFn(result.message)}`);
  if (result.details) {
    console.log(`    ${dim(result.details)}`);
  }
}
async function doctorCommand(_args) {
  header("dybur Diagnostics");
  const spinner = new Spinner("Running checks");
  spinner.start();
  const results = [];
  results.push(checkConfig());
  results.push(checkModel());
  results.push(await checkAudioDevice());
  results.push(checkHotkey());
  results.push(checkInputDevice());
  results.push(checkDirectories());
  spinner.stop();
  for (const result of results) {
    formatResult(result);
    console.log("");
  }
  divider();
  console.log("");
  const passed = results.filter((r) => r.status === "pass").length;
  const warnings = results.filter((r) => r.status === "warn").length;
  const failed = results.filter((r) => r.status === "fail").length;
  console.log(
    `  ${green("\u25CF")} ${passed} passed  ${yellow("\u25CF")} ${warnings} warnings  ${red("\u25CF")} ${failed} failed`
  );
  console.log("");
  if (failed > 0) {
    error("Some checks failed - see details above");
    process.exit(1);
  } else if (warnings > 0) {
    warning("All critical checks passed with warnings");
  } else {
    success("All checks passed - dybur is ready");
  }
  console.log("");
  console.log(`  ${dim("Log file:")} ${getLogFilePath()}`);
  console.log("");
}

// src/commands/models.ts
function showModelsHelp() {
  header("Model Management");
  console.log(
    `  ${dim("dybur supports multiple STT models with different accuracy/speed tradeoffs.")}`
  );
  console.log(`  ${dim("Models are downloaded from HuggingFace.")}`);
  console.log("");
  divider();
  console.log("");
  console.log(`  ${brand.accent("Commands")}`);
  command("m, m l, m list", "List installed models");
  command("m list -a", "Show all available models");
  command("m d, m download", "Download a model (interactive)");
  command("m s, m set", "Set active model (interactive)");
  command("m prefetch", "Download default model");
  command("m clean", "Remove unused models");
  console.log("");
  console.log(`  ${brand.accent("Examples")}`);
  console.log(`  ${cyan("dybur m d")}                    ${dim("Interactive model download")}`);
  console.log(`  ${cyan("dybur m s")}                    ${dim("Interactive model selection")}`);
  console.log(
    `  ${cyan("dybur m d whisper-large-v3-turbo-int8")}  ${dim("Download specific model")}`
  );
  console.log("");
  const defaultModel = getDefaultModelDefinition();
  console.log(`  ${brand.accent("Default Model")}`);
  console.log(`  ${dim("ID:")}     ${defaultModel.id}`);
  console.log(`  ${dim("Name:")}   ${defaultModel.displayName}`);
  console.log(`  ${dim("Size:")}   ${formatBytes(defaultModel.sizeBytes)}`);
  console.log("");
  console.log(`  ${brand.accent("Available Models")}`);
  const models = getAvailableModels();
  for (const m of models) {
    const badge = m.isDefault ? ` ${green("[default]")}` : "";
    console.log(`  ${dim(icons.bullet)} ${m.id}${badge} - ${formatBytes(m.sizeBytes)}`);
  }
  console.log("");
}
async function listCommand(showAvailable = false) {
  const modelsDir = getModelsDir();
  const config = loadConfig();
  const activeModelId = config.model ?? DEFAULT_MODEL;
  if (showAvailable) {
    header("Available Models");
    const availableModels = getAvailableModels();
    for (const model of availableModels) {
      const installed = isModelInstalled(model.id);
      const isActive = model.id === activeModelId;
      const badges = [];
      if (model.isDefault) badges.push(green("[default]"));
      if (installed) badges.push(green("[installed]"));
      if (isActive && installed) badges.push(cyan("[active]"));
      const badgeStr = badges.length > 0 ? ` ${badges.join(" ")}` : "";
      const size = formatBytes(model.sizeBytes);
      console.log(`  ${brand.accent(icons.bullet)} ${model.id}${badgeStr}`);
      console.log(`    ${dim("Name:")} ${model.displayName}`);
      console.log(`    ${dim("Description:")} ${model.description}`);
      console.log(`    ${dim("Size:")} ${size}`);
      console.log(`    ${dim("Architecture:")} ${model.architecture}`);
      if (model.languages.length > 0) {
        console.log(`    ${dim("Languages:")} ${model.languages.join(", ")}`);
      } else {
        console.log(`    ${dim("Languages:")} All (99+)`);
      }
      console.log("");
    }
    divider();
    console.log("");
    console.log(`  ${dim("To download a model:")} ${cyan("dybur models download <model-id>")}`);
    console.log(`  ${dim("To set active model:")} ${cyan("dybur models set <model-id>")}`);
    console.log("");
    return;
  }
  header("Installed Models");
  const models = listModels();
  if (models.length === 0) {
    info("No models installed");
    console.log("");
    console.log(`  ${dim("To install the default model:")}`);
    console.log(`  ${cyan(`dybur models download ${DEFAULT_MODEL}`)}`);
    console.log("");
    console.log(`  ${dim("To see all available models:")}`);
    console.log(`  ${cyan("dybur models list --available")}`);
    console.log("");
    console.log(`  ${dim("Models directory:")} ${formatPath(modelsDir, 45)}`);
    console.log("");
    return;
  }
  for (const model of models) {
    const isActive = model.name === activeModelId;
    const badges = [];
    if (model.isDefault) badges.push(green("[default]"));
    if (isActive) badges.push(cyan("[active]"));
    const badgeStr = badges.length > 0 ? ` ${badges.join(" ")}` : "";
    const size = formatSize(model.size);
    const modelDef = getModelDefinition(model.name);
    console.log(`  ${brand.accent(icons.bullet)} ${model.name}${badgeStr}`);
    if (modelDef) {
      console.log(`    ${dim("Name:")} ${modelDef.displayName}`);
    }
    console.log(`    ${dim("Size:")} ${size}`);
    if (model.metadata) {
      console.log(`    ${dim("Downloaded:")} ${model.metadata.downloadedAt.split("T")[0]}`);
      if (model.metadata.source) {
        console.log(`    ${dim("Source:")} ${model.metadata.source}`);
      }
    }
    console.log("");
  }
  divider();
  console.log("");
  console.log(`  ${dim("Active model:")} ${activeModelId}`);
  console.log(`  ${dim("Models directory:")} ${formatPath(modelsDir, 45)}`);
  console.log("");
}
async function selectModelForDownload() {
  const availableModels = getAvailableModels();
  const config = loadConfig();
  const activeModelId = config.model ?? DEFAULT_MODEL;
  const notInstalled = availableModels.filter((m) => !isModelInstalled(m.id));
  const installed = availableModels.filter((m) => isModelInstalled(m.id));
  if (notInstalled.length === 0) {
    info("All models are already installed");
    console.log("");
    return void 0;
  }
  const choices = [
    ...notInstalled.map((model) => ({
      label: `${model.displayName} (${formatBytes(model.sizeBytes)})`,
      value: model.id,
      hint: model.description
    })),
    // Add separator if there are installed models
    ...installed.length > 0 ? [
      {
        label: dim("\u2500\u2500\u2500 Already Installed \u2500\u2500\u2500"),
        value: "__separator__",
        hint: ""
      },
      ...installed.map((model) => ({
        label: `${model.displayName} (${formatBytes(model.sizeBytes)})`,
        value: model.id,
        hint: model.id === activeModelId ? `${green("[installed]")} ${cyan("[active]")}` : green("[installed]")
      }))
    ] : []
  ];
  const selected = await select({
    message: "Select model to download",
    choices,
    initial: 0
  });
  if (selected === null || selected === "__separator__") {
    return void 0;
  }
  return selected;
}
async function downloadCommand(modelId) {
  header("Download Model");
  if (!modelId) {
    const selectedId = await selectModelForDownload();
    if (!selectedId) {
      info("Download cancelled");
      console.log("");
      return;
    }
    modelId = selectedId;
  }
  const modelDef = getModelDefinition(modelId);
  if (!modelDef) {
    error(`Unknown model: ${modelId}`);
    console.log("");
    console.log(`  ${dim("Available models:")}`);
    for (const m of getAvailableModels()) {
      console.log(`  ${dim(icons.bullet)} ${m.id}`);
    }
    console.log("");
    process.exit(1);
  }
  if (isModelInstalled(modelId)) {
    success(`Model already installed: ${modelId}`);
    console.log("");
    console.log(`  ${dim("To set as active:")} ${cyan(`dybur models set ${modelId}`)}`);
    console.log("");
    return;
  }
  console.log(`  ${dim("Model:")}  ${modelDef.displayName}`);
  console.log(`  ${dim("ID:")}     ${modelDef.id}`);
  console.log(`  ${dim("Size:")}   ${formatBytes(modelDef.sizeBytes)}`);
  console.log(`  ${dim("Source:")} huggingface.co/${modelDef.repo}`);
  console.log("");
  divider();
  console.log("");
  let currentFile = "";
  try {
    await downloadModel(modelId, (downloaded, total, file) => {
      if (file && file !== currentFile) {
        if (currentFile) {
          process.stdout.write("\n");
        }
        currentFile = file;
        console.log(`  ${file}`);
      }
      if (total > 0) {
        const bar = progressBar(downloaded, total, 25);
        process.stdout.write(`\r  ${bar}`);
      }
    });
    console.log("\n");
    divider();
    console.log("");
    success(`Model downloaded successfully: ${modelId}`);
    console.log("");
    console.log(`  ${dim("To set as active:")} ${cyan(`dybur models set ${modelId}`)}`);
    console.log("");
  } catch (err) {
    console.log("\n");
    error(`Download failed: ${err}`);
    console.log("");
    info("Check your internet connection and try again");
    console.log("");
    process.exit(1);
  }
}
async function prefetchCommand() {
  await downloadCommand(DEFAULT_MODEL);
}
async function selectModelForSet() {
  const installedModels = listModels();
  const config = loadConfig();
  const activeModelId = config.model ?? DEFAULT_MODEL;
  if (installedModels.length === 0) {
    error("No models installed");
    console.log("");
    console.log(`  ${dim("To download a model:")}`);
    console.log(`  ${cyan("dybur models download")}`);
    console.log("");
    return void 0;
  }
  const choices = installedModels.map((model) => {
    const modelDef = getModelDefinition(model.name);
    const isActive = model.name === activeModelId;
    return {
      label: modelDef?.displayName ?? model.name,
      value: model.name,
      hint: isActive ? cyan("[active]") : modelDef?.description ?? ""
    };
  });
  const currentIndex = choices.findIndex((c2) => c2.value === activeModelId);
  const selected = await select({
    message: "Select active model",
    choices,
    initial: currentIndex >= 0 ? currentIndex : 0
  });
  return selected ?? void 0;
}
async function setCommand(modelId) {
  header("Set Active Model");
  if (!modelId) {
    const selectedId = await selectModelForSet();
    if (!selectedId) {
      info("Selection cancelled");
      console.log("");
      return;
    }
    modelId = selectedId;
  }
  const modelDef = getModelDefinition(modelId);
  if (!modelDef) {
    error(`Unknown model: ${modelId}`);
    console.log("");
    console.log(`  ${dim("Available models:")}`);
    for (const m of getAvailableModels()) {
      console.log(`  ${dim(icons.bullet)} ${m.id}`);
    }
    console.log("");
    process.exit(1);
  }
  if (!isModelInstalled(modelId)) {
    error(`Model not installed: ${modelId}`);
    console.log("");
    console.log(`  ${dim("To download this model:")}`);
    console.log(`  ${cyan(`dybur models download ${modelId}`)}`);
    console.log("");
    process.exit(1);
  }
  const config = loadConfig();
  const oldModelId = config.model ?? DEFAULT_MODEL;
  if (oldModelId === modelId) {
    info(`Model already active: ${modelId}`);
    console.log("");
    return;
  }
  updateConfig({ model: modelId });
  success(`Active model changed: ${oldModelId} -> ${modelId}`);
  console.log("");
  console.log(`  ${dim("Name:")} ${modelDef.displayName}`);
  console.log(`  ${dim("Architecture:")} ${modelDef.architecture}`);
  console.log("");
  info(`Restart dybur for changes to take effect`);
  console.log("");
}
async function cleanCommand() {
  header("Clean Models");
  const spinner = new Spinner("Scanning for unused models");
  spinner.start();
  const removed = cleanModels();
  spinner.stop();
  if (removed.length === 0) {
    info("No unused models to remove");
    console.log("");
    return;
  }
  success(`Removed ${removed.length} model(s):`);
  console.log("");
  for (const name of removed) {
    console.log(`  ${dim(icons.bullet)} ${name}`);
  }
  console.log("");
}
async function modelsCommand(args) {
  const subcommand = args[0];
  switch (subcommand) {
    case "list":
    case "l": {
      const showAvailable = args.includes("--available") || args.includes("-a");
      await listCommand(showAvailable);
      break;
    }
    case "download":
    case "d": {
      const downloadModelId = args[1];
      await downloadCommand(downloadModelId);
      break;
    }
    case "set":
    case "s": {
      const setModelId = args[1];
      await setCommand(setModelId);
      break;
    }
    case "prefetch":
      await prefetchCommand();
      break;
    case "clean":
      await cleanCommand();
      break;
    case void 0:
    case "--help":
    case "-h":
      showModelsHelp();
      break;
    default:
      error(`Unknown subcommand: ${subcommand}`);
      console.log("");
      showModelsHelp();
      process.exit(1);
  }
}

// src/commands/devices.ts
import { exec as exec6 } from "child_process";
import { promisify as promisify5 } from "util";
var execAsync5 = promisify5(exec6);
async function listAudioDevices() {
  const devices = [];
  try {
    if (isWindows()) {
      const { stdout } = await execAsync5(
        `powershell -Command "Get-CimInstance Win32_SoundDevice | Where-Object { $_.Status -eq 'OK' } | Select-Object -ExpandProperty Name"`,
        { timeout: 1e4 }
      );
      const lines = stdout.split("\n").map((l) => l.trim()).filter(Boolean);
      try {
        const { stdout: captureOutput } = await execAsync5(
          `powershell -Command "[System.Reflection.Assembly]::LoadWithPartialName('System.Speech') | Out-Null; $recognizer = New-Object System.Speech.Recognition.SpeechRecognizer; $recognizer.AudioDeviceNames | ForEach-Object { Write-Output $_ }; $recognizer.Dispose()"`,
          { timeout: 1e4 }
        );
        const captureLines = captureOutput.split("\n").map((l) => l.trim()).filter(Boolean);
        if (captureLines.length > 0) {
          lines.length = 0;
          lines.push(...captureLines);
        }
      } catch {
      }
      try {
        const { stdout: inputDevices } = await execAsync5(
          `powershell -Command "$audioDevices = Get-WmiObject Win32_PnPEntity | Where-Object { $_.Caption -match 'microphone|audio|input' -and $_.Status -eq 'OK' }; $audioDevices | Select-Object -ExpandProperty Caption"`,
          { timeout: 1e4 }
        );
        const inputLines = inputDevices.split("\n").map((l) => l.trim()).filter(Boolean);
        if (inputLines.length > 0) {
          lines.length = 0;
          lines.push(...inputLines);
        }
      } catch {
      }
      const seen = /* @__PURE__ */ new Set();
      for (let i = 0; i < lines.length; i++) {
        const name = lines[i];
        if (!seen.has(name)) {
          seen.add(name);
          devices.push({
            name,
            isDefault: i === 0 && devices.length === 0
          });
        }
      }
    } else if (isMacOS()) {
      const { stdout } = await execAsync5("system_profiler SPAudioDataType -json 2>/dev/null", {
        timeout: 1e4
      });
      try {
        const data = JSON.parse(stdout);
        const audioData = data?.SPAudioDataType?.[0]?._items;
        if (Array.isArray(audioData)) {
          for (let i = 0; i < audioData.length; i++) {
            const device = audioData[i];
            if (device?._name && device?.coreaudio_input_source) {
              devices.push({
                name: device._name,
                isDefault: i === 0
              });
            }
          }
        }
      } catch {
        const { stdout: altOutput } = await execAsync5(
          'system_profiler SPAudioDataType 2>/dev/null | grep "Input Source:"',
          { timeout: 1e4 }
        );
        const matches = altOutput.match(/Input Source:\s*(.+)/g);
        if (matches) {
          for (let i = 0; i < matches.length; i++) {
            const name = matches[i].replace("Input Source:", "").trim();
            if (name) {
              devices.push({
                name,
                isDefault: i === 0
              });
            }
          }
        }
      }
    }
  } catch (err) {
  }
  return devices;
}
function showDevicesHelp() {
  header("Input Device Management");
  console.log(`  ${dim("Configure which microphone to use for voice dictation.")}`);
  console.log(`  ${dim("Set to null/default to use system default microphone.")}`);
  console.log("");
  divider();
  console.log("");
  console.log(`  ${brand.accent("Commands")}`);
  command("d, d l, d list", "Select input device interactively");
  command("d set <name>", "Select a specific microphone");
  command("d reset", "Reset to system default");
  console.log("");
  console.log(`  ${brand.accent("Examples")}`);
  console.log(`  ${cyan("dybur d")}           ${dim("Interactive device selection")}`);
  console.log(`  ${cyan("dybur d l")}         ${dim("Same as above")}`);
  console.log(`  ${cyan('dybur d set "Mic"')} ${dim("Set device by name")}`);
  console.log(`  ${cyan("dybur d reset")}     ${dim("Use system default")}`);
  console.log("");
}
async function listCommand2() {
  header("Input Devices");
  const config = loadConfig();
  const currentDevice = config.inputDevice;
  console.log(
    `  ${dim("Current:")} ${currentDevice ? cyan(currentDevice) : dim("System default")}`
  );
  console.log("");
  const devices = await listAudioDevices();
  if (devices.length === 0) {
    warning("Could not enumerate audio devices");
    console.log("");
    console.log(`  ${dim("To set a device manually, use:")}`);
    console.log(`  ${cyan('dybur d set "Device Name"')}`);
    console.log("");
    console.log(`  ${dim("Note: The exact device name must match what the system sees.")}`);
    console.log(`  ${dim("You can find device names in your system sound settings.")}`);
    console.log("");
    return;
  }
  const choices = [
    {
      label: "System default",
      value: "__default__",
      hint: "use OS default microphone"
    },
    ...devices.map((device) => ({
      label: device.name,
      value: device.name,
      hint: device.isDefault ? "system default" : void 0
    }))
  ];
  const currentIndex = currentDevice ? choices.findIndex((c2) => c2.value === currentDevice) : 0;
  const selected = await select({
    message: "Select input device",
    choices,
    initial: currentIndex >= 0 ? currentIndex : 0
  });
  if (selected === null) {
    info("Selection cancelled");
    console.log("");
    return;
  }
  if (selected === "__default__") {
    updateConfig({ inputDevice: null });
    success("Input device reset to system default");
  } else {
    updateConfig({ inputDevice: selected });
    success(`Input device set to: ${cyan(selected)}`);
  }
  console.log("");
  info("Changes will take effect on the next recording");
  console.log("");
  console.log(`  ${yellow(icons.warning)} ${dim("If the service is running, restart it:")}`);
  console.log(`    ${cyan("dybur stop && dybur start")}`);
  console.log("");
}
async function setCommand2(deviceName) {
  header("Set Input Device");
  if (!deviceName || deviceName.trim().length === 0) {
    error("Device name is required");
    console.log("");
    console.log(`  ${dim("Usage:")} ${cyan('dybur devices set "<device name>"')}`);
    console.log("");
    console.log(`  ${dim("Example:")} ${cyan('dybur devices set "Microphone (Realtek)"')}`);
    console.log("");
    process.exit(1);
  }
  const cleanName = deviceName.replace(/^["']|["']$/g, "").trim();
  try {
    updateConfig({ inputDevice: cleanName });
    success(`Input device set to: ${cyan(cleanName)}`);
    console.log("");
    info("Changes will take effect on the next recording");
    console.log("");
    console.log(`  ${yellow(icons.warning)} ${dim("If the service is running, restart it:")}`);
    console.log(`    ${cyan("dybur stop && dybur start")}`);
    console.log("");
  } catch (err) {
    error(`Failed to update configuration: ${err}`);
    process.exit(1);
  }
}
async function resetCommand() {
  header("Reset Input Device");
  try {
    updateConfig({ inputDevice: null });
    success("Input device reset to system default");
    console.log("");
    info("Changes will take effect on the next recording");
    console.log("");
  } catch (err) {
    error(`Failed to update configuration: ${err}`);
    process.exit(1);
  }
}
async function devicesCommand(args) {
  const subcommand = args[0];
  switch (subcommand) {
    case "list":
    case "l":
    case void 0:
      await listCommand2();
      break;
    case "set":
    case "s": {
      const deviceName = args.slice(1).join(" ");
      await setCommand2(deviceName);
      break;
    }
    case "reset":
    case "default":
    case "r":
      await resetCommand();
      break;
    case "--help":
    case "-h":
    case "help":
    case "h":
      showDevicesHelp();
      break;
    default:
      error(`Unknown subcommand: ${subcommand}`);
      console.log("");
      showDevicesHelp();
      process.exit(1);
  }
}

// src/commands/vad.ts
function parseNumber(value, label) {
  if (value === void 0) {
    error(`${label} value is required`);
    return null;
  }
  const parsed = Number(value);
  if (!Number.isFinite(parsed)) {
    error(`${label} must be a number`);
    return null;
  }
  return parsed;
}
function setThreshold(config, value) {
  const threshold = parseNumber(value, "Threshold");
  if (threshold === null) return;
  if (threshold < 0 || threshold > 1) {
    error("Threshold must be between 0.0 and 1.0");
    return;
  }
  config.vadThreshold = threshold;
  saveConfig(config);
  success(`VAD threshold set to ${threshold}`);
  info("Higher values are stricter and may ignore quieter speech");
}
function setMinSpeech(config, value) {
  const duration = parseNumber(value, "Minimum speech duration");
  if (duration === null) return;
  if (duration < 0 || duration > 5e3) {
    error("Minimum speech duration must be between 0 and 5000ms");
    return;
  }
  config.vadMinSpeechMs = duration;
  saveConfig(config);
  success(`VAD minimum speech duration set to ${duration}ms`);
}
function setSilenceTimeout(config, value) {
  const timeout = parseNumber(value, "Silence timeout");
  if (timeout === null) return;
  if (timeout < 0 || timeout > 3e4) {
    error("Silence timeout must be between 0 and 30000ms");
    return;
  }
  config.silenceTimeoutMs = timeout;
  saveConfig(config);
  success(`Silence timeout set to ${timeout}ms`);
  info("This controls how long a pause can split speech segments");
}
async function vadCommand(args) {
  const config = loadConfig();
  const subcommand = args[0]?.toLowerCase();
  if (subcommand === "on" || subcommand === "enable") {
    config.vadEnabled = true;
    saveConfig(config);
    success("VAD enabled");
    info("Silence will be filtered before transcription");
    return;
  }
  if (subcommand === "off" || subcommand === "disable") {
    config.vadEnabled = false;
    saveConfig(config);
    success("VAD disabled");
    info("All audio will be sent to transcription");
    return;
  }
  if (subcommand === "status") {
    showStatus(config);
    return;
  }
  if (subcommand === "threshold") {
    setThreshold(config, args[1]);
    return;
  }
  if (subcommand === "min-speech" || subcommand === "min") {
    setMinSpeech(config, args[1]);
    return;
  }
  if (subcommand === "silence" || subcommand === "silence-timeout") {
    setSilenceTimeout(config, args[1]);
    return;
  }
  if (!subcommand) {
    config.vadEnabled = !config.vadEnabled;
    saveConfig(config);
    const status = config.vadEnabled ? "enabled" : "disabled";
    success(`VAD ${status}`);
    return;
  }
  showHelp();
}
function showStatus(config) {
  header("Voice Activity Detection");
  keyValue("Status", config.vadEnabled ? brand.accent("enabled") : dim("disabled"));
  keyValue("Threshold", `${config.vadThreshold}`);
  keyValue("Min speech duration", `${config.vadMinSpeechMs}ms`);
  keyValue("Silence timeout", `${config.silenceTimeoutMs}ms`);
  console.log("");
  console.log(`  ${dim("VAD filters silence and noise before transcription.")}`);
  console.log(`  ${dim("This improves accuracy and reduces processing time.")}`);
  console.log("");
}
function showHelp() {
  header("VAD Commands");
  console.log(`  ${brand.accent("dybur vad")}          Toggle VAD on/off`);
  console.log(`  ${brand.accent("dybur vad on")}       Enable VAD`);
  console.log(`  ${brand.accent("dybur vad off")}      Disable VAD`);
  console.log(`  ${brand.accent("dybur vad status")}   Show VAD settings`);
  console.log(`  ${brand.accent("dybur vad threshold 0.6")}      Set speech threshold`);
  console.log(`  ${brand.accent("dybur vad min-speech 250")}     Set minimum speech duration`);
  console.log(`  ${brand.accent("dybur vad silence 1000")}       Set silence timeout`);
  console.log("");
  info("VAD (Voice Activity Detection) filters silence before transcription");
  console.log("");
}

// src/commands/gpu.ts
async function gpuCommand(args) {
  const config = loadConfig();
  const subcommand = args[0]?.toLowerCase();
  if (subcommand === "on" || subcommand === "auto" || subcommand === "enable") {
    config.gpuMode = "auto";
    saveConfig(config);
    success("GPU acceleration enabled (auto-detect)");
    info("Will use DirectML (Windows) or CoreML (macOS) if available");
    return;
  }
  if (subcommand === "off" || subcommand === "cpu" || subcommand === "disable") {
    config.gpuMode = "cpu";
    saveConfig(config);
    success("GPU acceleration disabled (CPU-only mode)");
    info("All inference will run on CPU");
    return;
  }
  if (subcommand === "status") {
    showStatus2(config);
    return;
  }
  if (!subcommand) {
    config.gpuMode = config.gpuMode === "auto" ? "cpu" : "auto";
    saveConfig(config);
    const status = config.gpuMode === "auto" ? "enabled (auto)" : "disabled (CPU-only)";
    success(`GPU acceleration ${status}`);
    return;
  }
  showHelp2();
}
function showStatus2(config) {
  header("GPU Acceleration");
  const isAuto = config.gpuMode === "auto";
  keyValue("Mode", isAuto ? brand.accent("auto (GPU if available)") : dim("cpu (GPU disabled)"));
  console.log("");
  console.log(`  ${dim("Platform-specific GPU providers:")}`);
  console.log(`  ${dim("  Windows: DirectML (works with AMD, Intel, NVIDIA)")}`);
  console.log(`  ${dim("  macOS:   CoreML (Apple Silicon / Intel)")}`);
  console.log("");
  console.log(`  ${dim("GPU acceleration speeds up speech recognition.")}`);
  console.log(`  ${dim("If GPU fails, the app will automatically fall back to CPU.")}`);
  console.log("");
  info("Note: Restart the app for GPU mode changes to take effect");
  console.log("");
}
function showHelp2() {
  header("GPU Commands");
  console.log(`  ${brand.accent("dybur gpu")}          Toggle GPU mode`);
  console.log(`  ${brand.accent("dybur gpu on")}       Enable GPU (auto-detect)`);
  console.log(`  ${brand.accent("dybur gpu off")}      Disable GPU (CPU-only)`);
  console.log(`  ${brand.accent("dybur gpu status")}   Show GPU settings`);
  console.log("");
  info("GPU acceleration uses DirectML (Windows) or CoreML (macOS)");
  console.log("");
}

// src/cli.ts
var VERSION = "1.2.1";
function showHelp3() {
  banner();
  console.log(`  ${dim("Local voice dictation for macOS & Windows")}`);
  console.log(`  ${dim("Powered by")} ${cyan("NVIDIA Parakeet")} ${dim("- 100% offline")}`);
  console.log("");
  header("Commands");
  command("start", "Start the background service");
  command("stop", "Stop the background service");
  command("status, s", "Show service status & health");
  command("settings, config", "Open configuration file");
  command("doctor, diag", "Run diagnostics");
  command("models, m", "Manage speech models");
  command("devices, d", "Manage input devices");
  command("vad", "Tune Voice Activity Detection");
  command("gpu", "Toggle GPU acceleration");
  console.log("");
  header("Model Commands");
  command("models list", "List installed models");
  command("models download <model-id>", "Download a speech model");
  command("models clean", "Remove unused models");
  console.log("");
  header("Device Commands");
  command("d, d l", "List & select microphone interactively");
  command("d set <name>", "Select a specific microphone");
  command("d reset", "Reset to system default");
  console.log("");
  header("VAD Commands");
  command("vad status", "Show VAD settings");
  command("vad threshold 0.6", "Set speech sensitivity");
  command("vad silence 1000", "Set silence split timeout");
  console.log("");
  header("Options");
  command("-h, --help", "Show this help message");
  command("-v, --version", "Show version number");
  console.log("");
  header("Quick Start");
  info(`Run ${cyan("dybur start")} to begin`);
  info(`Press ${brand.accent("Ctrl+Shift+Space")} to dictate`);
  console.log("");
  console.log(`  ${dim("Docs:")} ${cyan("https://github.com/oshtz/dybur")}`);
  console.log("");
}
function showVersion() {
  boxMessage(
    [
      `Version: ${brand.accent(VERSION)}`,
      `Platform: ${process.platform}`,
      `Node: ${process.version}`
    ],
    "dybur"
  );
}
async function main() {
  const args = process.argv.slice(2);
  const { values, positionals } = parseArgs({
    args,
    options: {
      help: { type: "boolean", short: "h" },
      version: { type: "boolean", short: "v" }
    },
    allowPositionals: true,
    strict: false
  });
  if (values.version) {
    showVersion();
    return;
  }
  if (values.help || positionals.length === 0) {
    showHelp3();
    return;
  }
  const cmd = positionals[0];
  const commandArgs = positionals.slice(1);
  try {
    switch (cmd) {
      case "start":
        await startCommand(commandArgs);
        break;
      case "stop":
        await stopCommand(commandArgs);
        break;
      case "status":
      case "s":
        await statusCommand(commandArgs);
        break;
      case "settings":
      case "config":
        await settingsCommand(commandArgs);
        break;
      case "doctor":
      case "diag":
        await doctorCommand(commandArgs);
        break;
      case "models":
      case "m":
        await modelsCommand(commandArgs);
        break;
      case "devices":
      case "d":
        await devicesCommand(commandArgs);
        break;
      case "vad":
        await vadCommand(commandArgs);
        break;
      case "gpu":
        await gpuCommand(commandArgs);
        break;
      default:
        error(`Unknown command: ${cmd}`);
        console.log("");
        info(`Run ${cyan("dybur --help")} for usage information`);
        process.exit(1);
    }
  } catch (err) {
    console.log("");
    if (err instanceof Error) {
      error(err.message);
    } else {
      error("An unexpected error occurred");
    }
    process.exit(1);
  }
}
main();
