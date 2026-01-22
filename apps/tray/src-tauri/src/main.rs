//! dybur Tray Application
//!
//! Background service for voice dictation with system tray integration.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod doctor;
mod ftue;
mod hotkey;
mod injection;
mod logging;
mod models;
mod privacy;
mod single_instance;
mod state;
mod stt;

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::cell::RefCell;
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder,
};
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use audio::{AudioCapture, get_audio_error_help, list_input_devices};
use config::{get_config_path, save_config};
use injection::inject_text;
use models::{list_models, is_default_model_installed, format_bytes, DEFAULT_MODEL, is_download_in_progress, get_download_status};
use state::{AppState, RecordingState};
use stt::{SttEngine, get_model_paths};

// Thread-local storage for AudioCapture (not Send+Sync due to cpal::Stream)
thread_local! {
    static AUDIO_CAPTURE: RefCell<Option<AudioCapture>> = const { RefCell::new(None) };
}

// Global STT engine (wrapped in Mutex for thread safety)
lazy_static::lazy_static! {
    static ref STT_ENGINE: Mutex<SttEngine> = Mutex::new(SttEngine::new());
}

// Flag to track explicit quit request (to bypass prevent_exit)
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

const OVERLAY_LABEL: &str = "overlay";
const OVERLAY_WIDTH: f64 = 260.0;
const OVERLAY_HEIGHT: f64 = 64.0;
const OVERLAY_MARGIN: f64 = 28.0;

/// Application entry point
fn main() {
    // Check for single instance
    let _instance_guard = acquire_single_instance();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(Mutex::new(AppState::new()))
        .invoke_handler(tauri::generate_handler![
            ftue::ftue_get_config,
            ftue::ftue_get_platform,
            ftue::ftue_run_system_check,
            ftue::ftue_start_download,
            ftue::ftue_complete,
            ftue::ftue_skip,
            ftue::ftue_check_model_installed,
            ftue::ftue_close,
        ])
        .setup(|app| {
            // Load configuration
            let state = app.state::<Mutex<AppState>>();
            let model_name: String;
            {
                let mut state_guard = state.inner().lock().unwrap();
                if let Err(e) = state_guard.load_config() {
                    log_error!("config", "Failed to load config: {}", e);
                } else {
                    log_info!("config", "Configuration loaded successfully");
                }
                model_name = state_guard.config.model.clone();
            }

            // Load STT model
            if let Some(stt_config) = get_model_paths(&model_name) {
                let mut engine = STT_ENGINE.lock().unwrap();
                match engine.load(stt_config) {
                    Ok(()) => {
                        log_info!("model", "STT model '{}' loaded and ready", model_name);
                    }
                    Err(e) => {
                        log_error!("model", "Failed to load STT model '{}': {}", model_name, e);
                    }
                }
            } else {
                log_warn!(
                    "model",
                    "STT model '{}' not found. Run 'dybur models prefetch' to download it.",
                    model_name
                );
            }

            // Create tray menu
            let menu = create_tray_menu(app.handle())?;

            // Create overlay window for recording indicator
            create_overlay_window(app.handle());

            // Create tray icon
            let _tray = TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().unwrap())
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    handle_menu_event(app, event.id.as_ref());
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        // Toggle recording on left click
                        let app = tray.app_handle();
                        toggle_recording(app);
                    }
                })
                .build(app)?;

            // Register global hotkey
            let hotkey = {
                let state_guard = state.inner().lock().unwrap();
                state_guard.config.hotkey.clone()
            };

            if let Err(e) = register_hotkey(app.handle(), &hotkey) {
                report_hotkey_error(app.handle(), &hotkey, &e);
            } else {
                log_info!(
                    "hotkey",
                    "Global hotkey '{}' registered successfully",
                    hotkey
                );
            }

            log_info!("service", "dybur started. Press {} to dictate.", hotkey);

            // Auto-install CLI to PATH on macOS (first launch)
            #[cfg(target_os = "macos")]
            {
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    check_and_install_cli(&app_handle);
                });
            }

            // Auto-install CLI to PATH on Windows (first launch)
            #[cfg(target_os = "windows")]
            {
                let app_handle = app.handle().clone();
                std::thread::spawn(move || {
                    check_and_install_cli_windows(&app_handle);
                });
            }

            // Show FTUE window if needed (first launch or model not installed)
            if ftue::should_show_ftue() {
                let app_handle = app.handle().clone();
                // Delay slightly to let the tray icon appear first
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    if let Err(e) = ftue::show_ftue_window(&app_handle) {
                        log_error!("ftue", "Failed to show FTUE window: {}", e);
                    }
                });
            }

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app_handle, event| {
            if let RunEvent::ExitRequested { api, .. } = event {
                // Only keep running in background if quit wasn't explicitly requested
                if !QUIT_REQUESTED.load(Ordering::SeqCst) {
                    api.prevent_exit();
                }
            }
        });
}

