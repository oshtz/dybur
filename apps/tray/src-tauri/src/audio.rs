//! Audio capture using cpal
//!
//! Handles audio capture with proper permission and device error handling.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

/// Audio capture configuration
const SAMPLE_RATE: u32 = 16000;
const CHANNELS: u16 = 1;

/// Audio error types for user-friendly error messages
#[derive(Debug, Clone)]
pub enum AudioError {
    /// No input devices found
    NoInputDevice,
    /// Permission denied for microphone access
    PermissionDenied(String),
    /// Device is busy (being used by another application)
    DeviceBusy(String),
    /// Device disconnected or unavailable
    DeviceUnavailable(String),
    /// Stream creation failed
    StreamError(String),
    /// Generic error
    Other(String),
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::NoInputDevice => write!(
                f,
                "No microphone found. Please connect a microphone and try again."
            ),
            AudioError::PermissionDenied(msg) => write!(
                f,
                "Microphone access denied: {}. Please grant microphone permission.",
                msg
            ),
            AudioError::DeviceBusy(msg) => write!(
                f,
                "Microphone is busy: {}. Another application may be using it.",
                msg
            ),
            AudioError::DeviceUnavailable(msg) => write!(
                f,
                "Microphone unavailable: {}. Please check your audio device.",
                msg
            ),
            AudioError::StreamError(msg) => write!(f, "Audio stream error: {}", msg),
            AudioError::Other(msg) => write!(f, "Audio error: {}", msg),
        }
    }
}

impl From<AudioError> for String {
    fn from(err: AudioError) -> Self {
        err.to_string()
    }
}

/// Classify an error message into an AudioError type
fn classify_audio_error(error: &str) -> AudioError {
    let error_lower = error.to_lowercase();

    // Permission-related errors
    if error_lower.contains("permission")
        || error_lower.contains("denied")
        || error_lower.contains("access")
        || error_lower.contains("not authorized")
    {
        return AudioError::PermissionDenied(error.to_string());
    }

    // Device busy errors
    if error_lower.contains("busy")
        || error_lower.contains("in use")
        || error_lower.contains("exclusive")
        || error_lower.contains("occupied")
    {
        return AudioError::DeviceBusy(error.to_string());
    }

    // Device unavailable errors
    if error_lower.contains("not found")
        || error_lower.contains("disconnected")
        || error_lower.contains("unavailable")
        || error_lower.contains("device not")
    {
        return AudioError::DeviceUnavailable(error.to_string());
    }

    AudioError::StreamError(error.to_string())
}

/// Audio capture handler
pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    buffer: Arc<Mutex<Vec<f32>>>,
    last_error: Arc<Mutex<Option<AudioError>>>,
    sample_rate: u32,
    channels: u16,
}

impl AudioCapture {
    /// Create a new audio capture instance
    pub fn new() -> Result<Self, AudioError> {
        Ok(Self {
            stream: None,
            buffer: Arc::new(Mutex::new(Vec::new())),
            last_error: Arc::new(Mutex::new(None)),
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
        })
    }

    /// Check microphone permission status (platform-specific)
    pub fn check_permission() -> Result<(), AudioError> {
        // On Windows, we can try to enumerate devices to check access
        // On macOS, we would use AVFoundation, but cpal handles this at stream creation
        let host = cpal::default_host();

        // Try to get the default input device
        if host.default_input_device().is_none() {
            return Err(AudioError::NoInputDevice);
        }

        Ok(())
    }

