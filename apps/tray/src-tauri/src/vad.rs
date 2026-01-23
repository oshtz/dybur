//! Voice Activity Detection using Silero VAD
//!
//! Filters silence and noise from audio before STT processing.
//! Uses Silero VAD ONNX model for speech probability detection.

use ndarray::{Array1, ArrayD, IxDyn};
use ort::{session::Session, value::TensorRef};
use std::path::PathBuf;

/// VAD processing constants
const SAMPLE_RATE: i64 = 16000;
const CHUNK_SIZE: usize = 512; // 32ms at 16kHz

/// VAD configuration
#[derive(Debug, Clone)]
pub struct VadConfig {
    /// Speech probability threshold (0.0-1.0)
    pub threshold: f32,
    /// Minimum speech segment duration in ms
    pub min_speech_duration_ms: u32,
    /// Minimum silence duration to split segments in ms
    pub min_silence_duration_ms: u32,
    /// Padding before/after speech segments in ms
    pub speech_pad_ms: u32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: 0.5,
            min_speech_duration_ms: 250,
            min_silence_duration_ms: 300,
            speech_pad_ms: 30,
        }
    }
}

/// VAD engine errors
#[derive(Debug, Clone)]
pub enum VadError {
    /// Model not found
    ModelNotFound(String),
    /// Failed to load model
    ModelLoadFailed(String),
    /// Inference failed
    InferenceFailed(String),
    /// Model not loaded
    NotLoaded,
}

impl std::fmt::Display for VadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VadError::ModelNotFound(path) => write!(f, "VAD model not found: {}", path),
            VadError::ModelLoadFailed(msg) => write!(f, "Failed to load VAD model: {}", msg),
            VadError::InferenceFailed(msg) => write!(f, "VAD inference failed: {}", msg),
            VadError::NotLoaded => write!(f, "VAD model not loaded"),
        }
    }
}

/// Speech segment with sample positions
#[derive(Debug, Clone)]
pub struct SpeechSegment {
    pub start_sample: usize,
    pub end_sample: usize,
}

/// VAD engine state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadState {
    Unloaded,
    Ready,
    Error,
}

/// Voice Activity Detection engine using Silero VAD
pub struct VadEngine {
    session: Option<Session>,
    state: VadState,
    config: VadConfig,
    /// LSTM hidden state (h) - shape [2, 1, 64]
    h_state: ArrayD<f32>,
    /// LSTM cell state (c) - shape [2, 1, 64]
    c_state: ArrayD<f32>,
    last_error: Option<VadError>,
}

impl VadEngine {
    /// Create a new VAD engine (unloaded)
    pub fn new() -> Self {
        Self {
            session: None,
            state: VadState::Unloaded,
            config: VadConfig::default(),
            h_state: ArrayD::zeros(IxDyn(&[2, 1, 64])),
            c_state: ArrayD::zeros(IxDyn(&[2, 1, 64])),
            last_error: None,
        }
    }

    /// Get current state
    pub fn state(&self) -> VadState {
        self.state
    }

    /// Check if model is loaded and ready
    pub fn is_ready(&self) -> bool {
        self.state == VadState::Ready
    }

    /// Get last error
    pub fn last_error(&self) -> Option<&VadError> {
        self.last_error.as_ref()
    }

    /// Set VAD configuration
    pub fn set_config(&mut self, config: VadConfig) {
        self.config = config;
    }

    /// Load VAD model from path
    pub fn load(&mut self, model_path: PathBuf) -> Result<(), VadError> {
        self.last_error = None;

        if !model_path.exists() {
            let err = VadError::ModelNotFound(model_path.display().to_string());
            self.state = VadState::Error;
            self.last_error = Some(err.clone());
            return Err(err);
        }

        crate::log_info!("vad", "Loading VAD model from {:?}", model_path);

        // Load ONNX session
        let session = Session::builder()
            .and_then(|builder| builder.with_intra_threads(1))
            .and_then(|builder| builder.commit_from_file(&model_path))
            .map_err(|e| {
                let err = VadError::ModelLoadFailed(e.to_string());
                self.state = VadState::Error;
                self.last_error = Some(err.clone());
                err
            })?;

        self.session = Some(session);
        self.state = VadState::Ready;
        self.reset_state();

        crate::log_info!("vad", "VAD model loaded successfully");
        Ok(())
    }

