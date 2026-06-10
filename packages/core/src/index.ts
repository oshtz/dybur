/**
 * @dybur/core
 * Core business logic for dybur
 */

// Model management
export {
  // Model registry
  MODEL_REGISTRY,
  getModelDefinition,
  getDefaultModelDefinition,
  getAvailableModels,
  normalizeModelName,
  buildDownloadUrl,
  type ModelArchitecture,
  type VocabType,
  type ModelVisibility,
  type FileRole,
  type ModelFile,
  type ModelConfig,
  type ModelDefinition,
  // Model operations
  DEFAULT_MODEL,
  MODEL_REPO,
  ensureModelsDir,
  listModels,
  isModelInstalled,
  isDefaultModelInstalled,
  getModelMetadata,
  calculateChecksum,
  downloadModel,
  removeModel,
  cleanModels,
  formatBytes,
  getModelFiles,
  type ModelMetadata,
  type InstalledModel,
  type DownloadProgress,
} from './models.js';

// Experimental model candidates
export {
  MODEL_CANDIDATES,
  getModelCandidate,
  getModelCandidates,
  type CandidateRecommendation,
  type CandidateRuntime,
  type ModelCandidate,
} from './model-candidates.js';

// Post-processing
export {
  postProcess,
  postProcessWithConfig,
  getPostProcessOptions,
  trimWhitespace,
  normalizeWhitespace,
  capitalizeSentence,
  applySentenceCase,
  addBasicPunctuation,
  type PostProcessOptions,
} from './postprocess.js';

// Logging
export {
  configureLogger,
  createLogger,
  loggers,
  getLogFilePath,
  getLogsDir,
  type LogLevel,
  type LogEntry,
  type LoggerConfig,
} from './logging.js';

// Tray app management
export {
  GITHUB_REPO,
  GITHUB_RELEASES_URL,
  TRAY_APP_VERSION,
  getTrayAssetName,
  getTrayDownloadUrl,
  ensureBinDir,
  isTrayAppInstalled,
  getTrayAppMetadata,
  downloadTrayApp,
  isUpdateAvailable,
  removeTrayApp,
  type TrayAppMetadata,
  type TrayDownloadProgress,
} from './tray.js';