    /// Start capturing audio with improved error handling
    /// 
    /// # Arguments
    /// * `device_name` - Optional device name to use. If None, uses system default.
    pub fn start(&mut self, device_name: Option<&str>) -> Result<(), AudioError> {
        // Clear any previous error
        {
            let mut last_error = self.last_error.lock().unwrap();
            *last_error = None;
        }

        // Get the input device (specified or default)
        let device = get_input_device(device_name)?;

        // Log device info
        if let Ok(name) = device.name() {
            crate::log_info!("audio", "Using input device: {}", name);
        }

        let supported_config = select_input_config(&device)?;
        let sample_format = supported_config.sample_format();
        let config = supported_config.config();
        self.sample_rate = config.sample_rate.0;
        self.channels = config.channels;

        crate::log_info!(
            "audio",
            "Input config: {}Hz, {}ch, {:?}",
            self.sample_rate,
            self.channels,
            sample_format
        );

        let buffer = self.buffer.clone();
        let last_error = self.last_error.clone();

        // Error callback with proper classification
        let err_fn = move |err: cpal::StreamError| {
            let audio_err = classify_audio_error(&err.to_string());
            crate::log_error!("audio", "Stream error: {}", audio_err);
            let mut error_guard = last_error.lock().unwrap();
            *error_guard = Some(audio_err);
        };

        // Build the input stream with proper error handling
        let channels = self.channels;
        let stream = match sample_format {
            cpal::SampleFormat::F32 => {
                build_input_stream_f32(&device, &config, channels, buffer, err_fn)
            }
            cpal::SampleFormat::I16 => {
                build_input_stream_i16(&device, &config, channels, buffer, err_fn)
            }
            cpal::SampleFormat::U16 => {
                build_input_stream_u16(&device, &config, channels, buffer, err_fn)
            }
            _ => {
                return Err(AudioError::StreamError(format!(
                    "Unsupported sample format: {:?}",
                    sample_format
                )));
            }
        }
        .map_err(|e| {
            let err = classify_audio_error(&e.to_string());
            crate::log_error!("audio", "Failed to build input stream: {}", err);
            err
        })?;

        // Start the stream
        stream.play().map_err(|e| {
            let err = classify_audio_error(&e.to_string());
            crate::log_error!("audio", "Failed to start audio stream: {}", err);
            err
        })?;

        self.stream = Some(stream);
        crate::log_info!(
            "audio",
            "Audio capture started at {}Hz ({}ch)",
            self.sample_rate,
            self.channels
        );
        Ok(())
    }

    /// Stop capturing and return audio data
    pub fn stop(&mut self) -> Vec<f32> {
        // Drop the stream to stop recording
        self.stream.take();

        // Get and clear the buffer
        let mut buffer = self.buffer.lock().unwrap();
        let data = std::mem::take(&mut *buffer);
        let duration = data.len() as f32 / self.sample_rate as f32;

        crate::log_info!(
            "audio",
            "Audio capture stopped. Captured {:.2}s of audio",
            duration
        );

        if self.sample_rate != SAMPLE_RATE {
            crate::log_info!(
                "audio",
                "Resampling audio from {}Hz to {}Hz",
                self.sample_rate,
                SAMPLE_RATE
            );
            return resample_linear(&data, self.sample_rate, SAMPLE_RATE);
        }

        data
    }

    /// Check if currently recording
    pub fn is_recording(&self) -> bool {
        self.stream.is_some()
    }

    /// Get the current buffer length in seconds
    pub fn duration_seconds(&self) -> f32 {
        let buffer = self.buffer.lock().unwrap();
        buffer.len() as f32 / self.sample_rate as f32
    }

    /// Get the last error that occurred during recording
    pub fn last_error(&self) -> Option<AudioError> {
        let error = self.last_error.lock().unwrap();
        error.clone()
    }

    /// Clear the last error
    pub fn clear_error(&self) {
        let mut error = self.last_error.lock().unwrap();
        *error = None;
    }
}

impl Drop for AudioCapture {
    fn drop(&mut self) {
        // Ensure stream is stopped
        self.stream.take();
    }
}

