//! Model management for dybur
//!
//! Handles downloading, listing, and managing speech recognition models.
//! Supports multiple model architectures: TDT Transducer, Streaming Transducer,
//! Encoder-Decoder (Whisper), and LLM-style decoders.

use crate::config::get_models_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ============================================================================
// Model Architecture Types
// ============================================================================

/// Speech recognition model architecture type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelArchitecture {
    /// TDT Transducer (Parakeet v2/v3) - frame-by-frame decoding with duration tokens
    TdtTransducer,
    /// Standard Transducer (Nemotron) - encoder/decoder/joiner streaming
    StreamingTransducer,
    /// Encoder-Decoder with attention (Whisper) - KV-cache, BPE tokenization
    EncoderDecoder,
}

/// Vocabulary/tokenization type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VocabType {
    /// Simple text file with one token per line (Parakeet, Nemotron)
    TextFile,
    /// Byte-Pair Encoding with tokenizer.json (Whisper)
    Bpe,
}

/// Role of a model file
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileRole {
    Encoder,
    Decoder,
    DecoderWithPast, // For KV-cache models
    Joiner,          // For transducer models
    Vocab,
    Preprocessor,
    Embeddings, // For Canary
    Config,
    EncoderData, // External data files for large models
    DecoderData,
    EmbeddingsData,
}

/// A file that is part of a model
#[derive(Debug, Clone)]
pub struct ModelFile {
    pub name: &'static str,
    pub role: FileRole,
    pub required: bool,
}

/// Model-specific configuration
#[derive(Debug, Clone)]
pub struct ModelConfig {
    /// Vocabulary type
    pub vocab_type: VocabType,
    /// Sample rate required (Hz)
    pub sample_rate: u32,
    /// Number of mel bins for preprocessing
    pub n_mels: usize,
    /// Whether model supports streaming
    pub supports_streaming: bool,
    /// Max audio duration in seconds
    pub max_duration_s: f32,
}

/// Definition of a speech recognition model
#[derive(Debug, Clone)]
pub struct ModelDefinition {
    /// Unique model identifier (e.g., "parakeet-tdt-v3-int8")
    pub id: &'static str,
    /// Human-readable display name
    pub display_name: &'static str,
    /// Short description
    pub description: &'static str,
    /// Model architecture type
    pub architecture: ModelArchitecture,
    /// HuggingFace repository
    pub repo: &'static str,
    /// Files to download
    pub files: &'static [ModelFile],
    /// Total download size in bytes (approximate)
    pub size_bytes: u64,
    /// Supported languages (empty = all)
    pub languages: &'static [&'static str],
    /// Whether this is the default model
    pub is_default: bool,
    /// Whether this is a legacy model kept for explicit compatibility/benchmark use
    pub is_legacy: bool,
    /// Model-specific configuration
    pub config: ModelConfig,
}

// ============================================================================
// Model Registry - All Supported Models
// ============================================================================

/// Parakeet TDT v2 files (English-only, INT8)
const PARAKEET_V2_FILES: &[ModelFile] = &[
    ModelFile {
        name: "encoder-model.int8.onnx",
        role: FileRole::Encoder,
        required: true,
    },
    ModelFile {
        name: "decoder_joint-model.int8.onnx",
        role: FileRole::Decoder,
        required: true,
    },
    ModelFile {
        name: "nemo128.onnx",
        role: FileRole::Preprocessor,
        required: false,
    },
    ModelFile {
        name: "vocab.txt",
        role: FileRole::Vocab,
        required: true,
    },
    ModelFile {
        name: "config.json",
        role: FileRole::Config,
        required: false,
    },
];

/// Parakeet TDT v3 files (Multilingual, INT8)
const PARAKEET_V3_FILES: &[ModelFile] = &[
    ModelFile {
        name: "encoder-model.int8.onnx",
        role: FileRole::Encoder,
        required: true,
    },
    ModelFile {
        name: "decoder_joint-model.int8.onnx",
        role: FileRole::Decoder,
        required: true,
    },
    ModelFile {
        name: "nemo128.onnx",
        role: FileRole::Preprocessor,
        required: false,
    },
    ModelFile {
        name: "vocab.txt",
        role: FileRole::Vocab,
        required: true,
    },
    ModelFile {
        name: "config.json",
        role: FileRole::Config,
        required: false,
    },
];

