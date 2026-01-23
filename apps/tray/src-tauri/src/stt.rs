//! Speech-to-Text engine using ONNX Runtime
//!
//! Implements speech recognition using the Parakeet TDT model.
//! Pipeline: Audio -> Mel Spectrogram -> Encoder -> Decoder -> Text

use crate::execution_providers::{build_session, GpuPreference, SessionConfig};
use ndarray::{Array1, Array2, ArrayD, Axis, IxDyn};
use ort::{
    session::{Session, SessionOutputs},
    tensor::TensorElementType,
    value::{DynValue, Tensor, TensorRef, ValueType},
};
use rustfft::{num_complex::Complex, FftPlanner};
use std::path::PathBuf;
use std::time::Instant;

// Audio processing constants
const SAMPLE_RATE: u32 = 16000;
const N_FFT: usize = 512;
const HOP_LENGTH: usize = 160; // 10ms at 16kHz
const WIN_LENGTH: usize = 400; // 25ms at 16kHz
const N_MELS: usize = 128;
const MEL_FMIN: f32 = 0.0;
const MEL_FMAX: f32 = 8000.0;

// Special token IDs for Parakeet TDT
const BLANK_ID: i64 = 8192; // <blk>
const START_TOKEN_ID: i64 = 4; // <|startoftranscript|>

/// Speech recognition configuration
#[derive(Debug, Clone)]
pub struct SttConfig {
    /// Path to the encoder ONNX model
    pub encoder_path: PathBuf,
    /// Path to the decoder ONNX model
    pub decoder_path: PathBuf,
    /// Path to vocabulary file
    pub vocab_path: PathBuf,
    /// Inference timeout in milliseconds
    pub timeout_ms: u64,
}

/// Speech recognition result
#[derive(Debug, Clone)]
pub struct SttResult {
    /// Transcribed text
    pub text: String,
    /// Inference duration in milliseconds
    pub inference_time_ms: u64,
    /// Audio duration in seconds
    pub audio_duration_s: f32,
    /// Confidence score (0.0-1.0) if available
    pub confidence: Option<f32>,
}

/// STT Engine errors
#[derive(Debug, Clone)]
pub enum SttError {
    /// Model files not found
    ModelNotFound(String),
    /// Failed to load model
    ModelLoadFailed(String),
    /// Inference failed
    InferenceFailed(String),
    /// Inference timed out
    Timeout(u64),
    /// Invalid audio input
    InvalidInput(String),
    /// Model not loaded
    NotLoaded,
}

impl std::fmt::Display for SttError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SttError::ModelNotFound(path) => write!(f, "Model not found: {}", path),
            SttError::ModelLoadFailed(msg) => write!(f, "Failed to load model: {}", msg),
            SttError::InferenceFailed(msg) => write!(f, "Inference failed: {}", msg),
            SttError::Timeout(ms) => write!(f, "Inference timed out after {}ms", ms),
            SttError::InvalidInput(msg) => write!(f, "Invalid audio input: {}", msg),
            SttError::NotLoaded => write!(f, "Model not loaded"),
        }
    }
}

impl From<SttError> for String {
    fn from(err: SttError) -> Self {
        err.to_string()
    }
}

/// Speech-to-Text engine state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SttState {
    /// Not initialized
    Unloaded,
    /// Loading model
    Loading,
    /// Ready for inference
    Ready,
    /// Processing audio
    Processing,
    /// Error state
    Error,
}

/// Speech-to-Text engine
pub struct SttEngine {
    config: Option<SttConfig>,
    state: SttState,
    vocab: Option<Vec<String>>,
    preprocessor_session: Option<Session>,
    encoder_session: Option<Session>,
    decoder_session: Option<Session>,
    mel_filterbank: Option<Array2<f32>>,
    last_error: Option<SttError>,
    /// Whether GPU acceleration is active
    gpu_enabled: bool,
    /// Name of the execution provider being used
    execution_provider: String,
}

impl SttEngine {
    /// Create a new STT engine (unloaded)
    pub fn new() -> Self {
        Self {
            config: None,
            state: SttState::Unloaded,
            vocab: None,
            preprocessor_session: None,
            encoder_session: None,
            decoder_session: None,
            mel_filterbank: None,
            last_error: None,
            gpu_enabled: false,
            execution_provider: "None".to_string(),
        }
    }

    /// Get current state
    pub fn state(&self) -> SttState {
        self.state
    }

    /// Check if model is loaded and ready
    pub fn is_ready(&self) -> bool {
        self.state == SttState::Ready
    }

    /// Get last error
    pub fn last_error(&self) -> Option<&SttError> {
        self.last_error.as_ref()
    }

    /// Check if GPU acceleration is enabled
    pub fn is_gpu_enabled(&self) -> bool {
        self.gpu_enabled
    }

    /// Get the name of the execution provider being used
    pub fn execution_provider(&self) -> &str {
        &self.execution_provider
    }

    /// Load model from config
    pub fn load(&mut self, config: SttConfig, gpu_preference: GpuPreference) -> Result<(), SttError> {
        self.state = SttState::Loading;
        self.last_error = None;

        crate::log_info!("model", "Loading STT model...");

        // Validate paths exist
        if !config.encoder_path.exists() {
            let err = SttError::ModelNotFound(config.encoder_path.display().to_string());
            self.state = SttState::Error;
            self.last_error = Some(err.clone());
            crate::log_error!(
                "model",
                "Encoder model not found: {:?}",
                config.encoder_path
            );
            return Err(err);
        }

        if !config.decoder_path.exists() {
            let err = SttError::ModelNotFound(config.decoder_path.display().to_string());
            self.state = SttState::Error;
            self.last_error = Some(err.clone());
            crate::log_error!(
                "model",
                "Decoder model not found: {:?}",
                config.decoder_path
            );
            return Err(err);
        }

        if !config.vocab_path.exists() {
            let err = SttError::ModelNotFound(config.vocab_path.display().to_string());
            self.state = SttState::Error;
            self.last_error = Some(err.clone());
            crate::log_error!(
                "model",
                "Vocabulary file not found: {:?}",
                config.vocab_path
            );
            return Err(err);
        }

        // Load vocabulary
        let vocab = match load_vocabulary(&config.vocab_path) {
            Ok(v) => v,
            Err(e) => {
                let err = SttError::ModelLoadFailed(format!("Failed to load vocabulary: {}", e));
                self.state = SttState::Error;
                self.last_error = Some(err.clone());
                crate::log_error!("model", "{}", err);
                return Err(err);
            }
        };

        crate::log_info!("model", "Loaded vocabulary with {} tokens", vocab.len());

        // Try to load ONNX preprocessor (optional - fall back to manual computation if not found)
        // The preprocessor is named nemo128.onnx in the istupakov/parakeet-tdt-0.6b-v3-onnx repo
        let preprocessor_path = config.encoder_path.parent()
            .map(|p| p.join("nemo128.onnx"));

        let session_config = SessionConfig::for_stt().with_gpu_preference(gpu_preference);

        let preprocessor_session = if let Some(ref prep_path) = preprocessor_path {
            if prep_path.exists() {
                crate::log_info!("model", "Loading ONNX preprocessor...");
                match build_session(prep_path, &session_config) {
                    Ok((session, ep_result)) => {
                        crate::log_info!(
                            "model",
                            "ONNX preprocessor loaded (provider: {}, GPU: {})",
                            ep_result.provider_name,
                            ep_result.is_gpu
                        );
                        Some(session)
                    }
                    Err(e) => {
                        crate::log_warn!("model", "Failed to load ONNX preprocessor, using manual computation: {}", e);
                        None
                    }
                }
            } else {
                crate::log_info!("model", "No ONNX preprocessor found, using manual mel computation");
                None
            }
        } else {
            None
        };

        // Initialize ONNX Runtime sessions with GPU acceleration
        crate::log_info!("model", "Loading encoder model...");
        let (encoder_session, encoder_ep) = match build_session(&config.encoder_path, &session_config) {
            Ok(result) => result,
            Err(e) => {
                let err = SttError::ModelLoadFailed(format!("Failed to load encoder: {}", e));
                self.state = SttState::Error;
                self.last_error = Some(err.clone());
                crate::log_error!("model", "{}", err);
                return Err(err);
            }
        };
        crate::log_info!(
            "model",
            "Encoder loaded (provider: {}, GPU: {})",
            encoder_ep.provider_name,
            encoder_ep.is_gpu
        );

        crate::log_info!("model", "Loading decoder model...");
        let (decoder_session, decoder_ep) = match build_session(&config.decoder_path, &session_config) {
            Ok(result) => result,
            Err(e) => {
                let err = SttError::ModelLoadFailed(format!("Failed to load decoder: {}", e));
                self.state = SttState::Error;
                self.last_error = Some(err.clone());
                crate::log_error!("model", "{}", err);
                return Err(err);
            }
        };
        crate::log_info!(
            "model",
            "Decoder loaded (provider: {}, GPU: {})",
            decoder_ep.provider_name,
            decoder_ep.is_gpu
        );

        // Pre-compute mel filterbank (fallback if no ONNX preprocessor)
        let mel_filterbank = create_mel_filterbank(N_FFT, N_MELS, SAMPLE_RATE, MEL_FMIN, MEL_FMAX);

        log_session_io("encoder", &encoder_session);
        log_session_io("decoder", &decoder_session);

        self.vocab = Some(vocab);
        self.preprocessor_session = preprocessor_session;
        self.encoder_session = Some(encoder_session);
        self.decoder_session = Some(decoder_session);
        self.mel_filterbank = Some(mel_filterbank);
        self.config = Some(config);
        self.state = SttState::Ready;
        self.gpu_enabled = encoder_ep.is_gpu;
        self.execution_provider = encoder_ep.provider_name;

        crate::log_info!(
            "model",
            "STT model loaded successfully (GPU: {}, provider: {})",
            self.gpu_enabled,
            self.execution_provider
        );

        Ok(())
    }

