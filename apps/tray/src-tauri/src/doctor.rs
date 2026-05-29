//! Diagnostics for dybur
//!
//! Runs system checks to verify dybur is configured correctly.

use crate::audio::{has_input_device, list_input_devices};
use crate::config::{get_config_path, get_data_dir, get_models_dir, load_config};
use crate::models::{get_model_metadata, is_model_installed, normalize_model_name, DEFAULT_MODEL};

/// Diagnostic check status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticStatus {
    Pass,
    Warn,
    Fail,
}

/// Result of a diagnostic check
#[derive(Debug, Clone)]
pub struct DiagnosticResult {
    pub name: String,
    pub status: DiagnosticStatus,
    pub message: String,
    pub details: Option<String>,
}

/// Run all diagnostic checks
pub fn run_diagnostics() -> Vec<DiagnosticResult> {
    let mut results = Vec::new();

    results.push(check_config());
    results.push(check_model());
    results.push(check_audio_device());
    results.push(check_hotkey());
    results.push(check_input_device());
    results.push(check_directories());

    results
}

/// Check configuration validity
fn check_config() -> DiagnosticResult {
    let config_path = get_config_path();

    if !config_path.exists() {
        return DiagnosticResult {
            name: "Configuration".to_string(),
            status: DiagnosticStatus::Warn,
            message: "Config file not found".to_string(),
            details: Some(format!("Will be created at: {}", config_path.display())),
        };
    }

    match load_config() {
        Ok(config) => {
            // Basic validation
            if config.hotkey.is_empty() {
                return DiagnosticResult {
                    name: "Configuration".to_string(),
                    status: DiagnosticStatus::Warn,
                    message: "Hotkey not configured".to_string(),
                    details: Some("Set a hotkey in config.json".to_string()),
                };
            }

            DiagnosticResult {
                name: "Configuration".to_string(),
                status: DiagnosticStatus::Pass,
                message: "Valid configuration".to_string(),
                details: Some(format!("Hotkey: {}", config.hotkey)),
            }
        }
        Err(e) => DiagnosticResult {
            name: "Configuration".to_string(),
            status: DiagnosticStatus::Fail,
            message: "Failed to load config".to_string(),
            details: Some(e),
        },
    }
}

/// Check model installation
fn check_model() -> DiagnosticResult {
    let active_model = load_config()
        .map(|config| normalize_model_name(&config.model).to_string())
        .unwrap_or_else(|_| DEFAULT_MODEL.to_string());

    if !is_model_installed(&active_model) {
        return DiagnosticResult {
            name: "Speech Model".to_string(),
            status: DiagnosticStatus::Fail,
            message: format!("Model not installed: {}", active_model),
            details: Some("Download model from Models menu".to_string()),
        };
    }

    match get_model_metadata(&active_model) {
        Some(metadata) => {
            let variant = metadata.variant.as_deref().unwrap_or("full");
            let date = metadata
                .downloaded_at
                .split('T')
                .next()
                .unwrap_or("unknown");
            DiagnosticResult {
                name: "Speech Model".to_string(),
                status: DiagnosticStatus::Pass,
                message: active_model,
                details: Some(format!("{} variant, downloaded {}", variant, date)),
            }
        }
        None => DiagnosticResult {
            name: "Speech Model".to_string(),
            status: DiagnosticStatus::Warn,
            message: "Model installed but metadata missing".to_string(),
            details: None,
        },
    }
}

/// Check audio device availability
fn check_audio_device() -> DiagnosticResult {
    if has_input_device() {
        let devices = list_input_devices();
        let count = devices.len();
        let default = devices
            .iter()
            .find(|d| d.is_default)
            .map(|d| d.name.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        DiagnosticResult {
            name: "Audio Device".to_string(),
            status: DiagnosticStatus::Pass,
            message: "Audio device detected".to_string(),
            details: Some(format!("{} device(s), default: {}", count, default)),
        }
    } else {
        DiagnosticResult {
            name: "Audio Device".to_string(),
            status: DiagnosticStatus::Fail,
            message: "No audio input device found".to_string(),
            details: Some("Connect a microphone and restart dybur".to_string()),
        }
    }
}

/// Check hotkey configuration
fn check_hotkey() -> DiagnosticResult {
    match load_config() {
        Ok(config) => {
            if config.hotkey.is_empty() {
                return DiagnosticResult {
                    name: "Hotkey".to_string(),
                    status: DiagnosticStatus::Fail,
                    message: "No hotkey configured".to_string(),
                    details: Some("Set hotkey in config file".to_string()),
                };
            }

            // Basic format validation
            let parts: Vec<&str> = config.hotkey.split('+').collect();
            if parts.is_empty() || parts.len() > 4 {
                return DiagnosticResult {
                    name: "Hotkey".to_string(),
                    status: DiagnosticStatus::Warn,
                    message: "Hotkey format may be invalid".to_string(),
                    details: Some(format!("Current: {}", config.hotkey)),
                };
            }

            DiagnosticResult {
                name: "Hotkey".to_string(),
                status: DiagnosticStatus::Pass,
                message: config.hotkey,
                details: Some("Full test requires running service".to_string()),
            }
        }
        Err(_) => DiagnosticResult {
            name: "Hotkey".to_string(),
            status: DiagnosticStatus::Warn,
            message: "Could not check hotkey".to_string(),
            details: Some("Config not loaded".to_string()),
        },
    }
}

/// Check input device configuration
fn check_input_device() -> DiagnosticResult {
    match load_config() {
        Ok(config) => match config.input_device {
            Some(device) => {
                // Check if device exists
                let devices = list_input_devices();
                let exists = devices.iter().any(|d| d.name == device);

                if exists {
                    DiagnosticResult {
                        name: "Input Device".to_string(),
                        status: DiagnosticStatus::Pass,
                        message: device,
                        details: Some("Device found".to_string()),
                    }
                } else {
                    DiagnosticResult {
                        name: "Input Device".to_string(),
                        status: DiagnosticStatus::Warn,
                        message: device,
                        details: Some("Device not found, will use default".to_string()),
                    }
                }
            }
            None => DiagnosticResult {
                name: "Input Device".to_string(),
                status: DiagnosticStatus::Pass,
                message: "Using system default".to_string(),
                details: Some("Select device from Devices menu if needed".to_string()),
            },
        },
        Err(_) => DiagnosticResult {
            name: "Input Device".to_string(),
            status: DiagnosticStatus::Warn,
            message: "Could not check input device".to_string(),
            details: Some("Config not loaded".to_string()),
        },
    }
}

/// Check directories and permissions
fn check_directories() -> DiagnosticResult {
    let config_dir = get_config_path().parent().map(|p| p.to_path_buf());
    let data_dir = get_data_dir();
    let models_dir = get_models_dir();

    let mut issues = Vec::new();

    if let Some(dir) = config_dir {
        if !dir.exists() {
            issues.push("Config directory missing");
        }
    }

    if !data_dir.exists() {
        issues.push("Data directory missing");
    }

    if !models_dir.exists() {
        issues.push("Models directory missing");
    }

    if issues.is_empty() {
        DiagnosticResult {
            name: "Directories".to_string(),
            status: DiagnosticStatus::Pass,
            message: "All directories accessible".to_string(),
            details: None,
        }
    } else {
        DiagnosticResult {
            name: "Directories".to_string(),
            status: DiagnosticStatus::Warn,
            message: "Some directories missing".to_string(),
            details: Some("Will be created on first use".to_string()),
        }
    }
}