/// Nemotron Streaming files (INT8)
const NEMOTRON_STREAMING_FILES: &[ModelFile] = &[
    ModelFile {
        name: "encoder.int8.onnx",
        role: FileRole::Encoder,
        required: true,
    },
    ModelFile {
        name: "decoder.int8.onnx",
        role: FileRole::Decoder,
        required: true,
    },
    ModelFile {
        name: "joiner.int8.onnx",
        role: FileRole::Joiner,
        required: true,
    },
    ModelFile {
        name: "tokens.txt",
        role: FileRole::Vocab,
        required: true,
    },
];

/// Whisper Large v3 Turbo files (INT8)
const WHISPER_INT8_FILES: &[ModelFile] = &[
    ModelFile {
        name: "onnx/encoder_model_int8.onnx",
        role: FileRole::Encoder,
        required: true,
    },
    ModelFile {
        name: "onnx/decoder_model_int8.onnx",
        role: FileRole::Decoder,
        required: true,
    },
    ModelFile {
        name: "tokenizer.json",
        role: FileRole::Vocab,
        required: true,
    },
    ModelFile {
        name: "config.json",
        role: FileRole::Config,
        required: false,
    },
    ModelFile {
        name: "generation_config.json",
        role: FileRole::Config,
        required: false,
    },
];

/// Whisper Large v3 Turbo files (FP16)
const WHISPER_FP16_FILES: &[ModelFile] = &[
    ModelFile {
        name: "onnx/encoder_model_fp16.onnx",
        role: FileRole::Encoder,
        required: true,
    },
    ModelFile {
        name: "onnx/decoder_model_fp16.onnx",
        role: FileRole::Decoder,
        required: true,
    },
    ModelFile {
        name: "tokenizer.json",
        role: FileRole::Vocab,
        required: true,
    },
    ModelFile {
        name: "config.json",
        role: FileRole::Config,
        required: false,
    },
    ModelFile {
        name: "generation_config.json",
        role: FileRole::Config,
        required: false,
    },
];

/// All available models
pub const MODEL_REGISTRY: &[ModelDefinition] = &[
    // Parakeet TDT v2 - English only
    ModelDefinition {
        id: "parakeet-tdt-v2-int8",
        display_name: "Parakeet TDT v2 (English)",
        description: "Fast, English-optimized transducer model",
        architecture: ModelArchitecture::TdtTransducer,
        repo: "istupakov/parakeet-tdt-0.6b-v2-onnx",
        files: PARAKEET_V2_FILES,
        size_bytes: 661_000_000,
        languages: &["en"],
        is_default: false,
        is_legacy: true,
        config: ModelConfig {
            vocab_type: VocabType::TextFile,
            sample_rate: 16000,
            n_mels: 128,
            supports_streaming: false,
            max_duration_s: 1440.0, // 24 minutes
        },
    },
    // Parakeet TDT v3 - Multilingual (DEFAULT)
    ModelDefinition {
        id: "parakeet-tdt-v3-int8",
        display_name: "Parakeet TDT v3 (Multilingual)",
        description: "Balanced accuracy, 25 languages",
        architecture: ModelArchitecture::TdtTransducer,
        repo: "istupakov/parakeet-tdt-0.6b-v3-onnx",
        files: PARAKEET_V3_FILES,
        size_bytes: 670_000_000,
        languages: &[
            "en", "de", "es", "fr", "it", "pt", "nl", "pl", "ru", "uk", "ja", "ko", "zh",
        ],
        is_default: true,
        is_legacy: false,
        config: ModelConfig {
            vocab_type: VocabType::TextFile,
            sample_rate: 16000,
            n_mels: 128,
            supports_streaming: false,
            max_duration_s: 1440.0,
        },
    },
    // Nemotron Streaming - English
    ModelDefinition {
        id: "nemotron-streaming-int8",
        display_name: "Nemotron Streaming (English)",
        description: "Low-latency streaming transducer",
        architecture: ModelArchitecture::StreamingTransducer,
        repo: "csukuangfj/sherpa-onnx-nemotron-speech-streaming-en-0.6b-int8-2026-01-14",
        files: NEMOTRON_STREAMING_FILES,
        size_bytes: 663_000_000,
        languages: &["en"],
        is_default: false,
        is_legacy: false,
        config: ModelConfig {
            vocab_type: VocabType::TextFile,
            sample_rate: 16000,
            n_mels: 80,
            supports_streaming: true,
            max_duration_s: 1440.0,
        },
    },
    // Whisper Large v3 Turbo - INT8
    ModelDefinition {
        id: "whisper-large-v3-turbo-int8",
        display_name: "Whisper Large v3 Turbo (INT8)",
        description: "Popular model, 99 languages, balanced",
        architecture: ModelArchitecture::EncoderDecoder,
        repo: "onnx-community/whisper-large-v3-turbo",
        files: WHISPER_INT8_FILES,
        size_bytes: 1_100_000_000,
        languages: &[], // All languages
        is_default: false,
        is_legacy: false,
        config: ModelConfig {
            vocab_type: VocabType::Bpe,
            sample_rate: 16000,
            n_mels: 128,
            supports_streaming: false,
            max_duration_s: 30.0, // 30-second chunks
        },
    },
    // Whisper Large v3 Turbo - FP16
    ModelDefinition {
        id: "whisper-large-v3-turbo-fp16",
        display_name: "Whisper Large v3 Turbo (FP16)",
        description: "High accuracy, 99 languages",
        architecture: ModelArchitecture::EncoderDecoder,
        repo: "onnx-community/whisper-large-v3-turbo",
        files: WHISPER_FP16_FILES,
        size_bytes: 1_600_000_000,
        languages: &[],
        is_default: false,
        is_legacy: false,
        config: ModelConfig {
            vocab_type: VocabType::Bpe,
            sample_rate: 16000,
            n_mels: 128,
            supports_streaming: false,
            max_duration_s: 30.0,
        },
    },
];