    /// Unload model and free resources
    pub fn unload(&mut self) {
        self.config = None;
        self.vocab = None;
        self.preprocessor_session = None;
        self.encoder_session = None;
        self.decoder_session = None;
        self.mel_filterbank = None;
        self.state = SttState::Unloaded;
        self.last_error = None;
        self.gpu_enabled = false;
        self.execution_provider = "None".to_string();

        crate::log_info!("model", "STT model unloaded");
    }

    /// Transcribe audio data
    ///
    /// # Arguments
    /// * `audio` - Float32 PCM audio at 16kHz mono
    ///
    /// # Returns
    /// Transcription result or error
    pub fn transcribe(&mut self, audio: &[f32]) -> Result<SttResult, SttError> {
        if self.state != SttState::Ready {
            return Err(SttError::NotLoaded);
        }

        // Validate input
        if audio.is_empty() {
            return Err(SttError::InvalidInput("Empty audio buffer".to_string()));
        }

        let audio_duration_s = audio.len() as f32 / SAMPLE_RATE as f32;

        // Minimum audio length (100ms)
        if audio_duration_s < 0.1 {
            return Err(SttError::InvalidInput(format!(
                "Audio too short: {:.2}s (minimum 0.1s)",
                audio_duration_s
            )));
        }

        // Maximum audio length (24 minutes as per model spec)
        if audio_duration_s > 24.0 * 60.0 {
            return Err(SttError::InvalidInput(format!(
                "Audio too long: {:.2}s (maximum 24 minutes)",
                audio_duration_s
            )));
        }

        // Log audio statistics for debugging
        let audio_min = audio.iter().cloned().fold(f32::INFINITY, f32::min);
        let audio_max = audio.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let audio_rms = (audio.iter().map(|x| x * x).sum::<f32>() / audio.len() as f32).sqrt();

        crate::log_info!(
            "model",
            "Starting transcription of {:.2}s audio ({} samples)",
            audio_duration_s,
            audio.len()
        );
        crate::log_debug!(
            "model",
            "Audio stats: min={:.4}, max={:.4}, rms={:.4}",
            audio_min,
            audio_max,
            audio_rms
        );

        self.state = SttState::Processing;
        let start = Instant::now();

        let result = self.run_inference(audio);

        self.state = SttState::Ready;

        match result {
            Ok(text) => {
                let inference_time_ms = start.elapsed().as_millis() as u64;
                crate::log_info!(
                    "model",
                    "Transcription completed in {}ms ({} chars)",
                    inference_time_ms,
                    text.len()
                );

                Ok(SttResult {
                    text,
                    inference_time_ms,
                    audio_duration_s,
                    confidence: None,
                })
            }
            Err(e) => {
                self.last_error = Some(e.clone());
                crate::log_error!("model", "Transcription failed: {}", e);
                Err(e)
            }
        }
    }

