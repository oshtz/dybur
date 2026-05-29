//! Speech-to-Text engine using ONNX Runtime
//!
//! Supports multiple model architectures:
//! - TDT Transducer (Parakeet v2/v3)
//! - Encoder-Decoder (Whisper)
//! - Streaming Transducer (Nemotron)
//! - LLM Decoder (Canary)
//!
//! Pipeline: Audio -> Mel Spectrogram -> Encoder -> Decoder -> Text

use crate::execution_providers::{build_session, GpuPreference, SessionConfig};
use crate::models::ModelArchitecture;
use crate::tokenizer::BpeTokenizer;
use ndarray::{Array1, Array2, ArrayD, Axis, IxDyn};
use ort::{
    session::{Session, SessionOutputs},
    tensor::TensorElementType,
    value::{DynValue, Tensor, TensorRef, ValueType},
};
use rustfft::{num_complex::Complex, FftPlanner};
use std::path::PathBuf;
use std::time::Instant;

// Audio processing constants (Parakeet/NeMo default: 128 mels)
const SAMPLE_RATE: u32 = 16000;
const N_FFT: usize = 512;
const HOP_LENGTH: usize = 160; // 10ms at 16kHz
const WIN_LENGTH: usize = 400; // 25ms at 16kHz
const N_MELS: usize = 128;
const MEL_FMIN: f32 = 0.0;
const MEL_FMAX: f32 = 8000.0;

// Whisper-specific constants (80 mel bins, different FFT params)
const WHISPER_N_FFT: usize = 400;
const WHISPER_HOP_LENGTH: usize = 160;
const WHISPER_N_MELS: usize = 128; // Whisper v3 uses 128 mels
const WHISPER_SAMPLE_RATE: u32 = 16000;
const WHISPER_CHUNK_LENGTH: usize = 30; // 30 seconds per chunk
const WHISPER_N_SAMPLES: usize = WHISPER_CHUNK_LENGTH * WHISPER_SAMPLE_RATE as usize;

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
    /// Path to joiner ONNX model (for Streaming Transducer/Nemotron)
    pub joiner_path: Option<PathBuf>,
    /// Path to embeddings ONNX model (for LLM Decoder/Canary)
    pub embeddings_path: Option<PathBuf>,
    /// Path to vocabulary file (vocab.txt for TDT, tokenizer.json for BPE)
    pub vocab_path: PathBuf,
    /// Inference timeout in milliseconds
    pub timeout_ms: u64,
    /// Model architecture
    pub architecture: ModelArchitecture,
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

/// Streaming model metadata (extracted from ONNX model metadata)
/// These values are baked into the sherpa-onnx export and define the exact
/// chunk size and cache dimensions required for inference.
#[derive(Debug, Clone)]
pub struct StreamingMetadata {
    /// Window size in mel frames (chunk size to process)
    pub window_size: usize,
    /// Chunk shift (stride between chunks)
    pub chunk_shift: usize,
    /// Cache channel dimensions [batch, layers, time, features]
    pub cache_last_channel_dims: [usize; 4],
    /// Cache time dimensions [batch, layers, features, time]
    pub cache_last_time_dims: [usize; 4],
}

impl Default for StreamingMetadata {
    fn default() -> Self {
        // Default values for att_context_size = [70, 13] (1120ms chunks)
        Self {
            window_size: 112, // 14 output frames * 8 = 112 mel frames (1120ms)
            chunk_shift: 112,
            cache_last_channel_dims: [1, 24, 70, 1024], // [batch, layers, left_context, encoder_dim]
            cache_last_time_dims: [1, 24, 1024, 70], // [batch, layers, encoder_dim, left_context]
        }
    }
}

/// Speech-to-Text engine
pub struct SttEngine {
    config: Option<SttConfig>,
    state: SttState,
    /// Model architecture (determines inference pipeline)
    architecture: ModelArchitecture,
    /// SentencePiece vocabulary for TDT/Nemotron models
    vocab: Option<Vec<String>>,
    /// BPE tokenizer for Whisper/Canary models
    bpe_tokenizer: Option<BpeTokenizer>,
    preprocessor_session: Option<Session>,
    encoder_session: Option<Session>,
    decoder_session: Option<Session>,
    /// Joiner session for Streaming Transducer (Nemotron)
    joiner_session: Option<Session>,
    /// Embeddings session for LLM Decoder (Canary)
    embeddings_session: Option<Session>,
    /// Mel filterbank for TDT (128 bins)
    mel_filterbank: Option<Array2<f32>>,
    /// Mel filterbank for Whisper (128 bins, different params)
    whisper_mel_filterbank: Option<Array2<f32>>,
    /// Mel filterbank for Nemotron (128 bins)
    nemotron_mel_filterbank: Option<Array2<f32>>,
    /// Streaming metadata for Nemotron (from ONNX model metadata)
    streaming_metadata: Option<StreamingMetadata>,
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
            architecture: ModelArchitecture::TdtTransducer,
            vocab: None,
            bpe_tokenizer: None,
            preprocessor_session: None,
            encoder_session: None,
            decoder_session: None,
            joiner_session: None,
            embeddings_session: None,
            mel_filterbank: None,
            whisper_mel_filterbank: None,
            nemotron_mel_filterbank: None,
            streaming_metadata: None,
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

    /// Get the model architecture
    pub fn get_architecture(&self) -> ModelArchitecture {
        self.architecture
    }

    /// Get streaming metadata (for Nemotron model)
    pub fn get_streaming_metadata(&self) -> Option<&StreamingMetadata> {
        self.streaming_metadata.as_ref()
    }

    /// Get vocabulary (for SentencePiece models)
    pub fn get_vocab(&self) -> Option<&[String]> {
        self.vocab.as_deref()
    }

    /// Get Nemotron mel filterbank
    pub fn get_nemotron_mel_filterbank(&self) -> Option<&Array2<f32>> {
        self.nemotron_mel_filterbank.as_ref()
    }

    /// Get mutable reference to encoder session (for streaming)
    pub fn get_encoder_session_mut(&mut self) -> Option<&mut Session> {
        self.encoder_session.as_mut()
    }

    /// Get mutable reference to decoder session (for streaming)
    pub fn get_decoder_session_mut(&mut self) -> Option<&mut Session> {
        self.decoder_session.as_mut()
    }

    /// Get mutable reference to joiner session (for streaming)
    pub fn get_joiner_session_mut(&mut self) -> Option<&mut Session> {
        self.joiner_session.as_mut()
    }

    /// Load model from config
    pub fn load(
        &mut self,
        config: SttConfig,
        gpu_preference: GpuPreference,
    ) -> Result<(), SttError> {
        self.state = SttState::Loading;
        self.last_error = None;

        let architecture = config.architecture;
        crate::log_info!(
            "model",
            "Loading STT model (architecture: {:?})...",
            architecture
        );

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

        // Load vocabulary based on architecture
        match architecture {
            ModelArchitecture::TdtTransducer | ModelArchitecture::StreamingTransducer => {
                // Load SentencePiece vocabulary (vocab.txt)
                let vocab = match load_vocabulary(&config.vocab_path) {
                    Ok(v) => v,
                    Err(e) => {
                        let err =
                            SttError::ModelLoadFailed(format!("Failed to load vocabulary: {}", e));
                        self.state = SttState::Error;
                        self.last_error = Some(err.clone());
                        crate::log_error!("model", "{}", err);
                        return Err(err);
                    }
                };
                crate::log_info!(
                    "model",
                    "Loaded SentencePiece vocabulary with {} tokens",
                    vocab.len()
                );
                self.vocab = Some(vocab);
                self.bpe_tokenizer = None;
            }
            ModelArchitecture::EncoderDecoder => {
                // Load BPE tokenizer (tokenizer.json)
                let tokenizer = match BpeTokenizer::from_file(&config.vocab_path) {
                    Ok(t) => t,
                    Err(e) => {
                        let err =
                            SttError::ModelLoadFailed(format!("Failed to load tokenizer: {}", e));
                        self.state = SttState::Error;
                        self.last_error = Some(err.clone());
                        crate::log_error!("model", "{}", err);
                        return Err(err);
                    }
                };
                crate::log_info!(
                    "model",
                    "Loaded BPE tokenizer with {} tokens",
                    tokenizer.vocab_size()
                );
                self.bpe_tokenizer = Some(tokenizer);
                self.vocab = None;
            }
        }

        // Force CPU for models with DirectML compatibility issues
        let session_config = match architecture {
            ModelArchitecture::StreamingTransducer => {
                crate::log_info!(
                    "model",
                    "Using CPU for Nemotron model (DirectML incompatible)"
                );
                SessionConfig::cpu_only(4)
            }
            _ => SessionConfig::for_stt().with_gpu_preference(gpu_preference),
        };

        // Load ONNX preprocessor (only for TDT models)
        let preprocessor_session = match architecture {
            ModelArchitecture::TdtTransducer => {
                let preprocessor_path =
                    config.encoder_path.parent().map(|p| p.join("nemo128.onnx"));

                if let Some(ref prep_path) = preprocessor_path {
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
                        crate::log_info!(
                            "model",
                            "No ONNX preprocessor found, using manual mel computation"
                        );
                        None
                    }
                } else {
                    None
                }
            }
            _ => None, // Other architectures don't use NeMo preprocessor
        };

