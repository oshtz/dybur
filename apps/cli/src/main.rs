//! dybur CLI - Local voice dictation
//!
//! Command-line interface for managing the dybur tray application.

use std::path::PathBuf;
use std::process::Command;

mod audio;
mod config;
mod models;
mod ui;

use config::{get_config_path, get_data_dir, get_logs_dir, load_config, save_config};
use models::{
    clean_models, download_model_sync, format_bytes, is_default_model_installed, list_models,
    DEFAULT_MODEL,
};
use audio::list_input_devices;
use ui::*;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Handle flags
    if args.is_empty() || args.contains(&"--help".to_string()) || args.contains(&"-h".to_string()) {
        show_help();
        return;
    }

    if args.contains(&"--version".to_string()) || args.contains(&"-v".to_string()) {
        show_version();
        return;
    }

    // Route to command
    let cmd = args[0].clone();
    let cmd_args: Vec<String> = args.into_iter().skip(1).collect();

    match cmd.as_str() {
        "start" => cmd_start(),
        "stop" => cmd_stop(),
        "status" | "s" => cmd_status(),
        "settings" | "config" => cmd_settings(&cmd_args),
        "doctor" | "diag" => cmd_doctor(),
        "models" | "m" => cmd_models(&cmd_args),
        "devices" | "d" => cmd_devices(&cmd_args),
        "setup" => cmd_setup(),
        "uninstall" => cmd_uninstall(),
        _ => {
            error(&format!("Unknown command: {}", cmd));
            println!();
            info(&format!("Run {} for usage information", cyan("dybur --help")));
            std::process::exit(1);
        }
    }
}

fn show_help() {
    banner();

    println!("  {}", dim("Local voice dictation for macOS & Windows"));
    println!("  {} {} {}", dim("Powered by"), cyan("NVIDIA Parakeet"), dim("- 100% offline"));
    println!();

    header("Commands");
    command("start", "Start the background service");
    command("stop", "Stop the background service");
    command("status, s", "Show service status & health");
    command("settings, config", "Open configuration file");
    command("doctor, diag", "Run diagnostics");
    command("models, m", "Manage speech models");
    command("devices, d", "Manage input devices");
    command("setup", "Install CLI to PATH");
    command("uninstall", "Completely remove dybur");
    println!();

    header("Model Commands");
    command("models list", "List installed models");
    command("models prefetch", "Download default model");
    command("models clean", "Remove unused models");
    println!();

    header("Device Commands");
    command("d, d l", "List & select microphone interactively");
    command("d set <name>", "Select a specific microphone");
    command("d reset", "Reset to system default");
    println!();

    header("Options");
    command("-h, --help", "Show this help message");
    command("-v, --version", "Show version number");
    println!();

    header("Quick Start");
    info(&format!("Run {} to begin", cyan("dybur start")));
    info(&format!("Press {} to dictate", accent("Ctrl+Shift+Space")));
    println!();

    println!("  {} {}", dim("Docs:"), cyan("https://github.com/oshtz/dybur"));
    println!();
}

fn show_version() {
    box_message(
        &[
            &format!("Version: {}", accent(VERSION)),
            &format!("Platform: {}", std::env::consts::OS),
        ],
        Some("dybur"),
    );
}

// ============================================================================
// Start Command
// ============================================================================

fn cmd_start() {
    header("Starting dybur");

    let config = load_config().unwrap_or_default();

    key_value("Model", &config.model);
    key_value("Hotkey", &accent(&config.hotkey));
    println!();

    // Check if model is installed
    if !is_default_model_installed() {
        warning("Default model not found");
        info("Downloading model from HuggingFace...");
        println!("  {}", dim("This only needs to happen once"));
        println!();

        match download_model_sync(DEFAULT_MODEL, "int8") {
            Ok(path) => {
                println!();
                success("Model downloaded");
                println!("  {}", dim(&format!("Location: {}", path.display())));
                println!();
            }
            Err(e) => {
                println!();
                error(&format!("Failed to download model: {}", e));
                info(&format!("Run {} to try again", cyan("dybur models prefetch")));
                std::process::exit(1);
            }
        }
    }

    // Check if already running
    if is_dybur_running() {
        success("dybur is already running");
        println!();
        info(&format!("Press {} to begin dictating", accent(&config.hotkey)));
        println!();
        return;
    }

    // Find tray executable
    let tray_path = find_tray_executable();

    match tray_path {
        Some(path) => {
            // Show spinner
            for i in 0..10 {
                spinner_frame(i, "Launching tray application");
                std::thread::sleep(std::time::Duration::from_millis(80));
            }

            #[cfg(target_os = "windows")]
            {
                let _ = Command::new("cmd")
                    .args(["/c", "start", "", &path.to_string_lossy()])
                    .spawn();
            }

            #[cfg(target_os = "macos")]
            {
                let _ = Command::new("open").arg(&path).spawn();
            }

            #[cfg(target_os = "linux")]
            {
                let _ = Command::new(&path).spawn();
            }

            clear_line();
            success("dybur started");
            println!();
            info(&format!("Press {} to begin dictating", accent(&config.hotkey)));

            #[cfg(target_os = "macos")]
            {
                println!();
                println!("  {}", dim("Note: You may need to grant accessibility permissions"));
                println!("  {}", dim("System Settings > Privacy & Security > Accessibility"));
            }

            println!();
        }
        None => {
            error("Could not find dybur tray application");
            println!();
            info("You can try:");
            println!("  {} Check if dybur is installed correctly", dim("1."));
            println!("  {} Download from {}", dim("2."), cyan("https://github.com/oshtz/dybur/releases"));
            println!();
        }
    }
}