/// Create the tray context menu with submenus
fn create_tray_menu(app: &tauri::AppHandle) -> Result<tauri::menu::Menu<tauri::Wry>, tauri::Error> {
    // Main toggle and status
    let toggle = MenuItemBuilder::with_id("toggle", "Start Dictation").build(app)?;
    let status = MenuItemBuilder::with_id("status", "Status: Idle")
        .enabled(false)
        .build(app)?;

    // Models submenu
    let models_submenu = build_models_submenu(app)?;

    // Devices submenu
    let devices_submenu = build_devices_submenu(app)?;

    // Settings submenu
    let open_config = MenuItemBuilder::with_id("open_config", "Open Config File").build(app)?;
    let run_diagnostics = MenuItemBuilder::with_id("run_diagnostics", "Run Diagnostics").build(app)?;
    let run_setup = MenuItemBuilder::with_id("run_setup", "Run Setup Wizard...").build(app)?;
    #[cfg(not(target_os = "windows"))]
    let install_cli = MenuItemBuilder::with_id("install_cli", "Install Command Line Tool...").build(app)?;
    let about = MenuItemBuilder::with_id("about", "About dybur").build(app)?;

    let mut settings_builder = SubmenuBuilder::with_id(app, "settings", "Settings")
        .item(&open_config)
        .item(&run_diagnostics)
        .item(&run_setup);

    #[cfg(not(target_os = "windows"))]
    {
        settings_builder = settings_builder.item(&install_cli);
    }

    let settings_submenu = settings_builder
        .separator()
        .item(&about)
        .build()?;

    // Main menu items
    let logs = MenuItemBuilder::with_id("logs", "Open Logs").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit dybur").build(app)?;

    MenuBuilder::new(app)
        .item(&toggle)
        .item(&status)
        .separator()
        .item(&models_submenu)
        .item(&devices_submenu)
        .separator()
        .item(&settings_submenu)
        .item(&logs)
        .separator()
        .item(&quit)
        .build()
}

/// Build the Models submenu
fn build_models_submenu(app: &tauri::AppHandle) -> Result<tauri::menu::Submenu<tauri::Wry>, tauri::Error> {
    let mut submenu = SubmenuBuilder::with_id(app, "models", "Models");

    // Check for download in progress first
    if let Some(status) = get_download_status() {
        let status_item = MenuItemBuilder::with_id("download_status", &status)
            .enabled(false)
            .build(app)?;
        submenu = submenu.item(&status_item);
        submenu = submenu.separator();
    }

    // List installed models
    let installed_models = list_models();

    if installed_models.is_empty() {
        let no_models = MenuItemBuilder::with_id("no_models", "No models installed")
            .enabled(false)
            .build(app)?;
        submenu = submenu.item(&no_models);
    } else {
        for model in &installed_models {
            let check_mark = if model.is_default { "✓ " } else { "  " };
            let size_str = format_bytes(model.size);
            let label = format!("{}{} ({})", check_mark, model.name, size_str);
            let item = MenuItemBuilder::with_id(format!("model_{}", model.name), label)
                .enabled(false)
                .build(app)?;
            submenu = submenu.item(&item);
        }
    }

    submenu = submenu.separator();

    // Disable download button if download already in progress
    let download_in_progress = is_download_in_progress();
    let download_label = if download_in_progress {
        "Downloading..."
    } else {
        "Download Model..."
    };
    let download_model = MenuItemBuilder::with_id("download_model", download_label)
        .enabled(!download_in_progress)
        .build(app)?;
    let clean_models = MenuItemBuilder::with_id("clean_models", "Clean Unused").build(app)?;

    submenu = submenu.item(&download_model).item(&clean_models);

    submenu.build()
}

/// Build the Devices submenu
fn build_devices_submenu(app: &tauri::AppHandle) -> Result<tauri::menu::Submenu<tauri::Wry>, tauri::Error> {
    let mut submenu = SubmenuBuilder::with_id(app, "devices", "Devices");

    // Get current configured device
    let state = app.state::<Mutex<AppState>>();
    let configured_device = {
        let state_guard = state.inner().lock().unwrap();
        state_guard.config.input_device.clone()
    };

    // System default option
    let is_default_selected = configured_device.is_none();
    let default_label = if is_default_selected {
        "● System Default"
    } else {
        "  System Default"
    };
    let default_item = MenuItemBuilder::with_id("device_default", default_label).build(app)?;
    submenu = submenu.item(&default_item);

    submenu = submenu.separator();

    // List available devices
    let devices = list_input_devices();
    for device in &devices {
        let is_selected = configured_device.as_ref().map(|d| d == &device.name).unwrap_or(false);
        let prefix = if is_selected { "● " } else { "  " };
        let suffix = if device.is_default { " (default)" } else { "" };
        let label = format!("{}{}{}", prefix, device.name, suffix);

        // Use a sanitized ID for the device
        let device_id = format!("device_{}", sanitize_menu_id(&device.name));
        let item = MenuItemBuilder::with_id(&device_id, label).build(app)?;
        submenu = submenu.item(&item);
    }

    if devices.is_empty() {
        let no_devices = MenuItemBuilder::with_id("no_devices", "No devices found")
            .enabled(false)
            .build(app)?;
        submenu = submenu.item(&no_devices);
    }

    submenu.build()
}