        // Initialize ONNX Runtime sessions with GPU acceleration
        crate::log_info!("model", "Loading encoder model...");
        let (encoder_session, encoder_ep) =
            match build_session(&config.encoder_path, &session_config) {
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

        // Read streaming metadata for Nemotron models
        let streaming_metadata = if architecture == ModelArchitecture::StreamingTransducer {
            match Self::read_streaming_metadata(&encoder_session) {
                Some(meta) => {
                    crate::log_info!(
                        "model",
                        "Streaming metadata: window_size={}, chunk_shift={}, cache_channel={:?}, cache_time={:?}",
                        meta.window_size,
                        meta.chunk_shift,
                        meta.cache_last_channel_dims,
                        meta.cache_last_time_dims
                    );
                    Some(meta)
                }
                None => {
                    crate::log_warn!("model", "No streaming metadata in encoder, using defaults");
                    Some(StreamingMetadata::default())
                }
            }
        } else {
            None
        };

        crate::log_info!("model", "Loading decoder model...");
        let (decoder_session, decoder_ep) =
            match build_session(&config.decoder_path, &session_config) {
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

        // Load joiner for StreamingTransducer (Nemotron)
        let joiner_session = if let Some(ref joiner_path) = config.joiner_path {
            if joiner_path.exists() {
                crate::log_info!("model", "Loading joiner model...");
                match build_session(joiner_path, &session_config) {
                    Ok((session, ep_result)) => {
                        crate::log_info!(
                            "model",
                            "Joiner loaded (provider: {}, GPU: {})",
                            ep_result.provider_name,
                            ep_result.is_gpu
                        );
                        Some(session)
                    }
                    Err(e) => {
                        let err =
                            SttError::ModelLoadFailed(format!("Failed to load joiner: {}", e));
                        self.state = SttState::Error;
                        self.last_error = Some(err.clone());
                        crate::log_error!("model", "{}", err);
                        return Err(err);
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        // Load optional embeddings model (if provided)
        let embeddings_session = if let Some(ref embeddings_path) = config.embeddings_path {
            if embeddings_path.exists() {
                crate::log_info!("model", "Loading embeddings model...");
                match build_session(embeddings_path, &session_config) {
                    Ok((session, ep_result)) => {
                        crate::log_info!(
                            "model",
                            "Embeddings loaded (provider: {}, GPU: {})",
                            ep_result.provider_name,
                            ep_result.is_gpu
                        );
                        Some(session)
                    }
                    Err(e) => {
                        let err =
                            SttError::ModelLoadFailed(format!("Failed to load embeddings: {}", e));
                        self.state = SttState::Error;
                        self.last_error = Some(err.clone());
                        crate::log_error!("model", "{}", err);
                        return Err(err);
                    }
                }
            } else {
                None
            }
        } else {
            None
        };

        // Pre-compute mel filterbanks based on architecture
        let mel_filterbank = match architecture {
            ModelArchitecture::TdtTransducer => Some(create_mel_filterbank(
                N_FFT,
                N_MELS,
                SAMPLE_RATE,
                MEL_FMIN,
                MEL_FMAX,
            )),
            _ => None,
        };

        let whisper_mel_filterbank = match architecture {
            ModelArchitecture::EncoderDecoder => {
                // Whisper uses 128 mel bins with Whisper-style FFT params
                Some(create_whisper_mel_filterbank())
            }
            _ => None,
        };

        let nemotron_mel_filterbank = match architecture {
            ModelArchitecture::StreamingTransducer => {
                // Nemotron uses 128 mel bins
                Some(create_mel_filterbank(
                    N_FFT,
                    128,
                    SAMPLE_RATE,
                    MEL_FMIN,
                    MEL_FMAX,
                ))
            }
            _ => None,
        };

        log_session_io("encoder", &encoder_session);
        log_session_io("decoder", &decoder_session);

        self.architecture = architecture;
        self.preprocessor_session = preprocessor_session;
        self.encoder_session = Some(encoder_session);
        self.decoder_session = Some(decoder_session);
        self.joiner_session = joiner_session;
        self.embeddings_session = embeddings_session;
        self.mel_filterbank = mel_filterbank;
        self.whisper_mel_filterbank = whisper_mel_filterbank;
        self.nemotron_mel_filterbank = nemotron_mel_filterbank;
        self.streaming_metadata = streaming_metadata;
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

    /// Read streaming metadata from ONNX model metadata
    /// The sherpa-onnx export embeds streaming parameters in the model metadata
    fn read_streaming_metadata(session: &Session) -> Option<StreamingMetadata> {
        let metadata = session.metadata().ok()?;

        // Helper to read a custom metadata key and parse as usize
        let get_usize = |key: &str, default: usize| -> usize {
            metadata
                .custom(key)
                .and_then(|s| s.parse().ok())
                .unwrap_or(default)
        };

        // Log some metadata for debugging
        if let Some(name) = metadata.name() {
            crate::log_debug!("model", "ONNX model name: {}", name);
        }
        if let Some(desc) = metadata.description() {
            crate::log_debug!("model", "ONNX model description: {}", desc);
        }

        // Log the streaming parameters we find
        for key in &[
            "window_size",
            "chunk_shift",
            "cache_last_channel_dim1",
            "cache_last_channel_dim2",
            "cache_last_channel_dim3",
            "cache_last_time_dim1",
            "cache_last_time_dim2",
            "cache_last_time_dim3",
        ] {
            if let Some(val) = metadata.custom(key) {
                crate::log_debug!("model", "ONNX metadata: {} = {}", key, val);
            }
        }

        // Parse window_size
        let window_size = get_usize("window_size", 112);

        // Parse chunk_shift
        let chunk_shift = get_usize("chunk_shift", window_size);

        // Parse cache channel dimensions
        // Metadata gives us 3 dims (no batch): layers, context, encoder_dim
        // We need shape [batch=1, layers, context, encoder_dim]
        let cache_channel_dim1 = get_usize("cache_last_channel_dim1", 24); // layers
        let cache_channel_dim2 = get_usize("cache_last_channel_dim2", 70); // left_context
        let cache_channel_dim3 = get_usize("cache_last_channel_dim3", 1024); // encoder_dim

        // Parse cache time dimensions
        // Metadata gives us 3 dims (no batch): layers, encoder_dim, time_context
        // We need shape [batch=1, layers, encoder_dim, time_context]
        let cache_time_dim1 = get_usize("cache_last_time_dim1", 24); // layers
        let cache_time_dim2 = get_usize("cache_last_time_dim2", 1024); // encoder_dim
        let cache_time_dim3 = get_usize("cache_last_time_dim3", 8); // time_context

        Some(StreamingMetadata {
            window_size,
            chunk_shift,
            // Add batch dimension (1) as first element
            cache_last_channel_dims: [
                1,
                cache_channel_dim1,
                cache_channel_dim2,
                cache_channel_dim3,
            ],
            cache_last_time_dims: [1, cache_time_dim1, cache_time_dim2, cache_time_dim3],
        })
    }

    /// Unload model and free resources
    pub fn unload(&mut self) {
        self.config = None;
        self.architecture = ModelArchitecture::TdtTransducer;
        self.vocab = None;
        self.bpe_tokenizer = None;
        self.preprocessor_session = None;
        self.encoder_session = None;
        self.decoder_session = None;
        self.joiner_session = None;
        self.embeddings_session = None;
        self.mel_filterbank = None;
        self.whisper_mel_filterbank = None;
        self.nemotron_mel_filterbank = None;
        self.streaming_metadata = None;
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
        // Dispatch to architecture-specific inference
        match self.architecture {
            ModelArchitecture::TdtTransducer => self.run_tdt_inference(audio),
            ModelArchitecture::EncoderDecoder => self.run_whisper_inference(audio),
            ModelArchitecture::StreamingTransducer => self.run_nemotron_inference(audio),
        }
    }

    /// Run TDT (Parakeet) inference pipeline
    fn run_tdt_inference(&mut self, audio: &[f32]) -> Result<String, SttError> {
        // Extract vocab info first to avoid borrow conflicts
        let vocab = self.vocab.as_ref().ok_or(SttError::NotLoaded)?.clone();
        let vocab_len = vocab.len();

        // Get mel filterbank if needed (before mutable borrows)
        let mel_filterbank = self.mel_filterbank.clone();

        // Step 1: Compute mel spectrogram using ONNX preprocessor if available
        let (mel_input, audio_length) = if let Some(preprocessor) = &mut self.preprocessor_session {
            crate::log_debug!("model", "Using ONNX preprocessor...");

            // Prepare input: waveforms [batch=1, N], waveforms_lens [batch=1]
            let waveforms =
                Array2::from_shape_vec((1, audio.len()), audio.to_vec()).map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to create waveforms array: {}", e))
                })?;
            let waveforms_lens = Array1::from_vec(vec![audio.len() as i64]);

            // Run preprocessor - create TensorRefs from ndarrays
            let waveforms_tensor = TensorRef::from_array_view(waveforms.view()).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create waveforms tensor: {}", e))
            })?;
            let waveforms_lens_tensor =
                TensorRef::from_array_view(waveforms_lens.view()).map_err(|e| {
                    SttError::InferenceFailed(format!(
                        "Failed to create waveforms_lens tensor: {}",
                        e
                    ))
                })?;

            let prep_outputs = preprocessor
                .run(ort::inputs![waveforms_tensor, waveforms_lens_tensor])
                .map_err(|e| SttError::InferenceFailed(format!("Preprocessor failed: {}", e)))?;

            // Extract features and lengths
            let features = prep_outputs
                .iter()
                .find(|(name, _)| *name == "features")
                .map(|(_, v)| v)
                .ok_or_else(|| {
                    SttError::InferenceFailed("Missing 'features' output".to_string())
                })?;

            let features_lens = prep_outputs
                .iter()
                .find(|(name, _)| *name == "features_lens")
                .map(|(_, v)| v)
                .ok_or_else(|| {
                    SttError::InferenceFailed("Missing 'features_lens' output".to_string())
                })?;

            let (features_shape, features_data) =
                features.try_extract_tensor::<f32>().map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to extract features: {}", e))
                })?;
            let (_, features_lens_data) =
                features_lens.try_extract_tensor::<i64>().map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to extract features_lens: {}", e))
                })?;