// ============================================================================
// Stop Command
// ============================================================================

fn cmd_stop() {
    header("Stopping dybur");

    for i in 0..8 {
        spinner_frame(i, "Stopping service");
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    let stopped = stop_dybur_process();

    clear_line();

    if stopped {
        success("dybur stopped");
    } else {
        info("dybur was not running");
    }

    println!();
}

// ============================================================================
// Status Command
// ============================================================================

fn cmd_status() {
    header("dybur Status");

    let running = is_dybur_running();
    let model_installed = is_default_model_installed();
    let config = load_config().unwrap_or_default();

    // Service status
    let running_icon = if running { icons::status_on() } else { icons::status_off() };
    let running_text = if running { green("Running") } else { red("Stopped") };
    println!("  {} {} {}", running_icon, dim("Service:"), running_text);

    let model_icon = if model_installed { icons::status_on() } else { icons::status_off() };
    let model_text = if model_installed {
        green(DEFAULT_MODEL)
    } else {
        red("Not installed")
    };
    println!("  {} {} {}", model_icon, dim("Model:"), model_text);

    println!();
    divider();
    println!();

    // Configuration
    println!("  {}", accent("Configuration"));
    println!("  {} {}", dim("Hotkey:"), accent(&config.hotkey));
    println!(
        "  {} {}",
        dim("Punctuation:"),
        if config.auto_punctuation {
            green("enabled")
        } else {
            dim("disabled")
        }
    );
    println!(
        "  {} {}",
        dim("Sentence case:"),
        if config.sentence_case {
            green("enabled")
        } else {
            dim("disabled")
        }
    );
    println!("  {} {}ms", dim("Silence timeout:"), config.silence_timeout_ms);

    println!();
    divider();
    println!();

    // Paths
    println!("  {}", accent("Paths"));
    println!("  {} {}", dim("Config:"), format_path(&get_config_path().to_string_lossy(), 45));
    println!("  {} {}", dim("Models:"), format_path(&get_data_dir().join("models").to_string_lossy(), 45));
    println!("  {} {}", dim("Logs:"), format_path(&get_logs_dir(), 45));

    println!();
    divider();
    println!();

    // Overall status
    if running && model_installed {
        success(&format!(
            "Ready {} {} {}",
            dim("- press"),
            accent(&config.hotkey),
            dim("to dictate")
        ));
    } else if !model_installed {
        warning("Model required");
        info(&format!("Run {} to download", cyan("dybur models prefetch")));
    } else {
        warning("Service not running");
        info(&format!("Run {} to begin", cyan("dybur start")));
    }

    println!();
}

// ============================================================================
// Settings Command
// ============================================================================

fn cmd_settings(args: &[String]) {
    let config_path = get_config_path();

    if args.contains(&"--path".to_string()) {
        println!("{}", config_path.display());
        return;
    }

    if args.contains(&"--show".to_string()) {
        header("Current Configuration");

        let config = load_config().unwrap_or_default();

        key_value("Hotkey", &accent(&config.hotkey));
        key_value(
            "Auto punctuation",
            if config.auto_punctuation { "enabled" } else { "disabled" },
        );
        key_value(
            "Sentence case",
            if config.sentence_case { "enabled" } else { "disabled" },
        );
        key_value("Silence timeout", &format!("{}ms", config.silence_timeout_ms));
        key_value("Model", &config.model);
        key_value(
            "Clipboard cleanup",
            if config.clipboard_cleanup { "enabled" } else { "disabled" },
        );

        println!();
        println!("  {} {}", dim("Path:"), config_path.display());
        println!();
        return;
    }

    header("Settings");

    // Ensure config exists
    if !config_path.exists() {
        let _ = load_config();
        info("Created default config");
    }

    // Show spinner
    for i in 0..8 {
        spinner_frame(i, "Opening config in editor");
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    // Open in editor
    #[cfg(target_os = "windows")]
    {
        let _ = Command::new("cmd")
            .args(["/c", "start", "", &config_path.to_string_lossy()])
            .spawn();
    }

    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("open").arg(&config_path).spawn();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("xdg-open").arg(&config_path).spawn();
    }

    clear_line();
    success("Config opened");
    println!();
    println!("  {} {}", dim("Path:"), config_path.display());
    println!();
    info("Restart dybur after making changes");
    println!();
}

// ============================================================================
// Doctor Command
// ============================================================================

fn cmd_doctor() {
    header("dybur Diagnostics");

    // Show spinner
    for i in 0..10 {
        spinner_frame(i, "Running checks");
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    clear_line();

    let mut passed = 0;
    let mut warnings = 0;
    let mut failed = 0;

    // Check configuration
    let config_result = check_config();
    print_diagnostic(&config_result);
    match config_result.1.as_str() {
        "pass" => passed += 1,
        "warn" => warnings += 1,
        "fail" => failed += 1,
        _ => {}
    }

    // Check model
    let model_result = check_model();
    print_diagnostic(&model_result);
    match model_result.1.as_str() {
        "pass" => passed += 1,
        "warn" => warnings += 1,
        "fail" => failed += 1,
        _ => {}
    }

    // Check audio
    let audio_result = check_audio();
    print_diagnostic(&audio_result);
    match audio_result.1.as_str() {
        "pass" => passed += 1,
        "warn" => warnings += 1,
        "fail" => failed += 1,
        _ => {}
    }

    // Check hotkey
    let hotkey_result = check_hotkey();
    print_diagnostic(&hotkey_result);
    match hotkey_result.1.as_str() {
        "pass" => passed += 1,
        "warn" => warnings += 1,
        "fail" => failed += 1,
        _ => {}
    }

    // Check directories
    let dir_result = check_directories();
    print_diagnostic(&dir_result);
    match dir_result.1.as_str() {
        "pass" => passed += 1,
        "warn" => warnings += 1,
        "fail" => failed += 1,
        _ => {}
    }

    divider();
    println!();

    println!(
        "  {} {} passed  {} {} warnings  {} {} failed",
        green("●"),
        passed,
        yellow("●"),
        warnings,
        red("●"),
        failed
    );
    println!();

    if failed > 0 {
        error("Some checks failed - see details above");
        std::process::exit(1);
    } else if warnings > 0 {
        warning("All critical checks passed with warnings");
    } else {
        success("All checks passed - dybur is ready");
    }

    println!();
    println!("  {} {}", dim("Log file:"), get_logs_dir());
    println!();
}

fn print_diagnostic(result: &(String, String, String, Option<String>)) {
    let (name, status, message, details) = result;

    let icon = match status.as_str() {
        "pass" => green("●"),
        "warn" => yellow("●"),
        "fail" => red("●"),
        _ => "?".to_string(),
    };

    let msg_colored = match status.as_str() {
        "pass" => green(message),
        "warn" => yellow(message),
        "fail" => red(message),
        _ => message.clone(),
    };

    println!("  {} {}", icon, dim(name));
    println!("    {}", msg_colored);

    if let Some(d) = details {
        println!("    {}", dim(d));
    }
    println!();
}

fn check_config() -> (String, String, String, Option<String>) {
    let config_path = get_config_path();

    if !config_path.exists() {
        return (
            "Configuration".to_string(),
            "warn".to_string(),
            "Config file not found".to_string(),
            Some(format!("Will be created at: {}", config_path.display())),
        );
    }

    match load_config() {
        Ok(config) => (
            "Configuration".to_string(),
            "pass".to_string(),
            "Valid configuration".to_string(),
            Some(format!("Hotkey: {}", config.hotkey)),
        ),
        Err(e) => (
            "Configuration".to_string(),
            "fail".to_string(),
            "Failed to load config".to_string(),
            Some(e),
        ),
    }
}

fn check_model() -> (String, String, String, Option<String>) {
    if is_default_model_installed() {
        (
            "Speech Model".to_string(),
            "pass".to_string(),
            format!("{} installed", DEFAULT_MODEL),
            None,
        )
    } else {
        (
            "Speech Model".to_string(),
            "fail".to_string(),
            "Model not installed".to_string(),
            Some("Run: dybur models prefetch".to_string()),
        )
    }
}

fn check_audio() -> (String, String, String, Option<String>) {
    let devices = list_input_devices();
    if devices.is_empty() {
        (
            "Audio Device".to_string(),
            "fail".to_string(),
            "No input devices found".to_string(),
            Some("Connect a microphone".to_string()),
        )
    } else {
        (
            "Audio Device".to_string(),
            "pass".to_string(),
            format!("{} device(s) found", devices.len()),
            None,
        )
    }
}

fn check_hotkey() -> (String, String, String, Option<String>) {
    match load_config() {
        Ok(config) => {
            if config.hotkey.is_empty() {
                (
                    "Hotkey".to_string(),
                    "fail".to_string(),
                    "No hotkey configured".to_string(),
                    None,
                )
            } else {
                (
                    "Hotkey".to_string(),
                    "pass".to_string(),
                    config.hotkey,
                    None,
                )
            }
        }
        Err(_) => (
            "Hotkey".to_string(),
            "warn".to_string(),
            "Could not check".to_string(),
            None,
        ),
    }
}

fn check_directories() -> (String, String, String, Option<String>) {
    let data_dir = get_data_dir();

    if data_dir.exists() {
        (
            "Directories".to_string(),
            "pass".to_string(),
            "All directories accessible".to_string(),
            None,
        )
    } else {
        (
            "Directories".to_string(),
            "warn".to_string(),
            "Data directory missing".to_string(),
            Some("Will be created on first use".to_string()),
        )
    }
}

// ============================================================================
// Models Command
// ============================================================================

fn cmd_models(args: &[String]) {
    let subcmd = args.first().map(|s| s.as_str());

    match subcmd {
        Some("list") | None => cmd_models_list(),
        Some("prefetch") | Some("download") => cmd_models_prefetch(),
        Some("clean") => cmd_models_clean(),
        Some("--help") | Some("-h") => show_models_help(),
        Some(other) => {
            error(&format!("Unknown subcommand: {}", other));
            println!();
            show_models_help();
            std::process::exit(1);
        }
    }
}

fn show_models_help() {
    header("Model Management");

    println!("  {}", dim("dybur uses NVIDIA Parakeet for speech recognition."));
    println!("  {}", dim("Models are downloaded from HuggingFace on first use."));
    println!();

    divider();
    println!();

    println!("  {}", accent("Commands"));
    command("models list", "List installed models");
    command("models prefetch", "Download default model");
    command("models clean", "Remove unused models");
    println!();

    println!("  {}", accent("Default Model"));
    println!("  {} {}", dim("Name:"), DEFAULT_MODEL);
    println!("  {} huggingface.co/nvidia/parakeet-tdt-0.6b-v2", dim("Source:"));
    println!("  {} ~670 MB (INT8 quantized)", dim("Size:"));
    println!();
}

fn cmd_models_list() {
    header("Installed Models");

    let models = list_models();
    let models_dir = get_data_dir().join("models");

    if models.is_empty() {
        info("No models installed");
        println!();
        println!("  {}", dim("To install the default model:"));
        println!("  {}", cyan("dybur models prefetch"));
        println!();
        println!("  {} {}", dim("Models directory:"), format_path(&models_dir.to_string_lossy(), 45));
        println!();
        return;
    }

    for model in models {
        let default_badge = if model.is_default {
            format!(" {}", green("[default]"))
        } else {
            String::new()
        };
        let size = format_bytes(model.size);

        println!("  {} {}{}", accent("•"), model.name, default_badge);
        println!("    {} {}", dim("Size:"), size);
        println!();
    }

    divider();
    println!();
    println!("  {} {}", dim("Models directory:"), format_path(&models_dir.to_string_lossy(), 45));
    println!();
}

fn cmd_models_prefetch() {
    header("Download Model");

    if is_default_model_installed() {
        success(&format!("Model already installed: {}", DEFAULT_MODEL));
        println!();
        return;
    }

    println!("  {} {}", dim("Model:"), DEFAULT_MODEL);
    println!("  {} huggingface.co/nvidia/parakeet-tdt-0.6b-v2", dim("Source:"));
    println!("  {} INT8 quantized (~670 MB)", dim("Variant:"));
    println!();

    divider();
    println!();

    match download_model_sync(DEFAULT_MODEL, "int8") {
        Ok(_path) => {
            println!();
            divider();
            println!();
            success("Model downloaded successfully");
            println!();
            info(&format!("Run {} to begin", cyan("dybur start")));
            println!();
        }
        Err(e) => {
            println!();
            error(&format!("Download failed: {}", e));
            println!();
            info("Check your internet connection and try again");
            println!();
            std::process::exit(1);
        }
    }
}

fn cmd_models_clean() {
    header("Clean Models");

    for i in 0..8 {
        spinner_frame(i, "Scanning for unused models");
        std::thread::sleep(std::time::Duration::from_millis(80));
    }

    let removed = clean_models();

    clear_line();

    if removed.is_empty() {
        info("No unused models to remove");
        println!();
        return;
    }

    success(&format!("Removed {} model(s):", removed.len()));
    println!();

    for name in removed {
        println!("  {} {}", dim("•"), name);
    }

    println!();
}

// ============================================================================
// Devices Command
// ============================================================================

fn cmd_devices(args: &[String]) {
    let subcmd = args.first().map(|s| s.as_str());

    match subcmd {
        Some("list") | Some("l") | None => cmd_devices_list(),
        Some("set") | Some("s") => {
            let name = args.iter().skip(1).cloned().collect::<Vec<_>>().join(" ");
            cmd_devices_set(&name);
        }
        Some("reset") | Some("default") | Some("r") => cmd_devices_reset(),
        Some("--help") | Some("-h") | Some("help") | Some("h") => show_devices_help(),
        Some(other) => {
            error(&format!("Unknown subcommand: {}", other));
            println!();
            show_devices_help();
            std::process::exit(1);
        }
    }
}

fn show_devices_help() {
    header("Input Device Management");

    println!("  {}", dim("Configure which microphone to use for voice dictation."));
    println!("  {}", dim("Set to null/default to use system default microphone."));
    println!();

    divider();
    println!();

    println!("  {}", accent("Commands"));
    command("d, d l, d list", "Select input device interactively");
    command("d set <name>", "Select a specific microphone");
    command("d reset", "Reset to system default");
    println!();

    println!("  {}", accent("Examples"));
    println!("  {}           {}", cyan("dybur d"), dim("Interactive device selection"));
    println!("  {}         {}", cyan("dybur d l"), dim("Same as above"));
    println!("  {} {}", cyan("dybur d set \"Mic\""), dim("Set device by name"));
    println!("  {}     {}", cyan("dybur d reset"), dim("Use system default"));
    println!();
}

fn cmd_devices_list() {
    header("Input Devices");

    let config = load_config().unwrap_or_default();
    let current_device = config.input_device.as_deref();

    println!(
        "  {} {}",
        dim("Current:"),
        current_device.map(|d| cyan(d)).unwrap_or_else(|| dim("System default"))
    );
    println!();

    let devices = list_input_devices();

    if devices.is_empty() {
        warning("Could not enumerate audio devices");
        println!();
        println!("  {}", dim("To set a device manually, use:"));
        println!("  {}", cyan("dybur d set \"Device Name\""));
        println!();
        return;
    }

    // Build choices for interactive selection
    let mut choices: Vec<(String, Option<String>, Option<String>)> = vec![(
        "System default".to_string(),
        None,
        Some("use OS default microphone".to_string()),
    )];

    for device in &devices {
        let hint = if device.is_default {
            Some("system default".to_string())
        } else {
            None
        };
        choices.push((device.name.clone(), Some(device.name.clone()), hint));
    }

    // Find current selection
    let initial = if current_device.is_none() {
        0
    } else {
        choices
            .iter()
            .position(|(_, v, _)| v.as_deref() == current_device)
            .unwrap_or(0)
    };

    let selected = select("Select input device", &choices, initial);

    match selected {
        None => {
            info("Selection cancelled");
            println!();
        }
        Some(None) => {
            // Reset to system default
            let mut config = load_config().unwrap_or_default();
            config.input_device = None;
            let _ = save_config(&config);
            success("Input device reset to system default");
            println!();
            info("Changes will take effect on the next recording");
            println!();
        }
        Some(Some(device_name)) => {
            let mut config = load_config().unwrap_or_default();
            config.input_device = Some(device_name.clone());
            let _ = save_config(&config);
            success(&format!("Input device set to: {}", cyan(&device_name)));
            println!();
            info("Changes will take effect on the next recording");
            println!();
            println!("  {} {}", yellow("⚠"), dim("If the service is running, restart it:"));
            println!("    {}", cyan("dybur stop && dybur start"));
            println!();
        }
    }
}

fn cmd_devices_set(name: &str) {
    header("Set Input Device");

    if name.trim().is_empty() {
        error("Device name is required");
        println!();
        println!("  {} {}", dim("Usage:"), cyan("dybur devices set \"<device name>\""));
        println!();
        println!("  {} {}", dim("Example:"), cyan("dybur devices set \"Microphone (Realtek)\""));
        println!();
        std::process::exit(1);
    }

    let clean_name = name.trim_matches(|c| c == '"' || c == '\'').trim();

    let mut config = load_config().unwrap_or_default();
    config.input_device = Some(clean_name.to_string());

    match save_config(&config) {
        Ok(()) => {
            success(&format!("Input device set to: {}", cyan(clean_name)));
            println!();
            info("Changes will take effect on the next recording");
            println!();
            println!("  {} {}", yellow("⚠"), dim("If the service is running, restart it:"));
            println!("    {}", cyan("dybur stop && dybur start"));
            println!();
        }
        Err(e) => {
            error(&format!("Failed to update configuration: {}", e));
            std::process::exit(1);
        }
    }
}

fn cmd_devices_reset() {
    header("Reset Input Device");

    let mut config = load_config().unwrap_or_default();
    config.input_device = None;

    match save_config(&config) {
        Ok(()) => {
            success("Input device reset to system default");
            println!();
            info("Changes will take effect on the next recording");
            println!();
        }
        Err(e) => {
            error(&format!("Failed to update configuration: {}", e));
            std::process::exit(1);
        }
    }
}

// ============================================================================
// Setup Command
// ============================================================================

/// Check if the bin directory is in PATH (Windows)
#[cfg(target_os = "windows")]
fn check_windows_path(bin_dir: &std::path::Path) {
    let bin_dir_str = bin_dir.to_string_lossy();
    if let Ok(path) = std::env::var("PATH") {
        let in_path = path
            .split(';')
            .any(|p| p.eq_ignore_ascii_case(&bin_dir_str));
        if in_path {
            info("PATH is already configured.");
            info("You can now use 'dybur' from any terminal.");
        } else {
            warning(&format!(
                "{} is not in your PATH.",
                bin_dir_str
            ));
            info("Run 'dybur setup' again to add it, or add it manually.");
        }
    }
}

/// Add the bin directory to user PATH on Windows
#[cfg(target_os = "windows")]
fn add_to_windows_path(bin_dir: &std::path::Path) {
    use std::process::Command;

    let bin_dir_str = bin_dir.to_string_lossy().to_string();

    // Check if already in PATH
    if let Ok(path) = std::env::var("PATH") {
        let in_path = path
            .split(';')
            .any(|p| p.eq_ignore_ascii_case(&bin_dir_str));
        if in_path {
            info("PATH is already configured.");
            info("You can now use 'dybur' from any terminal.");
            return;
        }
    }

    info(&format!("Adding {} to PATH...", bin_dir_str));

    // Use PowerShell to modify the user PATH via registry
    // This is more reliable than setx which has a 1024 char limit
    let ps_script = format!(
        r#"
        $currentPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
        if ($currentPath -split ';' | Where-Object {{ $_.ToLower() -eq '{}'.ToLower() }}) {{
            Write-Host 'Already in PATH'
        }} else {{
            $newPath = if ($currentPath) {{ "$currentPath;{}" }} else {{ "{}" }}
            [Environment]::SetEnvironmentVariable('PATH', $newPath, 'User')
            Write-Host 'PATH updated'
        }}
        "#,
        bin_dir_str, bin_dir_str, bin_dir_str
    );

    match Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .output()
    {
        Ok(output) => {
            if output.status.success() {
                success("PATH updated successfully!");
                println!();
                warning("Please restart your terminal for changes to take effect.");
                info("After restarting, you can use 'dybur' from any terminal.");
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                error(&format!("Failed to update PATH: {}", stderr));
                println!();
                info("You can manually add the following to your PATH:");
                println!("  {}", bin_dir_str);
            }
        }
        Err(e) => {
            error(&format!("Failed to run PowerShell: {}", e));
            println!();
            info("You can manually add the following to your PATH:");
            println!("  {}", bin_dir_str);
        }
    }
}

fn cmd_setup() {
    #[cfg(target_os = "windows")]
    {
        let current_exe = match std::env::current_exe() {
            Ok(path) => path,
            Err(e) => {
                error(&format!("Failed to get current executable path: {}", e));
                return;
            }
        };

        let bin_dir = config::get_bin_dir();
        let target_path = bin_dir.join("dybur.exe");

        // Create bin directory if it doesn't exist
        if let Err(e) = std::fs::create_dir_all(&bin_dir) {
            error(&format!("Failed to create bin directory: {}", e));
            return;
        }

        // Check if already installed and up to date
        if target_path.exists() {
            // Compare file sizes as a simple check
            let current_size = std::fs::metadata(&current_exe).map(|m| m.len()).unwrap_or(0);
            let target_size = std::fs::metadata(&target_path).map(|m| m.len()).unwrap_or(0);
            if current_size == target_size && current_exe.canonicalize().ok() == target_path.canonicalize().ok() {
                success(&format!("dybur CLI is already installed at {}", target_path.display()));
                check_windows_path(&bin_dir);
                return;
            }
            info("Updating existing installation...");
        }

        // Copy executable to bin directory
        info(&format!("Installing dybur CLI to {}...", target_path.display()));
        if let Err(e) = std::fs::copy(&current_exe, &target_path) {
            error(&format!("Failed to copy executable: {}", e));
            return;
        }

        success("dybur CLI installed successfully!");
        println!();

        // Check and update PATH
        add_to_windows_path(&bin_dir);

        return;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let current_exe = match std::env::current_exe() {
            Ok(path) => path,
            Err(e) => {
                error(&format!("Failed to get current executable path: {}", e));
                return;
            }
        };

        let target_path = PathBuf::from("/usr/local/bin/dybur");

        let usr_local_bin = PathBuf::from("/usr/local/bin");
        if !usr_local_bin.exists() {
            error("/usr/local/bin does not exist.");
            info("Create it with: sudo mkdir -p /usr/local/bin");
            return;
        }

        if target_path.exists() {
            if let Ok(existing) = std::fs::read_link(&target_path) {
                if existing == current_exe {
                    success(&format!(
                        "dybur CLI is already installed at {}",
                        target_path.display()
                    ));
                    return;
                }
            }
            info("Removing existing installation...");
            if std::fs::remove_file(&target_path).is_err() {
                error(&format!(
                    "Failed to remove existing installation. Try: sudo rm {}",
                    target_path.display()
                ));
                info(&format!(
                    "Then run: sudo ln -s {} {}",
                    current_exe.display(),
                    target_path.display()
                ));
                return;
            }
        }

        info(&format!("Installing dybur CLI to {}...", target_path.display()));

        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            match symlink(&current_exe, &target_path) {
                Ok(()) => {
                    success("dybur CLI installed successfully!");
                    println!();
                    info("You can now use 'dybur' from any terminal.");
                    info("Run 'dybur --help' to see available commands.");
                }
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        error("Permission denied. Try running with sudo:");
                        println!(
                            "  sudo ln -s {} {}",
                            current_exe.display(),
                            target_path.display()
                        );
                    } else {
                        error(&format!("Failed to create symlink: {}", e));
                    }
                }
            }
        }
    }
}

