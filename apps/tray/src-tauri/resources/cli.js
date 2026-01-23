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
  model: "parakeet-tdt-0.6b-v3-onnx",
  clipboardCleanup: true,
  inputDevice: null,
  recordingMode: "toggle"
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
var DEFAULT_MODEL = "parakeet-tdt-0.6b-v3-onnx";
var MODEL_REPO = "istupakov/parakeet-tdt-0.6b-v3-onnx";
var MODEL_BASE_URL = `https://huggingface.co/${MODEL_REPO}/resolve/main`;
var MODEL_FILES = {
  full: [
    "encoder-model.onnx",
    "encoder-model.onnx.data",
    "decoder_joint-model.onnx",
    "nemo128.onnx",
    "vocab.txt",
    "config.json"
  ],
  int8: [
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
    "nemo128.onnx",
    "vocab.txt",
    "config.json"
  ]
};
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
  const modelPath = getModelPath(modelName);
  const metadataPath = join2(modelPath, "metadata.json");
  if (!existsSync2(modelPath) || !existsSync2(metadataPath)) {
    return false;
  }
  const hasEncoder = existsSync2(join2(modelPath, "encoder-model.int8.onnx")) || existsSync2(join2(modelPath, "encoder-model.onnx"));
  const hasDecoder = existsSync2(join2(modelPath, "decoder_joint-model.int8.onnx")) || existsSync2(join2(modelPath, "decoder_joint-model.onnx"));
  const hasVocab = existsSync2(join2(modelPath, "vocab.txt"));
  return hasEncoder && hasDecoder && hasVocab;
}
function isDefaultModelInstalled() {
  return isModelInstalled(DEFAULT_MODEL);
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
    while (true) {
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
async function downloadModel(modelName = DEFAULT_MODEL, onProgress, variant = "int8") {
  const modelDir = getModelPath(modelName);
  if (isModelInstalled(modelName)) {
    return modelDir;
  }
  mkdirSync2(modelDir, { recursive: true });
  const files = MODEL_FILES[variant];
  let totalDownloaded = 0;
  const downloadedFiles = [];
  try {
    for (const file of files) {
      const url = `${MODEL_BASE_URL}/${file}`;
      const destPath = join2(modelDir, file);
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
    const metadata = {
      name: modelName,
      version: "v3",
      checksum: "",
      // Would compute combined checksum if needed
      downloadedAt: (/* @__PURE__ */ new Date()).toISOString(),
      size: totalDownloaded,
      source: MODEL_REPO,
      variant,
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
  for (const model of models) {
    if (!model.isDefault) {
      if (removeModel(model.name)) {
        removed.push(model.name);
      }
    }
  }
  return removed;
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
import { createWriteStream as createWriteStream2, existsSync as existsSync4, mkdirSync as mkdirSync4, readFileSync as readFileSync3, writeFileSync as writeFileSync3, rmSync as rmSync2, chmodSync } from "fs";
import { join as join3 } from "path";
import { exec } from "child_process";
import { promisify } from "util";
var execAsync = promisify(exec);
var GITHUB_REPO = "oshtz/dybur";
var GITHUB_RELEASES_URL = `https://github.com/${GITHUB_REPO}/releases`;
var TRAY_APP_VERSION = "v1.0.0";
function getTrayAssetName() {
  const platform2 = getPlatform();
  const arch = getArch();
  if (platform2 === "darwin") {
    return `dybur-macos-${arch}.tar.gz`;
  }
  return `dybur-windows-${arch}.zip`;
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
    while (true) {
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
async function extractTarGz(archivePath, destDir) {
  await execAsync(`tar -xzf "${archivePath}" -C "${destDir}"`);
}
async function extractZip(archivePath, destDir) {
  await execAsync(`powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force"`);
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
  const archivePath = join3(binDir, assetName);
  try {
    if (onProgress) {
      onProgress(0, 0, "Downloading tray application...");
    }
    await downloadFile2(downloadUrl, archivePath, (downloaded, total) => {
      if (onProgress) {
        onProgress(downloaded, total);
      }
    });
    if (onProgress) {
      onProgress(0, 0, "Extracting...");
    }
    if (isMacOS()) {
      await extractTarGz(archivePath, binDir);
      if (existsSync4(trayPath)) {
        chmodSync(trayPath, 493);
      }
      try {
        await execAsync(`xattr -rd com.apple.quarantine "${bundlePath}"`);
      } catch {
      }
    } else {
      await extractZip(archivePath, binDir);
    }
    rmSync2(archivePath, { force: true });
    if (!existsSync4(trayPath)) {
      throw new Error("Extraction failed: tray app binary not found");
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
    if (existsSync4(archivePath)) {
      rmSync2(archivePath, { force: true });
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
    const result = execSync("pgrep -x dybur", { encoding: "utf-8", stdio: ["pipe", "pipe", "pipe"] });
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
  if (!isDefaultModelInstalled()) {
    warning("Default model not found");
    info("Downloading model from HuggingFace...");
    console.log(`  ${dim("This only needs to happen once")}`);
    console.log("");
    let lastFile = "";
    try {
      await downloadModel(DEFAULT_MODEL, (downloaded, total, file) => {
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
      info(`Run ${cyan("dybur models prefetch")} to try again`);
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
      console.log(`  ${dim("2.")} Download manually from ${cyan("https://github.com/oshtz/dybur/releases")}`);
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
  const running = await isTrayRunning();
  const modelInstalled = isDefaultModelInstalled();
  const modelMeta = modelInstalled ? getModelMetadata(DEFAULT_MODEL) : null;
  const config = loadConfig({ createIfMissing: false });
  const paths = getAllPaths();
  console.log(
    `  ${statusIcon(running)} ${dim("Service:")}     ${running ? green("Running") : red("Stopped")}`
  );
  console.log(
    `  ${statusIcon(modelInstalled)} ${dim("Model:")}       ${modelInstalled ? green(DEFAULT_MODEL) : red("Not installed")}`
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
  console.log(`  ${dim("Recording mode:")} ${config.recordingMode === "push_to_talk" ? "push-to-talk" : "toggle"}`);
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
    info(`Run ${cyan("dybur models prefetch")} to download`);
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
  if (!isDefaultModelInstalled()) {
    return {
      name: "Speech Model",
      status: "fail",
      message: `Model not installed`,
      details: `Run: dybur models prefetch`
    };
  }
  const metadata = getModelMetadata(DEFAULT_MODEL);
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
    message: DEFAULT_MODEL,
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
  console.log(`  ${dim("dybur uses NVIDIA Parakeet for speech recognition.")}`);
  console.log(`  ${dim("Models are downloaded from HuggingFace on first use.")}`);
  console.log("");
  divider();
  console.log("");
  console.log(`  ${brand.accent("Commands")}`);
  command("models list", "List installed models");
  command("models prefetch", "Download default model");
  command("models clean", "Remove unused models");
  console.log("");
  console.log(`  ${brand.accent("Default Model")}`);
  console.log(`  ${dim("Name:")}   ${DEFAULT_MODEL}`);
  console.log(`  ${dim("Source:")} huggingface.co/${MODEL_REPO}`);
  console.log(`  ${dim("Size:")}   ~670 MB (INT8 quantized)`);
  console.log("");
}
async function listCommand() {
  header("Installed Models");
  const models = listModels();
  const modelsDir = getModelsDir();
  if (models.length === 0) {
    info("No models installed");
    console.log("");
    console.log(`  ${dim("To install the default model:")}`);
    console.log(`  ${cyan("dybur models prefetch")}`);
    console.log("");
    console.log(`  ${dim("Models directory:")} ${formatPath(modelsDir, 45)}`);
    console.log("");
    return;
  }
  for (const model of models) {
    const defaultBadge = model.isDefault ? ` ${green("[default]")}` : "";
    const size = formatSize(model.size);
    console.log(`  ${brand.accent(icons.bullet)} ${model.name}${defaultBadge}`);
    console.log(`    ${dim("Size:")} ${size}`);
    if (model.metadata) {
      console.log(`    ${dim("Downloaded:")} ${model.metadata.downloadedAt.split("T")[0]}`);
      if (model.metadata.variant) {
        console.log(`    ${dim("Variant:")} ${model.metadata.variant}`);
      }
      if (model.metadata.source) {
        console.log(`    ${dim("Source:")} ${model.metadata.source}`);
      }
    }
    console.log("");
  }
  divider();
  console.log("");
  console.log(`  ${dim("Models directory:")} ${formatPath(modelsDir, 45)}`);
  console.log("");
}
async function prefetchCommand() {
  header("Download Model");
  if (isDefaultModelInstalled()) {
    success(`Model already installed: ${DEFAULT_MODEL}`);
    console.log("");
    return;
  }
  console.log(`  ${dim("Model:")}  ${DEFAULT_MODEL}`);
  console.log(`  ${dim("Source:")} huggingface.co/${MODEL_REPO}`);
  console.log(`  ${dim("Variant:")} INT8 quantized (~670 MB)`);
  console.log("");
  divider();
  console.log("");
  let currentFile = "";
  let fileCount = 0;
  try {
    await downloadModel(DEFAULT_MODEL, (downloaded, total, file) => {
      if (file && file !== currentFile) {
        if (currentFile) {
          process.stdout.write("\n");
        }
        currentFile = file;
        fileCount++;
        console.log(`  ${dim(`[${fileCount}/4]`)} ${file}`);
      }
      if (total > 0) {
        const bar = progressBar(downloaded, total, 25);
        process.stdout.write(`\r  ${bar}`);
      }
    });
    console.log("\n");
    divider();
    console.log("");
    success("Model downloaded successfully");
    console.log("");
    info(`Run ${cyan("dybur start")} to begin`);
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
      await listCommand();
      break;
    case "prefetch":
    case "download":
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
      value: null,
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
  if (selected === void 0) {
    info("Selection cancelled");
    console.log("");
    return;
  }
  if (selected === null) {
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
async function setCommand(deviceName) {
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
    case "s":
      const deviceName = args.slice(1).join(" ");
      await setCommand(deviceName);
      break;
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

// src/cli.ts
var VERSION = "1.0.0";
function showHelp() {
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
  console.log("");
  header("Model Commands");
  command("models list", "List installed models");
  command("models prefetch", "Download default model");
  command("models clean", "Remove unused models");
  console.log("");
  header("Device Commands");
  command("d, d l", "List & select microphone interactively");
  command("d set <name>", "Select a specific microphone");
  command("d reset", "Reset to system default");
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
    showHelp();
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