            let mel_array =
                ArrayD::from_shape_vec(features_shape.to_ixdyn(), features_data.to_vec()).map_err(
                    |e| SttError::InferenceFailed(format!("Failed to reshape features: {}", e)),
                )?;
            let n_frames = features_lens_data[0] as usize;

            crate::log_debug!(
                "model",
                "Preprocessor output shape: {:?}, frames: {}",
                mel_array.shape(),
                n_frames
            );

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
        let mel_tensor = TensorRef::from_array_view(mel_input.view()).map_err(|e| {
            SttError::InferenceFailed(format!("Failed to create mel tensor: {}", e))
        })?;
        let audio_length_tensor = TensorRef::from_array_view(audio_length.view()).map_err(|e| {
            SttError::InferenceFailed(format!("Failed to create audio_length tensor: {}", e))
        })?;

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
        let tokens = greedy_decode(
            decoder,
            &encoder_outputs,
            blank_id,
            start_token_id,
            vocab_len,
        )?;

        // Step 5: Convert tokens to text
        let text = decode_tokens(&tokens, &vocab);

        Ok(text)
    }

    /// Run Whisper (Encoder-Decoder) inference pipeline
    fn run_whisper_inference(&mut self, audio: &[f32]) -> Result<String, SttError> {
        // Get BPE tokenizer (required for Whisper)
        let tokenizer = self.bpe_tokenizer.as_ref().ok_or(SttError::NotLoaded)?;
        let eot_token_id = tokenizer.eot_token_id;
        let sot_token_id = tokenizer.sot_token_id;

        // Get Whisper mel filterbank
        let mel_filterbank = self
            .whisper_mel_filterbank
            .as_ref()
            .ok_or(SttError::NotLoaded)?;

        // Step 1: Compute Whisper-style mel spectrogram
        crate::log_debug!("model", "Computing Whisper mel spectrogram...");
        let mel_spec = compute_whisper_mel_spectrogram(audio, mel_filterbank);
        crate::log_debug!("model", "Mel spectrogram shape: {:?}", mel_spec.shape());

        // Shape: [batch=1, n_mels, time_steps] -> transpose to [batch, time, n_mels]
        // Whisper encoder expects shape [batch, n_mels, frames]
        let mel_input = mel_spec.insert_axis(Axis(0));
        let mel_input_dyn = mel_input.into_dyn();

        // Step 2: Run encoder
        crate::log_debug!("model", "Running Whisper encoder...");
        let mel_tensor = TensorRef::from_array_view(mel_input_dyn.view()).map_err(|e| {
            SttError::InferenceFailed(format!("Failed to create mel tensor: {}", e))
        })?;

        // Extract encoder outputs within a scope to release the borrow
        let (encoder_output, encoder_kv_cache) = {
            let encoder = self.encoder_session.as_mut().ok_or(SttError::NotLoaded)?;
            let encoder_outputs = encoder
                .run(ort::inputs![mel_tensor])
                .map_err(|e| SttError::InferenceFailed(format!("Whisper encoder failed: {}", e)))?;

            // Log all encoder outputs
            crate::log_debug!("model", "Whisper encoder outputs:");
            for (name, _) in encoder_outputs.iter() {
                crate::log_debug!("model", "  - '{}'", name);
            }

            // Extract encoder hidden states (first output)
            let (_, encoder_hidden) = encoder_outputs
                .iter()
                .next()
                .ok_or_else(|| SttError::InferenceFailed("No encoder output".to_string()))?;

            let (enc_shape, enc_data) =
                encoder_hidden.try_extract_tensor::<f32>().map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to extract encoder output: {}", e))
                })?;

            let hidden_states = ArrayD::from_shape_vec(enc_shape.to_ixdyn(), enc_data.to_vec())
                .map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to reshape encoder output: {}", e))
                })?;

            // Collect cross-attention KV cache outputs if present
            let mut kv_cache: Vec<(String, ArrayD<f32>)> = Vec::new();
            for (name, value) in encoder_outputs.iter() {
                if name.contains("key_values") || name.contains("cross_attention") {
                    if let Ok((shape, data)) = value.try_extract_tensor::<f32>() {
                        if let Ok(arr) = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()) {
                            kv_cache.push((name.to_string(), arr));
                        }
                    }
                }
            }

            (hidden_states, kv_cache)
        };

        crate::log_debug!(
            "model",
            "Encoder output shape: {:?}",
            encoder_output.shape()
        );
        if !encoder_kv_cache.is_empty() {
            crate::log_debug!(
                "model",
                "Encoder produced {} KV cache tensors",
                encoder_kv_cache.len()
            );
        }

        // Step 3: Autoregressive decoding with KV-cache
        crate::log_debug!("model", "Running Whisper decoder (autoregressive)...");
        let tokens =
            self.whisper_decode_with_kv_cache(&encoder_output, sot_token_id, eot_token_id)?;

        // Step 4: Decode tokens to text using BPE tokenizer
        let tokenizer = self.bpe_tokenizer.as_ref().ok_or(SttError::NotLoaded)?;
        let text = tokenizer.decode(&tokens, true);

        // Post-process: trim and capitalize
        let text = text.trim().to_string();
        let text = if let Some(first_char) = text.chars().next() {
            let mut result = first_char.to_uppercase().to_string();
            result.push_str(&text[first_char.len_utf8()..]);
            result
        } else {
            text
        };

        Ok(text)
    }

    /// Whisper autoregressive decoding
    /// Uses the standard decoder (not with_past) which takes encoder_hidden_states directly
    fn whisper_decode_with_kv_cache(
        &mut self,
        encoder_output: &ArrayD<f32>,
        sot_token_id: i64,
        eot_token_id: i64,
    ) -> Result<Vec<i64>, SttError> {
        use crate::tokenizer::whisper_tokens;

        let decoder = self.decoder_session.as_mut().ok_or(SttError::NotLoaded)?;

        // Log decoder inputs on first call
        crate::log_debug!("model", "Whisper decoder inputs:");
        for input in decoder.inputs() {
            crate::log_debug!("model", "  - '{}' {:?}", input.name(), input.dtype());
        }

        // Find input names dynamically
        let (token_input_name, encoder_input_name) = {
            let mut token_name = "input_ids".to_string();
            let mut encoder_name = "encoder_hidden_states".to_string();
            for input in decoder.inputs() {
                let name = input.name();
                let name_lower = name.to_lowercase();
                if name_lower.contains("input_id") || name_lower == "tokens" {
                    token_name = name.to_string();
                }
                if name_lower.contains("encoder") && name_lower.contains("hidden") {
                    encoder_name = name.to_string();
                }
            }
            (token_name, encoder_name)
        };
        crate::log_debug!(
            "model",
            "Using input names: token='{}', encoder='{}'",
            token_input_name,
            encoder_input_name
        );

        // Initial decoder input sequence: [SOT, language, task, notimestamps]
        let mut all_tokens: Vec<i64> = vec![
            sot_token_id,
            whisper_tokens::EN,            // English
            whisper_tokens::TRANSCRIBE,    // Transcribe task
            whisper_tokens::NO_TIMESTAMPS, // No timestamps
        ];

        let mut output_tokens: Vec<i64> = Vec::new();
        let max_tokens = 448; // Whisper's default max length

        // Pre-convert encoder tensor to avoid repeated allocations
        let encoder_dyn = encoder_output.clone().into_dyn();

        for step in 0..max_tokens {
            // Prepare input tensor with all tokens so far
            let input_ids = Array2::from_shape_vec((1, all_tokens.len()), all_tokens.clone())
                .map_err(|e| SttError::InferenceFailed(format!("input_ids: {}", e)))?;

            let input_ids_tensor = TensorRef::from_array_view(input_ids.view())
                .map_err(|e| SttError::InferenceFailed(format!("input_ids tensor: {}", e)))?;

            // Log tensor info before decoder run
            if step == 0 {
                crate::log_debug!(
                    "model",
                    "Decoder step 0: input_ids shape {:?}, encoder shape {:?}",
                    input_ids.shape(),
                    encoder_output.shape()
                );
            }

            // Run decoder with all tokens and encoder hidden states
            crate::log_debug!(
                "model",
                "Running decoder step {} with {} tokens: {:?}",
                step,
                all_tokens.len(),
                all_tokens
            );

            // Create tensor refs for this iteration
            let encoder_tensor = TensorRef::from_array_view(encoder_dyn.view())
                .map_err(|e| SttError::InferenceFailed(format!("encoder tensor: {}", e)))?;

            let outputs = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                decoder.run(ort::inputs![
                    token_input_name.as_str() => input_ids_tensor.into_dyn(),
                    encoder_input_name.as_str() => encoder_tensor.into_dyn()
                ])
            }));

            let outputs = match outputs {
                Ok(Ok(out)) => out,
                Ok(Err(e)) => {
                    crate::log_error!("model", "Decoder error at step {}: {}", step, e);
                    return Err(SttError::InferenceFailed(format!(
                        "Decoder failed at step {}: {}",
                        step, e
                    )));
                }
                Err(panic) => {
                    crate::log_error!("model", "Decoder panic at step {}: {:?}", step, panic);
                    return Err(SttError::InferenceFailed(format!(
                        "Decoder panicked at step {}",
                        step
                    )));
                }
            };
            crate::log_debug!("model", "Decoder step {} complete", step);

            // Extract logits
            let logits = outputs
                .iter()
                .find(|(name, _)| *name == "logits" || *name == "output_0")
                .map(|(_, v)| v)
                .ok_or_else(|| SttError::InferenceFailed("No logits output found".to_string()))?;

            let (logits_shape, logits_data) = logits.try_extract_tensor::<f32>().map_err(|e| {
                SttError::InferenceFailed(format!("Failed to extract logits: {}", e))
            })?;

            // Get the last token's logits: shape is [batch, seq_len, vocab_size]
            let vocab_size = logits_shape.last().copied().unwrap_or(0) as usize;
            let seq_len = if logits_shape.len() >= 2 {
                logits_shape[1] as usize
            } else {
                1
            };

            // Get logits for the last position
            let last_pos_start = (seq_len - 1) * vocab_size;
            let last_logits = &logits_data[last_pos_start..last_pos_start + vocab_size];

            // Greedy decoding: argmax over vocabulary
            let next_token = last_logits
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .map(|(idx, _)| idx as i64)
                .unwrap_or(eot_token_id);

            // Check for end of text
            if next_token == eot_token_id {
                crate::log_debug!("model", "Whisper decoder: EOT at step {}", step);
                break;
            }

            // Add predicted token to sequence
            all_tokens.push(next_token);
            output_tokens.push(next_token);
        }

        crate::log_debug!(
            "model",
            "Whisper decoder: generated {} tokens",
            output_tokens.len()
        );
        Ok(output_tokens)
    }

    /// Run Nemotron (Streaming Transducer) inference pipeline
    /// Uses encoder-decoder-joiner architecture similar to RNN-T
    fn run_nemotron_inference(&mut self, audio: &[f32]) -> Result<String, SttError> {
        // Get vocabulary (Nemotron uses text file vocab like TDT)
        let vocab = self.vocab.as_ref().ok_or(SttError::NotLoaded)?.clone();

        // Get streaming metadata (read from ONNX model at load time)
        let metadata = self
            .streaming_metadata
            .as_ref()
            .ok_or_else(|| {
                SttError::InferenceFailed("No streaming metadata available".to_string())
            })?
            .clone();

        // Get Nemotron mel filterbank (128 bins)
        let mel_filterbank = self
            .nemotron_mel_filterbank
            .as_ref()
            .ok_or(SttError::NotLoaded)?;

        // Step 1: Compute mel spectrogram (128 bins for Nemotron)
        crate::log_debug!("model", "Computing Nemotron mel spectrogram (128 bins)...");
        let mel_spec = compute_mel_spectrogram_generic(audio, mel_filterbank, 128);
        crate::log_debug!("model", "Mel spectrogram shape: {:?}", mel_spec.shape());

        // Use streaming parameters from metadata
        let chunk_size = metadata.window_size;
        let chunk_shift = metadata.chunk_shift;
        let cache_channel_dims = metadata.cache_last_channel_dims;
        let cache_time_dims = metadata.cache_last_time_dims;

        crate::log_debug!(
            "model",
            "Using streaming params: chunk_size={}, chunk_shift={}, cache_channel={:?}, cache_time={:?}",
            chunk_size, chunk_shift, cache_channel_dims, cache_time_dims
        );

        let total_frames = mel_spec.shape()[1];
        crate::log_debug!(
            "model",
            "Processing {} mel frames in chunks of {}",
            total_frames,
            chunk_size
        );

        // Initialize cache tensors using metadata dimensions
        let mut cache_channel = ArrayD::<f32>::zeros(IxDyn(&[
            cache_channel_dims[0],
            cache_channel_dims[1],
            cache_channel_dims[2],
            cache_channel_dims[3],
        ]));
        let mut cache_time = ArrayD::<f32>::zeros(IxDyn(&[
            cache_time_dims[0],
            cache_time_dims[1],
            cache_time_dims[2],
            cache_time_dims[3],
        ]));
        // Cache length starts at 0 (no cached frames yet)
        let mut cache_len_val: i64 = 0;

        // Initialize decoder states for streaming decode
        // LSTM has both hidden state (h) and cell state (c)
        let decoder_hidden_dim = 640;
        let mut decoder_h_state = ArrayD::<f32>::zeros(IxDyn(&[2, 1, decoder_hidden_dim])); // states.1 -> hidden
        let mut decoder_c_state = ArrayD::<f32>::zeros(IxDyn(&[2, 1, decoder_hidden_dim])); // onnx::Slice_3 -> cell

        // Find blank token
        let blank_id = find_token_id(&vocab, "<blk>")
            .or_else(|| find_token_id(&vocab, "<blank>"))
            .unwrap_or((vocab.len() - 1) as i64) as i32;

        crate::log_debug!(
            "model",
            "Blank token ID: {} (vocab size: {})",
            blank_id,
            vocab.len()
        );

        // Collect all tokens from streaming decode
        let mut all_tokens: Vec<i64> = Vec::new();
        let max_tokens = 500;
        let max_symbols_per_step = 10;

        // Initialize decoder output ONCE with BOS token (token 0, not blank)
        // Blank token (1024) is for joiner output only, not decoder input
        // Token 0 (<unk>) often serves as BOS in SentencePiece vocabularies
        let bos_token = 0i32;
        let (decoder_dim, mut current_dec_output) = {
            let decoder = self.decoder_session.as_mut().ok_or(SttError::NotLoaded)?;

            let init_targets = Array2::<i32>::from_elem((1, 1), bos_token);
            let init_target_length = Array1::<i32>::from_elem(1, 1);
            let init_targets_tensor = TensorRef::from_array_view(init_targets.view())
                .map_err(|e| SttError::InferenceFailed(format!("init_targets: {}", e)))?;
            let init_target_length_tensor =
                TensorRef::from_array_view(init_target_length.view())
                    .map_err(|e| SttError::InferenceFailed(format!("init_target_length: {}", e)))?;
            let init_h_tensor = TensorRef::from_array_view(decoder_h_state.view())
                .map_err(|e| SttError::InferenceFailed(format!("init_h: {}", e)))?;
            let init_c_tensor = TensorRef::from_array_view(decoder_c_state.view())
                .map_err(|e| SttError::InferenceFailed(format!("init_c: {}", e)))?;

            let init_dec_outputs = decoder
                .run(ort::inputs![
                    "targets" => init_targets_tensor,
                    "target_length" => init_target_length_tensor,
                    "states.1" => init_h_tensor,
                    "onnx::Slice_3" => init_c_tensor
                ])
                .map_err(|e| SttError::InferenceFailed(format!("Initial decoder failed: {}", e)))?;

            // Extract initial decoder representation
            let init_dec_out = init_dec_outputs.get("outputs").ok_or_else(|| {
                SttError::InferenceFailed("No initial decoder outputs".to_string())
            })?;
            let (init_dec_shape, init_dec_data) =
                init_dec_out.try_extract_tensor::<f32>().map_err(|e| {
                    SttError::InferenceFailed(format!(
                        "Failed to extract initial decoder output: {}",
                        e
                    ))
                })?;

            let dim = init_dec_shape[1] as usize;
            let output: Vec<f32> = init_dec_data.to_vec();

            // Update states from initial decoder run
            if let Some(h) = init_dec_outputs.get("states") {
                if let Ok((shape, data)) = h.try_extract_tensor::<f32>() {
                    if let Ok(arr) = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()) {
                        decoder_h_state = arr;
                    }
                }
            }
            if let Some(c) = init_dec_outputs.get("162") {
                if let Ok((shape, data)) = c.try_extract_tensor::<f32>() {
                    if let Ok(arr) = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()) {
                        decoder_c_state = arr;
                    }
                }
            }

            (dim, output)
        };

        crate::log_debug!(
            "model",
            "Initial decoder output: {} dims (BOS token: {})",
            decoder_dim,
            bos_token
        );

        // Step 2: Process audio in chunks WITH STREAMING DECODE
        crate::log_debug!("model", "Running Nemotron streaming encoder+decoder...");
        let mut offset = 0;
        let mut chunk_idx = 0;
        while offset < total_frames && all_tokens.len() < max_tokens {
            let chunk_end = (offset + chunk_size).min(total_frames);
            let chunk_len = chunk_end - offset;

            // Skip chunks that are too short (encoder needs minimum context)
            // Empty or very short chunks cause "Invalid input shape: {0}" errors
            const MIN_CHUNK_LEN: usize = 16;
            if chunk_len < MIN_CHUNK_LEN {
                crate::log_debug!(
                    "model",
                    "Skipping short final chunk ({} frames < {})",
                    chunk_len,
                    MIN_CHUNK_LEN
                );
                break;
            }

            // Extract chunk: [n_mels, chunk_len]
            let chunk = mel_spec
                .slice(ndarray::s![.., offset..chunk_end])
                .to_owned();
            // Shape: [batch=1, n_mels=128, chunk_len]
            let chunk_input = chunk.insert_axis(Axis(0)).into_dyn();

            let mel_tensor = TensorRef::from_array_view(chunk_input.view()).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create mel tensor: {}", e))
            })?;

            let length = Array1::<i64>::from_vec(vec![chunk_len as i64]);
            let length_tensor = TensorRef::from_array_view(length.view()).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create length tensor: {}", e))
            })?;

            let cache_channel_tensor =
                TensorRef::from_array_view(cache_channel.view()).map_err(|e| {
                    SttError::InferenceFailed(format!(
                        "Failed to create cache_channel tensor: {}",
                        e
                    ))
                })?;

            let cache_time_tensor = TensorRef::from_array_view(cache_time.view()).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create cache_time tensor: {}", e))
            })?;

            let cache_len = Array1::<i64>::from_vec(vec![cache_len_val]);
            let cache_len_tensor = TensorRef::from_array_view(cache_len.view()).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create cache_len tensor: {}", e))
            })?;

            // Run encoder on this chunk
            let encoder = self.encoder_session.as_mut().ok_or(SttError::NotLoaded)?;
            let encoder_outputs = encoder
                .run(ort::inputs![
                    "audio_signal" => mel_tensor,
                    "length" => length_tensor,
                    "cache_last_channel" => cache_channel_tensor,
                    "cache_last_time" => cache_time_tensor,
                    "cache_last_channel_len" => cache_len_tensor
                ])
                .map_err(|e| {
                    SttError::InferenceFailed(format!("Nemotron encoder chunk failed: {}", e))
                })?;

            // Extract encoder output for this chunk
            let enc_out = encoder_outputs.get("outputs").ok_or_else(|| {
                SttError::InferenceFailed("No 'outputs' from encoder".to_string())
            })?;
            let (enc_shape, enc_data) = enc_out.try_extract_tensor::<f32>().map_err(|e| {
                SttError::InferenceFailed(format!("Failed to extract encoder output: {}", e))
            })?;

            // Encoder output shape: [1, 1024, time_frames]
            let encoder_dim = enc_shape[1] as usize;
            let time_frames = enc_shape[2] as usize;

            if chunk_idx == 0 {
                crate::log_debug!(
                    "model",
                    "Chunk {} encoder output: {} frames x {} dim",
                    chunk_idx,
                    time_frames,
                    encoder_dim
                );
            }

            // STREAMING DECODE: Process each encoder frame from this chunk immediately
            let decoder = self.decoder_session.as_mut().ok_or(SttError::NotLoaded)?;
            let joiner = self.joiner_session.as_mut().ok_or(SttError::NotLoaded)?;

            // Skip decoding for chunk 0 - encoder cache isn't populated yet, output is unreliable
            // We still processed chunk 0's encoder to populate the cache for subsequent chunks
            if chunk_idx == 0 {
                crate::log_debug!(
                    "model",
                    "Chunk 0: skipping decode (warmup), cache will be populated"
                );
                // Update encoder cache and continue to next chunk
                let cache_channel_next = encoder_outputs
                    .get("cache_last_channel_next")
                    .ok_or_else(|| {
                        SttError::InferenceFailed("No cache_last_channel_next".to_string())
                    })?;
                let (shape, data) =
                    cache_channel_next
                        .try_extract_tensor::<f32>()
                        .map_err(|e| {
                            SttError::InferenceFailed(format!("Failed to extract cache: {}", e))
                        })?;
                cache_channel =
                    ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()).map_err(|e| {
                        SttError::InferenceFailed(format!("Failed to reshape cache: {}", e))
                    })?;

                let cache_time_next =
                    encoder_outputs.get("cache_last_time_next").ok_or_else(|| {
                        SttError::InferenceFailed("No cache_last_time_next".to_string())
                    })?;
                let (shape, data) = cache_time_next.try_extract_tensor::<f32>().map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to extract cache: {}", e))
                })?;
                cache_time =
                    ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()).map_err(|e| {
                        SttError::InferenceFailed(format!("Failed to reshape cache: {}", e))
                    })?;

                let cache_len_next = encoder_outputs
                    .get("cache_last_channel_next_len")
                    .ok_or_else(|| {
                        SttError::InferenceFailed("No cache_last_channel_next_len".to_string())
                    })?;
                let (_, len_data) = cache_len_next.try_extract_tensor::<i64>().map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to extract cache_len: {}", e))
                })?;
                cache_len_val = len_data[0];

                offset += chunk_shift;
                chunk_idx += 1;
                continue;
            }

            for t in 0..time_frames {
                if all_tokens.len() >= max_tokens {
                    break;
                }

                // Get encoder frame at timestep t: [1, encoder_dim, 1]
                let enc_frame_data: Vec<f32> = (0..encoder_dim)
                    .map(|f| enc_data[f * time_frames + t])
                    .collect();
                let enc_frame = ArrayD::from_shape_vec(IxDyn(&[1, encoder_dim, 1]), enc_frame_data)
                    .map_err(|e| SttError::InferenceFailed(format!("enc_frame: {}", e)))?;

                // Inner loop: emit symbols until blank
                // Use CACHED decoder output - only run decoder after emitting non-blank
                let mut symbols_emitted = 0;
                loop {
                    if symbols_emitted >= max_symbols_per_step || all_tokens.len() >= max_tokens {
                        break;
                    }

                    // Build decoder frame from cached output [1, decoder_dim, 1]
                    let dec_frame = ArrayD::from_shape_vec(
                        IxDyn(&[1, decoder_dim, 1]),
                        current_dec_output.clone(),
                    )
                    .map_err(|e| SttError::InferenceFailed(format!("dec_frame: {}", e)))?;

                    // Run joiner with encoder frame + cached decoder output
                    let enc_tensor = TensorRef::from_array_view(enc_frame.view())
                        .map_err(|e| SttError::InferenceFailed(format!("enc_tensor: {}", e)))?;
                    let dec_tensor = TensorRef::from_array_view(dec_frame.view())
                        .map_err(|e| SttError::InferenceFailed(format!("dec_tensor: {}", e)))?;

                    let joiner_outputs = joiner
                        .run(ort::inputs![
                            "encoder_outputs" => enc_tensor,
                            "decoder_outputs" => dec_tensor
                        ])
                        .map_err(|e| SttError::InferenceFailed(format!("Joiner failed: {}", e)))?;

                    // Extract logits
                    let logits = joiner_outputs
                        .iter()
                        .next()
                        .ok_or_else(|| SttError::InferenceFailed("No joiner output".to_string()))?;
                    let (_, logits_data) = logits.1.try_extract_tensor::<f32>().map_err(|e| {
                        SttError::InferenceFailed(format!("Failed to extract logits: {}", e))
                    })?;

                    // Greedy decode - find top tokens for debugging
                    let mut indexed: Vec<(usize, f32)> = logits_data
                        .iter()
                        .enumerate()
                        .map(|(i, &v)| (i, v))
                        .collect();
                    indexed
                        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

                    let next_token = indexed[0].0 as i64;

                    // Log top 3 tokens on first few frames for debugging
                    if chunk_idx == 0 && t < 3 {
                        let top3: Vec<String> = indexed
                            .iter()
                            .take(3)
                            .map(|(idx, score)| {
                                let tok = vocab.get(*idx).map(|s| s.as_str()).unwrap_or("?");
                                format!("{}:'{}' ({:.3})", idx, tok, score)
                            })
                            .collect();
                        crate::log_debug!("model", "Frame {} top tokens: {}", t, top3.join(", "));
                    }

                    // If blank, move to next encoder frame (keep decoder state unchanged)
                    if next_token == blank_id as i64 {
                        break;
                    }

                    // Emit token
                    if all_tokens.len() < 20 {
                        let token_str = vocab
                            .get(next_token as usize)
                            .map(|s| s.as_str())
                            .unwrap_or("?");
                        crate::log_debug!(
                            "model",
                            "Emit token {}: '{}' at chunk {} frame {}",
                            next_token,
                            token_str,
                            chunk_idx,
                            t
                        );
                    }
                    all_tokens.push(next_token);
                    symbols_emitted += 1;

                    // NOW run decoder with the newly emitted token to update decoder output
                    let new_targets = Array2::<i32>::from_elem((1, 1), next_token as i32);
                    let new_target_length = Array1::<i32>::from_elem(1, 1);

                    let new_targets_tensor = TensorRef::from_array_view(new_targets.view())
                        .map_err(|e| SttError::InferenceFailed(format!("new_targets: {}", e)))?;
                    let new_target_length_tensor =
                        TensorRef::from_array_view(new_target_length.view()).map_err(|e| {
                            SttError::InferenceFailed(format!("new_target_length: {}", e))
                        })?;
                    let h_tensor = TensorRef::from_array_view(decoder_h_state.view())
                        .map_err(|e| SttError::InferenceFailed(format!("h_state: {}", e)))?;
                    let c_tensor = TensorRef::from_array_view(decoder_c_state.view())
                        .map_err(|e| SttError::InferenceFailed(format!("c_state: {}", e)))?;

                    let decoder_outputs = decoder
                        .run(ort::inputs![
                            "targets" => new_targets_tensor,
                            "target_length" => new_target_length_tensor,
                            "states.1" => h_tensor,
                            "onnx::Slice_3" => c_tensor
                        ])
                        .map_err(|e| {
                            SttError::InferenceFailed(format!("Decoder update failed: {}", e))
                        })?;

                    // Update cached decoder output
                    if let Some(dec_out) = decoder_outputs.get("outputs") {
                        if let Ok((_, data)) = dec_out.try_extract_tensor::<f32>() {
                            current_dec_output = data.to_vec();
                        }
                    }

                    // Update decoder states
                    if let Some(h) = decoder_outputs.get("states") {
                        if let Ok((shape, data)) = h.try_extract_tensor::<f32>() {
                            if let Ok(arr) = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
                            {
                                decoder_h_state = arr;
                            }
                        }
                    }
                    if let Some(c) = decoder_outputs.get("162") {
                        if let Ok((shape, data)) = c.try_extract_tensor::<f32>() {
                            if let Ok(arr) = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
                            {
                                decoder_c_state = arr;
                            }
                        }
                    }
                }
            }

            // Update encoder cache for next chunk
            let cache_channel_next =
                encoder_outputs
                    .get("cache_last_channel_next")
                    .ok_or_else(|| {
                        SttError::InferenceFailed("No cache_last_channel_next".to_string())
                    })?;
            let (shape, data) = cache_channel_next
                .try_extract_tensor::<f32>()
                .map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to extract cache: {}", e))
                })?;
            cache_channel =
                ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()).map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to reshape cache: {}", e))
                })?;

            let cache_time_next = encoder_outputs
                .get("cache_last_time_next")
                .ok_or_else(|| SttError::InferenceFailed("No cache_last_time_next".to_string()))?;
            let (shape, data) = cache_time_next.try_extract_tensor::<f32>().map_err(|e| {
                SttError::InferenceFailed(format!("Failed to extract cache: {}", e))
            })?;
            cache_time = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to reshape cache: {}", e))
            })?;

            let cache_len_next = encoder_outputs
                .get("cache_last_channel_next_len")
                .ok_or_else(|| {
                    SttError::InferenceFailed("No cache_last_channel_next_len".to_string())
                })?;
            let (_, len_data) = cache_len_next.try_extract_tensor::<i64>().map_err(|e| {
                SttError::InferenceFailed(format!("Failed to extract cache_len: {}", e))
            })?;
            cache_len_val = len_data[0];

            // Move offset by chunk_shift
            offset += chunk_shift;
            chunk_idx += 1;
        }

        crate::log_debug!(
            "model",
            "Streaming decode complete: {} tokens from {} chunks",
            all_tokens.len(),
            chunk_idx
        );

        let tokens = all_tokens;

        // Step 4: Decode tokens to text
        let text = decode_tokens(&tokens, &vocab);

        Ok(text)
    }

    /// Streaming Transducer decoding (RNN-T style)
    /// Uses the Nemotron decoder with named inputs:
    /// - targets: Int32 [batch, seq]
    /// - target_length: Int32 [batch]
    /// - states.1: Float32 [2, batch, 640]
    /// - onnx::Slice_3: Float32 [2, 1, 640]
    fn streaming_transducer_decode(
        &mut self,
        encoder_output: &ArrayD<f32>,
        vocab: &[String],
    ) -> Result<Vec<i64>, SttError> {
        let decoder = self.decoder_session.as_mut().ok_or(SttError::NotLoaded)?;
        let joiner = self.joiner_session.as_mut().ok_or(SttError::NotLoaded)?;

        // Find blank token - in SentencePiece vocab, it's often the last token
        let blank_id = find_token_id(vocab, "<blk>")
            .or_else(|| find_token_id(vocab, "<blank>"))
            .or_else(|| find_token_id(vocab, "▁")) // Word boundary often used as blank
            .unwrap_or((vocab.len() - 1) as i64) as i32; // Default to last token

        crate::log_debug!(
            "model",
            "Blank token ID: {} (vocab size: {})",
            blank_id,
            vocab.len()
        );

        let mut output_tokens: Vec<i64> = Vec::new();
        let max_tokens = 500;
        let max_symbols_per_step = 10; // Max symbols to emit per encoder frame

        // Encoder output shape: [batch=1, features=1024, time]
        let enc_shape = encoder_output.shape();
        let time_steps = enc_shape[2]; // time dimension
        let encoder_dim = enc_shape[1]; // 1024

        crate::log_debug!(
            "model",
            "Transducer decode: {} time steps, {} encoder dim",
            time_steps,
            encoder_dim
        );

        // Initialize decoder state: [2, batch=1, 640]
        let decoder_hidden_dim = 640;
        let mut decoder_state = ArrayD::<f32>::zeros(IxDyn(&[2, 1, decoder_hidden_dim]));
        let slice_state = ArrayD::<f32>::zeros(IxDyn(&[2, 1, decoder_hidden_dim]));

        // Process each encoder timestep
        for t in 0..time_steps {
            if output_tokens.len() >= max_tokens {
                break;
            }

            // Get encoder frame at timestep t: [1, encoder_dim, 1] (joiner expects 3D)
            let encoder_frame: Vec<f32> = (0..encoder_dim)
                .map(|f| encoder_output[[0, f, t]])
                .collect();
            let enc_frame_arr = ArrayD::from_shape_vec(IxDyn(&[1, encoder_dim, 1]), encoder_frame)
                .map_err(|e| SttError::InferenceFailed(format!("enc_frame: {}", e)))?;

            // Inner loop: emit symbols until blank
            let mut symbols_emitted = 0;
            loop {
                if symbols_emitted >= max_symbols_per_step {
                    break;
                }

                // Prepare decoder inputs
                // targets: the last emitted token (or 0/BOS for start - NOT blank)
                // In RNN-T, blank means "no output", BOS (usually 0) is the start token
                let prev_token = output_tokens.last().map(|&t| t as i32).unwrap_or(0);
                let targets = Array2::<i32>::from_elem((1, 1), prev_token);
                let target_length = Array1::<i32>::from_elem(1, 1);

                let targets_tensor = TensorRef::from_array_view(targets.view())
                    .map_err(|e| SttError::InferenceFailed(format!("targets: {}", e)))?;
                let target_length_tensor = TensorRef::from_array_view(target_length.view())
                    .map_err(|e| SttError::InferenceFailed(format!("target_length: {}", e)))?;
                let states_tensor = TensorRef::from_array_view(decoder_state.view())
                    .map_err(|e| SttError::InferenceFailed(format!("states: {}", e)))?;
                let slice_tensor = TensorRef::from_array_view(slice_state.view())
                    .map_err(|e| SttError::InferenceFailed(format!("slice: {}", e)))?;

                // Run decoder
                let decoder_outputs = decoder
                    .run(ort::inputs![
                        "targets" => targets_tensor,
                        "target_length" => target_length_tensor,
                        "states.1" => states_tensor,
                        "onnx::Slice_3" => slice_tensor
                    ])
                    .map_err(|e| SttError::InferenceFailed(format!("Decoder failed: {}", e)))?;

                // Extract decoder output: 'outputs' shape [1, 640, 1]
                let dec_out = decoder_outputs
                    .get("outputs")
                    .ok_or_else(|| SttError::InferenceFailed("No decoder outputs".to_string()))?;
                let (dec_shape, dec_data) = dec_out.try_extract_tensor::<f32>().map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to extract decoder output: {}", e))
                })?;

                // Reshape decoder output for joiner: [1, 640, 1] (joiner expects 3D)
                let dec_dim = dec_shape[1] as usize;
                let dec_frame: Vec<f32> = (0..dec_dim).map(|d| dec_data[d]).collect();
                let dec_frame_arr = ArrayD::from_shape_vec(IxDyn(&[1, dec_dim, 1]), dec_frame)
                    .map_err(|e| SttError::InferenceFailed(format!("dec_frame: {}", e)))?;

                // Update decoder state from 'states' output (or '162' as fallback)
                let state_updated = if let Some(new_state) = decoder_outputs.get("states") {
                    if let Ok((shape, data)) = new_state.try_extract_tensor::<f32>() {
                        if let Ok(arr) = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()) {
                            decoder_state = arr;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else if let Some(new_state) = decoder_outputs.get("162") {
                    if let Ok((shape, data)) = new_state.try_extract_tensor::<f32>() {
                        if let Ok(arr) = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()) {
                            decoder_state = arr;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                if t == 0 && symbols_emitted == 0 && !state_updated {
                    crate::log_warn!("model", "Decoder state not updated!");
                }

                // Run joiner with encoder and decoder outputs (both 3D: [1, dim, 1])
                let enc_tensor = TensorRef::from_array_view(enc_frame_arr.view())
                    .map_err(|e| SttError::InferenceFailed(format!("enc_tensor: {}", e)))?;
                let dec_tensor = TensorRef::from_array_view(dec_frame_arr.view())
                    .map_err(|e| SttError::InferenceFailed(format!("dec_tensor: {}", e)))?;

                let joiner_outputs = joiner
                    .run(ort::inputs![
                        "encoder_outputs" => enc_tensor,
                        "decoder_outputs" => dec_tensor
                    ])
                    .map_err(|e| SttError::InferenceFailed(format!("Joiner failed: {}", e)))?;

                // Extract logits from joiner (first output)
                let logits = joiner_outputs
                    .iter()
                    .next()
                    .ok_or_else(|| SttError::InferenceFailed("No joiner output".to_string()))?;

                let (logits_shape, logits_data) =
                    logits.1.try_extract_tensor::<f32>().map_err(|e| {
                        SttError::InferenceFailed(format!("Failed to extract logits: {}", e))
                    })?;

                // Log joiner output shape on first iteration
                if t == 0 && symbols_emitted == 0 {
                    crate::log_debug!(
                        "model",
                        "Joiner output shape: {:?}, {} logits",
                        logits_shape,
                        logits_data.len()
                    );
                    // Log top 5 logits
                    let mut indexed: Vec<(usize, f32)> =
                        logits_data.iter().cloned().enumerate().collect();
                    indexed
                        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                    let top5: Vec<_> = indexed.iter().take(5).collect();
                    crate::log_debug!("model", "Top 5 logits: {:?}", top5);
                }

                // Greedy decode: argmax
                let next_token = logits_data
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(idx, _)| idx as i64)
                    .unwrap_or(blank_id as i64);

                // If blank, move to next encoder frame
                if next_token == blank_id as i64 {
                    break;
                }

                // Log emitted tokens
                if output_tokens.len() < 20 {
                    let token_str = vocab
                        .get(next_token as usize)
                        .map(|s| s.as_str())
                        .unwrap_or("?");
                    crate::log_debug!(
                        "model",
                        "Emit token {}: '{}' at frame {}",
                        next_token,
                        token_str,
                        t
                    );
                }

                // Emit non-blank token
                output_tokens.push(next_token);
                symbols_emitted += 1;
            }
        }

        crate::log_debug!(
            "model",
            "Nemotron decoder: generated {} tokens",
            output_tokens.len()
        );
        Ok(output_tokens)
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
    let (encoded_shape_obj, encoded_data) = encoded.try_extract_tensor::<f32>().map_err(|e| {
        SttError::InferenceFailed(format!("Failed to extract encoder output: {}", e))
    })?;

    let encoded_shape: Vec<usize> = encoded_shape_obj.iter().map(|&d| d as usize).collect();
    let encoded_view = ArrayD::from_shape_vec(IxDyn(&encoded_shape), encoded_data.to_vec())
        .map_err(|e| {
            SttError::InferenceFailed(format!("Failed to reshape encoder output: {}", e))
        })?;
    crate::log_debug!("model", "Encoder output shape: {:?}", encoded_shape);

    // Encoder output is [batch, features, time] = [1, 1024, T]
    // Need to transpose to [batch, time, features] = [1, T, 1024]
    let encoder_time = encoded_shape[2];
    crate::log_debug!("model", "Encoder time steps: {}", encoder_time);

    // Transpose encoder output: [1, 1024, T] -> [1, T, 1024]
    let encoded_transposed = encoded_view.permuted_axes(IxDyn(&[0, 2, 1]));
    crate::log_debug!(
        "model",
        "Transposed encoder shape: {:?}",
        encoded_transposed.shape()
    );

    // Run TDT decoding (per-timestep)
    let tokens = tdt_decode_per_step(
        decoder,
        &encoded_transposed,
        encoder_time,
        blank_id,
        vocab_len,
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

    crate::log_debug!(
        "model",
        "TDT per-step decoding: {} encoder steps, vocab_len={}, blank_id={}",
        encoder_time,
        vocab_len,
        blank_id
    );

    while t < encoder_time && tokens.len() < max_total_tokens {
        let mut symbols_this_step = 0;

        loop {
            // Get single encoder frame: encoded[0, t, :] -> shape [1, features]
            // Then reshape to [1, features, 1] for decoder (adding time dim)
            let encoder_frame: Vec<f32> = (0..encoded.shape()[2])
                .map(|f| encoded[[0, t, f]])
                .collect();

            // Create encoder_outputs with shape [1, features, 1]
            let encoder_out =
                Array3::<f32>::from_shape_vec((1, encoder_frame.len(), 1), encoder_frame).map_err(
                    |e| {
                        SttError::InferenceFailed(format!("Failed to reshape encoder frame: {}", e))
                    },
                )?;

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
                    let (shape, data) = value
                        .try_extract_tensor::<f32>()
                        .map_err(|e| SttError::InferenceFailed(format!("outputs: {}", e)))?;
                    logits_opt = Some(
                        ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()).map_err(|e| {
                            SttError::InferenceFailed(format!("reshape outputs: {}", e))
                        })?,
                    );
                } else if name == "output_states_1" {
                    let (shape, data) = value
                        .try_extract_tensor::<f32>()
                        .map_err(|e| SttError::InferenceFailed(format!("state1: {}", e)))?;
                    new_state_1 = Some(
                        ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()).map_err(|e| {
                            SttError::InferenceFailed(format!("reshape state1: {}", e))
                        })?,
                    );
                } else if name == "output_states_2" {
                    let (shape, data) = value
                        .try_extract_tensor::<f32>()
                        .map_err(|e| SttError::InferenceFailed(format!("state2: {}", e)))?;
                    new_state_2 = Some(
                        ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()).map_err(|e| {
                            SttError::InferenceFailed(format!("reshape state2: {}", e))
                        })?,
                    );
                }
            }

            let logits = logits_opt
                .ok_or_else(|| SttError::InferenceFailed("Missing outputs".to_string()))?;
            let ns1 = new_state_1
                .ok_or_else(|| SttError::InferenceFailed("Missing state1".to_string()))?;
            let ns2 = new_state_2
                .ok_or_else(|| SttError::InferenceFailed("Missing state2".to_string()))?;

            // Squeeze the output - output shape is typically [1, 1, 1, vocab+durations]
            let flat_logits: Vec<f32> = logits.iter().cloned().collect();
            let total_size = flat_logits.len();

            if t < 3 {
                crate::log_debug!(
                    "model",
                    "t={}: logits size={}, shape={:?}",
                    t,
                    total_size,
                    logits.shape()
                );
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
            let (best_vocab_idx, best_vocab_val) = vocab_logits
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or((0, &f32::NEG_INFINITY));

            // Find best duration (argmax over duration logits)
            // Duration values: 0=stay, 1=+1, 2=+2, etc.
            let best_duration = if !duration_logits.is_empty() {
                duration_logits
                    .iter()
                    .enumerate()
                    .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|(idx, _)| idx)
                    .unwrap_or(1)
            } else {
                1 // Default: advance by 1 if no duration output
            };

            if t < 5 || symbols_this_step == 0 {
                // Debug: show top vocab tokens and duration logits
                let mut top_vocab: Vec<(usize, f32)> =
                    vocab_logits.iter().cloned().enumerate().collect();
                top_vocab
                    .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                top_vocab.truncate(5);
                let top_str: Vec<String> = top_vocab
                    .iter()
                    .map(|(i, v)| format!("{}:{:.2}", i, v))
                    .collect();
                let dur_str: String = duration_logits
                    .iter()
                    .map(|v| format!("{:.2}", v))
                    .collect::<Vec<_>>()
                    .join(",");
                crate::log_debug!(
                    "model",
                    "t={}, sym={}: best_vocab={}({:.2}), duration={}, top5=[{}], dur_logits=[{}]",
                    t,
                    symbols_this_step,
                    best_vocab_idx,
                    best_vocab_val,
                    best_duration,
                    top_str.join(", "),
                    dur_str
                );
            }

            // Check if blank token wins
            if best_vocab_idx as i64 == blank_id {
                // Blank predicted - don't emit token, DON'T update state
                // Always advance by 1 frame when blank is predicted
                // (duration is only used for non-blank emissions)
                t += 1;
                break; // Exit inner loop, move to next encoder frame
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
                    let prev_pattern = &recent_tokens
                        [recent_tokens.len() - 2 * pattern_len..recent_tokens.len() - pattern_len];
                    if pattern == prev_pattern {
                        crate::log_debug!(
                            "model",
                            "Loop detected at t={}: pattern {:?} repeating, breaking out",
                            t,
                            pattern
                        );
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
    if let Ok((_shape_obj, data)) = output.try_extract_tensor::<i64>() {
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
    let decoder_inputs = build_decoder_inputs(
        decoder,
        encoded,
        encoder_len,
        target_token,
        state_1,
        state_2,
    )?;

    let decoder_outputs = decoder
        .run(decoder_inputs)
        .map_err(|e| SttError::InferenceFailed(format!("Decoder inference failed: {}", e)))?;

    let output = find_decoder_output(&decoder_outputs, "outputs")?;
    let (shape, data) = output.try_extract_tensor::<f32>().map_err(|e| {
        SttError::InferenceFailed(format!("Failed to extract decoder output: {}", e))
    })?;
    let output_array = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec()).map_err(|e| {
        SttError::InferenceFailed(format!("Failed to reshape decoder output: {}", e))
    })?;
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
        .ok_or_else(|| SttError::InferenceFailed(format!("Decoder output '{}' not found", name)))
}

fn extract_decoder_state(outputs: &SessionOutputs, name: &str) -> Result<ArrayD<f32>, SttError> {
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

    encoded_shape.iter().skip(1).copied().min().unwrap_or(0)
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

        let ValueType::Tensor { ty, shape, .. } = input_dtype else {
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
            crate::log_debug!(
                "model",
                "Decoder input '{}' uses encoder output",
                input_name
            );
            // Clone the encoder output to create an owned tensor
            let tensor = Tensor::from_array(encoded.to_owned()).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to build encoder input: {}", e))
            })?;
            tensor.into_dyn()
        } else if is_length_input(&name_lower) {
            if name_lower.contains("target") {
                crate::log_debug!("model", "Decoder input '{}' uses target length", input_name);
                build_length_tensor(ty.clone(), &dimensions, target_len)?
            } else {
                crate::log_debug!(
                    "model",
                    "Decoder input '{}' uses encoder length",
                    input_name
                );
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
                let tensor = Tensor::from_array(state.to_owned()).map_err(|e| {
                    SttError::InferenceFailed(format!("Failed to build decoder state input: {}", e))
                })?;
                tensor.into_dyn()
            } else {
                let dims = resolve_dims(
                    &dimensions,
                    &dimension_symbols,
                    encoded.shape(),
                    encoder_len as usize,
                );
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
            let dims = resolve_dims(
                &dimensions,
                &dimension_symbols,
                encoded.shape(),
                encoder_len as usize,
            );
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
            let tensor = Tensor::from_array(arr).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create f32 tensor: {}", e))
            })?;
            Ok(tensor.into_dyn())
        }
        TensorElementType::Int64 => {
            let arr = ArrayD::<i64>::zeros(IxDyn(dims));
            let tensor = Tensor::from_array(arr).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create i64 tensor: {}", e))
            })?;
            Ok(tensor.into_dyn())
        }
        TensorElementType::Int32 => {
            let arr = ArrayD::<i32>::zeros(IxDyn(dims));
            let tensor = Tensor::from_array(arr).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create i32 tensor: {}", e))
            })?;
            Ok(tensor.into_dyn())
        }
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
            let tensor = Tensor::from_array(arr).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create token tensor: {}", e))
            })?;
            Ok(tensor.into_dyn())
        }
        TensorElementType::Int32 => {
            let arr = ArrayD::<i32>::from_elem(IxDyn(&dims), value as i32);
            let tensor = Tensor::from_array(arr).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create token tensor: {}", e))
            })?;
            Ok(tensor.into_dyn())
        }
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
            let tensor = Tensor::from_array(arr).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create length tensor: {}", e))
            })?;
            Ok(tensor.into_dyn())
        }
        TensorElementType::Int32 => {
            let arr = ArrayD::<i32>::from_elem(IxDyn(&dims), value as i32);
            let tensor = Tensor::from_array(arr).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create length tensor: {}", e))
            })?;
            Ok(tensor.into_dyn())
        }
        TensorElementType::Float32 => {
            let arr = ArrayD::<f32>::from_elem(IxDyn(&dims), value as f32);
            let tensor = Tensor::from_array(arr).map_err(|e| {
                SttError::InferenceFailed(format!("Failed to create length tensor: {}", e))
            })?;
            Ok(tensor.into_dyn())
        }
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

        let symbol = symbols
            .get(idx)
            .and_then(|s| s.as_ref())
            .map(|s| s.to_lowercase());
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

    text.trim().to_string()
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