// ============================================================================
// Uninstall Command
// ============================================================================

fn cmd_uninstall() {
    header("Uninstall dybur");

    // Collect all paths that will be removed
    let data_dir = get_data_dir();
    let config_dir = config::get_config_dir();
    let logs_dir = get_logs_dir();

    println!("  {}", dim("The following will be removed:"));
    println!();

    // Data directory (models, bin)
    if data_dir.exists() {
        println!("  {} {} {}", red("•"), data_dir.display(), dim("(data, models, CLI)"));
    }

    // Config directory
    if config_dir.exists() {
        println!("  {} {} {}", red("•"), config_dir.display(), dim("(configuration)"));
    }

    // Logs directory
    let logs_path = PathBuf::from(&logs_dir);
    if logs_path.exists() {
        println!("  {} {} {}", red("•"), logs_dir, dim("(logs)"));
    }

    // Platform-specific paths
    #[cfg(target_os = "macos")]
    {
        let symlink_path = PathBuf::from("/usr/local/bin/dybur");
        if symlink_path.exists() {
            println!("  {} {} {}", red("•"), symlink_path.display(), dim("(CLI symlink)"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let bin_dir = config::get_bin_dir();
        println!("  {} {} {}", red("•"), dim("PATH entry"), dim(&format!("({})", bin_dir.display())));
    }

    println!();
    divider();
    println!();

    // Confirmation
    let choices: Vec<(String, bool, Option<String>)> = vec![
        ("No, cancel".to_string(), false, Some("keep dybur installed".to_string())),
        ("Yes, uninstall".to_string(), true, Some("remove everything".to_string())),
    ];

    let confirmed = select("Are you sure you want to completely remove dybur?", &choices, 0);

    match confirmed {
        Some(true) => {
            println!();
            perform_uninstall();
        }
        _ => {
            println!();
            info("Uninstall cancelled");
            println!();
        }
    }
}

fn perform_uninstall() {
    // Stop running process first
    if is_dybur_running() {
        for i in 0..8 {
            spinner_frame(i, "Stopping dybur service");
            std::thread::sleep(std::time::Duration::from_millis(80));
        }
        stop_dybur_process();
        clear_line();
        success("Service stopped");
    }

    let mut errors: Vec<String> = Vec::new();

    // Remove data directory (includes models and bin)
    let data_dir = get_data_dir();
    if data_dir.exists() {
        for i in 0..6 {
            spinner_frame(i, "Removing data directory");
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        if let Err(e) = std::fs::remove_dir_all(&data_dir) {
            errors.push(format!("Failed to remove {}: {}", data_dir.display(), e));
        }
        clear_line();
    }

    // Remove config directory
    let config_dir = config::get_config_dir();
    if config_dir.exists() {
        for i in 0..6 {
            spinner_frame(i, "Removing configuration");
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        if let Err(e) = std::fs::remove_dir_all(&config_dir) {
            errors.push(format!("Failed to remove {}: {}", config_dir.display(), e));
        }
        clear_line();
    }

    // Remove logs directory
    let logs_dir = PathBuf::from(get_logs_dir());
    if logs_dir.exists() {
        for i in 0..6 {
            spinner_frame(i, "Removing logs");
            std::thread::sleep(std::time::Duration::from_millis(60));
        }
        if let Err(e) = std::fs::remove_dir_all(&logs_dir) {
            errors.push(format!("Failed to remove {}: {}", logs_dir.display(), e));
        }
        clear_line();
    }

    // Platform-specific cleanup
    #[cfg(target_os = "macos")]
    {
        let symlink_path = PathBuf::from("/usr/local/bin/dybur");
        if symlink_path.exists() {
            for i in 0..6 {
                spinner_frame(i, "Removing CLI symlink");
                std::thread::sleep(std::time::Duration::from_millis(60));
            }
            if let Err(e) = std::fs::remove_file(&symlink_path) {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    errors.push(format!(
                        "Permission denied removing symlink. Run: sudo rm {}",
                        symlink_path.display()
                    ));
                } else {
                    errors.push(format!("Failed to remove symlink: {}", e));
                }
            }
            clear_line();
        }
    }

    #[cfg(target_os = "windows")]
    {
        remove_from_windows_path();
    }

    println!();

    if errors.is_empty() {
        success("dybur has been completely removed");
        println!();
        info("Thank you for using dybur!");
    } else {
        warning("Uninstall completed with some errors:");
        println!();
        for err in &errors {
            println!("  {} {}", red("•"), err);
        }
        println!();
        info("You may need to manually remove the items listed above");
    }

    println!();
}

/// Remove the bin directory from user PATH on Windows
#[cfg(target_os = "windows")]
fn remove_from_windows_path() {
    use std::process::Command;

    let bin_dir = config::get_bin_dir();
    let bin_dir_str = bin_dir.to_string_lossy().to_string();

    for i in 0..6 {
        spinner_frame(i, "Removing from PATH");
        std::thread::sleep(std::time::Duration::from_millis(60));
    }

    let ps_script = format!(
        r#"
        $currentPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
        $pathArray = $currentPath -split ';' | Where-Object {{ $_.ToLower() -ne '{}'.ToLower() -and $_ -ne '' }}
        $newPath = $pathArray -join ';'
        [Environment]::SetEnvironmentVariable('PATH', $newPath, 'User')
        Write-Host 'PATH updated'
        "#,
        bin_dir_str
    );

    let _ = Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .output();

    clear_line();
}

// ============================================================================
// Utility Functions
// ============================================================================

fn is_dybur_running() -> bool {
    #[cfg(target_os = "windows")]
    {
        let output = Command::new("tasklist")
            .args(["/FI", "IMAGENAME eq dybur.exe"])
            .output()
            .ok();

        if let Some(out) = output {
            let stdout = String::from_utf8_lossy(&out.stdout);
            return stdout.contains("dybur.exe");
        }
        false
    }

    #[cfg(not(target_os = "windows"))]
    {
        let output = Command::new("pgrep").args(["-f", "dybur"]).output().ok();

        if let Some(out) = output {
            return out.status.success();
        }
        false
    }
}

fn stop_dybur_process() -> bool {
    #[cfg(target_os = "windows")]
    {
        let result = Command::new("taskkill")
            .args(["/IM", "dybur.exe", "/F"])
            .output();

        matches!(result, Ok(output) if output.status.success())
    }

    #[cfg(not(target_os = "windows"))]
    {
        let result = Command::new("pkill").args(["-f", "dybur"]).output();

        matches!(result, Ok(output) if output.status.success())
    }
}

fn find_tray_executable() -> Option<PathBuf> {
    let candidates = vec![
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .map(|d| d.join(if cfg!(windows) { "dybur.exe" } else { "dybur-app" })),
        #[cfg(target_os = "windows")]
        Some(PathBuf::from(r"C:\Program Files\dybur\dybur.exe")),
        #[cfg(target_os = "windows")]
        dirs::data_local_dir().map(|d| d.join("dybur").join("dybur.exe")),
        #[cfg(target_os = "macos")]
        Some(PathBuf::from("/Applications/dybur.app")),
        #[cfg(target_os = "macos")]
        dirs::home_dir().map(|d| d.join("Applications/dybur.app")),
        #[cfg(target_os = "linux")]
        Some(PathBuf::from("/usr/bin/dybur")),
        #[cfg(target_os = "linux")]
        Some(PathBuf::from("/usr/local/bin/dybur")),
    ];

    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}