    /// Run the ONNX inference pipeline
    fn run_inference(&mut self, audio: &[f32]) -> Result<String, SttError> {
        // Extract vocab info first to avoid borrow conflicts
        let vocab = self.vocab.as_ref().ok_or(SttError::NotLoaded)?.clone();
        let vocab_len = vocab.len();
        
        // Get mel filterbank if needed (before mutable borrows)
        let mel_filterbank = self.mel_filterbank.clone();

        // Step 1: Compute mel spectrogram using ONNX preprocessor if available
        let (mel_input, audio_length) = if let Some(preprocessor) = &mut self.preprocessor_session {
            crate::log_debug!("model", "Using ONNX preprocessor...");
            
            // Prepare input: waveforms [batch=1, N], waveforms_lens [batch=1]
            let waveforms = Array2::from_shape_vec((1, audio.len()), audio.to_vec())
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create waveforms array: {}", e)))?;
            let waveforms_lens = Array1::from_vec(vec![audio.len() as i64]);
            
            // Run preprocessor - create TensorRefs from ndarrays
            let waveforms_tensor = TensorRef::from_array_view(waveforms.view())
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create waveforms tensor: {}", e)))?;
            let waveforms_lens_tensor = TensorRef::from_array_view(waveforms_lens.view())
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create waveforms_lens tensor: {}", e)))?;
            
            let prep_outputs = preprocessor
                .run(ort::inputs![waveforms_tensor, waveforms_lens_tensor])
                .map_err(|e| SttError::InferenceFailed(format!("Preprocessor failed: {}", e)))?;
            
            // Extract features and lengths
            let features = prep_outputs
                .iter()
                .find(|(name, _)| *name == "features")
                .map(|(_, v)| v)
                .ok_or_else(|| SttError::InferenceFailed("Missing 'features' output".to_string()))?;
            
            let features_lens = prep_outputs
                .iter()
                .find(|(name, _)| *name == "features_lens")
                .map(|(_, v)| v)
                .ok_or_else(|| SttError::InferenceFailed("Missing 'features_lens' output".to_string()))?;
            
            let (features_shape, features_data) = features
                .try_extract_tensor::<f32>()
                .map_err(|e| SttError::InferenceFailed(format!("Failed to extract features: {}", e)))?;
            let (_, features_lens_data) = features_lens
                .try_extract_tensor::<i64>()
                .map_err(|e| SttError::InferenceFailed(format!("Failed to extract features_lens: {}", e)))?;
            
            let mel_array = ArrayD::from_shape_vec(features_shape.to_ixdyn(), features_data.to_vec())
                .map_err(|e| SttError::InferenceFailed(format!("Failed to reshape features: {}", e)))?;
            let n_frames = features_lens_data[0] as usize;
            
            crate::log_debug!("model", "Preprocessor output shape: {:?}, frames: {}", mel_array.shape(), n_frames);
            
            (mel_array, Array1::from_vec(vec![n_frames as i64]))
        } else {
            // Fallback to manual mel computation
            let mel_filterbank = mel_filterbank.as_ref().ok_or(SttError::NotLoaded)?;
            
            crate::log_debug!("model", "Computing mel spectrogram (manual)...");
            let mel_spec = compute_mel_spectrogram(audio, mel_filterbank);
            let n_frames = mel_spec.shape()[1];
            crate::log_debug!("model", "Mel spectrogram shape: [{}, {}]", N_MELS, n_frames);

            // Shape: [batch=1, n_mels=128, time_steps]
            let mel_input = mel_spec.insert_axis(Axis(0)).into_dyn();
            (mel_input, Array1::from_vec(vec![n_frames as i64]))
        };

        // Step 3: Run encoder
        crate::log_debug!("model", "Running encoder...");
        let mel_tensor = TensorRef::from_array_view(mel_input.view())
            .map_err(|e| SttError::InferenceFailed(format!("Failed to create mel tensor: {}", e)))?;
        let audio_length_tensor = TensorRef::from_array_view(audio_length.view())
            .map_err(|e| SttError::InferenceFailed(format!("Failed to create audio_length tensor: {}", e)))?;
        
        let encoder = self.encoder_session.as_mut().ok_or(SttError::NotLoaded)?;
        let encoder_outputs = encoder
            .run(ort::inputs![mel_tensor, audio_length_tensor])
            .map_err(|e| SttError::InferenceFailed(format!("Encoder inference failed: {}", e)))?;

        // Step 4: Run decoder with greedy search
        crate::log_debug!("model", "Running decoder (greedy search)...");
        let blank_id = find_token_id(&vocab, "<blk>").unwrap_or(BLANK_ID);
        let mut start_token_id = find_token_id(&vocab, "<|startoftranscript|>")
            .or_else(|| find_token_id(&vocab, "<s>"))
            .unwrap_or(START_TOKEN_ID);
        if start_token_id < 0 || start_token_id as usize >= vocab_len {
            start_token_id = blank_id.max(0);
        }
        let decoder = self.decoder_session.as_mut().ok_or(SttError::NotLoaded)?;
        let tokens = greedy_decode(decoder, &encoder_outputs, blank_id, start_token_id, vocab_len)?;

        // Step 5: Convert tokens to text
        let text = decode_tokens(&tokens, &vocab);

        Ok(text)
    }
}

/// Greedy decoding for TDT model
fn greedy_decode(
    decoder: &mut Session,
    encoder_outputs: &SessionOutputs,
    blank_id: i64,
    _start_token_id: i64,
    vocab_len: usize,
) -> Result<Vec<i64>, SttError> {
        // Get the first encoder output (encoded features)
        let (_, encoded) = encoder_outputs
            .iter()
            .next()
            .ok_or_else(|| SttError::InferenceFailed("No encoder output found".to_string()))?;

        // Extract the encoded tensor to pass to decoder
        let (encoded_shape_obj, encoded_data) = encoded
            .try_extract_tensor::<f32>()
            .map_err(|e| SttError::InferenceFailed(format!("Failed to extract encoder output: {}", e)))?;

        let encoded_shape: Vec<usize> = encoded_shape_obj.iter().map(|&d| d as usize).collect();
        let encoded_view = ArrayD::from_shape_vec(IxDyn(&encoded_shape), encoded_data.to_vec())
            .map_err(|e| SttError::InferenceFailed(format!("Failed to reshape encoder output: {}", e)))?;
        crate::log_debug!("model", "Encoder output shape: {:?}", encoded_shape);

        // Encoder output is [batch, features, time] = [1, 1024, T]
        // Need to transpose to [batch, time, features] = [1, T, 1024]
        let encoder_time = encoded_shape[2];
        crate::log_debug!("model", "Encoder time steps: {}", encoder_time);
        
        // Transpose encoder output: [1, 1024, T] -> [1, T, 1024]
        let encoded_transposed = encoded_view.permuted_axes(IxDyn(&[0, 2, 1]));
        crate::log_debug!("model", "Transposed encoder shape: {:?}", encoded_transposed.shape());

        // Run TDT decoding (per-timestep)
        let tokens = tdt_decode_per_step(
            decoder, 
            &encoded_transposed, 
            encoder_time, 
            blank_id, 
            vocab_len
        )?;
        
        crate::log_debug!("model", "Decoded {} tokens", tokens.len());

        Ok(tokens)
}

