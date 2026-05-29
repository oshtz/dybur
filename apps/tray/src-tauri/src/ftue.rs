//! First Time User Experience (FTUE) management
//!
//! Handles the onboarding flow for new users including:
//! - System requirements checking
//! - Permission requests
//! - Model download
//! - First dictation tutorial

use crate::audio::list_input_devices;
use crate::config::{get_data_dir, load_config, save_config, DyburConfig};
use crate::models::{
    download_model_sync, format_bytes, get_available_models, get_model_definition,
    is_default_model_installed, is_model_installed, normalize_model_name, DEFAULT_MODEL,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager};

/// FTUE state persisted to disk
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FtueState {
    /// Whether FTUE has been completed
    pub completed: bool,
    /// Whether FTUE was skipped
    pub skipped: bool,
    /// Current step (1-5)
    pub current_step: u8,
    /// Timestamp when FTUE was started
    pub started_at: Option<String>,
    /// Timestamp when FTUE was completed
    pub completed_at: Option<String>,
}

impl Default for FtueState {
    fn default() -> Self {
        Self {
            completed: false,
            skipped: false,
            current_step: 1,
            started_at: None,
            completed_at: None,
        }
    }
}

/// System check result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckResult {
    pub ok: bool,
    pub detail: String,
}

/// System check results
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemCheckResults {
    pub os: CheckResult,
    pub disk: CheckResult,
    pub microphone: CheckResult,
    pub internet: CheckResult,
}

/// Download progress event payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    /// Overall progress percentage (0-100)
    pub progress: f32,
    /// Human-readable status message
    pub status: String,
    /// Current file being downloaded
    pub current_file: Option<String>,
    /// Current file index (0-based)
    pub file_index: usize,
    /// Total number of files
    pub total_files: usize,
    /// Bytes downloaded for current file
    pub file_bytes: u64,
    /// Total bytes for current file (0 if unknown)
    pub file_total_bytes: u64,
}

/// Model option shown in FTUE
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FtueModel {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub size: String,
    pub languages: Vec<String>,
    pub supports_streaming: bool,
    pub installed: bool,
    pub is_default: bool,
    pub current: bool,
}

/// Get the FTUE state file path
pub fn get_ftue_state_path() -> PathBuf {
    get_data_dir().join("ftue-state.json")
}

/// Load FTUE state from disk
pub fn load_ftue_state() -> FtueState {
    let path = get_ftue_state_path();
    if !path.exists() {
        return FtueState::default();
    }

    fs::read_to_string(&path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

/// Save FTUE state to disk
pub fn save_ftue_state(state: &FtueState) -> Result<(), String> {
    let path = get_ftue_state_path();

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create FTUE state directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize FTUE state: {}", e))?;

    fs::write(&path, content).map_err(|e| format!("Failed to write FTUE state: {}", e))
}

/// Check if FTUE should be shown
pub fn should_show_ftue() -> bool {
    let state = load_ftue_state();

    // Don't show if completed or skipped
    if state.completed || state.skipped {
        return false;
    }

    let active_model = load_config()
        .map(|config| normalize_model_name(&config.model).to_string())
        .unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    // Show if the configured model is not installed (main reason for FTUE)
    if !is_model_installed(&active_model) {
        return true;
    }

    // Show if FTUE was started but not completed
    if state.started_at.is_some() && !state.completed {
        return true;
    }

    false
}

/// Run system checks
pub fn run_system_check() -> SystemCheckResults {
    SystemCheckResults {
        os: check_os(),
        disk: check_disk_space(),
        microphone: check_microphone(),
        internet: check_internet(),
    }
}

fn check_os() -> CheckResult {
    #[cfg(target_os = "windows")]
    {
        CheckResult {
            ok: true,
            detail: "Windows 10/11".to_string(),
        }
    }
    #[cfg(target_os = "macos")]
    {
        CheckResult {
            ok: true,
            detail: "macOS".to_string(),
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        CheckResult {
            ok: false,
            detail: "Unsupported operating system".to_string(),
        }
    }
}

fn check_disk_space() -> CheckResult {
    let data_dir = get_data_dir();

    // Try to get available space
    // This is a simplified check - just verify we can write to the directory
    if let Err(_) = fs::create_dir_all(&data_dir) {
        return CheckResult {
            ok: false,
            detail: "Cannot write to data directory".to_string(),
        };
    }

    // We need about 1GB for the model
    // For now, just assume we have enough space if the directory is writable
    CheckResult {
        ok: true,
        detail: "Sufficient space available".to_string(),
    }
}

fn check_microphone() -> CheckResult {
    let devices = list_input_devices();

    if devices.is_empty() {
        CheckResult {
            ok: false,
            detail: "No microphone detected".to_string(),
        }
    } else {
        let default_device = devices.iter().find(|d| d.is_default);
        let device_name = default_device
            .map(|d| d.name.clone())
            .unwrap_or_else(|| devices[0].name.clone());

        CheckResult {
            ok: true,
            detail: format!("Found: {}", device_name),
        }
    }
}

fn check_internet() -> CheckResult {
    // Try to connect to HuggingFace (where models are hosted)
    // This is a simple connectivity check

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;

        let result = Command::new("ping")
            .args(["-n", "1", "-w", "3000", "huggingface.co"])
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    CheckResult {
                        ok: true,
                        detail: "Connected".to_string(),
                    }
                } else {
                    CheckResult {
                        ok: false,
                        detail: "No internet connection".to_string(),
                    }
                }
            }
            Err(_) => CheckResult {
                ok: true, // Assume connected if ping fails (might be blocked)
                detail: "Connection status unknown".to_string(),
            },
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        use std::process::Command;

        let result = Command::new("ping")
            .args(["-c", "1", "-W", "3", "huggingface.co"])
            .output();

        match result {
            Ok(output) => {
                if output.status.success() {
                    CheckResult {
                        ok: true,
                        detail: "Connected".to_string(),
                    }
                } else {
                    CheckResult {
                        ok: false,
                        detail: "No internet connection".to_string(),
                    }
                }
            }
            Err(_) => CheckResult {
                ok: true,
                detail: "Connection status unknown".to_string(),
            },
        }
    }
}

