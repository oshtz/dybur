//! Model management for dybur
//!
//! Handles downloading, listing, and managing speech recognition models.

use crate::config::get_models_dir;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Default model name
pub const DEFAULT_MODEL: &str = "parakeet-tdt-0.6b-v3-onnx";

/// HuggingFace model repository
pub const MODEL_REPO: &str = "istupakov/parakeet-tdt-0.6b-v3-onnx";

/// Base URL for model downloads
pub const MODEL_BASE_URL: &str = "https://huggingface.co/istupakov/parakeet-tdt-0.6b-v3-onnx/resolve/main";

/// VAD model constants
pub const VAD_MODEL_NAME: &str = "silero-vad";
pub const VAD_MODEL_URL: &str = "https://github.com/snakers4/silero-vad/raw/master/src/silero_vad/data/silero_vad.onnx";
pub const VAD_MODEL_FILENAME: &str = "silero_vad.onnx";

/// Model files for INT8 variant (smaller, ~670MB)
pub const MODEL_FILES_INT8: &[&str] = &[
    "encoder-model.int8.onnx",
    "decoder_joint-model.int8.onnx",
    "nemo128.onnx",
    "vocab.txt",
    "config.json",
];

/// Model files for full variant (~2.5GB)
pub const MODEL_FILES_FULL: &[&str] = &[
    "encoder-model.onnx",
    "encoder-model.onnx.data",
    "decoder_joint-model.onnx",
    "nemo128.onnx",
    "vocab.txt",
    "config.json",
];

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

        models.push(InstalledModel {
            name: name.clone(),
            path: model_path,
            metadata,
            size,
            is_default: name == DEFAULT_MODEL,
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
    let model_path = get_model_path(model_name);
    let metadata_path = model_path.join("metadata.json");

    if !model_path.exists() || !metadata_path.exists() {
        return false;
    }

    // Check for essential model files
    let has_encoder = model_path.join("encoder-model.int8.onnx").exists()
        || model_path.join("encoder-model.onnx").exists();
    let has_decoder = model_path.join("decoder_joint-model.int8.onnx").exists()
        || model_path.join("decoder_joint-model.onnx").exists();
    let has_vocab = model_path.join("vocab.txt").exists();

    has_encoder && has_decoder && has_vocab
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

/// Download a model synchronously (blocking)
/// This is used for the tray app which runs downloads in a separate thread
pub fn download_model_sync(model_name: &str, variant: &str) -> Result<PathBuf, String> {
    use crate::state::{DownloadState, DOWNLOAD_STATE, DOWNLOAD_IN_PROGRESS};
    use std::sync::atomic::Ordering;

    let model_dir = get_model_path(model_name);

    // Check if already installed
    if is_model_installed(model_name) {
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

    let files = if variant == "int8" {
        MODEL_FILES_INT8
    } else {
        MODEL_FILES_FULL
    };

    let total_files = files.len();
    let mut total_downloaded = 0u64;
    let mut downloaded_files = Vec::new();

    for (index, file) in files.iter().enumerate() {
        let url = format!("{}/{}", MODEL_BASE_URL, file);
        let dest_path = model_dir.join(file);

        // Update download state at start of file
        {
            let mut state = DOWNLOAD_STATE.write().unwrap();
            *state = DownloadState::Downloading {
                model_name: model_name.to_string(),
                current_file: file.to_string(),
                file_index: index,
                total_files,
                bytes_downloaded: total_downloaded,
                total_bytes: 0, // We don't know total until complete
                file_bytes_downloaded: 0,
                file_total_bytes: 0,
            };
        }

        crate::log_info!("models", "Downloading {} ({}/{})...", file, index + 1, total_files);

        // Create progress callback that updates the global state
        let model_name_clone = model_name.to_string();
        let file_clone = file.to_string();
        let total_downloaded_base = total_downloaded;
        let progress_callback: ProgressCallback = Box::new(move |file_downloaded, file_total| {
            let mut state = DOWNLOAD_STATE.write().unwrap();
            *state = DownloadState::Downloading {
                model_name: model_name_clone.clone(),
                current_file: file_clone.clone(),
                file_index: index,
                total_files,
                bytes_downloaded: total_downloaded_base,
                total_bytes: 0,
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
                        model_name: model_name.to_string(),
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

    // Write metadata
    let metadata = ModelMetadata {
        name: model_name.to_string(),
        version: "v3".to_string(),
        checksum: String::new(),
        downloaded_at: chrono::Utc::now().to_rfc3339(),
        size: total_downloaded,
        source: Some(MODEL_REPO.to_string()),
        variant: Some(variant.to_string()),
        files: downloaded_files,
    };

    let metadata_json = serde_json::to_string_pretty(&metadata)
        .map_err(|e| format!("Failed to serialize metadata: {}", e))?;

    fs::write(model_dir.join("metadata.json"), metadata_json)
        .map_err(|e| format!("Failed to write metadata: {}", e))?;

    crate::log_info!(
        "models",
        "Model {} downloaded successfully ({} bytes)",
        model_name,
        total_downloaded
    );

    // Update state to completed
    {
        let mut state = DOWNLOAD_STATE.write().unwrap();
        *state = DownloadState::Completed {
            model_name: model_name.to_string(),
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
        DownloadState::Downloading { current_file, file_index, total_files, .. } => {
            Some(format!("Downloading: {} ({}/{})", current_file, file_index + 1, total_files))
        }
        DownloadState::Completed { model_name } => {
            Some(format!("Completed: {}", model_name))
        }
        DownloadState::Failed { error, .. } => {
            Some(format!("Failed: {}", error))
        }
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
    let mut file = fs::File::create(dest_path)
        .map_err(|e| format!("Failed to create file: {}", e))?;

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

    for model in models {
        if !model.is_default {
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