/// TDT decoding - process one encoder timestep at a time
/// Based on NeMo's TDT greedy decoding algorithm
fn tdt_decode_per_step(
    decoder: &mut Session,
    encoded: &ArrayD<f32>,
    encoder_time: usize,
    blank_id: i64,
    vocab_len: usize,
) -> Result<Vec<i64>, SttError> {
        use ndarray::Array3;
        
        let mut tokens = Vec::new();
        let mut t = 0usize;
        
        // Initialize states - shape [2, 1, 640] based on model
        let mut state_1 = ArrayD::<f32>::zeros(IxDyn(&[2, 1, 640]));
        let mut state_2 = ArrayD::<f32>::zeros(IxDyn(&[2, 1, 640]));
        
        // Start with blank token as previous label (SOS equivalent)
        let mut prev_token = blank_id as i32;
        
        // Limit symbols per step to prevent runaway emission
        // TDT models typically emit 1-3 tokens per acoustic frame for normal speech
        let max_symbols_per_step = 5;
        let max_total_tokens = 500;

        // Number of duration outputs (TDT typically has 5: durations 0,1,2,3,4+)
        let num_durations = 5usize;

        // Track recent tokens for loop detection
        let mut recent_tokens: Vec<i64> = Vec::new();
        let loop_detect_window = 10; // Check last N tokens for repetition
        
        crate::log_debug!("model", "TDT per-step decoding: {} encoder steps, vocab_len={}, blank_id={}", 
            encoder_time, vocab_len, blank_id);

        while t < encoder_time && tokens.len() < max_total_tokens {
            let mut symbols_this_step = 0;
            
            loop {
                // Get single encoder frame: encoded[0, t, :] -> shape [1, features]
                // Then reshape to [1, features, 1] for decoder (adding time dim)
                let encoder_frame: Vec<f32> = (0..encoded.shape()[2])
                    .map(|f| encoded[[0, t, f]])
                    .collect();
                
                // Create encoder_outputs with shape [1, features, 1] 
                let encoder_out = Array3::<f32>::from_shape_vec(
                    (1, encoder_frame.len(), 1),
                    encoder_frame
                ).map_err(|e| SttError::InferenceFailed(format!("Failed to reshape encoder frame: {}", e)))?;
                
                // Build decoder inputs
                let targets = ndarray::Array2::<i32>::from_elem((1, 1), prev_token);
                let target_length = ndarray::Array1::<i32>::from_vec(vec![1]);
                
                // Create tensor refs
                let encoder_out_ref = TensorRef::from_array_view(encoder_out.view())
                    .map_err(|e| SttError::InferenceFailed(format!("encoder_outputs: {}", e)))?;
                let targets_ref = TensorRef::from_array_view(targets.view())
                    .map_err(|e| SttError::InferenceFailed(format!("targets: {}", e)))?;
                let target_length_ref = TensorRef::from_array_view(target_length.view())
                    .map_err(|e| SttError::InferenceFailed(format!("target_length: {}", e)))?;
                let state_1_ref = TensorRef::from_array_view(state_1.view())
                    .map_err(|e| SttError::InferenceFailed(format!("input_states_1: {}", e)))?;
                let state_2_ref = TensorRef::from_array_view(state_2.view())
                    .map_err(|e| SttError::InferenceFailed(format!("input_states_2: {}", e)))?;
                
                // Run decoder
                let inputs = vec![
                    ("encoder_outputs".to_string(), encoder_out_ref.into_dyn()),
                    ("targets".to_string(), targets_ref.into_dyn()),
                    ("target_length".to_string(), target_length_ref.into_dyn()),
                    ("input_states_1".to_string(), state_1_ref.into_dyn()),
                    ("input_states_2".to_string(), state_2_ref.into_dyn()),
                ];

                let outputs = decoder
                    .run(inputs)
                    .map_err(|e| SttError::InferenceFailed(format!("Decoder failed: {}", e)))?;

                // Extract outputs
                let mut logits_opt: Option<ArrayD<f32>> = None;
                let mut new_state_1: Option<ArrayD<f32>> = None;
                let mut new_state_2: Option<ArrayD<f32>> = None;

                for (name, value) in outputs.iter() {
                    if name == "outputs" {
                        let (shape, data) = value.try_extract_tensor::<f32>()
                            .map_err(|e| SttError::InferenceFailed(format!("outputs: {}", e)))?;
                        logits_opt = Some(ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
                            .map_err(|e| SttError::InferenceFailed(format!("reshape outputs: {}", e)))?);
                    } else if name == "output_states_1" {
                        let (shape, data) = value.try_extract_tensor::<f32>()
                            .map_err(|e| SttError::InferenceFailed(format!("state1: {}", e)))?;
                        new_state_1 = Some(ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
                            .map_err(|e| SttError::InferenceFailed(format!("reshape state1: {}", e)))?);
                    } else if name == "output_states_2" {
                        let (shape, data) = value.try_extract_tensor::<f32>()
                            .map_err(|e| SttError::InferenceFailed(format!("state2: {}", e)))?;
                        new_state_2 = Some(ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
                            .map_err(|e| SttError::InferenceFailed(format!("reshape state2: {}", e)))?);
                    }
                }

                let logits = logits_opt.ok_or_else(|| SttError::InferenceFailed("Missing outputs".to_string()))?;
                let ns1 = new_state_1.ok_or_else(|| SttError::InferenceFailed("Missing state1".to_string()))?;
                let ns2 = new_state_2.ok_or_else(|| SttError::InferenceFailed("Missing state2".to_string()))?;

                // Squeeze the output - output shape is typically [1, 1, 1, vocab+durations]
                let flat_logits: Vec<f32> = logits.iter().cloned().collect();
                let total_size = flat_logits.len();
                
                if t < 3 {
                    crate::log_debug!("model", "t={}: logits size={}, shape={:?}", t, total_size, logits.shape());
                }

                // TDT output layout: [vocab_logits..., duration_logits...]
                // vocab_logits: size = vocab_len (includes blank at index blank_id)
                // duration_logits: size = num_durations (typically 5)
                let actual_vocab_len = total_size.saturating_sub(num_durations);
                let vocab_logits = &flat_logits[..actual_vocab_len.min(total_size)];
                let duration_logits = if total_size > actual_vocab_len { 
                    &flat_logits[actual_vocab_len..] 
                } else { 
                    &[] as &[f32] 
                };

                // Find best vocab token (argmax over vocab logits)
                let (best_vocab_idx, best_vocab_val) = vocab_logits.iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap_or((0, &f32::NEG_INFINITY));

                // Find best duration (argmax over duration logits)
                // Duration values: 0=stay, 1=+1, 2=+2, etc.
                let best_duration = if !duration_logits.is_empty() {
                    duration_logits.iter()
                        .enumerate()
                        .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|(idx, _)| idx)
                        .unwrap_or(1)
                } else {
                    1  // Default: advance by 1 if no duration output
                };

                if t < 5 || symbols_this_step == 0 {
                    // Debug: show top vocab tokens and duration logits
                    let mut top_vocab: Vec<(usize, f32)> = vocab_logits.iter().cloned().enumerate().collect();
                    top_vocab.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    top_vocab.truncate(5);
                    let top_str: Vec<String> = top_vocab.iter().map(|(i, v)| format!("{}:{:.2}", i, v)).collect();
                    let dur_str: String = duration_logits.iter().map(|v| format!("{:.2}", v)).collect::<Vec<_>>().join(",");
                    crate::log_debug!("model", "t={}, sym={}: best_vocab={}({:.2}), duration={}, top5=[{}], dur_logits=[{}]",
                        t, symbols_this_step, best_vocab_idx, best_vocab_val, best_duration, top_str.join(", "), dur_str);
                }

                // Check if blank token wins
                if best_vocab_idx as i64 == blank_id {
                    // Blank predicted - don't emit token, DON'T update state
                    // Always advance by 1 frame when blank is predicted
                    // (duration is only used for non-blank emissions)
                    t += 1;
                    break;  // Exit inner loop, move to next encoder frame
                }

                // Non-blank: emit the token AND update state
                // CRITICAL: State should only be updated when we emit a non-blank token
                // The prediction network state tracks the sequence of emitted tokens
                state_1 = ns1;
                state_2 = ns2;
                
                tokens.push(best_vocab_idx as i64);
                prev_token = best_vocab_idx as i32;

                // Track for loop detection
                recent_tokens.push(best_vocab_idx as i64);
                if recent_tokens.len() > loop_detect_window {
                    recent_tokens.remove(0);
                }

                if tokens.len() <= 15 {
                    crate::log_debug!("model", "Emitted token {} at t={}", best_vocab_idx, t);
                }

                symbols_this_step += 1;

                // Loop detection: check if we're repeating a pattern
                // Look for the shortest repeating pattern in recent tokens
                let mut loop_detected = false;
                if recent_tokens.len() >= 6 {
                    let half = recent_tokens.len() / 2;
                    for pattern_len in 2..=half {
                        let pattern = &recent_tokens[recent_tokens.len() - pattern_len..];
                        let prev_pattern = &recent_tokens[recent_tokens.len() - 2 * pattern_len..recent_tokens.len() - pattern_len];
                        if pattern == prev_pattern {
                            crate::log_debug!("model", "Loop detected at t={}: pattern {:?} repeating, breaking out", t, pattern);
                            loop_detected = true;
                            break;
                        }
                    }
                }

                if loop_detected {
                    // Force advance to break the loop
                    t += 1;
                    break;
                }

                // Check termination conditions for this timestep
                if symbols_this_step >= max_symbols_per_step {
                    // Hit max symbols per step - force advance
                    t += best_duration.max(1);
                    break;
                }

                // In TDT, duration > 0 with non-blank means we should advance
                // Duration 0 means "stay and potentially emit more tokens"
                if best_duration > 0 {
                    t += best_duration;
                    break;
                }
                // Duration == 0: stay at this frame and try to emit another token
            }
        }

    crate::log_debug!("model", "Decoded {} tokens total", tokens.len());
    Ok(tokens)
}