/// Get a model definition by ID
pub fn get_model_definition(model_id: &str) -> Option<&'static ModelDefinition> {
    MODEL_REGISTRY.iter().find(|m| m.id == model_id)
}

/// Get the default model definition
pub fn get_default_model() -> &'static ModelDefinition {
    MODEL_REGISTRY
        .iter()
        .find(|m| m.is_default)
        .expect("No default model defined")
}

/// Get model definitions for normal picker/download flows.
pub fn get_available_models() -> Vec<&'static ModelDefinition> {
    MODEL_REGISTRY
        .iter()
        .filter(|model| !model.is_legacy)
        .collect()
}

// ============================================================================
// Legacy Constants (for backward compatibility)
// ============================================================================

/// Default model name (legacy - use get_default_model().id instead)
pub const DEFAULT_MODEL: &str = "parakeet-tdt-v3-int8";

/// Legacy model name mapping (old name -> new ID)
pub fn normalize_model_name(name: &str) -> &str {
    match name {
        "parakeet-tdt-0.6b-v3-onnx" => "parakeet-tdt-v3-int8",
        "parakeet-tdt-0.6b-v2-onnx" => "parakeet-tdt-v2-int8",
        other => other,
    }
}

/// VAD model constants
pub const VAD_MODEL_NAME: &str = "silero-vad";
pub const VAD_MODEL_URL: &str =
    "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx";
pub const VAD_MODEL_FILENAME: &str = "silero_vad.onnx";

/// Model metadata stored alongside each model
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelMetadata {
    pub name: String,
    pub version: String,
    pub checksum: String,
    pub downloaded_at: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub variant: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
}

/// Information about an installed model
#[derive(Debug, Clone)]
pub struct InstalledModel {
    pub name: String,
    pub path: PathBuf,
    pub metadata: Option<ModelMetadata>,
    pub size: u64,
    pub is_default: bool,
}

/// Get the path for a specific model
pub fn get_model_path(model_name: &str) -> PathBuf {
    get_models_dir().join(model_name)
}

/// Ensure the models directory exists
pub fn ensure_models_dir() -> PathBuf {
    let dir = get_models_dir();
    if !dir.exists() {
        let _ = fs::create_dir_all(&dir);
    }
    dir
}