/// Sanitize a string to be a valid menu ID
fn sanitize_menu_id(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// Handle tray menu events
fn handle_menu_event(app: &tauri::AppHandle, event_id: &str) {
    match event_id {
        "toggle" => {
            toggle_recording(app);
        }
        "logs" => {
            open_logs(app);
        }
        "quit" => {
            QUIT_REQUESTED.store(true, Ordering::SeqCst);
            app.exit(0);
        }
        "open_config" => {
            open_config_file(app);
        }
        "run_diagnostics" => {
            run_diagnostics(app);
        }
        "about" => {
            show_about(app);
        }
        "run_setup" => {
            run_setup_wizard(app);
        }
        #[cfg(not(target_os = "windows"))]
        "install_cli" => {
            install_cli_to_path(app);
        }
        "download_model" => {
            spawn_model_download(app);
        }
        "clean_models" => {
            clean_unused_models(app);
        }
        "device_default" => {
            select_device(app, None);
        }
        id if id.starts_with("device_") && id != "device_default" => {
            // Extract device name from ID
            let device_name = id.strip_prefix("device_").unwrap();
            // Reverse the sanitization to find the actual device
            let devices = list_input_devices();
            if let Some(device) = devices.iter().find(|d| sanitize_menu_id(&d.name) == device_name) {
                select_device(app, Some(device.name.clone()));
            }
        }
        _ => {}
    }
}

/// Open the config file in the default editor
fn open_config_file(app: &tauri::AppHandle) {
    let config_path = get_config_path();
    log_info!("service", "Opening config file: {}", config_path.display());

    #[allow(deprecated)]
    if let Err(e) = tauri_plugin_shell::ShellExt::shell(app)
        .open(config_path.to_string_lossy().as_ref(), None::<tauri_plugin_shell::open::Program>)
    {
        log_error!("service", "Failed to open config file: {:?}", e);
    }
}

/// Run diagnostics and show results via notification
fn run_diagnostics(app: &tauri::AppHandle) {
    log_info!("service", "Running diagnostics...");

    let results = doctor::run_diagnostics();

    let mut passed = 0;
    let mut warnings = 0;
    let mut failed = 0;

    for result in &results {
        match result.status {
            doctor::DiagnosticStatus::Pass => passed += 1,
            doctor::DiagnosticStatus::Warn => warnings += 1,
            doctor::DiagnosticStatus::Fail => failed += 1,
        }
        log_info!(
            "doctor",
            "[{:?}] {}: {}",
            result.status,
            result.name,
            result.message
        );
        if let Some(details) = &result.details {
            log_info!("doctor", "  Details: {}", details);
        }
    }

    let summary = format!(
        "Diagnostics complete: {} passed, {} warnings, {} failed",
        passed, warnings, failed
    );
    log_info!("doctor", "{}", summary);

    // Show notification with results
    #[cfg(target_os = "windows")]
    show_windows_notification("dybur Diagnostics", &summary);
    #[cfg(target_os = "macos")]
    show_macos_notification("dybur Diagnostics", &summary);

    // Open logs so user can see details
    open_logs(app);
}

/// Run the setup wizard (FTUE)
fn run_setup_wizard(app: &tauri::AppHandle) {
    log_info!("service", "Opening setup wizard...");

    // Reset FTUE state to allow re-running
    let mut state = ftue::load_ftue_state();
    state.completed = false;
    state.skipped = false;
    state.current_step = 1;
    let _ = ftue::save_ftue_state(&state);

    // Show the FTUE window
    let app_handle = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = ftue::show_ftue_window(&app_handle) {
            log_error!("service", "Failed to show setup wizard: {}", e);
        }
    });
}

/// Show about dialog
fn show_about(_app: &tauri::AppHandle) {
    let version = env!("CARGO_PKG_VERSION");
    let message = format!(
        "dybur v{}\n\nLocal voice dictation powered by AI.\n\nPress your hotkey to start dictating.",
        version
    );
    log_info!("service", "About: dybur v{}", version);

    #[cfg(target_os = "windows")]
    show_windows_notification("About dybur", &message);
    #[cfg(target_os = "macos")]
    show_macos_notification("About dybur", &message);
}

/// Check if CLI is installed and prompt to install on first launch (macOS only)
#[cfg(target_os = "macos")]
fn check_and_install_cli(app: &tauri::AppHandle) {
    use std::process::Command;
    use std::path::Path;
    use tauri_plugin_shell::ShellExt;

    let cli_path = Path::new("/usr/local/bin/dybur");

    // Check if already installed
    if cli_path.exists() {
        log_info!("service", "CLI already installed at /usr/local/bin/dybur");
        return;
    }

    // Get the sidecar path using Tauri's API
    // This returns the command, but we need the actual path for symlinking
    // The sidecar is in Contents/MacOS/ with target triple suffix
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log_error!("service", "Failed to get executable path: {}", e);
            return;
        }
    };

    let macos_dir = match exe_path.parent() {
        Some(p) => p,
        None => {
            log_error!("service", "Failed to get MacOS directory");
            return;
        }
    };

    // Try to find the sidecar - Tauri bundles it without the target triple suffix
    let possible_names = [
        "dybur",  // Tauri bundles sidecar without suffix
        "dybur-aarch64-apple-darwin",
        "dybur-x86_64-apple-darwin",
    ];

    let sidecar_path = possible_names
        .iter()
        .map(|name| macos_dir.join(name))
        .find(|p| p.exists());

    let sidecar_path = match sidecar_path {
        Some(p) => p,
        None => {
            // Log what we found in the directory for debugging
            if let Ok(entries) = std::fs::read_dir(macos_dir) {
                let files: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect();
                log_warn!("service", "Sidecar not found. Files in MacOS dir: {:?}", files);
            }
            log_warn!("service", "Sidecar binary not found in {}, skipping CLI install", macos_dir.display());
            return;
        }
    };

    log_info!("service", "Found sidecar at: {}", sidecar_path.display());
    log_info!("service", "Installing CLI to /usr/local/bin/dybur...");

    // Use osascript to run with administrator privileges
    // This will show the macOS password dialog
    let script = format!(
        r#"do shell script "mkdir -p /usr/local/bin && ln -sf '{}' /usr/local/bin/dybur" with administrator privileges"#,
        sidecar_path.display()
    );

    let result = Command::new("osascript")
        .args(["-e", &script])
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                log_info!("service", "CLI installed successfully to /usr/local/bin/dybur");
                show_macos_notification(
                    "CLI Installed",
                    "You can now use 'dybur' command in any terminal.",
                );
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                if stderr.contains("User canceled") || stderr.contains("canceled") {
                    log_info!("service", "User cancelled CLI installation");
                } else {
                    log_error!("service", "CLI installation failed: {}", stderr);
                }
            }
        }
        Err(e) => {
            log_error!("service", "Failed to run osascript: {}", e);
        }
    }
}

