//! Configuration management

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// dybur configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DyburConfig {
    pub hotkey: String,
    pub auto_punctuation: bool,
    pub sentence_case: bool,
    pub silence_timeout_ms: u32,
    pub model: String,
    pub clipboard_cleanup: bool,
    /// Input device (microphone) name to use, None for system default
    #[serde(default)]
    pub input_device: Option<String>,
    /// Recording mode: "toggle" (press to start/stop) or "push_to_talk" (hold to record)
    #[serde(default = "default_recording_mode")]
    pub recording_mode: String,
    /// Enable Voice Activity Detection to filter silence before transcription
    #[serde(default = "default_vad_enabled")]
    pub vad_enabled: bool,
    /// VAD speech probability threshold (0.0-1.0)
    #[serde(default = "default_vad_threshold")]
    pub vad_threshold: f32,
    /// Minimum speech duration in ms to keep
    #[serde(default = "default_vad_min_speech_ms")]
    pub vad_min_speech_ms: u32,
    /// GPU acceleration mode: "auto" (detect and use GPU if available) or "cpu" (CPU only)
    #[serde(default = "default_gpu_mode")]
    pub gpu_mode: String,
}

fn default_recording_mode() -> String {
    "toggle".to_string()
}

fn default_gpu_mode() -> String {
    "auto".to_string()
}

fn default_vad_enabled() -> bool {
    true
}

fn default_vad_threshold() -> f32 {
    0.5
}

fn default_vad_min_speech_ms() -> u32 {
    250
}

impl Default for DyburConfig {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+Space".to_string(),
            auto_punctuation: true,
            sentence_case: true,
            silence_timeout_ms: 1000,
            model: "parakeet-tdt-0.6b-v3-onnx".to_string(),
            clipboard_cleanup: true,
            input_device: None,
            recording_mode: default_recording_mode(),
            vad_enabled: default_vad_enabled(),
            vad_threshold: default_vad_threshold(),
            vad_min_speech_ms: default_vad_min_speech_ms(),
            gpu_mode: default_gpu_mode(),
        }
    }
}

/// Get the configuration directory path
pub fn get_config_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir()
            .unwrap_or_default()
            .join("Library/Application Support/dybur")
    }

    #[cfg(target_os = "windows")]
    {
        dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join("AppData/Roaming"))
            .join("dybur")
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir()
            .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
            .join("dybur")
    }
}

/// Get the config file path
pub fn get_config_path() -> PathBuf {
    get_config_dir().join("config.json")
}

/// Get the data directory path
pub fn get_data_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".dybur")
}

/// Get the models directory path
pub fn get_models_dir() -> PathBuf {
    get_data_dir().join("models")
}

/// Get the logs directory path
pub fn get_logs_dir() -> String {
    get_data_dir().join("logs").to_string_lossy().to_string()
}

/// Load configuration from disk
pub fn load_config() -> Result<DyburConfig, String> {
    let config_path = get_config_path();

    if !config_path.exists() {
        // Create default config
        let config = DyburConfig::default();
        save_config(&config)?;
        return Ok(config);
    }

    let content =
        fs::read_to_string(&config_path).map_err(|e| format!("Failed to read config: {}", e))?;

    serde_json::from_str(&content).map_err(|e| format!("Failed to parse config: {}", e))
}

/// Save configuration to disk
pub fn save_config(config: &DyburConfig) -> Result<(), String> {
    let config_path = get_config_path();

    // Ensure directory exists
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }

    let content = serde_json::to_string_pretty(config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    fs::write(&config_path, content).map_err(|e| format!("Failed to write config: {}", e))
}