/// Extract token IDs from decoder output
fn extract_tokens_from_output(output: &ort::value::ValueRef<'_>) -> Result<Vec<i64>, SttError> {
        // Try to extract as different possible types
        
        // First, try to extract as i64 array (direct token IDs)
        if let Ok((shape_obj, data)) = output.try_extract_tensor::<i64>() {
            let tokens: Vec<i64> = data.iter().cloned().collect();
            // Filter out blank tokens
            let filtered: Vec<i64> = tokens
                .into_iter()
                .filter(|&t| t != BLANK_ID && t >= 0)
                .collect();
            return Ok(filtered);
        }

        // Try as f32 logits (need to argmax)
        if let Ok((shape_obj, data)) = output.try_extract_tensor::<f32>() {
            let shape: Vec<usize> = shape_obj.iter().map(|&d| d as usize).collect();
            let logits_view = ArrayD::from_shape_vec(IxDyn(&shape), data.to_vec())
                .map_err(|e| SttError::InferenceFailed(format!("Failed to reshape logits: {}", e)))?;
            
            if shape.len() >= 2 {
                // Shape is typically [batch, time, vocab] or [batch, time]
                let mut tokens = Vec::new();
                
                if shape.len() == 4 {
                    // [batch, time, target, vocab] or [batch, target, time, vocab]
                    let time_dim = if shape[1] == 1 && shape[2] > 1 {
                        2
                    } else if shape[2] == 1 && shape[1] > 1 {
                        1
                    } else if shape[1] >= shape[2] {
                        1
                    } else {
                        2
                    };
                    let target_dim = if time_dim == 1 { 2 } else { 1 };
                    let time_steps = shape[time_dim];
                    let vocab_size = shape[3];
                    let target_index = shape[target_dim].saturating_sub(1);

                    for t in 0..time_steps {
                        let mut max_idx = 0i64;
                        let mut max_val = f32::NEG_INFINITY;

                        for v in 0..vocab_size {
                            let val = if time_dim == 1 {
                                logits_view[[0, t, target_index, v]]
                            } else {
                                logits_view[[0, target_index, t, v]]
                            };
                            if val > max_val {
                                max_val = val;
                                max_idx = v as i64;
                            }
                        }

                        if max_idx != BLANK_ID {
                            tokens.push(max_idx);
                        }
                    }
                } else if shape.len() == 3 {
                    // [batch, time, vocab] - need argmax over vocab dimension
                    let time_steps = shape[1];
                    let vocab_size = shape[2];
                    
                    for t in 0..time_steps {
                        let mut max_idx = 0i64;
                        let mut max_val = f32::NEG_INFINITY;
                        
                        for v in 0..vocab_size {
                            let val = logits_view[[0, t, v]];
                            if val > max_val {
                                max_val = val;
                                max_idx = v as i64;
                            }
                        }
                        
                        if max_idx != BLANK_ID {
                            tokens.push(max_idx);
                        }
                    }
                } else if shape.len() == 2 {
                    // [batch, time] - already token indices as floats
                    let time_steps = shape[1];
                    for t in 0..time_steps {
                        let token = logits_view[[0, t]] as i64;
                        if token != BLANK_ID && token >= 0 {
                            tokens.push(token);
                        }
                    }
                }
                
                // Remove consecutive duplicates (CTC-style)
                let deduped = remove_consecutive_duplicates(&tokens);
                return Ok(deduped);
            }
        }

        // Try as i32 array
        if let Ok((_, data)) = output.try_extract_tensor::<i32>() {
            let tokens: Vec<i64> = data.iter().map(|&t| t as i64).collect();
            let filtered: Vec<i64> = tokens
                .into_iter()
                .filter(|&t| t != BLANK_ID && t >= 0)
                .collect();
            return Ok(filtered);
        }

    Err(SttError::InferenceFailed(
        "Could not extract tokens from decoder output".to_string(),
    ))
}

struct DecoderStep {
    output: ArrayD<f32>,
    state_1: ArrayD<f32>,
    state_2: ArrayD<f32>,
}

struct OutputLayout {
    time_dim: usize,
    target_dim: Option<usize>,
    target_index: usize,
    vocab_dim: Option<usize>,
    time_steps: usize,
}

fn run_decoder_step(
    decoder: &mut Session,
    encoded: &ndarray::ArrayViewD<'_, f32>,
    encoder_len: i64,
    target_token: i64,
    state_1: Option<&ArrayD<f32>>,
    state_2: Option<&ArrayD<f32>>,
) -> Result<DecoderStep, SttError> {
    let decoder_inputs =
        build_decoder_inputs(decoder, encoded, encoder_len, target_token, state_1, state_2)?;

    let decoder_outputs = decoder
        .run(decoder_inputs)
        .map_err(|e| SttError::InferenceFailed(format!("Decoder inference failed: {}", e)))?;

    let output = find_decoder_output(&decoder_outputs, "outputs")?;
    let (shape, data) = output
        .try_extract_tensor::<f32>()
        .map_err(|e| SttError::InferenceFailed(format!("Failed to extract decoder output: {}", e)))?;
    let output_array = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
        .map_err(|e| SttError::InferenceFailed(format!("Failed to reshape decoder output: {}", e)))?;
    let state_1 = extract_decoder_state(&decoder_outputs, "output_states_1")?;
    let state_2 = extract_decoder_state(&decoder_outputs, "output_states_2")?;

    Ok(DecoderStep {
        output: output_array,
        state_1,
        state_2,
    })
}

fn find_decoder_output<'a>(
    outputs: &'a SessionOutputs,
    name: &str,
) -> Result<ort::value::ValueRef<'a>, SttError> {
    outputs
        .iter()
        .find(|(output_name, _)| *output_name == name)
        .map(|(_, value)| value)
        .ok_or_else(|| {
            SttError::InferenceFailed(format!("Decoder output '{}' not found", name))
        })
}

fn extract_decoder_state(
    outputs: &SessionOutputs,
    name: &str,
) -> Result<ArrayD<f32>, SttError> {
    let value = find_decoder_output(outputs, name)?;
    let (shape_obj, data) = value
        .try_extract_tensor::<f32>()
        .map_err(|e| SttError::InferenceFailed(format!("Failed to extract {}: {}", name, e)))?;
    let shape_slice: Vec<usize> = shape_obj.iter().map(|&d| d as usize).collect();
    ArrayD::from_shape_vec(IxDyn(&shape_slice), data.to_vec())
        .map_err(|e| SttError::InferenceFailed(format!("Failed to reshape {}: {}", name, e)))
}

fn infer_output_layout(shape: &[usize]) -> Option<OutputLayout> {
    match shape.len() {
        4 => {
            let time_dim = if shape[1] == 1 && shape[2] > 1 {
                2
            } else if shape[2] == 1 && shape[1] > 1 {
                1
            } else if shape[1] >= shape[2] {
                1
            } else {
                2
            };
            let target_dim = if time_dim == 1 { 2 } else { 1 };
            let time_steps = shape[time_dim];
            let target_index = shape[target_dim].saturating_sub(1);

            Some(OutputLayout {
                time_dim,
                target_dim: Some(target_dim),
                target_index,
                vocab_dim: Some(3),
                time_steps,
            })
        }
        3 => Some(OutputLayout {
            time_dim: 1,
            target_dim: None,
            target_index: 0,
            vocab_dim: Some(2),
            time_steps: shape[1],
        }),
        2 => Some(OutputLayout {
            time_dim: 1,
            target_dim: None,
            target_index: 0,
            vocab_dim: None,
            time_steps: shape[1],
        }),
        _ => None,
    }
}