    /// Unload the model
    pub fn unload(&mut self) {
        self.session = None;
        self.state = VadState::Unloaded;
        self.reset_state();
    }

    /// Reset LSTM states (call between recordings)
    pub fn reset_state(&mut self) {
        self.h_state = ArrayD::zeros(IxDyn(&[2, 1, 64]));
        self.c_state = ArrayD::zeros(IxDyn(&[2, 1, 64]));
    }

    /// Process a single audio chunk and return speech probability
    ///
    /// Chunk must be exactly 512 samples (32ms at 16kHz)
    fn process_chunk(&mut self, chunk: &[f32]) -> Result<f32, VadError> {
        let session = self.session.as_mut().ok_or(VadError::NotLoaded)?;

        if chunk.len() != CHUNK_SIZE {
            return Err(VadError::InferenceFailed(format!(
                "Chunk must be {} samples, got {}",
                CHUNK_SIZE,
                chunk.len()
            )));
        }

        // Prepare input tensor [batch=1, chunk_size=512]
        let input_array = Array1::from_vec(chunk.to_vec())
            .into_shape_with_order((1, CHUNK_SIZE))
            .map_err(|e| VadError::InferenceFailed(e.to_string()))?;

        // Sample rate tensor [1]
        let sr_array = Array1::from_vec(vec![SAMPLE_RATE]);

        // Create tensor refs from views
        let input_tensor = TensorRef::from_array_view(input_array.view())
            .map_err(|e| VadError::InferenceFailed(format!("Failed to create input tensor: {}", e)))?;

        let sr_tensor = TensorRef::from_array_view(sr_array.view())
            .map_err(|e| VadError::InferenceFailed(format!("Failed to create sr tensor: {}", e)))?;

        let h_tensor = TensorRef::from_array_view(self.h_state.view())
            .map_err(|e| VadError::InferenceFailed(format!("Failed to create h tensor: {}", e)))?;

        let c_tensor = TensorRef::from_array_view(self.c_state.view())
            .map_err(|e| VadError::InferenceFailed(format!("Failed to create c tensor: {}", e)))?;

        // Run inference
        let outputs = session
            .run(ort::inputs![
                "input" => input_tensor,
                "sr" => sr_tensor,
                "h" => h_tensor,
                "c" => c_tensor,
            ])
            .map_err(|e| VadError::InferenceFailed(format!("Inference failed: {}", e)))?;

        // Extract output probability
        let output = outputs.get("output")
            .ok_or_else(|| VadError::InferenceFailed("Missing 'output' in model outputs".to_string()))?;

        let (_, output_data) = output
            .try_extract_tensor::<f32>()
            .map_err(|e| VadError::InferenceFailed(format!("Failed to extract output: {}", e)))?;

        let prob = output_data[0];

        // Extract and update LSTM states
        let hn = outputs.get("hn")
            .ok_or_else(|| VadError::InferenceFailed("Missing 'hn' in model outputs".to_string()))?;
        let cn = outputs.get("cn")
            .ok_or_else(|| VadError::InferenceFailed("Missing 'cn' in model outputs".to_string()))?;

        let (hn_shape, hn_data) = hn
            .try_extract_tensor::<f32>()
            .map_err(|e| VadError::InferenceFailed(format!("Failed to extract hn: {}", e)))?;

        let (cn_shape, cn_data) = cn
            .try_extract_tensor::<f32>()
            .map_err(|e| VadError::InferenceFailed(format!("Failed to extract cn: {}", e)))?;

        // Update states
        self.h_state = ArrayD::from_shape_vec(hn_shape.to_ixdyn(), hn_data.to_vec())
            .map_err(|e| VadError::InferenceFailed(format!("Failed to reshape hn: {}", e)))?;

        self.c_state = ArrayD::from_shape_vec(cn_shape.to_ixdyn(), cn_data.to_vec())
            .map_err(|e| VadError::InferenceFailed(format!("Failed to reshape cn: {}", e)))?;

        Ok(prob)
    }