/// List all installed models
pub fn list_models() -> Vec<InstalledModel> {
    let models_dir = get_models_dir();

    if !models_dir.exists() {
        return Vec::new();
    }

    let entries = match fs::read_dir(&models_dir) {
        Ok(entries) => entries,
        Err(_) => return Vec::new(),
    };

    let mut models = Vec::new();

    for entry in entries.flatten() {
        if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }

        let model_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();

        // Try to load metadata
        let metadata_path = model_path.join("metadata.json");
        let metadata = if metadata_path.exists() {
            fs::read_to_string(&metadata_path)
                .ok()
                .and_then(|content| serde_json::from_str(&content).ok())
        } else {
            None
        };

        // Calculate directory size
        let size = get_directory_size(&model_path);

        // Check if this is the default model (by ID or legacy name)
        let is_default = get_model_definition(&name)
            .map(|m| m.is_default)
            .unwrap_or(false)
            || name == get_default_model().id;

        models.push(InstalledModel {
            name: name.clone(),
            path: model_path,
            metadata,
            size,
            is_default,
        });
    }

    // Sort: default first, then alphabetically
    models.sort_by(|a, b| {
        if a.is_default {
            std::cmp::Ordering::Less
        } else if b.is_default {
            std::cmp::Ordering::Greater
        } else {
            a.name.cmp(&b.name)
        }
    });

    models
}

/// Get the size of a directory recursively
fn get_directory_size(path: &PathBuf) -> u64 {
    let mut size = 0u64;

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let entry_path = entry.path();
            if entry_path.is_dir() {
                size += get_directory_size(&entry_path);
            } else if let Ok(metadata) = entry.metadata() {
                size += metadata.len();
            }
        }
    }

    size
}

/// Check if a model is installed (has required files)
pub fn is_model_installed(model_name: &str) -> bool {
    // Normalize legacy model names
    let model_id = normalize_model_name(model_name);
    let model_path = get_model_path(model_id);
    let metadata_path = model_path.join("metadata.json");

    if !model_path.exists() || !metadata_path.exists() {
        return false;
    }

    // Get model definition to check required files
    if let Some(model_def) = get_model_definition(model_id) {
        // Check all required files exist
        for file in model_def.files {
            if file.required {
                // Handle nested paths (e.g., "onnx/encoder.onnx")
                let file_path = model_path.join(file.name);
                if !file_path.exists() {
                    return false;
                }
            }
        }
        true
    } else {
        // Fallback for unknown models: check for basic files
        let has_encoder = model_path.join("encoder-model.int8.onnx").exists()
            || model_path.join("encoder-model.onnx").exists()
            || model_path.join("encoder.int8.onnx").exists();
        let has_decoder = model_path.join("decoder_joint-model.int8.onnx").exists()
            || model_path.join("decoder_joint-model.onnx").exists()
            || model_path.join("decoder.int8.onnx").exists();
        let has_vocab = model_path.join("vocab.txt").exists()
            || model_path.join("tokens.txt").exists()
            || model_path.join("tokenizer.json").exists();

        has_encoder && has_decoder && has_vocab
    }
}

/// Check if the default model is installed
pub fn is_default_model_installed() -> bool {
    is_model_installed(DEFAULT_MODEL)
}