fn argmax_from_output(
    output: &ndarray::ArrayViewD<'_, f32>,
    layout: &OutputLayout,
    time_step: usize,
) -> Result<i64, SttError> {
    let shape = output.shape();
    if time_step >= layout.time_steps {
        return Ok(BLANK_ID);
    }

    match layout.vocab_dim {
        Some(vocab_dim) => {
            let vocab_size = shape[vocab_dim];
            let mut max_idx = 0i64;
            let mut max_val = f32::NEG_INFINITY;

            for v in 0..vocab_size {
                let val = match (shape.len(), layout.time_dim, layout.target_dim) {
                    (4, 1, Some(2)) => output[[0, time_step, layout.target_index, v]],
                    (4, 2, Some(1)) => output[[0, layout.target_index, time_step, v]],
                    (3, 1, None) => output[[0, time_step, v]],
                    _ => {
                        return Err(SttError::InferenceFailed(
                            "Unsupported decoder output layout".to_string(),
                        ))
                    }
                };

                if val > max_val {
                    max_val = val;
                    max_idx = v as i64;
                }
            }

            Ok(max_idx)
        }
        None => {
            if layout.time_dim == 1 && shape.len() >= 2 {
                Ok(output[[0, time_step]] as i64)
            } else {
                Err(SttError::InferenceFailed(
                    "Unsupported decoder output layout".to_string(),
                ))
            }
        }
    }
}

fn infer_time_length(encoded_shape: &[usize]) -> usize {
    if encoded_shape.len() <= 1 {
        return 0;
    }

    encoded_shape
        .iter()
        .skip(1)
        .copied()
        .min()
        .unwrap_or(0)
}

fn build_decoder_inputs(
    decoder: &Session,
    encoded: &ndarray::ArrayViewD<'_, f32>,
    encoder_len: i64,
    target_token: i64,
    state_1: Option<&ArrayD<f32>>,
    state_2: Option<&ArrayD<f32>>,
) -> Result<Vec<(String, DynValue)>, SttError> {
    let decoder_inputs = decoder.inputs();
    let mut inputs = Vec::with_capacity(decoder_inputs.len());
    let target_len = 1_i64;

    for input in decoder_inputs {
        let input_name = input.name();
        let input_dtype = input.dtype();
        
        let ValueType::Tensor {
            ty,
            shape,
            ..
        } = input_dtype else {
            return Err(SttError::InferenceFailed(format!(
                "Unsupported decoder input type for '{}'",
                input_name
            )));
        };

        // Get dimensions from shape - Shape derefs to &[i64]
        let dimensions: Vec<i64> = shape.iter().cloned().collect();
        let dimension_symbols: Vec<Option<String>> = Vec::new(); // Symbolic dims not easily accessible

        let name_lower = input_name.to_lowercase();
        let value = if is_encoder_input(&name_lower) {
            crate::log_debug!("model", "Decoder input '{}' uses encoder output", input_name);
            // Clone the encoder output to create an owned tensor
            let tensor = Tensor::from_array(encoded.to_owned())
                .map_err(|e| SttError::InferenceFailed(format!("Failed to build encoder input: {}", e)))?;
            tensor.into_dyn()
        } else if is_length_input(&name_lower) {
            if name_lower.contains("target") {
                crate::log_debug!("model", "Decoder input '{}' uses target length", input_name);
                build_length_tensor(ty.clone(), &dimensions, target_len)?
            } else {
                crate::log_debug!("model", "Decoder input '{}' uses encoder length", input_name);
                build_length_tensor(ty.clone(), &dimensions, encoder_len)?
            }
        } else if let Some(state_index) = state_input_index(&name_lower) {
            let state = match state_index {
                1 => state_1,
                2 => state_2,
                _ => None,
            };
            if let Some(state) = state {
                crate::log_debug!("model", "Decoder input '{}' uses cached state", input_name);
                // Clone the state to create an owned tensor
                let tensor = Tensor::from_array(state.to_owned())
                    .map_err(|e| {
                        SttError::InferenceFailed(format!(
                            "Failed to build decoder state input: {}",
                            e
                        ))
                    })?;
                tensor.into_dyn()
            } else {
                let dims = resolve_dims(&dimensions, &dimension_symbols, encoded.shape(), encoder_len as usize);
                crate::log_debug!(
                    "model",
                    "Decoder input '{}' uses zeros with shape {:?}",
                    input_name,
                    dims
                );
                build_zero_tensor(ty.clone(), &dims)?
            }
        } else if is_token_input(&name_lower) {
            crate::log_debug!(
                "model",
                "Decoder input '{}' uses target token {}",
                input_name,
                target_token
            );
            build_filled_int_tensor(ty.clone(), &dimensions, target_token)?
        } else {
            let dims = resolve_dims(&dimensions, &dimension_symbols, encoded.shape(), encoder_len as usize);
            crate::log_debug!(
                "model",
                "Decoder input '{}' uses zeros with shape {:?}",
                input_name,
                dims
            );
            build_zero_tensor(ty.clone(), &dims)?
        };

        inputs.push((input_name.to_string(), value));
    }

    Ok(inputs)
}

fn is_encoder_input(name: &str) -> bool {
    name.contains("encoder") || name.contains("encoded") || name.contains("memory")
}

fn is_length_input(name: &str) -> bool {
    name.contains("len") || name.contains("length")
}

fn is_token_input(name: &str) -> bool {
    name.contains("token") || name.contains("input_id") || name.contains("target")
}

fn state_input_index(name: &str) -> Option<usize> {
    if name.contains("input_states_1") {
        Some(1)
    } else if name.contains("input_states_2") {
        Some(2)
    } else {
        None
    }
}

fn build_zero_tensor(ty: TensorElementType, dims: &[usize]) -> Result<DynValue, SttError> {
    match ty {
        TensorElementType::Float32 => {
            let arr = ArrayD::<f32>::zeros(IxDyn(dims));
            let tensor = Tensor::from_array(arr)
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create f32 tensor: {}", e)))?;
            Ok(tensor.into_dyn())
        },
        TensorElementType::Int64 => {
            let arr = ArrayD::<i64>::zeros(IxDyn(dims));
            let tensor = Tensor::from_array(arr)
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create i64 tensor: {}", e)))?;
            Ok(tensor.into_dyn())
        },
        TensorElementType::Int32 => {
            let arr = ArrayD::<i32>::zeros(IxDyn(dims));
            let tensor = Tensor::from_array(arr)
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create i32 tensor: {}", e)))?;
            Ok(tensor.into_dyn())
        },
        _ => Err(SttError::InferenceFailed(format!(
            "Unsupported decoder tensor element type: {:?}",
            ty
        ))),
    }
}

fn build_filled_int_tensor(
    ty: TensorElementType,
    dims: &[i64],
    value: i64,
) -> Result<DynValue, SttError> {
    let dims = resolve_fixed_dims(dims);
    match ty {
        TensorElementType::Int64 => {
            let arr = ArrayD::<i64>::from_elem(IxDyn(&dims), value);
            let tensor = Tensor::from_array(arr)
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create token tensor: {}", e)))?;
            Ok(tensor.into_dyn())
        },
        TensorElementType::Int32 => {
            let arr = ArrayD::<i32>::from_elem(IxDyn(&dims), value as i32);
            let tensor = Tensor::from_array(arr)
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create token tensor: {}", e)))?;
            Ok(tensor.into_dyn())
        },
        _ => Err(SttError::InferenceFailed(format!(
            "Unsupported token tensor element type: {:?}",
            ty
        ))),
    }
}