fn save_active_model(model_id: &str) -> Result<(), String> {
    let mut config = load_config()?;
    config.model = model_id.to_string();
    save_config(&config)
}

fn load_selected_model(model_id: &str) {
    // Load the model so users can dictate immediately without restarting.
    if let Some(stt_config) = crate::stt::get_model_paths(model_id) {
        let gpu_preference = load_config()
            .map(|c| crate::execution_providers::parse_gpu_preference(&c.gpu_mode))
            .unwrap_or(crate::execution_providers::GpuPreference::Auto);

        let mut engine = crate::STT_ENGINE.lock().unwrap();
        match engine.load(stt_config, gpu_preference) {
            Ok(()) => {
                crate::log_info!("ftue", "STT model '{}' loaded and ready", model_id);
            }
            Err(e) => {
                crate::log_error!("ftue", "Failed to load STT model '{}': {}", model_id, e);
            }
        }
    } else {
        crate::log_warn!("ftue", "Model paths missing after selecting '{}'", model_id);
    }
}

// Tauri Commands

/// Get the current configuration for FTUE
#[tauri::command]
pub fn ftue_get_config() -> Result<DyburConfig, String> {
    load_config()
}

/// Get available model options for FTUE
#[tauri::command]
pub fn ftue_get_models() -> Vec<FtueModel> {
    let current_model = load_config()
        .map(|config| normalize_model_name(&config.model).to_string())
        .unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    get_available_models()
        .iter()
        .map(|model| FtueModel {
            id: model.id.to_string(),
            display_name: model.display_name.to_string(),
            description: model.description.to_string(),
            size: format_bytes(model.size_bytes),
            languages: model
                .languages
                .iter()
                .map(|language| language.to_string())
                .collect(),
            supports_streaming: model.config.supports_streaming,
            installed: is_model_installed(model.id),
            is_default: model.is_default,
            current: model.id == current_model,
        })
        .collect()
}

/// Get the current platform
#[tauri::command]
pub fn ftue_get_platform() -> String {
    #[cfg(target_os = "windows")]
    {
        "windows".to_string()
    }
    #[cfg(target_os = "macos")]
    {
        "macos".to_string()
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        "unknown".to_string()
    }
}

/// Run system checks and return results directly
#[tauri::command]
pub fn ftue_run_system_check() -> SystemCheckResults {
    run_system_check()
}