/// Check if CLI is installed and install on first launch (Windows only)
#[cfg(target_os = "windows")]
fn check_and_install_cli_windows(_app: &tauri::AppHandle) {
    use std::process::Command;

    // Get the bin directory path (~/.dybur/bin)
    let home_dir = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log_error!("service", "Failed to get home directory");
            return;
        }
    };

    let bin_dir = home_dir.join(".dybur").join("bin");
    let cli_path = bin_dir.join("dybur.exe");

    // Check if already installed
    if cli_path.exists() {
        log_info!("service", "CLI already installed at {}", cli_path.display());
        return;
    }

    // Get the sidecar path - the CLI bundled with the app
    let exe_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            log_error!("service", "Failed to get executable path: {}", e);
            return;
        }
    };

    let exe_dir = match exe_path.parent() {
        Some(p) => p,
        None => {
            log_error!("service", "Failed to get executable directory");
            return;
        }
    };

    // Try to find the sidecar binary
    let possible_names = [
        "dybur.exe",
        "dybur-x86_64-pc-windows-msvc.exe",
    ];

    let sidecar_path = possible_names
        .iter()
        .map(|name| exe_dir.join(name))
        .find(|p| p.exists() && p != &exe_path); // Don't use the main app exe

    let sidecar_path = match sidecar_path {
        Some(p) => p,
        None => {
            // Log what we found in the directory for debugging
            if let Ok(entries) = std::fs::read_dir(exe_dir) {
                let files: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect();
                log_warn!("service", "Sidecar not found. Files in exe dir: {:?}", files);
            }
            log_warn!("service", "Sidecar binary not found in {}, skipping CLI install", exe_dir.display());
            return;
        }
    };

    log_info!("service", "Found sidecar at: {}", sidecar_path.display());
    log_info!("service", "Installing CLI to {}...", cli_path.display());

    // Create bin directory
    if let Err(e) = std::fs::create_dir_all(&bin_dir) {
        log_error!("service", "Failed to create bin directory: {}", e);
        return;
    }

    // Copy sidecar to bin directory
    if let Err(e) = std::fs::copy(&sidecar_path, &cli_path) {
        log_error!("service", "Failed to copy CLI: {}", e);
        return;
    }

    log_info!("service", "CLI copied to {}", cli_path.display());

    // Add to PATH using PowerShell
    let bin_dir_str = bin_dir.to_string_lossy().to_string();
    let ps_script = format!(
        r#"
        $currentPath = [Environment]::GetEnvironmentVariable('PATH', 'User')
        if (-not ($currentPath -split ';' | Where-Object {{ $_.ToLower() -eq '{}'.ToLower() }})) {{
            $newPath = if ($currentPath) {{ "$currentPath;{}" }} else {{ "{}" }}
            [Environment]::SetEnvironmentVariable('PATH', $newPath, 'User')
            'PATH_UPDATED'
        }} else {{
            'ALREADY_IN_PATH'
        }}
        "#,
        bin_dir_str, bin_dir_str, bin_dir_str
    );

    match Command::new("powershell")
        .args(["-NoProfile", "-Command", &ps_script])
        .output()
    {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if output.status.success() {
                if stdout.contains("PATH_UPDATED") {
                    log_info!("service", "CLI installed and added to PATH successfully");
                    show_windows_notification(
                        "CLI Installed",
                        "You can now use 'dybur' command in any new terminal window.",
                    );
                } else {
                    log_info!("service", "CLI installed. PATH already configured.");
                }
            } else {
                let stderr = String::from_utf8_lossy(&output.stderr);
                log_error!("service", "Failed to update PATH: {}", stderr);
            }
        }
        Err(e) => {
            log_error!("service", "Failed to run PowerShell: {}", e);
        }
    }
}