fn build_length_tensor(
    ty: TensorElementType,
    dims: &[i64],
    value: i64,
) -> Result<DynValue, SttError> {
    let dims = resolve_fixed_dims(dims);
    match ty {
        TensorElementType::Int64 => {
            let arr = ArrayD::<i64>::from_elem(IxDyn(&dims), value);
            let tensor = Tensor::from_array(arr)
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create length tensor: {}", e)))?;
            Ok(tensor.into_dyn())
        },
        TensorElementType::Int32 => {
            let arr = ArrayD::<i32>::from_elem(IxDyn(&dims), value as i32);
            let tensor = Tensor::from_array(arr)
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create length tensor: {}", e)))?;
            Ok(tensor.into_dyn())
        },
        TensorElementType::Float32 => {
            let arr = ArrayD::<f32>::from_elem(IxDyn(&dims), value as f32);
            let tensor = Tensor::from_array(arr)
                .map_err(|e| SttError::InferenceFailed(format!("Failed to create length tensor: {}", e)))?;
            Ok(tensor.into_dyn())
        },
        _ => Err(SttError::InferenceFailed(format!(
            "Unsupported length tensor element type: {:?}",
            ty
        ))),
    }
}

fn resolve_fixed_dims(dims: &[i64]) -> Vec<usize> {
    if dims.is_empty() {
        return vec![1];
    }

    dims.iter()
        .map(|d| if *d > 0 { *d as usize } else { 1 })
        .collect()
}

fn resolve_dims(
    dims: &[i64],
    symbols: &[Option<String>],
    encoded_shape: &[usize],
    encoder_len: usize,
) -> Vec<usize> {
    let mut resolved = Vec::with_capacity(dims.len());
    let encoded_feature = encoded_shape.iter().skip(1).copied().max().unwrap_or(1);

    for (idx, dim) in dims.iter().enumerate() {
        if *dim > 0 {
            resolved.push(*dim as usize);
            continue;
        }

        let symbol = symbols.get(idx).and_then(|s| s.as_ref()).map(|s| s.to_lowercase());
        if let Some(sym) = symbol {
            if sym.contains("batch") {
                resolved.push(1);
                continue;
            }
            if sym.contains("time") || sym.contains("seq") || sym.contains("length") {
                resolved.push(encoder_len.max(1));
                continue;
            }
            if sym.contains("feat") || sym.contains("hidden") || sym.contains("dim") {
                resolved.push(encoded_feature.max(1));
                continue;
            }
        }

        if idx == 0 {
            resolved.push(1);
        } else {
            resolved.push(1);
        }
    }

    resolved
}

fn log_session_io(label: &str, session: &Session) {
    for input in session.inputs() {
        crate::log_debug!(
            "model",
            "{} input '{}' => {:?}",
            label,
            input.name(),
            input.dtype()
        );
    }

    for output in session.outputs() {
        crate::log_debug!(
            "model",
            "{} output '{}' => {:?}",
            label,
            output.name(),
            output.dtype()
        );
    }
}

impl Default for SttEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Remove consecutive duplicate tokens (for CTC/TDT decoding)
fn remove_consecutive_duplicates(tokens: &[i64]) -> Vec<i64> {
    let mut result = Vec::new();
    let mut prev = -1i64;
    
    for &token in tokens {
        if token != prev {
            result.push(token);
            prev = token;
        }
    }
    
    result
}

fn parse_vocab_line(line: &str) -> Option<(String, usize)> {
    let mut parts = line.rsplitn(2, ' ');
    let id_part = parts.next()?;
    let token_part = parts.next()?;
    let id = id_part.parse::<usize>().ok()?;
    Some((token_part.to_string(), id))
}

/// Load vocabulary from file
fn load_vocabulary(path: &PathBuf) -> Result<Vec<String>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read vocabulary file: {}", e))?;

    let mut vocab: Vec<String> = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if let Some((token, id)) = parse_vocab_line(line) {
            if vocab.len() <= id {
                vocab.resize(id + 1, String::new());
            }
            vocab[id] = token;
        } else {
            vocab.push(line.to_string());
        }
    }

    if vocab.is_empty() {
        return Err("Vocabulary file is empty".to_string());
    }

    Ok(vocab)
}

fn find_token_id(vocab: &[String], token: &str) -> Option<i64> {
    vocab
        .iter()
        .position(|entry| entry == token)
        .map(|idx| idx as i64)
}

/// Decode token IDs to text using vocabulary
fn decode_tokens(tokens: &[i64], vocab: &[String]) -> String {
    let mut text = String::new();
    const WORD_BOUNDARY: &str = "\u{2581}";

    for &token_id in tokens {
        if token_id >= 0 && (token_id as usize) < vocab.len() {
            let token = &vocab[token_id as usize];
            if token.is_empty() {
                continue;
            }

            // Handle SentencePiece encoding
            // Tokens starting with the word boundary marker indicate word starts.
            if let Some(rest) = token.strip_prefix(WORD_BOUNDARY) {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(rest);
            } else if token == "<space>" {
                text.push(' ');
            } else if !token.starts_with('<') {
                // Skip special tokens like <blank>, <unk>, etc.
                text.push_str(token);
            }
        }
    }

    // Clean up the text
    let text = text.trim().to_string();

    // Basic post-processing: capitalize first letter
    if let Some(first_char) = text.chars().next() {
        let mut result = first_char.to_uppercase().to_string();
        result.push_str(&text[first_char.len_utf8()..]);
        return result;
    }

    text
}

/// Create mel filterbank matrix
fn create_mel_filterbank(
    n_fft: usize,
    n_mels: usize,
    sample_rate: u32,
    fmin: f32,
    fmax: f32,
) -> Array2<f32> {
    let n_freqs = n_fft / 2 + 1;
    
    // Convert Hz to Mel
    let hz_to_mel = |hz: f32| 2595.0 * (1.0 + hz / 700.0).log10();
    let mel_to_hz = |mel: f32| 700.0 * (10.0_f32.powf(mel / 2595.0) - 1.0);
    
    let mel_min = hz_to_mel(fmin);
    let mel_max = hz_to_mel(fmax);
    
    // Create mel points
    let mut mel_points = Vec::with_capacity(n_mels + 2);
    for i in 0..=(n_mels + 1) {
        let mel = mel_min + (mel_max - mel_min) * (i as f32) / ((n_mels + 1) as f32);
        mel_points.push(mel_to_hz(mel));
    }
    
    // Convert to FFT bins
    let fft_freqs: Vec<f32> = (0..n_freqs)
        .map(|i| (i as f32) * (sample_rate as f32) / (n_fft as f32))
        .collect();
    
    // Create filterbank
    let mut filterbank = Array2::zeros((n_mels, n_freqs));
    
    for m in 0..n_mels {
        let f_left = mel_points[m];
        let f_center = mel_points[m + 1];
        let f_right = mel_points[m + 2];
        
        for (k, &freq) in fft_freqs.iter().enumerate() {
            if freq >= f_left && freq <= f_center {
                filterbank[[m, k]] = (freq - f_left) / (f_center - f_left);
            } else if freq > f_center && freq <= f_right {
                filterbank[[m, k]] = (f_right - freq) / (f_right - f_center);
            }
        }
    }
    
    filterbank
}