    /// Detect speech segments in audio buffer
    ///
    /// Returns list of (start_sample, end_sample) tuples for speech regions
    pub fn detect_speech_segments(&mut self, audio: &[f32]) -> Result<Vec<SpeechSegment>, VadError> {
        if !self.is_ready() {
            return Err(VadError::NotLoaded);
        }

        // Reset state for fresh detection
        self.reset_state();

        let threshold = self.config.threshold;
        let min_speech_samples = self.config.min_speech_duration_ms as usize * 16; // samples per ms at 16kHz
        let min_silence_samples = self.config.min_silence_duration_ms as usize * 16;
        let pad_samples = self.config.speech_pad_ms as usize * 16;

        let mut segments: Vec<SpeechSegment> = Vec::new();
        let mut in_speech = false;
        let mut speech_start: usize = 0;
        let mut silence_start: usize = 0;

        // Process audio in chunks
        let num_chunks = audio.len() / CHUNK_SIZE;

        for i in 0..num_chunks {
            let start = i * CHUNK_SIZE;
            let chunk = &audio[start..start + CHUNK_SIZE];

            let prob = self.process_chunk(chunk)?;
            let is_speech = prob >= threshold;

            if is_speech {
                if !in_speech {
                    // Speech started
                    speech_start = start;
                    in_speech = true;
                }
                // Reset silence counter when speech detected
                silence_start = start + CHUNK_SIZE;
            } else if in_speech {
                // In speech but this chunk is silence
                let silence_duration = (start + CHUNK_SIZE) - silence_start;

                if silence_duration >= min_silence_samples {
                    // Enough silence to end segment
                    let speech_duration = silence_start - speech_start;

                    if speech_duration >= min_speech_samples {
                        // Add padding
                        let padded_start = speech_start.saturating_sub(pad_samples);
                        let padded_end = (silence_start + pad_samples).min(audio.len());

                        segments.push(SpeechSegment {
                            start_sample: padded_start,
                            end_sample: padded_end,
                        });
                    }

                    in_speech = false;
                }
            }
        }

        // Handle final segment if still in speech
        if in_speech {
            let speech_end = num_chunks * CHUNK_SIZE;
            let speech_duration = speech_end - speech_start;

            if speech_duration >= min_speech_samples {
                let padded_start = speech_start.saturating_sub(pad_samples);
                let padded_end = audio.len();

                segments.push(SpeechSegment {
                    start_sample: padded_start,
                    end_sample: padded_end,
                });
            }
        }

        Ok(segments)
    }

    /// Filter audio to keep only speech portions
    ///
    /// Returns a new audio buffer containing only speech segments
    pub fn filter_speech(&mut self, audio: &[f32]) -> Result<Vec<f32>, VadError> {
        let segments = self.detect_speech_segments(audio)?;

        if segments.is_empty() {
            crate::log_info!("vad", "No speech detected in audio");
            return Ok(Vec::new());
        }

        // Concatenate speech segments
        let mut filtered: Vec<f32> = Vec::new();

        for segment in &segments {
            filtered.extend_from_slice(&audio[segment.start_sample..segment.end_sample]);
        }

        let original_duration = audio.len() as f32 / 16000.0;
        let filtered_duration = filtered.len() as f32 / 16000.0;

        crate::log_info!(
            "vad",
            "Filtered {:.2}s -> {:.2}s ({} segments, {:.0}% speech)",
            original_duration,
            filtered_duration,
            segments.len(),
            (filtered_duration / original_duration) * 100.0
        );

        Ok(filtered)
    }
}

impl Default for VadEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Get the VAD model directory path
pub fn get_vad_model_dir() -> PathBuf {
    crate::config::get_models_dir().join("silero-vad")
}

/// Get the VAD model file path
pub fn get_vad_model_path() -> PathBuf {
    get_vad_model_dir().join("silero_vad.onnx")
}

/// Check if VAD model is installed
pub fn is_vad_model_installed() -> bool {
    get_vad_model_path().exists()
}
