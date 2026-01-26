//! Execution Provider configuration for ONNX Runtime
//!
//! Handles GPU acceleration with graceful fallback to CPU.
//! - Windows: DirectML (any GPU: AMD, Intel, NVIDIA)
//! - macOS: CoreML (Apple Silicon / Intel Macs)
//! - All platforms: CPU fallback

use ort::session::Session;
use std::path::Path;

/// Execution provider preference
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GpuPreference {
    /// Automatically detect and use best available GPU
    #[default]
    Auto,
    /// Force CPU only (disable GPU)
    CpuOnly,
}

/// Result of execution provider registration
#[derive(Debug, Clone)]
pub struct ExecutionProviderResult {
    /// Name of the provider that was registered
    pub provider_name: String,
    /// Whether GPU acceleration is active
    pub is_gpu: bool,
}

/// Configuration for session creation
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Number of intra-op threads (CPU parallelism)
    pub intra_threads: usize,
    /// GPU preference
    pub gpu_preference: GpuPreference,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            intra_threads: 4,
            gpu_preference: GpuPreference::Auto,
        }
    }
}

impl SessionConfig {
    /// Create config for STT models (encoder/decoder)
    pub fn for_stt() -> Self {
        Self {
            intra_threads: 4,
            gpu_preference: GpuPreference::Auto,
        }
    }

    /// Create config for VAD (lightweight model)
    pub fn for_vad() -> Self {
        Self {
            intra_threads: 1,
            gpu_preference: GpuPreference::Auto,
        }
    }

    /// Create CPU-only config
    pub fn cpu_only(threads: usize) -> Self {
        Self {
            intra_threads: threads,
            gpu_preference: GpuPreference::CpuOnly,
        }
    }

    /// Set GPU preference
    pub fn with_gpu_preference(mut self, pref: GpuPreference) -> Self {
        self.gpu_preference = pref;
        self
    }
}

/// Build an ONNX session with configured execution providers
pub fn build_session(
    model_path: &Path,
    config: &SessionConfig,
) -> Result<(Session, ExecutionProviderResult), ort::Error> {
    // CPU-only mode - skip GPU provider setup
    if config.gpu_preference == GpuPreference::CpuOnly {
        crate::log_info!("ort", "GPU disabled, using CPU execution provider");
        let session = Session::builder()?
            .with_intra_threads(config.intra_threads)?
            .commit_from_file(model_path)?;

        return Ok((session, ExecutionProviderResult {
            provider_name: "CPU".to_string(),
            is_gpu: false,
        }));
    }

    // Try GPU provider based on platform
    #[cfg(target_os = "windows")]
    {
        return build_session_windows(model_path, config);
    }

    #[cfg(target_os = "macos")]
    {
        return build_session_macos(model_path, config);
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        crate::log_info!("ort", "No GPU providers available for this platform, using CPU");
        let session = Session::builder()?
            .with_intra_threads(config.intra_threads)?
            .commit_from_file(model_path)?;

        Ok((session, ExecutionProviderResult {
            provider_name: "CPU".to_string(),
            is_gpu: false,
        }))
    }
}

/// Build session with DirectML on Windows
#[cfg(target_os = "windows")]
fn build_session_windows(
    model_path: &Path,
    config: &SessionConfig,
) -> Result<(Session, ExecutionProviderResult), ort::Error> {
    use ort::execution_providers::DirectMLExecutionProvider;

    crate::log_info!("ort", "Attempting to register DirectML execution provider");

    // Try DirectML first
    let dml_result = Session::builder()?
        .with_execution_providers([DirectMLExecutionProvider::default().build()])?
        .with_intra_threads(config.intra_threads)?
        .commit_from_file(model_path);

    match dml_result {
        Ok(session) => {
            crate::log_info!("ort", "DirectML execution provider registered successfully");
            Ok((session, ExecutionProviderResult {
                provider_name: "DirectML".to_string(),
                is_gpu: true,
            }))
        }
        Err(e) => {
            crate::log_warn!("ort", "DirectML registration failed: {}, falling back to CPU", e);

            // Fall back to CPU
            let session = Session::builder()?
                .with_intra_threads(config.intra_threads)?
                .commit_from_file(model_path)?;

            Ok((session, ExecutionProviderResult {
                provider_name: "CPU".to_string(),
                is_gpu: false,
            }))
        }
    }
}

/// Build session with CoreML on macOS
#[cfg(target_os = "macos")]
fn build_session_macos(
    model_path: &Path,
    config: &SessionConfig,
) -> Result<(Session, ExecutionProviderResult), ort::Error> {
    use ort::execution_providers::CoreMLExecutionProvider;

    crate::log_info!("ort", "Attempting to register CoreML execution provider");

    // Try CoreML
    let coreml_result = Session::builder()?
        .with_execution_providers([CoreMLExecutionProvider::default().build()])?
        .with_intra_threads(config.intra_threads)?
        .commit_from_file(model_path);

    match coreml_result {
        Ok(session) => {
            crate::log_info!("ort", "CoreML execution provider registered successfully");
            Ok((session, ExecutionProviderResult {
                provider_name: "CoreML".to_string(),
                is_gpu: true,
            }))
        }
        Err(e) => {
            crate::log_warn!("ort", "CoreML registration failed: {}, falling back to CPU", e);

            // Fall back to CPU
            let session = Session::builder()?
                .with_intra_threads(config.intra_threads)?
                .commit_from_file(model_path)?;

            Ok((session, ExecutionProviderResult {
                provider_name: "CPU".to_string(),
                is_gpu: false,
            }))
        }
    }
}

/// Get information about available execution providers on this platform
pub fn get_available_providers() -> Vec<&'static str> {
    let mut providers = vec!["CPU"];

    #[cfg(target_os = "windows")]
    providers.push("DirectML");

    #[cfg(target_os = "macos")]
    providers.push("CoreML");

    providers
}

/// Parse GPU preference from string (for config)
///
/// On macOS, defaults to CPU because CoreML has compatibility issues with many models
/// (especially Whisper and quantized models). Users can explicitly enable GPU with "gpu".
pub fn parse_gpu_preference(mode: &str) -> GpuPreference {
    match mode.to_lowercase().as_str() {
        "cpu" | "cpu_only" | "cpuonly" | "disabled" | "off" | "false" => GpuPreference::CpuOnly,
        "gpu" | "coreml" | "directml" => GpuPreference::Auto, // Explicit GPU request
        #[cfg(target_os = "macos")]
        "auto" => {
            // On macOS, default to CPU due to CoreML compatibility issues with many models
            crate::log_info!("ort", "macOS detected, defaulting to CPU (CoreML has compatibility issues)");
            GpuPreference::CpuOnly
        }
        _ => GpuPreference::Auto, // "auto" on Windows, or any other value
    }
}