/// Compute mel spectrogram from audio (NeMo-compatible)
fn compute_mel_spectrogram(audio: &[f32], mel_filterbank: &Array2<f32>) -> Array2<f32> {
    let n_fft = N_FFT;
    let hop_length = HOP_LENGTH;
    let win_length = WIN_LENGTH;
    
    // Pre-emphasis filter (NeMo default: 0.97)
    // y[n] = x[n] - preemph * x[n-1]
    let preemph = 0.97f32;
    let mut preemphasized: Vec<f32> = Vec::with_capacity(audio.len());
    for i in 0..audio.len() {
        if i == 0 {
            preemphasized.push(audio[i]);
        } else {
            preemphasized.push(audio[i] - preemph * audio[i - 1]);
        }
    }
    
    // Add small dither noise for numerical stability (NeMo default: 1e-5)
    let dither = 1e-5f32;
    for sample in &mut preemphasized {
        *sample += dither * (rand_simple() * 2.0 - 1.0);
    }
    
    // Create Hann window (periodic=True in NeMo)
    let window: Vec<f32> = (0..win_length)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / win_length as f32).cos()))
        .collect();
    
    // Pad audio - NeMo uses reflect padding, but we'll use zero padding
    let pad_length = n_fft / 2;
    let mut padded_audio = vec![0.0f32; pad_length];
    padded_audio.extend_from_slice(&preemphasized);
    padded_audio.extend(vec![0.0f32; pad_length]);
    
    // Calculate number of frames
    let n_frames = 1 + (padded_audio.len() - n_fft) / hop_length;
    
    // Create FFT planner
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(n_fft);
    
    // Compute STFT
    let n_freqs = n_fft / 2 + 1;
    let mut power_spec = Array2::zeros((n_freqs, n_frames));
    
    for frame_idx in 0..n_frames {
        let start = frame_idx * hop_length;
        
        // Extract frame and apply window
        let mut buffer: Vec<Complex<f32>> = (0..n_fft)
            .map(|i| {
                let sample = if i < win_length && start + i < padded_audio.len() {
                    padded_audio[start + i] * window[i]
                } else {
                    0.0
                };
                Complex::new(sample, 0.0)
            })
            .collect();
        
        // Apply FFT
        fft.process(&mut buffer);
        
        // Compute power spectrum (only positive frequencies)
        // NeMo uses power spectrum (magnitude squared), not magnitude
        for (k, &val) in buffer.iter().take(n_freqs).enumerate() {
            power_spec[[k, frame_idx]] = val.norm_sqr();
        }
    }
    
    // Apply mel filterbank
    let mel_spec = mel_filterbank.dot(&power_spec);
    
    // Convert to log scale (NeMo style: natural log with small guard value)
    let log_mel_spec = mel_spec.mapv(|x| (x + 1e-5).ln());
    
    // Per-feature normalization (normalize each mel bin independently)
    // This is NeMo's default "per_feature" normalization
    let mut normalized = log_mel_spec.clone();
    for mel_idx in 0..normalized.shape()[0] {
        let row = normalized.row(mel_idx);
        let mean = row.mean().unwrap_or(0.0);
        let std = row.std(0.0);
        let std = if std < 1e-6 { 1.0 } else { std };
        
        for frame_idx in 0..normalized.shape()[1] {
            normalized[[mel_idx, frame_idx]] = (normalized[[mel_idx, frame_idx]] - mean) / std;
        }
    }
    
    normalized
}

// Simple pseudo-random number generator (deterministic, no external deps)
fn rand_simple() -> f32 {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEED: AtomicU32 = AtomicU32::new(12345);
    let mut s = SEED.load(Ordering::Relaxed);
    s = s.wrapping_mul(1103515245).wrapping_add(12345);
    SEED.store(s, Ordering::Relaxed);
    (s >> 16) as f32 / 65536.0
}

/// Get model paths from the models directory
pub fn get_model_paths(model_name: &str) -> Option<SttConfig> {
    let models_dir = crate::config::get_models_dir();
    let model_dir = models_dir.join(model_name);

    if !model_dir.exists() {
        return None;
    }

    // Prefer INT8 quantized models
    let encoder_int8 = model_dir.join("encoder-model.int8.onnx");
    let decoder_int8 = model_dir.join("decoder_joint-model.int8.onnx");
    let encoder_full = model_dir.join("encoder-model.onnx");
    let decoder_full = model_dir.join("decoder_joint-model.onnx");
    let vocab = model_dir.join("vocab.txt");

    let encoder_path = if encoder_int8.exists() {
        encoder_int8
    } else {
        encoder_full
    };

    let decoder_path = if decoder_int8.exists() {
        decoder_int8
    } else {
        decoder_full
    };

    if !encoder_path.exists() || !decoder_path.exists() || !vocab.exists() {
        return None;
    }

    Some(SttConfig {
        encoder_path,
        decoder_path,
        vocab_path: vocab,
        timeout_ms: 30000, // 30 second default timeout
    })
}

/// Check if model files exist and are valid
pub fn validate_model(model_name: &str) -> Result<(), SttError> {
    let config = get_model_paths(model_name)
        .ok_or_else(|| SttError::ModelNotFound(format!("Model '{}' not found", model_name)))?;

    // Check file sizes (basic validation)
    let encoder_size = std::fs::metadata(&config.encoder_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let decoder_size = std::fs::metadata(&config.decoder_path)
        .map(|m| m.len())
        .unwrap_or(0);

    // Encoder should be at least 10MB
    if encoder_size < 10_000_000 {
        return Err(SttError::ModelNotFound(format!(
            "Encoder model appears incomplete ({}KB)",
            encoder_size / 1024
        )));
    }

    // Decoder should be at least 1MB
    if decoder_size < 1_000_000 {
        return Err(SttError::ModelNotFound(format!(
            "Decoder model appears incomplete ({}KB)",
            decoder_size / 1024
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stt_engine_new() {
        let engine = SttEngine::new();
        assert_eq!(engine.state(), SttState::Unloaded);
        assert!(!engine.is_ready());
    }

    #[test]
    fn test_transcribe_without_load() {
        let mut engine = SttEngine::new();
        let audio = vec![0.0f32; 16000]; // 1 second of silence
        let result = engine.transcribe(&audio);
        assert!(matches!(result, Err(SttError::NotLoaded)));
    }

    #[test]
    fn test_mel_filterbank_shape() {
        let filterbank = create_mel_filterbank(512, 128, 16000, 0.0, 8000.0);
        assert_eq!(filterbank.shape(), &[128, 257]);
    }

    #[test]
    fn test_mel_spectrogram() {
        let audio = vec![0.0f32; 16000]; // 1 second of silence
        let filterbank = create_mel_filterbank(512, 80, 16000, 0.0, 8000.0);
        let mel_spec = compute_mel_spectrogram(&audio, &filterbank);
        assert_eq!(mel_spec.shape()[0], 128); // 128 mel bins
    }

    #[test]
    fn test_decode_tokens() {
        let vocab = vec![
            "\u{2581}hello".to_string(),
            "\u{2581}world".to_string(),
            "test".to_string(),
        ];
        let tokens = vec![0, 1];
        let text = decode_tokens(&tokens, &vocab);
        assert_eq!(text, "Hello world");
    }

    #[test]
    fn test_remove_consecutive_duplicates() {
        let tokens = vec![1, 1, 2, 2, 2, 3, 1, 1];
        let result = remove_consecutive_duplicates(&tokens);
        assert_eq!(result, vec![1, 2, 3, 1]);
    }
}