fn select_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig, AudioError> {
    let configs = device.supported_input_configs().map_err(|e| {
        AudioError::StreamError(format!("Failed to query supported configs: {}", e))
    })?;

    let mut best: Option<(cpal::SupportedStreamConfig, (u32, u16, u8))> = None;

    for range in configs {
        if range.channels() < 1 {
            continue;
        }

        let sample_format = range.sample_format();
        if !matches!(
            sample_format,
            cpal::SampleFormat::F32 | cpal::SampleFormat::I16 | cpal::SampleFormat::U16
        ) {
            continue;
        }

        let min_rate = range.min_sample_rate().0;
        let max_rate = range.max_sample_rate().0;
        let target_rate = SAMPLE_RATE;
        let selected_rate = clamp_sample_rate(min_rate, max_rate, target_rate);

        let Some(config) = range.try_with_sample_rate(cpal::SampleRate(selected_rate)) else {
            continue;
        };

        let rate_diff = if selected_rate > target_rate {
            selected_rate - target_rate
        } else {
            target_rate - selected_rate
        };

        let channel_penalty = if config.channels() == 1 { 0 } else { 1 };
        let format_penalty = sample_format_rank(config.sample_format());
        let score = (rate_diff, channel_penalty, format_penalty);

        if best.as_ref().map_or(true, |(_, best_score)| score < *best_score) {
            best = Some((config, score));
        }
    }

    if let Some((config, _)) = best {
        return Ok(config);
    }

    device.default_input_config().map_err(|e| {
        AudioError::StreamError(format!("Failed to get default input config: {}", e))
    })
}

fn clamp_sample_rate(min: u32, max: u32, target: u32) -> u32 {
    if target < min {
        min
    } else if target > max {
        max
    } else {
        target
    }
}

fn sample_format_rank(format: cpal::SampleFormat) -> u8 {
    match format {
        cpal::SampleFormat::F32 => 0,
        cpal::SampleFormat::I16 => 1,
        cpal::SampleFormat::U16 => 2,
        _ => 9,
    }
}

fn build_input_stream_f32(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    buffer: Arc<Mutex<Vec<f32>>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError>
{
    let channels = channels as usize;

    device.build_input_stream(
        config,
        move |data: &[f32], _: &cpal::InputCallbackInfo| {
            let mut buffer = buffer.lock().unwrap();

            if channels <= 1 {
                buffer.extend_from_slice(data);
                return;
            }

            for frame in data.chunks(channels) {
                let mut sum = 0.0f32;
                for sample in frame {
                    sum += *sample;
                }
                buffer.push(sum / channels as f32);
            }
        },
        err_fn,
        None,
    )
}

fn build_input_stream_i16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    buffer: Arc<Mutex<Vec<f32>>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let channels = channels as usize;
    let scale = i16::MAX as f32;

    device.build_input_stream(
        config,
        move |data: &[i16], _: &cpal::InputCallbackInfo| {
            let mut buffer = buffer.lock().unwrap();

            if channels <= 1 {
                buffer.extend(data.iter().map(|s| *s as f32 / scale));
                return;
            }

            for frame in data.chunks(channels) {
                let mut sum = 0.0f32;
                for sample in frame {
                    sum += *sample as f32 / scale;
                }
                buffer.push(sum / channels as f32);
            }
        },
        err_fn,
        None,
    )
}

fn build_input_stream_u16(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    channels: u16,
    buffer: Arc<Mutex<Vec<f32>>>,
    err_fn: impl FnMut(cpal::StreamError) + Send + 'static,
) -> Result<cpal::Stream, cpal::BuildStreamError> {
    let channels = channels as usize;
    let scale = 32768.0f32;

    device.build_input_stream(
        config,
        move |data: &[u16], _: &cpal::InputCallbackInfo| {
            let mut buffer = buffer.lock().unwrap();

            if channels <= 1 {
                buffer.extend(data.iter().map(|s| (*s as f32 - scale) / scale));
                return;
            }

            for frame in data.chunks(channels) {
                let mut sum = 0.0f32;
                for sample in frame {
                    sum += (*sample as f32 - scale) / scale;
                }
                buffer.push(sum / channels as f32);
            }
        },
        err_fn,
        None,
    )
}