/// Install CLI to system PATH (macOS/Linux only) - triggered from menu
#[cfg(not(target_os = "windows"))]
fn install_cli_to_path(app: &tauri::AppHandle) {
    use tauri_plugin_shell::ShellExt;

    log_info!("service", "Installing CLI to PATH...");

    // Get the sidecar command
    let sidecar = app.shell().sidecar("dybur").unwrap();

    // Run the setup command
    let (mut rx, _child) = match sidecar.args(["setup"]).spawn() {
        Ok(result) => result,
        Err(e) => {
            log_error!("service", "Failed to run CLI setup: {}", e);
            show_macos_notification("CLI Install Failed", &format!("Error: {}", e));
            return;
        }
    };

    // Handle output asynchronously
    std::thread::spawn(move || {
        use tauri_plugin_shell::process::CommandEvent;

        let mut output = String::new();
        while let Some(event) = rx.blocking_recv() {
            match event {
                CommandEvent::Stdout(line) => {
                    if let Ok(text) = String::from_utf8(line) {
                        output.push_str(&text);
                        log_info!("service", "CLI setup: {}", text.trim());
                    }
                }
                CommandEvent::Stderr(line) => {
                    if let Ok(text) = String::from_utf8(line) {
                        output.push_str(&text);
                        log_warn!("service", "CLI setup: {}", text.trim());
                    }
                }
                CommandEvent::Terminated(status) => {
                    if status.code == Some(0) || output.contains("successfully") || output.contains("already installed") {
                        log_info!("service", "CLI installed to PATH successfully");
                        show_macos_notification(
                            "CLI Installed",
                            "You can now use 'dybur' from any terminal.",
                        );
                    } else if output.contains("Permission denied") {
                        log_warn!("service", "CLI install needs sudo");
                        show_macos_notification(
                            "CLI Install",
                            "Run this in terminal:\nsudo /Applications/dybur.app/Contents/MacOS/dybur setup",
                        );
                    } else {
                        log_error!("service", "CLI install failed: {}", output.trim());
                        show_macos_notification("CLI Install Failed", &output.trim());
                    }
                    break;
                }
                _ => {}
            }
        }
    });
}

/// Select an audio input device
fn select_device(app: &tauri::AppHandle, device_name: Option<String>) {
    let state = app.state::<Mutex<AppState>>();
    let mut state_guard = state.inner().lock().unwrap();

    let prev_device = state_guard.config.input_device.clone();
    state_guard.config.input_device = device_name.clone();

    // Save config
    if let Err(e) = save_config(&state_guard.config) {
        log_error!("config", "Failed to save config: {}", e);
        // Restore previous value
        state_guard.config.input_device = prev_device;
        return;
    }

    let device_display = device_name.as_deref().unwrap_or("System Default");
    log_info!("config", "Input device changed to: {}", device_display);

    // Drop the lock before rebuilding menu
    drop(state_guard);

    // Rebuild the menu to reflect the new selection
    rebuild_tray_menu(app);
}