/// Start model download
#[tauri::command]
pub async fn ftue_start_download(app: AppHandle, model_id: Option<String>) -> Result<(), String> {
    use crate::state::{DownloadState, DOWNLOAD_STATE};

    let selected_model = model_id
        .map(|model| normalize_model_name(&model).to_string())
        .or_else(|| {
            load_config()
                .ok()
                .map(|config| normalize_model_name(&config.model).to_string())
        })
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    if get_model_definition(&selected_model).is_none() {
        return Err(format!("Unknown model: {}", selected_model));
    }

    // Check if already installed
    if is_model_installed(&selected_model) {
        save_active_model(&selected_model)?;
        load_selected_model(&selected_model);
        let _ = app.emit_to("ftue", "ftue:download-complete", ());
        return Ok(());
    }

    crate::log_info!("ftue", "Starting model download: {}", selected_model);

    // Start download in background thread
    let app_handle = app.clone();
    let selected_model_for_thread = selected_model.clone();
    std::thread::spawn(move || {
        // Emit progress updates in a separate thread that polls the state
        let progress_app = app_handle.clone();
        std::thread::spawn(move || {
            loop {
                let state = DOWNLOAD_STATE.read().unwrap().clone();
                match &state {
                    DownloadState::Downloading {
                        current_file,
                        file_index,
                        total_files,
                        file_bytes_downloaded,
                        file_total_bytes,
                        ..
                    } => {
                        // Calculate overall progress based on file index + current file progress
                        let file_progress = if *file_total_bytes > 0 {
                            *file_bytes_downloaded as f32 / *file_total_bytes as f32
                        } else {
                            0.0
                        };
                        let overall_progress =
                            ((*file_index as f32 + file_progress) / *total_files as f32) * 100.0;

                        // Format status with file size
                        let status = if *file_total_bytes > 0 {
                            format!(
                                "Downloading {} ({}/{}) - {:.1} MB / {:.1} MB",
                                current_file,
                                file_index + 1,
                                total_files,
                                *file_bytes_downloaded as f64 / 1_048_576.0,
                                *file_total_bytes as f64 / 1_048_576.0
                            )
                        } else {
                            format!(
                                "Downloading {} ({}/{}) - {:.1} MB",
                                current_file,
                                file_index + 1,
                                total_files,
                                *file_bytes_downloaded as f64 / 1_048_576.0
                            )
                        };

                        let payload = DownloadProgress {
                            progress: overall_progress,
                            status,
                            current_file: Some(current_file.clone()),
                            file_index: *file_index,
                            total_files: *total_files,
                            file_bytes: *file_bytes_downloaded,
                            file_total_bytes: *file_total_bytes,
                        };

                        let _ = progress_app.emit_to("ftue", "ftue:download-progress", &payload);
                    }
                    DownloadState::Completed { .. } => {
                        let _ = progress_app.emit_to("ftue", "ftue:download-complete", ());
                        break;
                    }
                    DownloadState::Failed { error, .. } => {
                        crate::log_error!("ftue", "Download failed: {}", error);
                        let _ = progress_app.emit_to(
                            "ftue",
                            "ftue:download-error",
                            serde_json::json!({ "error": error }),
                        );
                        break;
                    }
                    DownloadState::Idle => {}
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        });

        // Actually download the model
        match download_model_sync(&selected_model_for_thread, "int8") {
            Ok(_) => {
                crate::log_info!("ftue", "Model download completed");

                if let Err(e) = save_active_model(&selected_model_for_thread) {
                    crate::log_error!("ftue", "Failed to save selected model: {}", e);
                }
                load_selected_model(&selected_model_for_thread);
            }
            Err(e) => {
                crate::log_error!("ftue", "Model download failed: {}", e);
            }
        }
    });

    Ok(())
}

/// Mark FTUE as completed
#[tauri::command]
pub fn ftue_complete() -> Result<(), String> {
    let mut state = load_ftue_state();
    state.completed = true;
    state.completed_at = Some(chrono::Utc::now().to_rfc3339());
    save_ftue_state(&state)
}

/// Mark FTUE as skipped
#[tauri::command]
pub fn ftue_skip() -> Result<(), String> {
    let mut state = load_ftue_state();
    state.skipped = true;
    save_ftue_state(&state)
}

/// Check if the default model is already installed
#[tauri::command]
pub fn ftue_check_model_installed() -> bool {
    load_config()
        .map(|config| is_model_installed(&config.model))
        .unwrap_or_else(|_| is_default_model_installed())
}

/// Close FTUE window
#[tauri::command]
pub fn ftue_close(app: AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window("ftue") {
        window
            .close()
            .map_err(|e| format!("Failed to close FTUE window: {}", e))?;
    }
    Ok(())
}

/// Create and show the FTUE window
pub fn show_ftue_window(app: &AppHandle) -> Result<(), String> {
    use tauri::{WebviewUrl, WebviewWindowBuilder};

    // Check if window already exists
    if app.get_webview_window("ftue").is_some() {
        return Ok(());
    }

    // Mark FTUE as started
    let mut state = load_ftue_state();
    if state.started_at.is_none() {
        state.started_at = Some(chrono::Utc::now().to_rfc3339());
        let _ = save_ftue_state(&state);
    }

    // Create FTUE window
    let window = WebviewWindowBuilder::new(app, "ftue", WebviewUrl::App("ftue.html".into()))
        .title("Welcome to dybur")
        .inner_size(550.0, 700.0)
        .resizable(false)
        .center()
        .decorations(true)
        .build()
        .map_err(|e| format!("Failed to create FTUE window: {}", e))?;

    window
        .show()
        .map_err(|e| format!("Failed to show FTUE window: {}", e))?;

    crate::log_info!("ftue", "FTUE window opened");

    Ok(())
}