/// Get model metadata
pub fn get_model_metadata(model_name: &str) -> Option<ModelMetadata> {
    let metadata_path = get_model_path(model_name).join("metadata.json");

    if !metadata_path.exists() {
        return None;
    }

    fs::read_to_string(&metadata_path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
}

/// Build HuggingFace download URL for a model file
fn build_download_url(repo: &str, file: &str) -> String {
    format!("https://huggingface.co/{}/resolve/main/{}", repo, file)
}

/// Download a model synchronously (blocking)
/// This is used for the tray app which runs downloads in a separate thread
pub fn download_model_sync(model_id: &str, _variant: &str) -> Result<PathBuf, String> {
    use crate::state::{DownloadState, DOWNLOAD_IN_PROGRESS, DOWNLOAD_STATE};
    use std::sync::atomic::Ordering;

    // Normalize legacy model names
    let model_id = normalize_model_name(model_id);

    // Get model definition from registry
    let model_def =
        get_model_definition(model_id).ok_or_else(|| format!("Unknown model: {}", model_id))?;

    let model_dir = get_model_path(model_id);

    // Check if already installed
    if is_model_installed(model_id) {
        return Ok(model_dir);
    }

    // Check if download already in progress
    if DOWNLOAD_IN_PROGRESS.load(Ordering::SeqCst) {
        return Err("A download is already in progress".to_string());
    }

    // Mark download as in progress
    DOWNLOAD_IN_PROGRESS.store(true, Ordering::SeqCst);

    // Create model directory
    if let Err(e) = fs::create_dir_all(&model_dir) {
        DOWNLOAD_IN_PROGRESS.store(false, Ordering::SeqCst);
        return Err(format!("Failed to create model directory: {}", e));
    }

    // Get files from model definition
    let files: Vec<&str> = model_def.files.iter().map(|f| f.name).collect();
    let total_files = files.len();
    let mut total_downloaded = 0u64;
    let mut downloaded_files = Vec::new();

    for (index, file) in files.iter().enumerate() {
        let url = build_download_url(model_def.repo, file);

        // Handle nested paths (e.g., "onnx/encoder.onnx") - create subdirectories
        let dest_path = model_dir.join(file);
        if let Some(parent) = dest_path.parent() {
            if !parent.exists() {
                if let Err(e) = fs::create_dir_all(parent) {
                    DOWNLOAD_IN_PROGRESS.store(false, Ordering::SeqCst);
                    let _ = fs::remove_dir_all(&model_dir);
                    return Err(format!("Failed to create directory for {}: {}", file, e));
                }
            }
        }

        // Update download state at start of file
        {
            let mut state = DOWNLOAD_STATE.write().unwrap();
            *state = DownloadState::Downloading {
                model_name: model_id.to_string(),
                current_file: file.to_string(),
                file_index: index,
                total_files,
                bytes_downloaded: total_downloaded,
                total_bytes: model_def.size_bytes,
                file_bytes_downloaded: 0,
                file_total_bytes: 0,
            };
        }

        crate::log_info!(
            "models",
            "Downloading {} ({}/{})...",
            file,
            index + 1,
            total_files
        );

        // Create progress callback that updates the global state
        let model_id_clone = model_id.to_string();
        let file_clone = file.to_string();
        let total_downloaded_base = total_downloaded;
        let estimated_total = model_def.size_bytes;
        let progress_callback: ProgressCallback = Box::new(move |file_downloaded, file_total| {
            let mut state = DOWNLOAD_STATE.write().unwrap();
            *state = DownloadState::Downloading {
                model_name: model_id_clone.clone(),
                current_file: file_clone.clone(),
                file_index: index,
                total_files,
                bytes_downloaded: total_downloaded_base + file_downloaded,
                total_bytes: estimated_total,
                file_bytes_downloaded: file_downloaded,
                file_total_bytes: file_total,
            };
        });

        match download_file_with_progress(&url, &dest_path, Some(progress_callback)) {
            Ok(size) => {
                total_downloaded += size;
                downloaded_files.push(file.to_string());
                crate::log_info!("models", "Downloaded {} ({} bytes)", file, size);
            }
            Err(e) => {
                // Update state to failed
                {
                    let mut state = DOWNLOAD_STATE.write().unwrap();
                    *state = DownloadState::Failed {
                        model_name: model_id.to_string(),
                        error: e.clone(),
                    };
                }
                DOWNLOAD_IN_PROGRESS.store(false, Ordering::SeqCst);
                // Clean up partial download
                let _ = fs::remove_dir_all(&model_dir);
                return Err(format!("Failed to download {}: {}", file, e));
            }
        }
    }

    // Determine version from model ID
    let version = if model_id.contains("v2") {
        "v2"
    } else if model_id.contains("v3") {
        "v3"
    } else {
        "v1"
    };

    // Write metadata
    let metadata = ModelMetadata {
        name: model_id.to_string(),
        version: version.to_string(),
        checksum: String::new(),
        downloaded_at: chrono::Utc::now().to_rfc3339(),
        size: total_downloaded,
        source: Some(model_def.repo.to_string()),
        variant: Some(model_id.to_string()),
        files: downloaded_files,
    };

    let metadata_json = serde_json::to_string_pretty(&metadata)
        .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

    fs::write(model_dir.join("metadata.json"), metadata_json)
        .map_err(|e| format!("Failed to write metadata: {}", e))?;

    crate::log_info!(
        "models",
        "Model {} downloaded successfully ({} bytes)",
        model_id,
        total_downloaded
    );

    // Update state to completed
    {
        let mut state = DOWNLOAD_STATE.write().unwrap();
        *state = DownloadState::Completed {
            model_name: model_id.to_string(),
        };
    }
    DOWNLOAD_IN_PROGRESS.store(false, Ordering::SeqCst);

    Ok(model_dir)
}

/// Check if a download is currently in progress
pub fn is_download_in_progress() -> bool {
    use crate::state::DOWNLOAD_IN_PROGRESS;
    use std::sync::atomic::Ordering;
    DOWNLOAD_IN_PROGRESS.load(Ordering::SeqCst)
}

/// Get current download status string for tray menu
pub fn get_download_status() -> Option<String> {
    use crate::state::{DownloadState, DOWNLOAD_STATE};
    let state = DOWNLOAD_STATE.read().unwrap();
    match &*state {
        DownloadState::Idle => None,
        DownloadState::Downloading {
            model_name: _,
            current_file,
            file_index,
            total_files,
            bytes_downloaded,
            total_bytes,
            file_bytes_downloaded,
            file_total_bytes,
        } => {
            // Show overall progress if we know total size
            if *total_bytes > 0 {
                let percent = (*bytes_downloaded as f64 / *total_bytes as f64 * 100.0) as u32;
                let downloaded_mb = *bytes_downloaded as f64 / 1_000_000.0;
                let total_mb = *total_bytes as f64 / 1_000_000.0;
                Some(format!(
                    "Downloading: {:.0}/{:.0} MB ({}%) - file {}/{}",
                    downloaded_mb,
                    total_mb,
                    percent,
                    file_index + 1,
                    total_files
                ))
            } else if *file_total_bytes > 0 {
                // Show file progress if we know file size
                let percent =
                    (*file_bytes_downloaded as f64 / *file_total_bytes as f64 * 100.0) as u32;
                Some(format!(
                    "Downloading: {} ({}%) - {}/{}",
                    current_file,
                    percent,
                    file_index + 1,
                    total_files
                ))
            } else {
                Some(format!(
                    "Downloading: {} ({}/{})",
                    current_file,
                    file_index + 1,
                    total_files
                ))
            }
        }
        DownloadState::Completed { model_name } => Some(format!("Completed: {}", model_name)),
        DownloadState::Failed { error, .. } => Some(format!("Failed: {}", error)),
    }
}

/// Callback for download progress updates
pub type ProgressCallback = Box<dyn Fn(u64, u64) + Send>;

/// Download a single file synchronously with optional progress callback
fn download_file_sync(url: &str, dest_path: &PathBuf) -> Result<u64, String> {
    download_file_with_progress(url, dest_path, None)
}

/// Download a single file with progress reporting
pub fn download_file_with_progress(
    url: &str,
    dest_path: &PathBuf,
    progress_callback: Option<ProgressCallback>,
) -> Result<u64, String> {
    use std::io::{Read, Write};

    // Make HTTP request with ureq (no terminal window needed)
    let response = ureq::get(url)
        .call()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    // Get content length if available
    let content_length: u64 = response
        .header("content-length")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    // Create output file
    let mut file =
        fs::File::create(dest_path).map_err(|e| format!("Failed to create file: {}", e))?;

    // Read response body in chunks with progress reporting
    let mut reader = response.into_reader();
    let mut buffer = [0u8; 65536]; // 64KB buffer
    let mut total_downloaded: u64 = 0;
    let mut last_progress_report: u64 = 0;

    loop {
        let bytes_read = reader
            .read(&mut buffer)
            .map_err(|e| format!("Failed to read from response: {}", e))?;

        if bytes_read == 0 {
            break;
        }

        file.write_all(&buffer[..bytes_read])
            .map_err(|e| format!("Failed to write to file: {}", e))?;

        total_downloaded += bytes_read as u64;

        // Report progress every 100KB or so to avoid flooding
        if let Some(ref callback) = progress_callback {
            if total_downloaded - last_progress_report >= 102400 || bytes_read == 0 {
                callback(total_downloaded, content_length);
                last_progress_report = total_downloaded;
            }
        }
    }

    // Final progress callback
    if let Some(callback) = progress_callback {
        callback(total_downloaded, content_length);
    }

    Ok(total_downloaded)
}

/// Remove a model
pub fn remove_model(model_name: &str) -> bool {
    let model_path = get_model_path(model_name);

    if !model_path.exists() {
        return false;
    }

    fs::remove_dir_all(&model_path).is_ok()
}

/// Remove all models except the default
pub fn clean_models() -> Vec<String> {
    let models = list_models();
    let mut removed = Vec::new();
    let active_model = crate::config::load_config()
        .map(|config| normalize_model_name(&config.model).to_string())
        .unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    for model in models {
        if !model.is_default && normalize_model_name(&model.name) != active_model {
            if remove_model(&model.name) {
                removed.push(model.name);
            }
        }
    }

    removed
}

/// Format bytes as human-readable string
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.0} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