/// Spawn model download in background
fn spawn_model_download(app: &tauri::AppHandle) {
    if is_default_model_installed() {
        log_info!("models", "Default model already installed");
        #[cfg(target_os = "windows")]
        show_windows_notification("Model Status", "Default model is already installed.");
        #[cfg(target_os = "macos")]
        show_macos_notification("Model Status", "Default model is already installed.");
        return;
    }

    if is_download_in_progress() {
        log_info!("models", "Download already in progress");
        return;
    }

    log_info!("models", "Starting model download...");
    #[cfg(target_os = "windows")]
    show_windows_notification("Model Download", "Downloading model... This may take a few minutes.");
    #[cfg(target_os = "macos")]
    show_macos_notification("Model Download", "Downloading model... This may take a few minutes.");

    // Rebuild menu immediately to show "Downloading..." status
    rebuild_tray_menu(app);

    // Start menu refresh thread (updates every 2 seconds during download)
    let app_handle_refresh = app.clone();
    std::thread::spawn(move || {
        while is_download_in_progress() {
            std::thread::sleep(std::time::Duration::from_secs(2));
            rebuild_tray_menu(&app_handle_refresh);
        }
    });

    // Start download thread
    let app_handle = app.clone();
    std::thread::spawn(move || {
        match models::download_model_sync(DEFAULT_MODEL, "int8") {
            Ok(path) => {
                log_info!("models", "Model downloaded to: {}", path.display());
                #[cfg(target_os = "windows")]
                show_windows_notification("Model Download", "Model downloaded successfully! Restart dybur to use it.");
                #[cfg(target_os = "macos")]
                show_macos_notification("Model Download", "Model downloaded successfully! Restart dybur to use it.");

                // Rebuild menu to show the new model
                rebuild_tray_menu(&app_handle);

                // Also try to load the model so user doesn't need to restart
                let state = app_handle.state::<Mutex<AppState>>();
                let model_name = {
                    let state_guard = state.inner().lock().unwrap();
                    state_guard.config.model.clone()
                };

                if let Some(stt_config) = get_model_paths(&model_name) {
                    let mut engine = STT_ENGINE.lock().unwrap();
                    match engine.load(stt_config) {
                        Ok(()) => {
                            log_info!("model", "STT model '{}' loaded and ready", model_name);
                            #[cfg(target_os = "windows")]
                            show_windows_notification("Model Ready", "Speech model is now ready to use!");
                            #[cfg(target_os = "macos")]
                            show_macos_notification("Model Ready", "Speech model is now ready to use!");
                        }
                        Err(e) => {
                            log_error!("model", "Failed to load STT model after download: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                log_error!("models", "Model download failed: {}", e);
                #[cfg(target_os = "windows")]
                show_windows_notification("Model Download Failed", &format!("Error: {}", e));
                #[cfg(target_os = "macos")]
                show_macos_notification("Model Download Failed", &format!("Error: {}", e));

                // Rebuild menu to show error state
                rebuild_tray_menu(&app_handle);
            }
        }
    });
}

/// Clean unused models
fn clean_unused_models(app: &tauri::AppHandle) {
    let removed = models::clean_models();

    if removed.is_empty() {
        log_info!("models", "No unused models to clean");
        #[cfg(target_os = "windows")]
        show_windows_notification("Clean Models", "No unused models found.");
        #[cfg(target_os = "macos")]
        show_macos_notification("Clean Models", "No unused models found.");
    } else {
        log_info!("models", "Removed {} unused model(s): {:?}", removed.len(), removed);
        #[cfg(target_os = "windows")]
        show_windows_notification("Clean Models", &format!("Removed {} unused model(s).", removed.len()));
        #[cfg(target_os = "macos")]
        show_macos_notification("Clean Models", &format!("Removed {} unused model(s).", removed.len()));

        // Rebuild menu to reflect changes
        rebuild_tray_menu(app);
    }
}

/// Rebuild the tray menu (used after settings changes)
fn rebuild_tray_menu(app: &tauri::AppHandle) {
    if let Some(tray) = app.tray_by_id("main") {
        match create_tray_menu(app) {
            Ok(menu) => {
                if let Err(e) = tray.set_menu(Some(menu)) {
                    log_error!("service", "Failed to update tray menu: {:?}", e);
                }
            }
            Err(e) => {
                log_error!("service", "Failed to create new tray menu: {:?}", e);
            }
        }
    }
}

/// Toggle recording state
fn toggle_recording(app: &tauri::AppHandle) {
    let state = app.state::<Mutex<AppState>>();
    let mut state_guard = state.inner().lock().unwrap();

    if state_guard.is_recording {
        // Stop recording and process audio
        let audio_data: Option<Vec<f32>> = AUDIO_CAPTURE.with(|capture| {
            capture.borrow_mut().take().map(|mut cap| cap.stop())
        });
        
        state_guard.set_recording(false);
        set_overlay_state(app, RecordingState::Idle);
        log_info!("audio", "Recording stopped");
        
        // Get config for clipboard cleanup setting
        let restore_clipboard = state_guard.config.clipboard_cleanup;
        
        // Release the state lock before processing
        drop(state_guard);
        
        // Process audio with STT if we have audio data
        if let Some(audio) = audio_data {
            if audio.len() < 1600 {
                // Less than 100ms of audio - too short
                log_warn!("audio", "Recording too short ({} samples), skipping transcription", audio.len());
                update_tray_status(app, "Too short");
                return;
            }
            
            update_tray_status(app, "Transcribing...");
            
            // Run STT inference
            let transcription_result = {
                let mut engine = STT_ENGINE.lock().unwrap();
                if !engine.is_ready() {
                    log_warn!("model", "STT model not loaded, cannot transcribe");
                    None
                } else {
                    match engine.transcribe(&audio) {
                        Ok(result) => Some(result),
                        Err(e) => {
                            log_error!("model", "Transcription failed: {}", e);
                            None
                        }
                    }
                }
            };
            
            // Inject transcribed text
            if let Some(result) = transcription_result {
                if result.text.is_empty() {
                    log_info!("model", "Transcription returned empty text");
                    update_tray_status(app, "No speech detected");
                } else {
                    log_info!(
                        "model",
                        "Transcribed in {}ms ({}x realtime): '{}'",
                        result.inference_time_ms,
                        format!("{:.1}", result.audio_duration_s * 1000.0 / result.inference_time_ms as f32),
                        result.text
                    );
                    
                    // Inject the text into the active application
                    match inject_text(&result.text, restore_clipboard) {
                        Ok(()) => {
                            log_info!("injection", "Text injected successfully");
                            update_tray_status(app, "Done");
                        }
                        Err(e) => {
                            log_error!("injection", "Failed to inject text: {}", e);
                            update_tray_status(app, "Injection failed");
                        }
                    }
                }
            } else {
                update_tray_status(app, "Transcription failed");
            }
        } else {
            update_tray_status(app, "No audio captured");
        }
        
        // Reset status after a short delay
        let app_handle = app.clone();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            update_tray_status(&app_handle, "Idle");
        });
    } else {
        // Start recording
        state_guard.clear_error();
        
        // Check if STT model is loaded
        {
            let engine = STT_ENGINE.lock().unwrap();
            if !engine.is_ready() {
                log_error!("model", "STT model not loaded. Run 'dybur models prefetch' first.");
                state_guard.set_error("STT model not loaded. Run 'dybur models prefetch' first.".to_string());
                update_tray_status(app, "Model not loaded");

                // Show native alert to user - must be done in a separate thread to not block
                let app_handle = app.clone();
                std::thread::spawn(move || {
                    let title = "Speech Model Required";
                    let message = "The speech recognition model is not installed.\n\n\
                        Dictation requires the model to be downloaded first.\n\n\
                        Right-click the dybur tray icon and select:\n\
                        Models > Download Model";

                    #[cfg(target_os = "windows")]
                    show_windows_alert(title, message);

                    #[cfg(target_os = "macos")]
                    show_macos_alert(title, message);
                });
                return;
            }
        }
        
        // Initialize and start audio capture
        let input_device = state_guard.config.input_device.clone();
        match AudioCapture::new() {
            Ok(mut capture) => {
                if let Err(e) = capture.start(input_device.as_deref()) {
                    let help = get_audio_error_help(&e);
                    let error_msg = format!("{}\n\n{}", e, help);
                    state_guard.set_error(error_msg.clone());
                    log_error!("audio", "Failed to start recording: {}", error_msg);
                    update_tray_status(app, "Error: Recording failed");
                    return;
                }
                AUDIO_CAPTURE.with(|cap| {
                    *cap.borrow_mut() = Some(capture);
                });
                state_guard.set_recording(true);
                set_overlay_state(app, RecordingState::Recording);
                log_info!("audio", "Recording started");
                update_tray_status(app, "Recording...");
            }
            Err(e) => {
                let help = get_audio_error_help(&e);
                let error_msg = format!("{}\n\n{}", e, help);
                state_guard.set_error(error_msg.clone());
                set_overlay_state(app, RecordingState::Idle);
                log_error!("audio", "Failed to initialize audio: {}", error_msg);
                update_tray_status(app, "Error: Audio init failed");
            }
        }
    }
}

/// Update tray menu status
fn update_tray_status(_app: &tauri::AppHandle, status: &str) {
    // In a full implementation, we would update the menu item text
    log_debug!("service", "Status changed: {}", status);
}

fn create_overlay_window(app: &tauri::AppHandle) {
    if app.get_webview_window(OVERLAY_LABEL).is_some() {
        return;
    }

    let mut builder = WebviewWindowBuilder::new(
        app,
        OVERLAY_LABEL,
        WebviewUrl::App("index.html".into()),
    )
    .title("dybur Overlay")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .resizable(false)
    .visible(false)
    .focusable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .visible_on_all_workspaces(true)
    .inner_size(OVERLAY_WIDTH, OVERLAY_HEIGHT);

    if let Some((x, y)) = overlay_position(app) {
        builder = builder.position(x, y);
    }

    match builder.build() {
        Ok(window) => {
            if let Err(e) = window.set_ignore_cursor_events(true) {
                log_warn!("service", "Failed to set overlay click-through: {}", e);
            }
            
            // Remove window border/shadow on Windows
            #[cfg(target_os = "windows")]
            {
                use windows::Win32::Graphics::Dwm::{
                    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE,
                    DWM_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
                };
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetWindowLongW, SetWindowLongW, GWL_EXSTYLE,
                    WS_EX_LAYERED, WS_EX_TRANSPARENT, WS_EX_TOOLWINDOW, WS_EX_NOACTIVATE,
                };
                use windows::Win32::Foundation::HWND;
                
                if let Ok(hwnd) = window.hwnd() {
                    let hwnd = HWND(hwnd.0);
                    unsafe {
                        // Disable rounded corners
                        let corner_pref = DWMWCP_DONOTROUND;
                        let _ = DwmSetWindowAttribute(
                            hwnd,
                            DWMWA_WINDOW_CORNER_PREFERENCE,
                            &corner_pref as *const DWM_WINDOW_CORNER_PREFERENCE as *const _,
                            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
                        );
                        
                        // Set extended window styles for a borderless overlay
                        let ex_style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
                        let new_style = ex_style 
                            | WS_EX_LAYERED.0 
                            | WS_EX_TRANSPARENT.0 
                            | WS_EX_TOOLWINDOW.0 
                            | WS_EX_NOACTIVATE.0;
                        SetWindowLongW(hwnd, GWL_EXSTYLE, new_style as i32);
                    }
                }
            }
        }
        Err(e) => {
            log_warn!("service", "Failed to create overlay window: {}", e);
        }
    }
}

fn overlay_position(app: &tauri::AppHandle) -> Option<(f64, f64)> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let work_area = monitor.work_area();
    let scale = monitor.scale_factor();

    let work_width = work_area.size.width as f64 / scale;
    let work_height = work_area.size.height as f64 / scale;
    let origin_x = work_area.position.x as f64 / scale;
    let origin_y = work_area.position.y as f64 / scale;

    let x = origin_x + (work_width - OVERLAY_WIDTH) / 2.0;
    let y = origin_y + work_height - OVERLAY_HEIGHT - OVERLAY_MARGIN;

    Some((x.max(origin_x), y.max(origin_y)))
}

