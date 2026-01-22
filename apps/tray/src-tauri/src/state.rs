//! Application state management
//!
//! Note: AudioCapture is managed separately (not in Tauri state) because
//! cpal::Stream is not Send+Sync. See main.rs for audio handling.

use crate::config::{load_config, DyburConfig};

/// Recording state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingState {
    Idle,
    Recording,
    Processing,
    Error,
}

/// Main application state (Send + Sync safe)
/// 
/// Audio capture is handled separately via thread-local storage
/// because cpal::Stream cannot be sent between threads.
pub struct AppState {
    pub config: DyburConfig,
    pub recording_state: RecordingState,
    pub is_recording: bool,
    pub last_audio_error: Option<String>,
}

impl AppState {
    /// Create new application state with defaults
    pub fn new() -> Self {
        Self {
            config: DyburConfig::default(),
            recording_state: RecordingState::Idle,
            is_recording: false,
            last_audio_error: None,
        }
    }

    /// Load configuration from disk
    pub fn load_config(&mut self) -> Result<(), String> {
        self.config = load_config()?;
        Ok(())
    }

    /// Set recording state
    pub fn set_recording(&mut self, recording: bool) {
        self.is_recording = recording;
        self.recording_state = if recording {
            RecordingState::Recording
        } else {
            RecordingState::Idle
        };
    }

    /// Set error state
    pub fn set_error(&mut self, error: String) {
        self.last_audio_error = Some(error);
        self.recording_state = RecordingState::Error;
    }

    /// Clear error
    pub fn clear_error(&mut self) {
        self.last_audio_error = None;
    }

    /// Get current recording state
    pub fn get_state(&self) -> RecordingState {
        self.recording_state
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Model download state for tracking progress
#[derive(Debug, Clone)]
pub enum DownloadState {
    /// No download in progress
    Idle,
    /// Download in progress
    Downloading {
        model_name: String,
        current_file: String,
        file_index: usize,
        total_files: usize,
        bytes_downloaded: u64,
        total_bytes: u64,
        /// Bytes downloaded for the current file
        file_bytes_downloaded: u64,
        /// Total bytes for the current file (if known)
        file_total_bytes: u64,
    },
    /// Download completed
    Completed {
        model_name: String,
    },
    /// Download failed
    Failed {
        model_name: String,
        error: String,
    },
}

impl Default for DownloadState {
    fn default() -> Self {
        DownloadState::Idle
    }
}

/// Global download state (separate from AppState for thread safety)
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

lazy_static::lazy_static! {
    pub static ref DOWNLOAD_STATE: RwLock<DownloadState> = RwLock::new(DownloadState::Idle);
    pub static ref DOWNLOAD_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
}

impl DownloadState {
    pub fn is_downloading(&self) -> bool {
        matches!(self, DownloadState::Downloading { .. })
    }

    pub fn progress_percent(&self) -> Option<u8> {
        if let DownloadState::Downloading { file_index, total_files, .. } = self {
            Some(((*file_index as f32 / *total_files as f32) * 100.0) as u8)
        } else {
            None
        }
    }

    pub fn status_string(&self) -> String {
        match self {
            DownloadState::Idle => "Idle".to_string(),
            DownloadState::Downloading { current_file, file_index, total_files, .. } => {
                format!("Downloading {} ({}/{})", current_file, file_index + 1, total_files)
            }
            DownloadState::Completed { model_name } => {
                format!("Downloaded {}", model_name)
            }
            DownloadState::Failed { error, .. } => {
                format!("Failed: {}", error)
            }
        }
    }
}
