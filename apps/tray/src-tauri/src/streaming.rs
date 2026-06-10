//! Real-time streaming transcription for Nemotron model
//!
//! Processes audio chunks incrementally during recording and emits partial
//! transcription results as text is decoded.

use ndarray::{Array1, Array2, ArrayD, Axis, IxDyn};
use ort::value::TensorRef;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use crate::audio::resample_linear;
use crate::stt::{StreamingMetadata, SttEngine, SttError};

/// Audio processing constants (must match stt.rs)
const SAMPLE_RATE: u32 = 16000;
const HOP_LENGTH: usize = 160;
const WIN_LENGTH: usize = 400;
const N_MELS: usize = 128;
const MAX_CHUNKS_PER_INCREMENTAL_CALL: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct StreamingStageTimings {
    pub feature_ms: f64,
    pub encoder_ms: f64,
    pub decoder_ms: f64,
    pub joiner_ms: f64,
    pub total_ms: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StreamingMetrics {
    pub chunk_index: usize,
    pub available_audio_ms: u64,
    pub processed_audio_ms: u64,
    pub backlog_ms: u64,
    pub tokens_emitted: usize,
    pub partial_chars: usize,
    pub feature_ms: f64,
    pub encoder_ms: f64,
    pub decoder_ms: f64,
    pub joiner_ms: f64,
    pub total_ms: f64,
}

impl StreamingMetrics {
    pub fn new(
        chunk_index: usize,
        total_samples: usize,
        processed_samples: usize,
        source_sample_rate: u32,
        tokens_before: usize,
        tokens_after: usize,
        partial_chars: usize,
        timings: StreamingStageTimings,
    ) -> Self {
        let processed_samples = processed_samples.min(total_samples);

        Self {
            chunk_index,
            available_audio_ms: samples_to_ms(total_samples, source_sample_rate),
            processed_audio_ms: samples_to_ms(processed_samples, source_sample_rate),
            backlog_ms: samples_to_ms(
                total_samples.saturating_sub(processed_samples),
                source_sample_rate,
            ),
            tokens_emitted: tokens_after.saturating_sub(tokens_before),
            partial_chars,
            feature_ms: timings.feature_ms,
            encoder_ms: timings.encoder_ms,
            decoder_ms: timings.decoder_ms,
            joiner_ms: timings.joiner_ms,
            total_ms: timings.total_ms,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct StreamingProcessResult {
    pub partial_text: Option<String>,
    pub chunks_processed: usize,
    pub metrics: Vec<StreamingMetrics>,
}

enum IncrementalChunkResult {
    Processed {
        partial_text: Option<String>,
        metrics: StreamingMetrics,
    },
    NoChunkReady,
}

/// Streaming transcription state
/// Holds all LSTM cache state needed between inference calls
pub struct StreamingState {
    /// Encoder cache (channel dimension)
    pub cache_channel: ArrayD<f32>,
    /// Encoder cache (time dimension)
    pub cache_time: ArrayD<f32>,
    /// Encoder cache length counter
    pub cache_len: i64,
    /// Decoder hidden state (LSTM h)
    pub decoder_h_state: ArrayD<f32>,
    /// Decoder cell state (LSTM c)
    pub decoder_c_state: ArrayD<f32>,
    /// Cached decoder output (reused when blank emitted)
    pub decoder_output: Vec<f32>,
    /// Accumulated tokens from all chunks
    pub tokens: Vec<i64>,
    /// Number of chunks processed
    pub chunk_count: usize,
    /// Samples already processed from audio buffer (in source sample rate)
    pub processed_samples: usize,
    /// Source sample rate of the audio buffer (may differ from target 16kHz)
    pub source_sample_rate: u32,
    /// Streaming metadata from model
    pub metadata: StreamingMetadata,
    /// Vocabulary for token decoding
    pub vocab: Vec<String>,
    /// Mel filterbank for spectrogram computation
    pub mel_filterbank: Array2<f32>,
    /// Blank token ID
    pub blank_id: i32,
    /// Decoder hidden dimension
    pub decoder_dim: usize,
}

impl StreamingState {
    /// Initialize streaming state from STT engine
    /// Returns None if engine is not ready or not using Nemotron model
    ///
    /// # Arguments
    /// * `engine` - The STT engine to initialize from
    /// * `source_sample_rate` - The sample rate of the audio buffer (will be resampled to 16kHz if different)
    pub fn from_engine(engine: &mut SttEngine, source_sample_rate: u32) -> Option<Self> {
        use crate::models::ModelArchitecture;

        // Check if engine is ready and using streaming transducer
        if !engine.is_ready() {
            crate::log_debug!("streaming", "Engine not ready");
            return None;
        }

        if engine.get_architecture() != ModelArchitecture::StreamingTransducer {
            crate::log_debug!("streaming", "Not a streaming model");
            return None;
        }

        // Get required components from engine
        let metadata = engine.get_streaming_metadata()?.clone();
        let vocab = engine.get_vocab()?.to_vec();
        let mel_filterbank = engine.get_nemotron_mel_filterbank()?.clone();

        // Find blank token ID
        let blank_id = find_token_id(&vocab, "<blk>")
            .or_else(|| find_token_id(&vocab, "<blank>"))
            .unwrap_or((vocab.len() - 1) as i64) as i32;

        // Initialize LSTM cache dimensions from metadata
        let cache_channel_dims = metadata.cache_last_channel_dims;
        let cache_time_dims = metadata.cache_last_time_dims;

        let cache_channel = ArrayD::<f32>::zeros(IxDyn(&[
            cache_channel_dims[0],
            cache_channel_dims[1],
            cache_channel_dims[2],
            cache_channel_dims[3],
        ]));

        let cache_time = ArrayD::<f32>::zeros(IxDyn(&[
            cache_time_dims[0],
            cache_time_dims[1],
            cache_time_dims[2],
            cache_time_dims[3],
        ]));

        // Initialize decoder output with the model's blank token.
        match initialize_decoder_output(engine, blank_id) {
            Ok((dim, output, h, c)) => Some(Self {
                cache_channel,
                cache_time,
                cache_len: 0,
                decoder_h_state: h,
                decoder_c_state: c,
                decoder_output: output,
                tokens: Vec::new(),
                chunk_count: 0,
                processed_samples: 0,
                source_sample_rate,
                metadata,
                vocab,
                mel_filterbank,
                blank_id,
                decoder_dim: dim,
            }),
            Err(e) => {
                crate::log_error!("streaming", "Failed to initialize decoder: {}", e);
                None
            }
        }
    }

    /// Get the current partial transcription text
    pub fn get_partial_text(&self) -> String {
        decode_tokens(&self.tokens, &self.vocab)
    }

    /// Reset state for a new recording session
    pub fn reset(&mut self, engine: &mut SttEngine) {
        // Re-initialize all cache state
        let cache_channel_dims = self.metadata.cache_last_channel_dims;
        let cache_time_dims = self.metadata.cache_last_time_dims;

        self.cache_channel = ArrayD::<f32>::zeros(IxDyn(&[
            cache_channel_dims[0],
            cache_channel_dims[1],
            cache_channel_dims[2],
            cache_channel_dims[3],
        ]));

        self.cache_time = ArrayD::<f32>::zeros(IxDyn(&[
            cache_time_dims[0],
            cache_time_dims[1],
            cache_time_dims[2],
            cache_time_dims[3],
        ]));

        self.cache_len = 0;
        self.decoder_h_state = ArrayD::<f32>::zeros(IxDyn(&[2, 1, self.decoder_dim]));
        self.decoder_c_state = ArrayD::<f32>::zeros(IxDyn(&[2, 1, self.decoder_dim]));
        self.tokens.clear();
        self.chunk_count = 0;
        self.processed_samples = 0;

        // Re-initialize decoder output
        if let Ok((_, output, h, c)) = initialize_decoder_output(engine, self.blank_id) {
            self.decoder_output = output;
            self.decoder_h_state = h;
            self.decoder_c_state = c;
        }
    }
}

/// Initialize decoder with the blank token and return initial state
fn initialize_decoder_output(
    engine: &mut SttEngine,
    blank_id: i32,
) -> Result<(usize, Vec<f32>, ArrayD<f32>, ArrayD<f32>), SttError> {
    let decoder = engine
        .get_decoder_session_mut()
        .ok_or(SttError::NotLoaded)?;

    let decoder_hidden_dim = 640;
    let mut decoder_h_state = ArrayD::<f32>::zeros(IxDyn(&[2, 1, decoder_hidden_dim]));
    let mut decoder_c_state = ArrayD::<f32>::zeros(IxDyn(&[2, 1, decoder_hidden_dim]));

    let init_targets = Array2::<i32>::from_elem((1, 1), blank_id);
    let init_target_length = Array1::<i32>::from_elem(1, 1);

    let init_targets_tensor = TensorRef::from_array_view(init_targets.view())
        .map_err(|e| SttError::InferenceFailed(format!("init_targets: {}", e)))?;
    let init_target_length_tensor = TensorRef::from_array_view(init_target_length.view())
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

    let init_dec_out = init_dec_outputs
        .get("outputs")
        .ok_or_else(|| SttError::InferenceFailed("No initial decoder outputs".to_string()))?;
    let (init_dec_shape, init_dec_data) =
        init_dec_out.try_extract_tensor::<f32>().map_err(|e| {
            SttError::InferenceFailed(format!("Failed to extract initial decoder output: {}", e))
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

    Ok((dim, output, decoder_h_state, decoder_c_state))
}

/// Process all currently available chunks and return text plus privacy-safe metrics.
pub fn process_incremental_with_metrics(
    state: &mut StreamingState,
    audio_buffer: &Arc<Mutex<Vec<f32>>>,
    engine: &mut SttEngine,
) -> Result<StreamingProcessResult, SttError> {
    let mut result = StreamingProcessResult::default();

    for _ in 0..MAX_CHUNKS_PER_INCREMENTAL_CALL {
        match process_one_chunk(state, audio_buffer, engine)? {
            IncrementalChunkResult::Processed {
                partial_text,
                metrics,
            } => {
                result.chunks_processed += 1;
                if partial_text.is_some() {
                    result.partial_text = partial_text;
                }
                result.metrics.push(metrics);
            }
            IncrementalChunkResult::NoChunkReady => break,
        }
    }

    if result.chunks_processed == MAX_CHUNKS_PER_INCREMENTAL_CALL {
        crate::log_warn!(
            "streaming",
            "Reached streaming chunk processing limit; backlog may remain"
        );
    }

    Ok(result)
}

fn process_one_chunk(
    state: &mut StreamingState,
    audio_buffer: &Arc<Mutex<Vec<f32>>>,
    engine: &mut SttEngine,
) -> Result<IncrementalChunkResult, SttError> {
    let total_start = Instant::now();
    let mut timings = StreamingStageTimings::default();

    // Try to lock the audio buffer without blocking
    let buffer = match audio_buffer.try_lock() {
        Ok(b) => b,
        Err(_) => {
            crate::log_debug!("streaming", "Buffer busy, skipping iteration");
            return Ok(IncrementalChunkResult::NoChunkReady);
        }
    };

    let total_samples = buffer.len();

    // Calculate shift accounting for sample rate difference.
    let chunk_shift_samples_source =
        chunk_samples_for_source_rate(state.metadata.chunk_shift.max(1), state.source_sample_rate);

    // Check if we have enough new samples for a chunk (in source sample rate)
    if pending_chunk_count(
        total_samples,
        state.processed_samples,
        state.metadata.window_size,
        state.metadata.chunk_shift,
        state.source_sample_rate,
    ) == 0
    {
        return Ok(IncrementalChunkResult::NoChunkReady);
    }

    let chunk_window_samples_source =
        source_samples_for_mel_frames(state.metadata.window_size, state.source_sample_rate);
    let chunk_end_sample =
        (state.processed_samples + chunk_window_samples_source).min(total_samples);

    // Extract only the current chunk window instead of recomputing features for the full backlog.
    let audio_slice_source = &buffer[state.processed_samples..chunk_end_sample];

    // Resample to 16kHz if needed
    let feature_start = Instant::now();
    let audio_slice: std::borrow::Cow<[f32]> = if state.source_sample_rate != SAMPLE_RATE {
        std::borrow::Cow::Owned(resample_linear(
            audio_slice_source,
            state.source_sample_rate,
            SAMPLE_RATE,
        ))
    } else {
        std::borrow::Cow::Borrowed(audio_slice_source)
    };

    // Compute mel spectrogram for new audio (now at 16kHz)
    let mel_spec = compute_mel_spectrogram(&audio_slice, &state.mel_filterbank);
    let total_frames = mel_spec.shape()[1];
    timings.feature_ms = elapsed_ms(feature_start);

    if total_frames < state.metadata.window_size {
        return Ok(IncrementalChunkResult::NoChunkReady);
    }

    // Track tokens before processing to detect new emissions
    let tokens_before = state.tokens.len();

    // Process one chunk
    let chunk_size = state.metadata.window_size;
    let chunk_end = chunk_size.min(total_frames);

    // Extract chunk: [n_mels, chunk_len]
    let chunk = mel_spec.slice(ndarray::s![.., 0..chunk_end]).to_owned();
    let chunk_input = chunk.insert_axis(Axis(0)).into_dyn();

    // Run encoder and extract all outputs to owned values
    // This allows us to release the encoder borrow before getting decoder/joiner
    let (
        enc_data_owned,
        encoder_dim,
        time_frames,
        new_cache_channel,
        new_cache_time,
        new_cache_len,
    ) = {
        let encoder = engine
            .get_encoder_session_mut()
            .ok_or(SttError::NotLoaded)?;

        let mel_tensor = TensorRef::from_array_view(chunk_input.view())
            .map_err(|e| SttError::InferenceFailed(format!("mel tensor: {}", e)))?;
        let length = Array1::<i64>::from_vec(vec![chunk_end as i64]);
        let length_tensor = TensorRef::from_array_view(length.view())
            .map_err(|e| SttError::InferenceFailed(format!("length tensor: {}", e)))?;
        let cache_channel_tensor = TensorRef::from_array_view(state.cache_channel.view())
            .map_err(|e| SttError::InferenceFailed(format!("cache_channel tensor: {}", e)))?;
        let cache_time_tensor = TensorRef::from_array_view(state.cache_time.view())
            .map_err(|e| SttError::InferenceFailed(format!("cache_time tensor: {}", e)))?;
        let cache_len = Array1::<i64>::from_vec(vec![state.cache_len]);
        let cache_len_tensor = TensorRef::from_array_view(cache_len.view())
            .map_err(|e| SttError::InferenceFailed(format!("cache_len tensor: {}", e)))?;

        let encoder_start = Instant::now();
        let encoder_outputs = encoder
            .run(ort::inputs![
                "audio_signal" => mel_tensor,
                "length" => length_tensor,
                "cache_last_channel" => cache_channel_tensor,
                "cache_last_time" => cache_time_tensor,
                "cache_last_channel_len" => cache_len_tensor
            ])
            .map_err(|e| SttError::InferenceFailed(format!("Encoder chunk failed: {}", e)))?;
        timings.encoder_ms += elapsed_ms(encoder_start);

        // Extract encoder output to owned
        let enc_out = encoder_outputs
            .get("outputs")
            .ok_or_else(|| SttError::InferenceFailed("No encoder outputs".to_string()))?;
        let (enc_shape, enc_data) = enc_out.try_extract_tensor::<f32>().map_err(|e| {
            SttError::InferenceFailed(format!("Failed to extract encoder output: {}", e))
        })?;

        let encoder_dim = enc_shape[1] as usize;
        let time_frames = enc_shape[2] as usize;
        let enc_data_owned: Vec<f32> = enc_data.to_vec();

        // Extract cache updates to owned
        let cache_channel_next = encoder_outputs
            .get("cache_last_channel_next")
            .ok_or_else(|| SttError::InferenceFailed("No cache_last_channel_next".to_string()))?;
        let (shape, data) = cache_channel_next
            .try_extract_tensor::<f32>()
            .map_err(|e| SttError::InferenceFailed(format!("Failed to extract cache: {}", e)))?;
        let new_cache_channel = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
            .map_err(|e| SttError::InferenceFailed(format!("Failed to reshape cache: {}", e)))?;

        let cache_time_next = encoder_outputs
            .get("cache_last_time_next")
            .ok_or_else(|| SttError::InferenceFailed("No cache_last_time_next".to_string()))?;
        let (shape, data) = cache_time_next
            .try_extract_tensor::<f32>()
            .map_err(|e| SttError::InferenceFailed(format!("Failed to extract cache: {}", e)))?;
        let new_cache_time = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
            .map_err(|e| SttError::InferenceFailed(format!("Failed to reshape cache: {}", e)))?;

        let cache_len_next = encoder_outputs
            .get("cache_last_channel_next_len")
            .ok_or_else(|| {
                SttError::InferenceFailed("No cache_last_channel_next_len".to_string())
            })?;
        let (_, len_data) = cache_len_next.try_extract_tensor::<i64>().map_err(|e| {
            SttError::InferenceFailed(format!("Failed to extract cache_len: {}", e))
        })?;
        let new_cache_len = len_data[0];

        (
            enc_data_owned,
            encoder_dim,
            time_frames,
            new_cache_channel,
            new_cache_time,
            new_cache_len,
        )
    };
    // encoder borrow is now released

    if time_frames > 0 {
        let max_tokens = 500;
        let max_symbols_per_step = 10;

        for t in 0..time_frames {
            if state.tokens.len() >= max_tokens {
                break;
            }

            // Get encoder frame at timestep t: [1, encoder_dim, 1]
            let enc_frame_data: Vec<f32> = (0..encoder_dim)
                .map(|f| enc_data_owned[f * time_frames + t])
                .collect();
            let enc_frame = ArrayD::from_shape_vec(IxDyn(&[1, encoder_dim, 1]), enc_frame_data)
                .map_err(|e| SttError::InferenceFailed(format!("enc_frame: {}", e)))?;

            let mut symbols_emitted = 0;
            loop {
                if symbols_emitted >= max_symbols_per_step || state.tokens.len() >= max_tokens {
                    break;
                }

                // Build decoder frame from cached output
                let dec_frame = ArrayD::from_shape_vec(
                    IxDyn(&[1, state.decoder_dim, 1]),
                    state.decoder_output.clone(),
                )
                .map_err(|e| SttError::InferenceFailed(format!("dec_frame: {}", e)))?;

                // Run joiner in its own scope to release borrow
                let next_token = {
                    let joiner = engine.get_joiner_session_mut().ok_or(SttError::NotLoaded)?;

                    let enc_tensor = TensorRef::from_array_view(enc_frame.view())
                        .map_err(|e| SttError::InferenceFailed(format!("enc_tensor: {}", e)))?;
                    let dec_tensor = TensorRef::from_array_view(dec_frame.view())
                        .map_err(|e| SttError::InferenceFailed(format!("dec_tensor: {}", e)))?;

                    let joiner_start = Instant::now();
                    let joiner_outputs = joiner
                        .run(ort::inputs![
                            "encoder_outputs" => enc_tensor,
                            "decoder_outputs" => dec_tensor
                        ])
                        .map_err(|e| SttError::InferenceFailed(format!("Joiner failed: {}", e)))?;
                    timings.joiner_ms += elapsed_ms(joiner_start);

                    // Extract logits and find argmax
                    let logits = joiner_outputs
                        .iter()
                        .next()
                        .ok_or_else(|| SttError::InferenceFailed("No joiner output".to_string()))?;
                    let (_, logits_data) = logits.1.try_extract_tensor::<f32>().map_err(|e| {
                        SttError::InferenceFailed(format!("Failed to extract logits: {}", e))
                    })?;

                    argmax_index(logits_data).ok_or_else(|| {
                        SttError::InferenceFailed("Empty joiner logits".to_string())
                    })? as i64
                };
                // joiner borrow released here

                // If blank, move to next encoder frame
                if next_token == state.blank_id as i64 {
                    break;
                }

                // Emit token
                state.tokens.push(next_token);
                symbols_emitted += 1;

                // Update decoder with new token in its own scope
                {
                    let decoder = engine
                        .get_decoder_session_mut()
                        .ok_or(SttError::NotLoaded)?;

                    let new_targets = Array2::<i32>::from_elem((1, 1), next_token as i32);
                    let new_target_length = Array1::<i32>::from_elem(1, 1);

                    let new_targets_tensor = TensorRef::from_array_view(new_targets.view())
                        .map_err(|e| SttError::InferenceFailed(format!("new_targets: {}", e)))?;
                    let new_target_length_tensor =
                        TensorRef::from_array_view(new_target_length.view()).map_err(|e| {
                            SttError::InferenceFailed(format!("new_target_length: {}", e))
                        })?;
                    let h_tensor = TensorRef::from_array_view(state.decoder_h_state.view())
                        .map_err(|e| SttError::InferenceFailed(format!("h_state: {}", e)))?;
                    let c_tensor = TensorRef::from_array_view(state.decoder_c_state.view())
                        .map_err(|e| SttError::InferenceFailed(format!("c_state: {}", e)))?;

                    let decoder_start = Instant::now();
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
                    timings.decoder_ms += elapsed_ms(decoder_start);

                    // Update cached decoder output
                    if let Some(dec_out) = decoder_outputs.get("outputs") {
                        if let Ok((_, data)) = dec_out.try_extract_tensor::<f32>() {
                            state.decoder_output = data.to_vec();
                        }
                    }

                    // Update decoder states
                    if let Some(h) = decoder_outputs.get("states") {
                        if let Ok((shape, data)) = h.try_extract_tensor::<f32>() {
                            if let Ok(arr) = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
                            {
                                state.decoder_h_state = arr;
                            }
                        }
                    }
                    if let Some(c) = decoder_outputs.get("162") {
                        if let Ok((shape, data)) = c.try_extract_tensor::<f32>() {
                            if let Ok(arr) = ArrayD::from_shape_vec(shape.to_ixdyn(), data.to_vec())
                            {
                                state.decoder_c_state = arr;
                            }
                        }
                    }
                }
                // decoder borrow released here
            }
        }
    }

    // Update encoder cache for next chunk (from owned values extracted earlier)
    state.cache_channel = new_cache_channel;
    state.cache_time = new_cache_time;
    state.cache_len = new_cache_len;

    // Update processed samples count (in source sample rate)
    state.processed_samples =
        (state.processed_samples + chunk_shift_samples_source).min(total_samples);
    state.chunk_count += 1;
    timings.total_ms = elapsed_ms(total_start);

    let current_text = state.get_partial_text();
    let partial_text = if state.tokens.len() > tokens_before {
        Some(current_text.clone())
    } else {
        None
    };
    let metrics = StreamingMetrics::new(
        state.chunk_count.saturating_sub(1),
        total_samples,
        state.processed_samples,
        state.source_sample_rate,
        tokens_before,
        state.tokens.len(),
        current_text.chars().count(),
        timings,
    );

    Ok(IncrementalChunkResult::Processed {
        partial_text,
        metrics,
    })
}

/// Find token ID in vocabulary
fn find_token_id(vocab: &[String], token: &str) -> Option<i64> {
    vocab.iter().position(|t| t == token).map(|i| i as i64)
}

fn chunk_samples_for_source_rate(window_size: usize, source_sample_rate: u32) -> usize {
    let chunk_samples_16k = window_size * HOP_LENGTH;

    if source_sample_rate != SAMPLE_RATE {
        (chunk_samples_16k as f64 * source_sample_rate as f64 / SAMPLE_RATE as f64).ceil() as usize
    } else {
        chunk_samples_16k
    }
}

fn pending_chunk_count(
    total_samples: usize,
    processed_samples: usize,
    window_size: usize,
    chunk_shift: usize,
    source_sample_rate: u32,
) -> usize {
    let window_samples = source_samples_for_mel_frames(window_size, source_sample_rate);
    let shift_samples = chunk_samples_for_source_rate(chunk_shift.max(1), source_sample_rate);
    let available_samples = total_samples.saturating_sub(processed_samples);

    if available_samples < window_samples {
        0
    } else {
        ((available_samples - window_samples) / shift_samples) + 1
    }
}

fn source_samples_for_mel_frames(frame_count: usize, source_sample_rate: u32) -> usize {
    if frame_count == 0 {
        return 0;
    }

    let samples_16k = WIN_LENGTH + HOP_LENGTH * frame_count.saturating_sub(1);
    if source_sample_rate != SAMPLE_RATE {
        (samples_16k as f64 * source_sample_rate as f64 / SAMPLE_RATE as f64).ceil() as usize
    } else {
        samples_16k
    }
}

fn samples_to_ms(samples: usize, sample_rate: u32) -> u64 {
    if sample_rate == 0 {
        return 0;
    }

    ((samples as f64 / sample_rate as f64) * 1000.0).round() as u64
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn argmax_index(values: &[f32]) -> Option<usize> {
    let mut best: Option<(usize, f32)> = None;

    for (idx, &value) in values.iter().enumerate() {
        match best {
            None => best = Some((idx, value)),
            Some((_, best_value)) if value > best_value => best = Some((idx, value)),
            _ => {}
        }
    }

    best.map(|(idx, _)| idx)
}

/// Decode tokens to text (simplified version from stt.rs)
fn decode_tokens(tokens: &[i64], vocab: &[String]) -> String {
    let mut text = String::new();
    const WORD_BOUNDARY: &str = "\u{2581}";

    for &token_id in tokens {
        if token_id >= 0 && (token_id as usize) < vocab.len() {
            let token = &vocab[token_id as usize];
            if token.is_empty() {
                continue;
            }

            if let Some(rest) = token.strip_prefix(WORD_BOUNDARY) {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(rest);
            } else if token == "<space>" {
                text.push(' ');
            } else if !token.starts_with('<') {
                text.push_str(token);
            }
        }
    }

    text.trim().to_string()
}

/// Compute mel spectrogram for audio samples
fn compute_mel_spectrogram(audio: &[f32], mel_filterbank: &Array2<f32>) -> Array2<f32> {
    use rustfft::{num_complex::Complex, FftPlanner};

    const N_FFT: usize = 512;

    let n_mels = mel_filterbank.shape()[0];
    if audio.len() < WIN_LENGTH {
        return Array2::<f32>::zeros((n_mels, 0));
    }

    let num_frames = (audio.len().saturating_sub(WIN_LENGTH)) / HOP_LENGTH + 1;

    let mut mel_spec = Array2::<f32>::zeros((n_mels, num_frames));
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(N_FFT);

    // Hann window
    let window: Vec<f32> = (0..WIN_LENGTH)
        .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / WIN_LENGTH as f32).cos()))
        .collect();

    for frame_idx in 0..num_frames {
        let start = frame_idx * HOP_LENGTH;
        let end = (start + WIN_LENGTH).min(audio.len());

        // Apply window and prepare FFT buffer
        let mut fft_buffer: Vec<Complex<f32>> = vec![Complex::new(0.0, 0.0); N_FFT];
        for (i, &sample) in audio[start..end].iter().enumerate() {
            fft_buffer[i] = Complex::new(sample * window[i], 0.0);
        }

        fft.process(&mut fft_buffer);

        // Power spectrum
        let power_spec: Vec<f32> = fft_buffer
            .iter()
            .take(N_FFT / 2 + 1)
            .map(|c| c.norm_sqr())
            .collect();

        // Apply mel filterbank
        for mel_idx in 0..n_mels {
            let mut mel_energy = 0.0f32;
            for (freq_idx, &power) in power_spec.iter().enumerate() {
                mel_energy += mel_filterbank[[mel_idx, freq_idx]] * power;
            }
            mel_spec[[mel_idx, frame_idx]] = (mel_energy + 1e-10).ln();
        }
    }

    mel_spec
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_vocab() -> Vec<String> {
        vec![
            "<blk>".to_string(),
            "\u{2581}hello".to_string(),
            "\u{2581}world".to_string(),
            "!".to_string(),
            "<space>".to_string(),
            "<noise>".to_string(),
        ]
    }

    #[test]
    fn test_find_token_id() {
        let vocab = test_vocab();

        assert_eq!(find_token_id(&vocab, "<blk>"), Some(0));
        assert_eq!(find_token_id(&vocab, "\u{2581}world"), Some(2));
        assert_eq!(find_token_id(&vocab, "<missing>"), None);
    }

    #[test]
    fn test_decode_tokens_ignores_special_and_out_of_range_tokens() {
        let vocab = test_vocab();
        let tokens = vec![1, 2, 3, 4, 5, 99, -1];

        assert_eq!(decode_tokens(&tokens, &vocab), "hello world!");
    }

    #[test]
    fn test_partial_text_uses_accumulated_tokens() {
        let vocab = test_vocab();
        let state = StreamingState {
            cache_channel: ArrayD::<f32>::zeros(IxDyn(&[1, 1, 1, 1])),
            cache_time: ArrayD::<f32>::zeros(IxDyn(&[1, 1, 1, 1])),
            cache_len: 0,
            decoder_h_state: ArrayD::<f32>::zeros(IxDyn(&[2, 1, 4])),
            decoder_c_state: ArrayD::<f32>::zeros(IxDyn(&[2, 1, 4])),
            decoder_output: vec![0.0; 4],
            tokens: vec![1, 2, 3],
            chunk_count: 0,
            processed_samples: 0,
            source_sample_rate: SAMPLE_RATE,
            metadata: StreamingMetadata::default(),
            vocab,
            mel_filterbank: Array2::<f32>::zeros((N_MELS, 257)),
            blank_id: 0,
            decoder_dim: 4,
        };

        assert_eq!(state.get_partial_text(), "hello world!");
    }

    #[test]
    fn test_chunk_samples_for_source_rate() {
        assert_eq!(chunk_samples_for_source_rate(112, SAMPLE_RATE), 17_920);
        assert_eq!(chunk_samples_for_source_rate(112, 48_000), 53_760);
        assert_eq!(chunk_samples_for_source_rate(112, 44_100), 49_392);
    }

    #[test]
    fn test_source_samples_for_mel_frames_include_window_length() {
        assert_eq!(source_samples_for_mel_frames(0, SAMPLE_RATE), 0);
        assert_eq!(source_samples_for_mel_frames(1, SAMPLE_RATE), 400);
        assert_eq!(source_samples_for_mel_frames(4, SAMPLE_RATE), 880);
        assert_eq!(source_samples_for_mel_frames(4, 48_000), 2_640);
    }

    #[test]
    fn test_pending_chunk_count_uses_window_and_shift() {
        assert_eq!(pending_chunk_count(879, 0, 4, 2, SAMPLE_RATE), 0);
        assert_eq!(pending_chunk_count(880, 0, 4, 2, SAMPLE_RATE), 1);
        assert_eq!(pending_chunk_count(1_200, 0, 4, 2, SAMPLE_RATE), 2);
        assert_eq!(pending_chunk_count(1_520, 0, 4, 2, SAMPLE_RATE), 3);
    }

    #[test]
    fn test_streaming_metrics_are_privacy_safe_and_track_backlog() {
        let metrics = StreamingMetrics::new(
            7,
            48_000,
            24_000,
            48_000,
            2,
            12,
            31,
            StreamingStageTimings::default(),
        );

        assert_eq!(metrics.chunk_index, 7);
        assert_eq!(metrics.available_audio_ms, 1_000);
        assert_eq!(metrics.processed_audio_ms, 500);
        assert_eq!(metrics.backlog_ms, 500);
        assert_eq!(metrics.tokens_emitted, 10);
        assert_eq!(metrics.partial_chars, 31);

        let serialized = serde_json::to_string(&metrics).unwrap();
        assert!(!serialized.contains("transcript"));
        assert!(!serialized.contains("text"));
    }

    #[test]
    fn test_argmax_index_returns_first_maximum_without_sorting() {
        let logits = [-4.0, 1.5, 3.2, 3.2, -1.0];

        assert_eq!(argmax_index(&logits), Some(2));
        assert_eq!(argmax_index(&[]), None);
    }

    #[test]
    fn test_compute_mel_spectrogram_empty_and_shape() {
        let mel_filterbank = Array2::<f32>::ones((N_MELS, 257));

        let empty = compute_mel_spectrogram(&[], &mel_filterbank);
        assert_eq!(empty.shape(), &[N_MELS, 0]);

        let audio = vec![0.0; 400 + HOP_LENGTH * 2];
        let spec = compute_mel_spectrogram(&audio, &mel_filterbank);
        assert_eq!(spec.shape(), &[N_MELS, 3]);
        assert!(spec.iter().all(|value| value.is_finite()));
    }
}