// ============================================================================
// VAD Model Management
// ============================================================================

/// Get the VAD model directory path
pub fn get_vad_model_dir() -> PathBuf {
    get_models_dir().join(VAD_MODEL_NAME)
}

/// Get the VAD model file path
pub fn get_vad_model_path() -> PathBuf {
    get_vad_model_dir().join(VAD_MODEL_FILENAME)
}

/// Check if VAD model is installed
pub fn is_vad_model_installed() -> bool {
    get_vad_model_path().exists()
}

/// Download VAD model synchronously
pub fn download_vad_model_sync() -> Result<PathBuf, String> {
    let vad_dir = get_vad_model_dir();
    let vad_path = get_vad_model_path();

    // Check if already installed
    if vad_path.exists() {
        crate::log_info!("models", "VAD model already installed");
        return Ok(vad_path);
    }

    crate::log_info!("models", "Downloading VAD model...");

    // Create directory
    if let Err(e) = fs::create_dir_all(&vad_dir) {
        return Err(format!("Failed to create VAD model directory: {}", e));
    }

    // Download the model file
    match download_file_with_progress(VAD_MODEL_URL, &vad_path, None) {
        Ok(size) => {
            crate::log_info!("models", "VAD model downloaded ({} bytes)", size);
            Ok(vad_path)
        }
        Err(e) => {
            // Clean up on failure
            let _ = fs::remove_dir_all(&vad_dir);
            Err(format!("Failed to download VAD model: {}", e))
        }
    }
}