fn set_overlay_state(app: &tauri::AppHandle, state: RecordingState) {
    let Some(window) = app.get_webview_window(OVERLAY_LABEL) else {
        return;
    };

    let state_label = match state {
        RecordingState::Recording => "recording",
        RecordingState::Processing => "processing",
        RecordingState::Error => "error",
        RecordingState::Idle => "idle",
    };

    // Emit state for the JS to update the label
    let _ = window.emit("recording-state", state_label);

    // Control visibility at the window level
    if matches!(state, RecordingState::Recording) {
        let _ = window.show();
    } else {
        let _ = window.hide();
    }
}

/// Open logs directory
fn open_logs(app: &tauri::AppHandle) {
    let logs_dir = config::get_logs_dir();
    log_info!("service", "Opening logs directory: {}", logs_dir);
    
    // Use shell open with no specific program (let OS choose)
    #[allow(deprecated)]
    if let Err(e) = tauri_plugin_shell::ShellExt::shell(app).open(&logs_dir, None::<tauri_plugin_shell::open::Program>) {
        log_error!("service", "Failed to open logs directory: {:?}", e);
    }
}

/// Register global hotkey
fn register_hotkey(app: &tauri::AppHandle, hotkey: &str) -> Result<(), String> {
    use tauri_plugin_global_shortcut::Shortcut;

    let shortcut: Shortcut = hotkey.parse().map_err(|e| {
        let msg = format!("Invalid hotkey format '{}': {}", hotkey, e);
        log_error!("hotkey", "{}", msg);
        msg
    })?;

    let app_handle = app.clone();
    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if event.state == ShortcutState::Pressed {
                toggle_recording(&app_handle);
            }
        })
        .map_err(|e| {
            let msg = format!("Failed to register shortcut '{}': {}", hotkey, e);
            log_error!("hotkey", "{}", msg);
            msg
        })?;

    Ok(())
}