/// Create Whisper-specific mel filterbank (128 bins, 80Hz-7600Hz range)
fn create_whisper_mel_filterbank() -> Array2<f32> {
    // Whisper v3 uses 128 mel bins (v1/v2 used 80)
    // FFT size 400, sample rate 16000
    // Frequency range: 0 Hz to 8000 Hz (Nyquist for 16kHz)
    create_mel_filterbank(
        WHISPER_N_FFT,
        WHISPER_N_MELS,
        WHISPER_SAMPLE_RATE,
        0.0,
        8000.0,
    )
}

/// Compute Whisper-style mel spectrogram from audio
fn compute_whisper_mel_spectrogram(audio: &[f32], mel_filterbank: &Array2<f32>) -> Array2<f32> {
    let n_fft = WHISPER_N_FFT;
    let hop_length = WHISPER_HOP_LENGTH;

    // Whisper expects exactly 30 seconds of audio (padded or truncated)
    let n_samples = WHISPER_N_SAMPLES;
    let mut audio_chunk = vec![0.0f32; n_samples];
    let copy_len = audio.len().min(n_samples);
    audio_chunk[..copy_len].copy_from_slice(&audio[..copy_len]);

    // Center-pad audio with n_fft // 2 on each side (this matches PyTorch's stft behavior)
    let pad_size = n_fft / 2;
    let mut padded_audio = vec![0.0f32; pad_size];
    padded_audio.extend_from_slice(&audio_chunk);
    padded_audio.extend(vec![0.0f32; pad_size]);

    // Create Hann window
    let window: Vec<f32> = (0..n_fft)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / n_fft as f32).cos()))
        .collect();

    // Calculate number of frames (with center padding, this gives exactly n_samples / hop_length = 3000)
    let n_frames = n_samples / hop_length;

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
                let sample = if start + i < padded_audio.len() {
                    padded_audio[start + i] * window[i]
                } else {
                    0.0
                };
                Complex::new(sample, 0.0)
            })
            .collect();

        // Apply FFT
        fft.process(&mut buffer);

        // Compute magnitude squared
        for (k, &val) in buffer.iter().take(n_freqs).enumerate() {
            power_spec[[k, frame_idx]] = val.norm_sqr();
        }
    }

    // Apply mel filterbank
    let mel_spec = mel_filterbank.dot(&power_spec);

    // Apply log10 transform with clamping (Whisper style)
    // log_spec = torch.clamp(mel_spec, min=1e-10).log10()
    // log_spec = torch.maximum(log_spec, log_spec.max() - 8.0)
    // log_spec = (log_spec + 4.0) / 4.0
    let log_mel_spec = mel_spec.mapv(|x| x.max(1e-10).log10());

    // Find max for clamping
    let max_val = log_mel_spec
        .iter()
        .cloned()
        .fold(f32::NEG_INFINITY, f32::max);
    let clamped = log_mel_spec.mapv(|x| x.max(max_val - 8.0));

    // Normalize to [-1, 1] range
    let normalized = clamped.mapv(|x| (x + 4.0) / 4.0);

    normalized
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