// ============================================================================
// Model File Path Helpers (for STT engine)
// ============================================================================

/// Model file paths for the STT engine
#[derive(Debug, Clone)]
pub struct ModelPaths {
    pub model_id: String,
    pub architecture: ModelArchitecture,
    pub encoder_path: PathBuf,
    pub decoder_path: PathBuf,
    pub joiner_path: Option<PathBuf>,
    pub vocab_path: PathBuf,
    pub preprocessor_path: Option<PathBuf>,
    pub embeddings_path: Option<PathBuf>,
    pub config: ModelConfig,
}

/// Get file paths for a model by ID
pub fn get_model_paths(model_id: &str) -> Option<ModelPaths> {
    // Normalize legacy model names
    let model_id = normalize_model_name(model_id);

    // Get model definition
    let model_def = get_model_definition(model_id)?;
    let model_dir = get_model_path(model_id);

    // Find required file paths by role
    let mut encoder_path = None;
    let mut decoder_path = None;
    let mut joiner_path = None;
    let mut vocab_path = None;
    let mut preprocessor_path = None;
    let mut embeddings_path = None;

    for file in model_def.files {
        let file_path = model_dir.join(file.name);
        match file.role {
            FileRole::Encoder => encoder_path = Some(file_path),
            FileRole::Decoder | FileRole::DecoderWithPast => decoder_path = Some(file_path),
            FileRole::Joiner => joiner_path = Some(file_path),
            FileRole::Vocab => vocab_path = Some(file_path),
            FileRole::Preprocessor => preprocessor_path = Some(file_path),
            FileRole::Embeddings => embeddings_path = Some(file_path),
            _ => {} // Config, data files, etc.
        }
    }

    // Encoder, decoder, and vocab are required
    let encoder_path = encoder_path?;
    let decoder_path = decoder_path?;
    let vocab_path = vocab_path?;

    Some(ModelPaths {
        model_id: model_id.to_string(),
        architecture: model_def.architecture,
        encoder_path,
        decoder_path,
        joiner_path,
        vocab_path,
        preprocessor_path,
        embeddings_path,
        config: model_def.config.clone(),
    })
}

/// Get file path for a specific role in a model
pub fn get_model_file_path(model_id: &str, role: FileRole) -> Option<PathBuf> {
    let model_id = normalize_model_name(model_id);
    let model_def = get_model_definition(model_id)?;
    let model_dir = get_model_path(model_id);

    for file in model_def.files {
        if file.role == role {
            return Some(model_dir.join(file.name));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parakeet_v2_is_legacy_but_still_explicitly_known() {
        let legacy = get_model_definition("parakeet-tdt-v2-int8").unwrap();
        let available_ids: Vec<&str> = get_available_models()
            .iter()
            .map(|model| model.id)
            .collect();

        assert!(legacy.is_legacy);
        assert!(!available_ids.contains(&"parakeet-tdt-v2-int8"));
        assert!(available_ids.contains(&"parakeet-tdt-v3-int8"));
    }
}