/// Report hotkey registration error to user
fn report_hotkey_error(app: &tauri::AppHandle, hotkey: &str, error: &str) {
    log_error!(
        "hotkey",
        "Hotkey registration failed for '{}': {}",
        hotkey,
        error
    );

    // Provide helpful troubleshooting info
    let help_msg = get_hotkey_error_help(error);
    log_info!("hotkey", "Troubleshooting: {}", help_msg);

    // Show system notification if available
    #[cfg(target_os = "windows")]
    {
        show_windows_notification(
            "dybur - Hotkey Error",
            &format!("Failed to register hotkey '{}'. {}", hotkey, help_msg),
        );
    }

    #[cfg(target_os = "macos")]
    {
        show_macos_notification(
            "dybur - Hotkey Error",
            &format!("Failed to register hotkey '{}'. {}", hotkey, help_msg),
        );
    }

    // Update tray status to show error
    update_tray_status(app, &format!("Error: Hotkey '{}' failed", hotkey));
}

/// Get helpful troubleshooting message for hotkey errors
fn get_hotkey_error_help(error: &str) -> String {
    let error_lower = error.to_lowercase();

    if error_lower.contains("already registered") || error_lower.contains("in use") {
        return "Another application may be using this hotkey. Try changing it in the config file."
            .to_string();
    }

    if error_lower.contains("invalid") || error_lower.contains("parse") {
        return "Check the hotkey format in config. Expected format: 'Ctrl+Shift+Space'"
            .to_string();
    }

    if error_lower.contains("permission") || error_lower.contains("access") {
        #[cfg(target_os = "macos")]
        return "Grant Accessibility permissions to dybur in System Preferences > Security & Privacy > Privacy > Accessibility".to_string();

        #[cfg(target_os = "windows")]
        return "Try running dybur as administrator.".to_string();
    }

    "Check the logs for more details. Try a different hotkey combination.".to_string()
}

/// Show Windows notification (toast style)
#[cfg(target_os = "windows")]
fn show_windows_notification(title: &str, message: &str) {
    // Use Windows toast notification API
    // For now, just log - full implementation would use windows-rs
    log_info!("service", "Notification: {} - {}", title, message);
}

/// Show Windows alert dialog (blocking message box)
#[cfg(target_os = "windows")]
fn show_windows_alert(title: &str, message: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_OK, MB_ICONWARNING};
    use windows::core::PCWSTR;

    let title_wide: Vec<u16> = OsStr::new(title).encode_wide().chain(std::iter::once(0)).collect();
    let message_wide: Vec<u16> = OsStr::new(message).encode_wide().chain(std::iter::once(0)).collect();

    unsafe {
        MessageBoxW(
            None,
            PCWSTR(message_wide.as_ptr()),
            PCWSTR(title_wide.as_ptr()),
            MB_OK | MB_ICONWARNING,
        );
    }
    log_info!("service", "Alert shown: {} - {}", title, message);
}

/// Show macOS notification
#[cfg(target_os = "macos")]
fn show_macos_notification(title: &str, message: &str) {
    // Use macOS notification center
    // For now, just log - full implementation would use objc/cocoa
    log_info!("service", "Notification: {} - {}", title, message);
}

/// Show macOS alert dialog (blocking dialog)
#[cfg(target_os = "macos")]
fn show_macos_alert(title: &str, message: &str) {
    use std::process::Command;

    // Escape quotes in title and message for AppleScript
    let title_escaped = title.replace("\"", "\\\"").replace("'", "'\\''");
    let message_escaped = message.replace("\"", "\\\"").replace("'", "'\\''");

    let script = format!(
        r#"display dialog "{}" with title "{}" buttons {{"OK"}} default button "OK" with icon caution"#,
        message_escaped, title_escaped
    );

    let _ = Command::new("osascript")
        .args(["-e", &script])
        .output();

    log_info!("service", "Alert shown: {} - {}", title, message);
}

/// Acquire single instance lock or exit if already running
fn acquire_single_instance() -> single_instance::InstanceGuard {
    match single_instance::try_acquire_lock() {
        Ok(single_instance::LockResult::Acquired(guard)) => {
            log_info!("service", "Single instance lock acquired");
            guard
        }
        Ok(single_instance::LockResult::AlreadyRunning(pid)) => {
            log_error!(
                "service",
                "dybur is already running (PID: {}). Use 'dybur stop' to stop the existing instance.",
                pid
            );
            std::process::exit(1);
        }
        Err(e) => {
            log_warn!("service", "Failed to check for existing instance: {}", e);
            // Try once more, if it fails again, exit
            match single_instance::try_acquire_lock() {
                Ok(single_instance::LockResult::Acquired(guard)) => guard,
                Ok(single_instance::LockResult::AlreadyRunning(pid)) => {
                    log_error!("service", "dybur is already running (PID: {})", pid);
                    std::process::exit(1);
                }
                Err(e) => {
                    log_error!("service", "Failed to acquire instance lock: {}", e);
                    std::process::exit(1);
                }
            }
        }
    }
}
