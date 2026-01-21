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