/// Compute mel spectrogram with configurable number of mel bins (for Nemotron)
fn compute_mel_spectrogram_generic(
    audio: &[f32],
    mel_filterbank: &Array2<f32>,
    n_mels: usize,
) -> Array2<f32> {
    let n_fft = N_FFT;
    let hop_length = HOP_LENGTH;
    let win_length = WIN_LENGTH;

    // Pre-emphasis filter
    let preemph = 0.97f32;
    let mut preemphasized: Vec<f32> = Vec::with_capacity(audio.len());
    for i in 0..audio.len() {
        if i == 0 {
            preemphasized.push(audio[i]);
        } else {
            preemphasized.push(audio[i] - preemph * audio[i - 1]);
        }
    }

    // Add dither
    let dither = 1e-5f32;
    for sample in &mut preemphasized {
        *sample += dither * (rand_simple() * 2.0 - 1.0);
    }

    // Create Hann window
    let window: Vec<f32> = (0..win_length)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / win_length as f32).cos()))
        .collect();

    // Pad audio
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

        fft.process(&mut buffer);

        for (k, &val) in buffer.iter().take(n_freqs).enumerate() {
            power_spec[[k, frame_idx]] = val.norm_sqr();
        }
    }

    // Apply mel filterbank (use provided filterbank, assumes correct n_mels)
    let _ = n_mels; // Used to indicate expected mel bins, filterbank determines actual
    let mel_spec = mel_filterbank.dot(&power_spec);

    // Convert to log scale (use log10 for NeMo/Nemotron compatibility)
    // NeMo models expect raw log mel without per-feature normalization
    let log_mel_spec = mel_spec.mapv(|x| (x.max(1e-10)).log10());

    log_mel_spec
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
    use crate::models::{
        get_model_path, get_model_paths as get_registry_paths, normalize_model_name,
    };

    // First try to get paths from the model registry
    let model_id = normalize_model_name(model_name);

    if let Some(model_paths) = get_registry_paths(model_id) {
        // Check if required files exist based on architecture
        let files_exist = match model_paths.architecture {
            ModelArchitecture::TdtTransducer | ModelArchitecture::EncoderDecoder => {
                model_paths.encoder_path.exists()
                    && model_paths.decoder_path.exists()
                    && model_paths.vocab_path.exists()
            }
            ModelArchitecture::StreamingTransducer => {
                model_paths.encoder_path.exists()
                    && model_paths.decoder_path.exists()
                    && model_paths
                        .joiner_path
                        .as_ref()
                        .map_or(false, |p| p.exists())
                    && model_paths.vocab_path.exists()
            }
        };

        if files_exist {
            return Some(SttConfig {
                encoder_path: model_paths.encoder_path,
                decoder_path: model_paths.decoder_path,
                joiner_path: model_paths.joiner_path,
                embeddings_path: model_paths.embeddings_path,
                vocab_path: model_paths.vocab_path,
                timeout_ms: 30000,
                architecture: model_paths.architecture,
            });
        }
    }

    // Fallback: Legacy path resolution for unknown models
    let models_dir = crate::config::get_models_dir();
    let model_dir = models_dir.join(model_name);

    if !model_dir.exists() {
        // Also try with normalized name
        let model_dir_normalized = get_model_path(model_id);
        if !model_dir_normalized.exists() {
            return None;
        }
        return get_model_paths_legacy(&model_dir_normalized);
    }

    get_model_paths_legacy(&model_dir)
}

/// Legacy model path resolution for backward compatibility
fn get_model_paths_legacy(model_dir: &std::path::Path) -> Option<SttConfig> {
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
        joiner_path: None,
        embeddings_path: None,
        vocab_path: vocab,
        timeout_ms: 30000,
        architecture: ModelArchitecture::TdtTransducer, // Legacy models are always TDT
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
        let filterbank = create_mel_filterbank(512, 128, 16000, 0.0, 8000.0);
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
        assert_eq!(text, "hello world");
    }

    #[test]
    fn test_remove_consecutive_duplicates() {
        let tokens = vec![1, 1, 2, 2, 2, 3, 1, 1];
        let result = remove_consecutive_duplicates(&tokens);
        assert_eq!(result, vec![1, 2, 3, 1]);
    }
}
