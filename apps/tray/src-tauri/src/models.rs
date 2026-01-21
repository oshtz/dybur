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
    let model_dir = get_model_path(model_name);

    // Check if already installed
    if is_model_installed(model_name) {
        return Ok(model_dir);
    }

    // Create model directory
    fs::create_dir_all(&model_dir)
        .map_err(|e| format!("Failed to create model directory: {}", e))?;

    let files = if variant == "int8" {
        MODEL_FILES_INT8
    } else {
        MODEL_FILES_FULL
    };

    let mut total_downloaded = 0u64;
    let mut downloaded_files = Vec::new();

    for file in files {
        let url = format!("{}/{}", MODEL_BASE_URL, file);
        let dest_path = model_dir.join(file);

        crate::log_info!("models", "Downloading {}...", file);

        match download_file_sync(&url, &dest_path) {
            Ok(size) => {
                total_downloaded += size;
                downloaded_files.push(file.to_string());
                crate::log_info!("models", "Downloaded {} ({} bytes)", file, size);
            }
            Err(e) => {
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

    Ok(model_dir)
}

/// Download a single file synchronously
fn download_file_sync(url: &str, dest_path: &PathBuf) -> Result<u64, String> {
    // Use ureq for blocking HTTP requests (lighter than reqwest for sync)
    // Since we don't have ureq, we'll use std::process::Command with curl/powershell

    #[cfg(target_os = "windows")]
    {
        download_file_windows(url, dest_path)
    }

    #[cfg(not(target_os = "windows"))]
    {
        download_file_curl(url, dest_path)
    }
}

#[cfg(target_os = "windows")]
fn download_file_windows(url: &str, dest_path: &PathBuf) -> Result<u64, String> {
    use std::process::Command;

    let dest_str = dest_path.to_string_lossy();

    // Try curl first (available on Windows 10+)
    let curl_result = Command::new("curl")
        .args(["-L", "-o", &dest_str, url])
        .output();

    if let Ok(output) = curl_result {
        if output.status.success() {
            let size = fs::metadata(dest_path)
                .map(|m| m.len())
                .unwrap_or(0);
            return Ok(size);
        }
    }

    // Fall back to PowerShell
    let ps_cmd = format!(
        "Invoke-WebRequest -Uri '{}' -OutFile '{}'",
        url, dest_str
    );

    let output = Command::new("powershell")
        .args(["-Command", &ps_cmd])
        .output()
        .map_err(|e| format!("Failed to execute download command: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Download failed: {}", stderr));
    }

    let size = fs::metadata(dest_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(size)
}

#[cfg(not(target_os = "windows"))]
fn download_file_curl(url: &str, dest_path: &PathBuf) -> Result<u64, String> {
    use std::process::Command;

    let dest_str = dest_path.to_string_lossy();

    let output = Command::new("curl")
        .args(["-L", "-o", &dest_str, url])
        .output()
        .map_err(|e| format!("Failed to execute curl: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Download failed: {}", stderr));
    }

    let size = fs::metadata(dest_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(size)
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