fn resample_linear(input: &[f32], in_rate: u32, out_rate: u32) -> Vec<f32> {
    if input.is_empty() || in_rate == out_rate {
        return input.to_vec();
    }

    let ratio = out_rate as f64 / in_rate as f64;
    let out_len = ((input.len() as f64) * ratio).round() as usize;
    let mut output = Vec::with_capacity(out_len);

    for i in 0..out_len {
        let src_pos = (i as f64) / ratio;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;

        let s0 = input.get(idx).copied().unwrap_or(0.0);
        let s1 = input.get(idx + 1).copied().unwrap_or(s0);
        output.push(s0 + (s1 - s0) * frac);
    }

    output
}

/// Get available input devices with their status
pub fn list_input_devices() -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                let is_default = host
                    .default_input_device()
                    .and_then(|d| d.name().ok())
                    .map(|n| n == name)
                    .unwrap_or(false);

                devices.push(DeviceInfo { name, is_default });
            }
        }
    }

    devices
}

/// Information about an audio device
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub name: String,
    pub is_default: bool,
}

/// Check if default input device is available
pub fn has_input_device() -> bool {
    let host = cpal::default_host();
    host.default_input_device().is_some()
}

/// Find an input device by name
/// Returns None if not found
pub fn find_input_device_by_name(name: &str) -> Option<cpal::Device> {
    let host = cpal::default_host();
    
    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if let Ok(device_name) = device.name() {
                if device_name == name {
                    return Some(device);
                }
            }
        }
    }
    
    None
}

/// Get an input device by name, or fall back to default
/// Returns an error if no device is available
pub fn get_input_device(device_name: Option<&str>) -> Result<cpal::Device, AudioError> {
    let host = cpal::default_host();
    
    if let Some(name) = device_name {
        // Try to find the specified device
        if let Some(device) = find_input_device_by_name(name) {
            crate::log_info!("audio", "Using specified input device: {}", name);
            return Ok(device);
        } else {
            crate::log_warn!(
                "audio",
                "Specified input device '{}' not found, falling back to default",
                name
            );
        }
    }
    
    // Fall back to default device
    host.default_input_device().ok_or_else(|| {
        let err = AudioError::NoInputDevice;
        crate::log_error!("audio", "{}", err);
        err
    })
}

/// Get diagnostic information about audio devices
pub fn get_audio_diagnostics() -> AudioDiagnostics {
    let host = cpal::default_host();

    let default_device = host.default_input_device().and_then(|d| d.name().ok());

    let devices = list_input_devices();
    let device_count = devices.len();

    AudioDiagnostics {
        host_name: host.id().name().to_string(),
        default_device,
        device_count,
        devices,
    }
}

/// Audio system diagnostics
#[derive(Debug)]
pub struct AudioDiagnostics {
    pub host_name: String,
    pub default_device: Option<String>,
    pub device_count: usize,
    pub devices: Vec<DeviceInfo>,
}

/// Get user-friendly help message for audio errors
pub fn get_audio_error_help(error: &AudioError) -> String {
    match error {
        AudioError::NoInputDevice => {
            "Please connect a microphone or headset with a microphone and restart dybur.".to_string()
        }
        AudioError::PermissionDenied(_) => {
            #[cfg(target_os = "macos")]
            return "Grant microphone access in System Preferences > Security & Privacy > Privacy > Microphone.".to_string();

            #[cfg(target_os = "windows")]
            return "Grant microphone access in Settings > Privacy > Microphone. Ensure 'Allow apps to access your microphone' is On.".to_string();

            #[cfg(not(any(target_os = "macos", target_os = "windows")))]
            "Check your system's privacy settings to allow microphone access.".to_string()
        }
        AudioError::DeviceBusy(_) => {
            "Close other applications that may be using the microphone (video calls, voice recorders, etc.) and try again.".to_string()
        }
        AudioError::DeviceUnavailable(_) => {
            "Check that your microphone is properly connected. Try unplugging and reconnecting it.".to_string()
        }
        AudioError::StreamError(_) | AudioError::Other(_) => {
            "Try restarting dybur. If the problem persists, check the logs for more details.".to_string()
        }
    }
}
