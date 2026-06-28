//! dybur Tray Application
//!
//! Background service for voice dictation with system tray integration.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod audio_session;
mod config;
mod doctor;
mod execution_providers;
mod ftue;
mod hotkey;
mod injection;
mod logging;
mod models;
mod privacy;
mod single_instance;
mod state;
mod streaming;
mod stt;
mod tokenizer;
mod updater;
mod vad;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::menu::{MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use audio::{get_audio_error_help, list_input_devices};
use audio_session::AudioSessionController;
use config::{get_config_path, save_config};
use injection::inject_text;
use models::{
    format_bytes, get_available_models, get_download_status, get_model_definition,
    is_download_in_progress, is_model_installed, normalize_model_name,
};
use state::{AppState, RecordingState};
use stt::{get_model_paths, SttEngine};

// Global audio buffer for cross-thread access (the buffer Arc IS Send+Sync)
lazy_static::lazy_static! {
    static ref RECORDING_BUFFER: Mutex<Option<Arc<Mutex<Vec<f32>>>>> = Mutex::new(None);
}

lazy_static::lazy_static! {
    static ref AUDIO_SESSION: AudioSessionController = AudioSessionController::spawn();
}

// Global sample rate for the recording buffer (needed for resampling)
lazy_static::lazy_static! {
    static ref RECORDING_SAMPLE_RATE: Mutex<u32> = Mutex::new(16000);
}

// Global STT engine (wrapped in Mutex for thread safety)
lazy_static::lazy_static! {
    static ref STT_ENGINE: Mutex<SttEngine> = Mutex::new(SttEngine::new());
}

// Global VAD engine (wrapped in Mutex for thread safety)
lazy_static::lazy_static! {
    static ref VAD_ENGINE: Mutex<vad::VadEngine> = Mutex::new(vad::VadEngine::new());
}

// Global streaming state (wrapped in Mutex for thread safety)
lazy_static::lazy_static! {
    static ref STREAMING_STATE: Mutex<Option<streaming::StreamingState>> = Mutex::new(None);
}

// Flag to signal streaming thread to stop
static STREAMING_RUNNING: AtomicBool = AtomicBool::new(false);

// Flag to track explicit quit request (to bypass prevent_exit)
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

const OVERLAY_LABEL: &str = "overlay";
const OVERLAY_WIDTH: f64 = 280.0;
const OVERLAY_HEIGHT: f64 = 100.0; // Increased to fit streaming text
const OVERLAY_MARGIN: f64 = 28.0;
const STREAMING_POLL_INTERVAL_MS: u64 = 100;

/// Application entry point
fn main() {
    match updater::run_helper_from_env() {
        Ok(true) => return,
        Ok(false) => {}
        Err(error) => {
            eprintln!("dybur update helper failed: {}", error);
            std::process::exit(1);
        }
    }

    // Check for single instance
    let _instance_guard = acquire_single_instance();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
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
            ftue::ftue_get_models,
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

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

            // Get GPU preference from config
            let gpu_preference = {
                let state_guard = state.inner().lock().unwrap();
                execution_providers::parse_gpu_preference(&state_guard.config.gpu_mode)
            };

            // Load STT model
            if let Some(stt_config) = get_model_paths(&model_name) {
                let mut engine = STT_ENGINE.lock().unwrap();
                match engine.load(stt_config, gpu_preference) {
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
                    "STT model '{}' not found. Run 'dybur models download {}' to download it.",
                    model_name,
                    model_name
                );
            }

            // Load VAD model if available
            let vad_model_path = models::get_vad_model_path();
            if vad_model_path.exists() {
                let mut vad_engine = VAD_ENGINE.lock().unwrap();
                match vad_engine.load(vad_model_path, gpu_preference) {
                    Ok(()) => {
                        log_info!("vad", "VAD model loaded and ready");
                    }
                    Err(e) => {
                        log_warn!(
                            "vad",
                            "Failed to load VAD model: {} (VAD filtering disabled)",
                            e
                        );
                    }
                }
            } else {
                log_info!("vad", "VAD model not found, downloading...");
                // Try to download VAD model
                match models::download_vad_model_sync() {
                    Ok(path) => {
                        let mut vad_engine = VAD_ENGINE.lock().unwrap();
                        if let Err(e) = vad_engine.load(path, gpu_preference) {
                            log_warn!("vad", "Failed to load downloaded VAD model: {}", e);
                        } else {
                            log_info!("vad", "VAD model downloaded and loaded");
                        }
                    }
                    Err(e) => {
                        log_warn!(
                            "vad",
                            "Failed to download VAD model: {} (VAD filtering disabled)",
                            e
                        );
                    }
                }
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
            let (hotkey, recording_mode) = {
                let state_guard = state.inner().lock().unwrap();
                (
                    state_guard.config.hotkey.clone(),
                    state_guard.config.recording_mode.clone(),
                )
            };

            if let Err(e) = register_hotkey(app.handle(), &hotkey, &recording_mode) {
                report_hotkey_error(app.handle(), &hotkey, &e);
            } else {
                log_info!(
                    "hotkey",
                    "Global hotkey '{}' registered successfully",
                    hotkey
                );
            }

            log_info!("service", "dybur started. Press {} to dictate.", hotkey);

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

            start_update_check(app.handle().clone(), false);

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

    // Recording mode submenu
    let recording_mode_submenu = build_recording_mode_submenu(app)?;

    // VAD controls
    let vad_submenu = build_vad_submenu(app)?;

    // GPU mode submenu
    let gpu_mode_submenu = build_gpu_mode_submenu(app)?;

    // Settings submenu
    let open_config = MenuItemBuilder::with_id("open_config", "Open Config File").build(app)?;
    let run_diagnostics =
        MenuItemBuilder::with_id("run_diagnostics", "Run Diagnostics").build(app)?;
    let run_setup = MenuItemBuilder::with_id("run_setup", "Run Setup Wizard...").build(app)?;
    let launch_on_startup =
        MenuItemBuilder::with_id("launch_on_startup", launch_on_startup_menu_label(app))
            .build(app)?;
    let check_updates =
        MenuItemBuilder::with_id("check_updates", "Check for Updates...").build(app)?;
    let install_cli =
        MenuItemBuilder::with_id("install_cli", "Install Command Line Tool...").build(app)?;
    let about = MenuItemBuilder::with_id("about", "About dybur").build(app)?;

    let settings_builder = SubmenuBuilder::with_id(app, "settings", "Settings")
        .item(&open_config)
        .item(&run_diagnostics)
        .item(&run_setup)
        .item(&launch_on_startup)
        .item(&check_updates);

    let settings_builder = settings_builder.item(&install_cli);

    let settings_submenu = settings_builder.separator().item(&about).build()?;

    // Main menu items
    let logs = MenuItemBuilder::with_id("logs", "Open Logs").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit dybur").build(app)?;

    MenuBuilder::new(app)
        .item(&toggle)
        .item(&status)
        .separator()
        .item(&models_submenu)
        .item(&devices_submenu)
        .item(&recording_mode_submenu)
        .item(&vad_submenu)
        .item(&gpu_mode_submenu)
        .separator()
        .item(&settings_submenu)
        .item(&logs)
        .separator()
        .item(&quit)
        .build()
}

/// Build the Models submenu
fn build_models_submenu(
    app: &tauri::AppHandle,
) -> Result<tauri::menu::Submenu<tauri::Wry>, tauri::Error> {
    let mut submenu = SubmenuBuilder::with_id(app, "models", "Models");

    // Get current selected model from config
    let current_model = {
        let state = app.state::<Mutex<AppState>>();
        let state_guard = state.inner().lock().unwrap();
        normalize_model_name(&state_guard.config.model).to_string()
    };

    // Check for download in progress first
    let download_in_progress = is_download_in_progress();
    if let Some(status) = get_download_status() {
        let status_item = MenuItemBuilder::with_id("download_status", &status)
            .enabled(false)
            .build(app)?;
        submenu = submenu.item(&status_item);
        submenu = submenu.separator();
    }

    // List all available models from registry
    let available_models = get_available_models();

    for model_def in available_models {
        let is_installed = is_model_installed(model_def.id);
        let is_selected = current_model == model_def.id;
        let size_str = format_bytes(model_def.size_bytes);

        // Show selection indicator and install status
        let prefix = if is_selected { "● " } else { "  " };
        let suffix = if is_installed { "" } else { " [Not installed]" };

        let label = format!(
            "{}{} ({}){}",
            prefix, model_def.display_name, size_str, suffix
        );

        // Can select if installed and not currently downloading
        // Can trigger download if not installed
        let enabled = if is_installed {
            !download_in_progress // Can select installed models when not downloading
        } else {
            !download_in_progress // Can download when not already downloading
        };

        let item = MenuItemBuilder::with_id(format!("model_select_{}", model_def.id), label)
            .enabled(enabled)
            .build(app)?;
        submenu = submenu.item(&item);
    }

    submenu = submenu.separator();

    // Clean unused models button
    let clean_models =
        MenuItemBuilder::with_id("clean_models", "Remove Unused Models").build(app)?;
    submenu = submenu.item(&clean_models);

    submenu.build()
}

/// Build the Devices submenu
fn build_devices_submenu(
    app: &tauri::AppHandle,
) -> Result<tauri::menu::Submenu<tauri::Wry>, tauri::Error> {
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
        let is_selected = configured_device
            .as_ref()
            .map(|d| d == &device.name)
            .unwrap_or(false);
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
        .map(|c| {
            if c.is_alphanumeric() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Build the Recording Mode submenu
fn build_recording_mode_submenu(
    app: &tauri::AppHandle,
) -> Result<tauri::menu::Submenu<tauri::Wry>, tauri::Error> {
    let mut submenu = SubmenuBuilder::with_id(app, "recording_mode", "Recording Mode");

    // Get current recording mode
    let state = app.state::<Mutex<AppState>>();
    let current_mode = {
        let state_guard = state.inner().lock().unwrap();
        state_guard.config.recording_mode.clone()
    };

    let is_toggle = current_mode != "push_to_talk";
    let is_ptt = current_mode == "push_to_talk";

    // Toggle mode option
    let toggle_label = if is_toggle {
        "● Toggle (press to start/stop)"
    } else {
        "  Toggle (press to start/stop)"
    };
    let toggle_item = MenuItemBuilder::with_id("mode_toggle", toggle_label).build(app)?;
    submenu = submenu.item(&toggle_item);

    // Push-to-talk mode option
    let ptt_label = if is_ptt {
        "● Push-to-Talk (hold to record)"
    } else {
        "  Push-to-Talk (hold to record)"
    };
    let ptt_item = MenuItemBuilder::with_id("mode_push_to_talk", ptt_label).build(app)?;
    submenu = submenu.item(&ptt_item);

    submenu.build()
}

/// Build VAD toggle menu item
fn build_vad_toggle(
    app: &tauri::AppHandle,
) -> Result<tauri::menu::MenuItem<tauri::Wry>, tauri::Error> {
    let state = app.state::<Mutex<AppState>>();
    let vad_enabled = {
        let state_guard = state.inner().lock().unwrap();
        state_guard.config.vad_enabled
    };

    let label = if vad_enabled {
        "✓ Filter Silence (VAD)"
    } else {
        "  Filter Silence (VAD)"
    };

    MenuItemBuilder::with_id("vad_toggle", label).build(app)
}

/// Build VAD controls submenu
fn build_vad_submenu(
    app: &tauri::AppHandle,
) -> Result<tauri::menu::Submenu<tauri::Wry>, tauri::Error> {
    let state = app.state::<Mutex<AppState>>();
    let (vad_threshold, vad_min_speech_ms, silence_timeout_ms) = {
        let state_guard = state.inner().lock().unwrap();
        (
            state_guard.config.vad_threshold,
            state_guard.config.vad_min_speech_ms,
            state_guard.config.silence_timeout_ms,
        )
    };

    let mut submenu = SubmenuBuilder::with_id(app, "vad", "Voice Activity Detection");

    let toggle_item = build_vad_toggle(app)?;
    submenu = submenu.item(&toggle_item);

    let status = MenuItemBuilder::with_id(
        "vad_status",
        format!(
            "Threshold {:.2} / Speech {}ms / Silence {}ms",
            vad_threshold, vad_min_speech_ms, silence_timeout_ms
        ),
    )
    .enabled(false)
    .build(app)?;
    submenu = submenu.item(&status).separator();

    let threshold_header = MenuItemBuilder::with_id("vad_threshold_header", "Threshold")
        .enabled(false)
        .build(app)?;
    submenu = submenu.item(&threshold_header);
    for (id, value, label) in [
        ("vad_threshold_035", 0.35_f32, "Sensitive - 0.35"),
        ("vad_threshold_050", 0.50_f32, "Balanced - 0.50"),
        ("vad_threshold_065", 0.65_f32, "Strict - 0.65"),
    ] {
        let item = MenuItemBuilder::with_id(
            id,
            format!(
                "{}{}",
                selection_prefix(approx_eq_f32(vad_threshold, value)),
                label
            ),
        )
        .build(app)?;
        submenu = submenu.item(&item);
    }

    submenu = submenu.separator();
    let min_speech_header = MenuItemBuilder::with_id("vad_min_speech_header", "Minimum Speech")
        .enabled(false)
        .build(app)?;
    submenu = submenu.item(&min_speech_header);
    for (id, value, label) in [
        ("vad_min_speech_150", 150_u32, "Short - 150ms"),
        ("vad_min_speech_250", 250_u32, "Balanced - 250ms"),
        ("vad_min_speech_500", 500_u32, "Deliberate - 500ms"),
    ] {
        let item = MenuItemBuilder::with_id(
            id,
            format!("{}{}", selection_prefix(vad_min_speech_ms == value), label),
        )
        .build(app)?;
        submenu = submenu.item(&item);
    }

    submenu = submenu.separator();
    let silence_header = MenuItemBuilder::with_id("vad_silence_header", "Silence Timeout")
        .enabled(false)
        .build(app)?;
    submenu = submenu.item(&silence_header);
    for (id, value, label) in [
        ("vad_silence_700", 700_u32, "Responsive - 700ms"),
        ("vad_silence_1000", 1000_u32, "Balanced - 1000ms"),
        ("vad_silence_1500", 1500_u32, "Patient - 1500ms"),
    ] {
        let item = MenuItemBuilder::with_id(
            id,
            format!("{}{}", selection_prefix(silence_timeout_ms == value), label),
        )
        .build(app)?;
        submenu = submenu.item(&item);
    }

    submenu.build()
}

fn selection_prefix(selected: bool) -> &'static str {
    if selected {
        "[x] "
    } else {
        "[ ] "
    }
}

fn approx_eq_f32(left: f32, right: f32) -> bool {
    (left - right).abs() < 0.001
}

/// Build GPU mode submenu
fn build_gpu_mode_submenu(
    app: &tauri::AppHandle,
) -> Result<tauri::menu::Submenu<tauri::Wry>, tauri::Error> {
    let mut submenu = SubmenuBuilder::with_id(app, "gpu_mode", "GPU Acceleration");

    let state = app.state::<Mutex<AppState>>();
    let current_mode = {
        let state_guard = state.inner().lock().unwrap();
        state_guard.config.gpu_mode.clone()
    };

    let is_auto = current_mode != "cpu";
    let is_cpu = current_mode == "cpu";

    // Auto mode option
    let auto_label = if is_auto {
        "● Auto (use GPU if available)"
    } else {
        "  Auto (use GPU if available)"
    };
    let auto_item = MenuItemBuilder::with_id("gpu_auto", auto_label).build(app)?;
    submenu = submenu.item(&auto_item);

    // CPU-only mode option
    let cpu_label = if is_cpu {
        "● CPU Only (disable GPU)"
    } else {
        "  CPU Only (disable GPU)"
    };
    let cpu_item = MenuItemBuilder::with_id("gpu_cpu", cpu_label).build(app)?;
    submenu = submenu.item(&cpu_item);

    submenu.build()
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
        "launch_on_startup" => {
            toggle_launch_on_startup(app);
        }
        "check_updates" => {
            start_update_check(app.clone(), true);
        }
        "install_cli" => {
            install_cli_to_path(app);
        }
        "clean_models" => {
            clean_unused_models(app);
        }
        id if id.starts_with("model_select_") => {
            let model_id = id.strip_prefix("model_select_").unwrap();
            handle_model_selection(app, model_id);
        }
        "device_default" => {
            select_device(app, None);
        }
        id if id.starts_with("device_") && id != "device_default" => {
            // Extract device name from ID
            let device_name = id.strip_prefix("device_").unwrap();
            // Reverse the sanitization to find the actual device
            let devices = list_input_devices();
            if let Some(device) = devices
                .iter()
                .find(|d| sanitize_menu_id(&d.name) == device_name)
            {
                select_device(app, Some(device.name.clone()));
            }
        }
        "mode_toggle" => {
            select_recording_mode(app, "toggle");
        }
        "mode_push_to_talk" => {
            select_recording_mode(app, "push_to_talk");
        }
        "vad_toggle" => {
            toggle_vad(app);
        }
        "vad_threshold_035" => {
            set_vad_threshold(app, 0.35);
        }
        "vad_threshold_050" => {
            set_vad_threshold(app, 0.50);
        }
        "vad_threshold_065" => {
            set_vad_threshold(app, 0.65);
        }
        "vad_min_speech_150" => {
            set_vad_min_speech_ms(app, 150);
        }
        "vad_min_speech_250" => {
            set_vad_min_speech_ms(app, 250);
        }
        "vad_min_speech_500" => {
            set_vad_min_speech_ms(app, 500);
        }
        "vad_silence_700" => {
            set_vad_silence_timeout_ms(app, 700);
        }
        "vad_silence_1000" => {
            set_vad_silence_timeout_ms(app, 1000);
        }
        "vad_silence_1500" => {
            set_vad_silence_timeout_ms(app, 1500);
        }
        "gpu_auto" => {
            select_gpu_mode(app, "auto");
        }
        "gpu_cpu" => {
            select_gpu_mode(app, "cpu");
        }
        _ => {}
    }
}

/// Open the config file in the default editor
fn open_config_file(app: &tauri::AppHandle) {
    let config_path = get_config_path();
    log_info!("service", "Opening config file: {}", config_path.display());

    #[allow(deprecated)]
    if let Err(e) = tauri_plugin_shell::ShellExt::shell(app).open(
        config_path.to_string_lossy().as_ref(),
        None::<tauri_plugin_shell::open::Program>,
    ) {
        log_error!("service", "Failed to open config file: {:?}", e);
    }
}

fn launch_on_startup_menu_label(app: &tauri::AppHandle) -> String {
    format!(
        "{}Launch on startup",
        selection_prefix(is_launch_on_startup_enabled(app))
    )
}

fn is_launch_on_startup_enabled(app: &tauri::AppHandle) -> bool {
    match app.autolaunch().is_enabled() {
        Ok(enabled) => enabled,
        Err(e) => {
            log_error!("service", "Failed to read launch-on-startup state: {}", e);
            false
        }
    }
}

fn toggle_launch_on_startup(app: &tauri::AppHandle) {
    let autolaunch = app.autolaunch();
    let result = match autolaunch.is_enabled() {
        Ok(true) => autolaunch.disable().map(|_| false),
        Ok(false) => autolaunch.enable().map(|_| true),
        Err(e) => Err(e),
    };

    match result {
        Ok(enabled) => {
            let status = if enabled { "enabled" } else { "disabled" };
            log_info!("service", "Launch on startup {}", status);
            show_user_notification("dybur", &format!("Launch on startup {}", status));
        }
        Err(e) => {
            let message = format!("Failed to update launch on startup: {}", e);
            log_error!("service", "{}", message);
            show_user_alert("dybur", &message);
        }
    }

    rebuild_tray_menu(app);
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

fn start_update_check(app: tauri::AppHandle, interactive: bool) {
    std::thread::spawn(move || {
        if !interactive {
            std::thread::sleep(std::time::Duration::from_secs(5));
            if cfg!(debug_assertions) {
                log_info!("updates", "Skipping automatic update check in debug build");
                return;
            }
            if std::env::var_os("DYBUR_DISABLE_AUTO_UPDATE").is_some() {
                log_info!("updates", "Automatic update check disabled by environment");
                return;
            }
        }

        if let Err(error) = run_update_check(&app, interactive) {
            log_error!("updates", "Update check failed: {}", error);
            if interactive {
                show_user_alert("dybur Update", &format!("Update check failed: {}", error));
            }
        }
    });
}

fn run_update_check(app: &tauri::AppHandle, interactive: bool) -> Result<(), String> {
    let current_version = env!("CARGO_PKG_VERSION");
    log_info!("updates", "Checking for updates from v{}", current_version);

    let Some(update) = updater::check_for_update(current_version)? else {
        log_info!("updates", "dybur is up to date");
        if interactive {
            show_user_notification("dybur Update", "dybur is already up to date.");
        }
        return Ok(());
    };

    if is_recording_or_processing(app) {
        let message =
            "Update available, but dybur is recording or processing. Try again when idle.";
        log_info!("updates", "{}", message);
        if interactive {
            show_user_alert("dybur Update", message);
        }
        return Ok(());
    }

    let platform = updater::install_platform_for_key(&update.platform_key)
        .ok_or_else(|| format!("Unsupported update platform: {}", update.platform_key))?;

    log_info!(
        "updates",
        "Downloading dybur v{} for {}",
        update.version,
        update.platform_key
    );
    show_user_notification(
        "dybur Update",
        &format!("Downloading dybur v{}...", update.version),
    );

    let artifact_path = updater::download_update_artifact(&update, &updater::update_work_dir())?;
    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to locate current executable: {}", e))?;
    let helper_args = updater::helper_install_args_for(
        platform,
        std::process::id(),
        artifact_path,
        current_exe.clone(),
        updater::updater_log_path(),
    )?;

    log_info!(
        "updates",
        "Launching update helper for dybur v{}",
        update.version
    );
    updater::spawn_update_helper(&helper_args, &current_exe)?;

    QUIT_REQUESTED.store(true, Ordering::SeqCst);
    app.exit(0);
    Ok(())
}

fn is_recording_or_processing(app: &tauri::AppHandle) -> bool {
    let state = app.state::<Mutex<AppState>>();
    let state_guard = state.inner().lock().unwrap();
    matches!(
        state_guard.recording_state,
        RecordingState::Recording | RecordingState::Processing
    ) || state_guard.is_recording
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

/// Check if Node.js is installed and return the path to node binary
#[cfg(target_os = "macos")]
fn find_node_path() -> Option<String> {
    use std::path::Path;
    use std::process::Command;

    // Common Node.js installation paths on macOS
    let common_paths = [
        "/opt/homebrew/bin/node", // Homebrew on Apple Silicon
        "/usr/local/bin/node",    // Homebrew on Intel Macs
        "/usr/bin/node",          // System installation
    ];

    // Check common paths first
    for path in &common_paths {
        if Path::new(path).exists() {
            if let Ok(output) = Command::new(path).arg("--version").output() {
                if output.status.success() {
                    return Some(path.to_string());
                }
            }
        }
    }

    // Check nvm installation (most common version manager)
    if let Some(home) = dirs::home_dir() {
        let nvm_dir = home.join(".nvm/versions/node");
        if nvm_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&nvm_dir) {
                // Get the most recent version (sorted descending)
                let mut versions: Vec<_> =
                    entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
                versions.sort();
                versions.reverse();

                for version_dir in versions {
                    let node_path = version_dir.join("bin/node");
                    if node_path.exists() {
                        if let Ok(output) = Command::new(&node_path).arg("--version").output() {
                            if output.status.success() {
                                return Some(node_path.to_string_lossy().to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    // Fallback: try PATH (works if launched from terminal)
    if let Ok(output) = Command::new("node").arg("--version").output() {
        if output.status.success() {
            return Some("node".to_string());
        }
    }

    None
}

#[cfg(target_os = "macos")]
fn check_node_installed() -> bool {
    find_node_path().is_some()
}

/// Install the CLI wrapper on macOS.
#[cfg(target_os = "macos")]
fn check_and_install_cli(app: &tauri::AppHandle) {
    use std::path::Path;
    use std::process::Command;

    // Check if Node.js is installed and get the path
    let node_path = match find_node_path() {
        Some(p) => p,
        None => {
            log_info!("service", "Node.js not found, skipping CLI installation. Users can install Node.js to enable CLI.");
            return;
        }
    };

    log_info!("service", "Found Node.js at: {}", node_path);

    let cli_path = Path::new("/usr/local/bin/dybur");

    // Check if already installed
    if cli_path.exists() {
        log_info!("service", "CLI already installed at /usr/local/bin/dybur");
        return;
    }

    // Get the home directory for ~/.dybur
    let home_dir = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log_error!("service", "Failed to get home directory");
            return;
        }
    };

    let dybur_dir = home_dir.join(".dybur");
    let cli_js_path = dybur_dir.join("cli.js");
    let wrapper_path = dybur_dir.join("bin").join("dybur");

    // Find the bundled cli.js resource
    let cli_js_source = app
        .path()
        .resolve("resources/cli.js", tauri::path::BaseDirectory::Resource);

    let cli_js_source = match cli_js_source {
        Ok(p) if p.exists() => p,
        Ok(p) => {
            // Try Resources folder in app bundle
            let exe_path = std::env::current_exe().ok();
            let resources_path = exe_path
                .as_ref()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.join("Resources").join("resources").join("cli.js"));

            if let Some(res) = resources_path {
                if res.exists() {
                    res
                } else {
                    log_warn!(
                        "service",
                        "CLI resource not found at {} or {:?}",
                        p.display(),
                        res
                    );
                    return;
                }
            } else {
                log_warn!("service", "CLI resource not found at {}", p.display());
                return;
            }
        }
        Err(e) => {
            log_warn!("service", "Failed to resolve CLI resource path: {}", e);
            return;
        }
    };

    log_info!("service", "Found CLI at: {}", cli_js_source.display());
    log_info!("service", "Installing CLI...");

    // Create directories
    if let Err(e) = std::fs::create_dir_all(&dybur_dir) {
        log_error!("service", "Failed to create dybur directory: {}", e);
        return;
    }
    if let Err(e) = std::fs::create_dir_all(dybur_dir.join("bin")) {
        log_error!("service", "Failed to create bin directory: {}", e);
        return;
    }

    // Copy cli.js to ~/.dybur/
    if let Err(e) = std::fs::copy(&cli_js_source, &cli_js_path) {
        log_error!("service", "Failed to copy CLI: {}", e);
        return;
    }

    // Create wrapper shell script with absolute node path
    let wrapper_content = format!(
        "#!/bin/bash\nexec \"{}\" \"{}\" \"$@\"\n",
        node_path,
        cli_js_path.display()
    );
    if let Err(e) = std::fs::write(&wrapper_path, &wrapper_content) {
        log_error!("service", "Failed to create CLI wrapper: {}", e);
        return;
    }

    // Make wrapper executable
    let _ = Command::new("chmod")
        .args(["+x", &wrapper_path.to_string_lossy()])
        .output();

    log_info!(
        "service",
        "CLI wrapper created at {}",
        wrapper_path.display()
    );

    // Use osascript to symlink with administrator privileges
    let script = format!(
        r#"do shell script "mkdir -p /usr/local/bin && ln -sf '{}' /usr/local/bin/dybur" with administrator privileges"#,
        wrapper_path.display()
    );

    let result = Command::new("osascript").args(["-e", &script]).output();

    match result {
        Ok(output) => {
            if output.status.success() {
                log_info!(
                    "service",
                    "CLI installed successfully to /usr/local/bin/dybur"
                );
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

/// Check if Node.js is installed
#[cfg(target_os = "windows")]
fn check_node_installed() -> bool {
    use std::process::Command;
    Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Install the CLI wrapper on Windows.
#[cfg(target_os = "windows")]
fn check_and_install_cli_windows(app: &tauri::AppHandle) {
    use std::process::Command;

    // Check if Node.js is installed
    if !check_node_installed() {
        log_info!("service", "Node.js not found, skipping CLI installation. Users can install Node.js to enable CLI.");
        return;
    }

    // Get the dybur directory path (~/.dybur)
    let home_dir = match dirs::home_dir() {
        Some(h) => h,
        None => {
            log_error!("service", "Failed to get home directory");
            return;
        }
    };

    let dybur_dir = home_dir.join(".dybur");
    let bin_dir = dybur_dir.join("bin");
    let cli_js_path = dybur_dir.join("cli.js");
    let cli_cmd_path = bin_dir.join("dybur.cmd");

    // Check if already installed
    if cli_cmd_path.exists() && cli_js_path.exists() {
        log_info!(
            "service",
            "CLI already installed at {}",
            cli_cmd_path.display()
        );
        return;
    }

    // Find the bundled cli.js resource
    let cli_js_source = app
        .path()
        .resolve("resources/cli.js", tauri::path::BaseDirectory::Resource);

    let cli_js_source = match cli_js_source {
        Ok(p) if p.exists() => p,
        Ok(p) => {
            // Try alternative paths for development
            let exe_path = std::env::current_exe().ok();
            let dev_path = exe_path
                .as_ref()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.join("resources").join("cli.js"));

            if let Some(dev) = dev_path {
                if dev.exists() {
                    dev
                } else {
                    log_warn!(
                        "service",
                        "CLI resource not found at {} or {}",
                        p.display(),
                        dev.display()
                    );
                    return;
                }
            } else {
                log_warn!("service", "CLI resource not found at {}", p.display());
                return;
            }
        }
        Err(e) => {
            log_warn!("service", "Failed to resolve CLI resource path: {}", e);
            return;
        }
    };

    log_info!("service", "Found CLI at: {}", cli_js_source.display());
    log_info!("service", "Installing CLI...");

    // Create directories
    if let Err(e) = std::fs::create_dir_all(&dybur_dir) {
        log_error!("service", "Failed to create dybur directory: {}", e);
        return;
    }
    if let Err(e) = std::fs::create_dir_all(&bin_dir) {
        log_error!("service", "Failed to create bin directory: {}", e);
        return;
    }

    // Copy cli.js to ~/.dybur/
    if let Err(e) = std::fs::copy(&cli_js_source, &cli_js_path) {
        log_error!("service", "Failed to copy CLI: {}", e);
        return;
    }

    // Create wrapper script (dybur.cmd)
    let wrapper_content = format!(
        "@echo off\r\nnode \"{}\" %*\r\n",
        cli_js_path.to_string_lossy().replace('/', "\\")
    );
    if let Err(e) = std::fs::write(&cli_cmd_path, &wrapper_content) {
        log_error!("service", "Failed to create CLI wrapper: {}", e);
        return;
    }

    log_info!("service", "CLI installed to {}", cli_cmd_path.display());

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

/// Install CLI to user PATH (Windows only) - triggered from menu
#[cfg(target_os = "windows")]
fn install_cli_to_path(app: &tauri::AppHandle) {
    check_and_install_cli_windows(app);
}

/// Install CLI to system PATH (macOS/Linux only) - triggered from menu
#[cfg(not(target_os = "windows"))]
fn install_cli_to_path(app: &tauri::AppHandle) {
    // Check if Node.js is installed first
    if !check_node_installed() {
        log_warn!("service", "Node.js not found, cannot install CLI");
        show_macos_notification(
            "Node.js Required",
            "Please install Node.js to use the CLI.\nVisit: https://nodejs.org",
        );
        return;
    }

    check_and_install_cli(app);
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

/// Select recording mode (toggle or push_to_talk)
fn select_recording_mode(app: &tauri::AppHandle, mode: &str) {
    let state = app.state::<Mutex<AppState>>();
    let (prev_mode, hotkey) = {
        let mut state_guard = state.inner().lock().unwrap();

        let prev_mode = state_guard.config.recording_mode.clone();

        // No change needed
        if prev_mode == mode {
            return;
        }

        state_guard.config.recording_mode = mode.to_string();

        // Save config
        if let Err(e) = save_config(&state_guard.config) {
            log_error!("config", "Failed to save config: {}", e);
            // Restore previous value
            state_guard.config.recording_mode = prev_mode;
            return;
        }

        let hotkey = state_guard.config.hotkey.clone();
        (prev_mode, hotkey)
    };

    let mode_display = if mode == "push_to_talk" {
        "push-to-talk"
    } else {
        "toggle"
    };
    log_info!("config", "Recording mode changed to: {}", mode_display);

    // Unregister old hotkey and register with new mode
    if let Err(e) = reregister_hotkey(app, &hotkey, mode) {
        log_error!("hotkey", "Failed to re-register hotkey: {}", e);
        // Restore previous mode in config
        let mut state_guard = state.inner().lock().unwrap();
        state_guard.config.recording_mode = prev_mode.clone();
        let _ = save_config(&state_guard.config);
        return;
    }

    // Rebuild the menu to reflect the new selection
    rebuild_tray_menu(app);
}

/// Toggle VAD (Voice Activity Detection) enabled/disabled
fn toggle_vad(app: &tauri::AppHandle) {
    let state = app.state::<Mutex<AppState>>();
    let new_state = {
        let mut state_guard = state.inner().lock().unwrap();

        // Toggle the state
        state_guard.config.vad_enabled = !state_guard.config.vad_enabled;
        let new_state = state_guard.config.vad_enabled;

        // Save config
        if let Err(e) = save_config(&state_guard.config) {
            log_error!("config", "Failed to save config: {}", e);
            // Restore previous value
            state_guard.config.vad_enabled = !new_state;
            return;
        }

        new_state
    };

    let status = if new_state { "enabled" } else { "disabled" };
    log_info!("vad", "Voice Activity Detection {}", status);

    // Rebuild the menu to reflect the new state
    rebuild_tray_menu(app);
}

fn set_vad_threshold(app: &tauri::AppHandle, threshold: f32) {
    update_vad_config(
        app,
        format!("VAD threshold set to {:.2}", threshold),
        |config| {
            config.vad_threshold = threshold;
        },
    );
}

fn set_vad_min_speech_ms(app: &tauri::AppHandle, duration_ms: u32) {
    update_vad_config(
        app,
        format!("VAD minimum speech set to {}ms", duration_ms),
        |config| {
            config.vad_min_speech_ms = duration_ms;
        },
    );
}

fn set_vad_silence_timeout_ms(app: &tauri::AppHandle, duration_ms: u32) {
    update_vad_config(
        app,
        format!("VAD silence timeout set to {}ms", duration_ms),
        |config| {
            config.silence_timeout_ms = duration_ms;
        },
    );
}

fn update_vad_config<F>(app: &tauri::AppHandle, log_message: String, update: F)
where
    F: FnOnce(&mut config::DyburConfig),
{
    let state = app.state::<Mutex<AppState>>();
    {
        let mut state_guard = state.inner().lock().unwrap();
        let previous_config = state_guard.config.clone();

        update(&mut state_guard.config);

        if let Err(e) = save_config(&state_guard.config) {
            log_error!("config", "Failed to save config: {}", e);
            state_guard.config = previous_config;
            return;
        }
    }

    log_info!("vad", "{}", log_message);
    rebuild_tray_menu(app);
}

/// Select GPU acceleration mode
fn select_gpu_mode(app: &tauri::AppHandle, mode: &str) {
    let state = app.state::<Mutex<AppState>>();
    {
        let mut state_guard = state.inner().lock().unwrap();

        let prev_mode = state_guard.config.gpu_mode.clone();

        // No change needed
        if prev_mode == mode {
            return;
        }

        state_guard.config.gpu_mode = mode.to_string();

        // Save config
        if let Err(e) = save_config(&state_guard.config) {
            log_error!("config", "Failed to save config: {}", e);
            // Restore previous value
            state_guard.config.gpu_mode = prev_mode;
            return;
        }
    }

    let mode_display = if mode == "cpu" {
        "CPU only"
    } else {
        "Auto (GPU if available)"
    };
    log_info!("config", "GPU mode changed to: {}", mode_display);

    // Show notification that restart is needed
    #[cfg(target_os = "windows")]
    show_windows_notification(
        "GPU Mode Changed",
        &format!("GPU mode set to: {}. Restart app to apply.", mode_display),
    );
    #[cfg(target_os = "macos")]
    show_macos_notification(
        "GPU Mode Changed",
        &format!("GPU mode set to: {}. Restart app to apply.", mode_display),
    );

    // Rebuild the menu to reflect the new state
    rebuild_tray_menu(app);
}

/// Unregister and re-register the hotkey with a new recording mode
fn reregister_hotkey(
    app: &tauri::AppHandle,
    hotkey: &str,
    recording_mode: &str,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::Shortcut;

    let shortcut: Shortcut = hotkey
        .parse()
        .map_err(|e| format!("Invalid hotkey format '{}': {}", hotkey, e))?;

    // Unregister the existing shortcut
    app.global_shortcut()
        .unregister(shortcut)
        .map_err(|e| format!("Failed to unregister shortcut: {}", e))?;

    // Re-register with new mode
    register_hotkey(app, hotkey, recording_mode)?;

    Ok(())
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
        log_info!(
            "models",
            "Removed {} unused model(s): {:?}",
            removed.len(),
            removed
        );
        #[cfg(target_os = "windows")]
        show_windows_notification(
            "Clean Models",
            &format!("Removed {} unused model(s).", removed.len()),
        );
        #[cfg(target_os = "macos")]
        show_macos_notification(
            "Clean Models",
            &format!("Removed {} unused model(s).", removed.len()),
        );

        // Rebuild menu to reflect changes
        rebuild_tray_menu(app);
    }
}

/// Handle model selection from the menu
fn handle_model_selection(app: &tauri::AppHandle, model_id: &str) {
    // Check if model exists in registry
    let model_def = match get_model_definition(model_id) {
        Some(def) => def,
        None => {
            log_error!("models", "Unknown model ID: {}", model_id);
            return;
        }
    };

    // Check if model is installed
    if is_model_installed(model_id) {
        // Model is installed - switch to it
        switch_to_model(app, model_id);
    } else {
        // Model needs to be downloaded first
        log_info!(
            "models",
            "Model '{}' not installed, starting download...",
            model_id
        );
        spawn_model_download_by_id(app, model_id, model_def.display_name);
    }
}

/// Switch to a different model (assumes model is already installed)
fn switch_to_model(app: &tauri::AppHandle, model_id: &str) {
    log_info!("models", "Switching to model: {}", model_id);

    // Update config
    {
        let state = app.state::<Mutex<AppState>>();
        let mut state_guard = state.inner().lock().unwrap();
        state_guard.config.model = model_id.to_string();

        // Save config
        if let Err(e) = save_config(&state_guard.config) {
            log_error!("config", "Failed to save config: {}", e);
        }
    }

    // Load the new model
    let state = app.state::<Mutex<AppState>>();
    let gpu_preference = {
        let state_guard = state.inner().lock().unwrap();
        execution_providers::parse_gpu_preference(&state_guard.config.gpu_mode)
    };

    if let Some(stt_config) = get_model_paths(model_id) {
        let mut engine = STT_ENGINE.lock().unwrap();

        // Unload current model first
        engine.unload();

        match engine.load(stt_config, gpu_preference) {
            Ok(()) => {
                log_info!("model", "Switched to model '{}' successfully", model_id);
                #[cfg(target_os = "windows")]
                show_windows_notification("Model Switched", &format!("Now using: {}", model_id));
                #[cfg(target_os = "macos")]
                show_macos_notification("Model Switched", &format!("Now using: {}", model_id));
            }
            Err(e) => {
                log_error!("model", "Failed to load model '{}': {}", model_id, e);
                #[cfg(target_os = "windows")]
                show_windows_notification("Model Error", &format!("Failed to load model: {}", e));
                #[cfg(target_os = "macos")]
                show_macos_notification("Model Error", &format!("Failed to load model: {}", e));
            }
        }
    } else {
        log_error!(
            "model",
            "Model '{}' not supported or files missing",
            model_id
        );
        #[cfg(target_os = "windows")]
        show_windows_notification("Model Error", "Model not supported or files missing");
        #[cfg(target_os = "macos")]
        show_macos_notification("Model Error", "Model not supported or files missing");
    }

    // Rebuild menu to show new selection
    rebuild_tray_menu(app);
}

/// Download a specific model by ID
fn spawn_model_download_by_id(app: &tauri::AppHandle, model_id: &str, display_name: &str) {
    if is_download_in_progress() {
        log_info!("models", "Download already in progress");
        return;
    }

    log_info!("models", "Starting download of model: {}", model_id);
    #[cfg(target_os = "windows")]
    show_windows_notification(
        "Model Download",
        &format!(
            "Downloading {}... This may take a few minutes.",
            display_name
        ),
    );
    #[cfg(target_os = "macos")]
    show_macos_notification(
        "Model Download",
        &format!(
            "Downloading {}... This may take a few minutes.",
            display_name
        ),
    );

    // Rebuild menu immediately to show "Downloading..." status
    rebuild_tray_menu(app);

    // Start menu refresh thread (updates every 500ms during download for smooth progress)
    let app_handle_refresh = app.clone();
    std::thread::spawn(move || {
        while is_download_in_progress() {
            std::thread::sleep(std::time::Duration::from_millis(500));
            rebuild_tray_menu(&app_handle_refresh);
        }
        // One final refresh after download completes
        rebuild_tray_menu(&app_handle_refresh);
    });

    // Start download thread
    let app_handle = app.clone();
    let model_id_owned = model_id.to_string();
    let display_name_owned = display_name.to_string();
    std::thread::spawn(move || {
        match models::download_model_sync(&model_id_owned, "int8") {
            Ok(path) => {
                log_info!("models", "Model downloaded to: {}", path.display());
                #[cfg(target_os = "windows")]
                show_windows_notification(
                    "Model Download",
                    &format!("{} downloaded successfully!", display_name_owned),
                );
                #[cfg(target_os = "macos")]
                show_macos_notification(
                    "Model Download",
                    &format!("{} downloaded successfully!", display_name_owned),
                );

                // Rebuild menu to show the new model
                rebuild_tray_menu(&app_handle);

                // Automatically switch to the downloaded model
                switch_to_model(&app_handle, &model_id_owned);
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

/// Start recording audio
fn start_recording(app: &tauri::AppHandle) {
    let state = app.state::<Mutex<AppState>>();
    let mut state_guard = state.inner().lock().unwrap();

    // Already recording, nothing to do
    if state_guard.is_recording {
        log_info!("audio", "Already recording, ignoring start request");
        return;
    }

    state_guard.clear_error();

    // Check if STT model is loaded
    {
        let engine = STT_ENGINE.lock().unwrap();
        if !engine.is_ready() {
            let model_name = state_guard.config.model.clone();
            log_error!(
                "model",
                "STT model '{}' not loaded. Run 'dybur models download {}' first.",
                model_name,
                model_name
            );
            state_guard.set_error(format!(
                "STT model '{}' not loaded. Run 'dybur models download {}' first.",
                model_name, model_name
            ));
            update_tray_status(app, "Model not loaded");

            // Show native alert to user - must be done in a separate thread to not block
            std::thread::spawn(move || {
                let title = "Speech Model Required";
                let message = "The speech recognition model is not installed.\n\n\
                    Dictation requires the model to be downloaded first.\n\n\
                    Right-click the dybur tray icon and select:\n\
                    Models > your selected model";

                #[cfg(target_os = "windows")]
                show_windows_alert(title, message);

                #[cfg(target_os = "macos")]
                show_macos_alert(title, message);
            });
            return;
        }
    }

    let input_device = state_guard.config.input_device.clone();
    match AUDIO_SESSION.start(input_device) {
        Ok(active_recording) => {
            {
                let mut global_buffer = RECORDING_BUFFER.lock().unwrap();
                *global_buffer = Some(active_recording.buffer);
                let mut global_sample_rate = RECORDING_SAMPLE_RATE.lock().unwrap();
                *global_sample_rate = active_recording.sample_rate;
                log_info!(
                    "audio",
                    "Recording buffer stored globally (sample_rate: {}Hz)",
                    active_recording.sample_rate
                );
            }

            state_guard.set_recording(true);
            set_overlay_state(app, RecordingState::Recording);
            log_info!("audio", "Recording started");
            update_tray_status(app, "Recording...");

            let streaming_enabled = state_guard.config.streaming_enabled;
            drop(state_guard);

            if streaming_enabled {
                start_streaming_inference(app);
            }
        }
        Err(e) => {
            let help = get_audio_error_help(&e);
            let error_msg = format!("{}\n\n{}", e, help);
            state_guard.set_error(error_msg.clone());
            set_overlay_state(app, RecordingState::Idle);
            log_error!("audio", "Failed to start recording: {}", error_msg);
            update_tray_status(app, "Error: Recording failed");
        }
    }
}

fn post_process_text(text: &str, sentence_case: bool, auto_punctuation: bool) -> String {
    let mut processed = normalize_transcription_whitespace(text.trim());

    if processed.is_empty() {
        return processed;
    }

    if sentence_case {
        processed = apply_sentence_case(&processed);
    }

    if auto_punctuation {
        processed = add_basic_punctuation(&processed);
    }

    processed
}

fn normalize_transcription_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn apply_sentence_case(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut capitalize_next = true;

    for ch in text.chars() {
        if capitalize_next && ch.is_alphabetic() {
            for upper in ch.to_uppercase() {
                result.push(upper);
            }
            capitalize_next = false;
            continue;
        }

        result.push(ch);

        if matches!(ch, '.' | '!' | '?') {
            capitalize_next = true;
        } else if !ch.is_whitespace() {
            capitalize_next = false;
        }
    }

    result
}

fn add_basic_punctuation(text: &str) -> String {
    let trimmed = text.trim_end();
    let Some(last_char) = trimmed.chars().next_back() else {
        return String::new();
    };

    if matches!(last_char, '.' | '!' | '?' | ',' | ';' | ':') {
        trimmed.to_string()
    } else {
        format!("{}.", trimmed)
    }
}

/// Stop recording and process audio
fn stop_recording(app: &tauri::AppHandle) {
    let state = app.state::<Mutex<AppState>>();
    let mut state_guard = state.inner().lock().unwrap();

    // Not recording, nothing to do
    if !state_guard.is_recording {
        log_info!("audio", "Not recording, ignoring stop request");
        return;
    }

    // Stop the streaming worker loop, but release the microphone before waiting
    // on any streaming finalization work.
    signal_streaming_inference_stop();

    let audio_data: Option<Vec<f32>> = match AUDIO_SESSION.stop() {
        Ok(Some(recorded)) => {
            let duration = recorded.samples.len() as f32 / recorded.sample_rate as f32;
            log_info!(
                "audio",
                "Audio session stopped. Captured {:.2}s of audio",
                duration
            );
            Some(recorded.samples)
        }
        Ok(None) => {
            log_warn!("audio", "No active audio capture session found");
            None
        }
        Err(e) => {
            let help = get_audio_error_help(&e);
            let error_msg = format!("{}\n\n{}", e, help);
            state_guard.set_error(error_msg.clone());
            log_error!("audio", "Failed to stop recording: {}", error_msg);
            None
        }
    };

    {
        let mut global_buffer = RECORDING_BUFFER.lock().unwrap();
        *global_buffer = None;
    }

    state_guard.set_recording(false);
    set_overlay_state(app, RecordingState::Idle);
    log_info!("audio", "Recording stopped");

    // Get config for clipboard cleanup and VAD settings
    let restore_clipboard = state_guard.config.clipboard_cleanup;
    let vad_enabled = state_guard.config.vad_enabled;
    let vad_threshold = state_guard.config.vad_threshold;
    let vad_min_speech_ms = state_guard.config.vad_min_speech_ms;
    let vad_min_silence_ms = state_guard.config.silence_timeout_ms;
    let auto_punctuation = state_guard.config.auto_punctuation;
    let sentence_case = state_guard.config.sentence_case;

    // Release the state lock before processing
    drop(state_guard);

    // Emit final streaming result after the microphone stream has been released.
    let streaming_text = finish_streaming_inference();
    if let Some(ref text) = streaming_text {
        log_info!("streaming", "Final streaming text: {} chars", text.len());
        let _ = app.emit(
            "streaming-transcription",
            serde_json::json!({
                "text": text,
                "is_final": true
            }),
        );
    }

    // Process audio with STT if we have audio data
    if let Some(audio) = audio_data {
        let mut source_audio = audio;

        if source_audio.len() < 1600 {
            // Less than 100ms of audio - too short
            log_warn!(
                "audio",
                "Recording too short ({} samples), skipping transcription",
                source_audio.len()
            );
            update_tray_status(app, "Too short");
            privacy::secure_clear_audio_buffer(&mut source_audio);
            return;
        }

        // Apply VAD filtering if enabled
        let mut filtered_audio = if vad_enabled {
            let mut vad_engine = VAD_ENGINE.lock().unwrap();
            if vad_engine.is_ready() {
                // Configure VAD with current settings
                vad_engine.set_config(vad::VadConfig {
                    threshold: vad_threshold,
                    min_speech_duration_ms: vad_min_speech_ms,
                    min_silence_duration_ms: vad_min_silence_ms,
                    ..Default::default()
                });

                match vad_engine.filter_speech(&source_audio) {
                    Ok(mut filtered) => {
                        if filtered.is_empty() {
                            log_info!("vad", "No speech detected by VAD");
                            update_tray_status(app, "No speech detected");
                            privacy::secure_clear_audio_buffer(&mut source_audio);
                            return;
                        }
                        if filtered.len() < 1600 {
                            log_info!(
                                "vad",
                                "Speech too short after VAD filtering ({} samples)",
                                filtered.len()
                            );
                            update_tray_status(app, "Too short");
                            privacy::secure_clear_audio_buffer(&mut filtered);
                            privacy::secure_clear_audio_buffer(&mut source_audio);
                            return;
                        }
                        filtered
                    }
                    Err(e) => {
                        log_warn!("vad", "VAD filtering failed: {}, using original audio", e);
                        source_audio.clone()
                    }
                }
            } else {
                source_audio.clone()
            }
        } else {
            source_audio.clone()
        };
        privacy::secure_clear_audio_buffer(&mut source_audio);

        update_tray_status(app, "Transcribing...");

        // Run STT inference
        let transcription_result = {
            let mut engine = STT_ENGINE.lock().unwrap();
            if !engine.is_ready() {
                log_warn!("model", "STT model not loaded, cannot transcribe");
                None
            } else {
                match engine.transcribe(&filtered_audio) {
                    Ok(result) => Some(result),
                    Err(e) => {
                        log_error!("model", "Transcription failed: {}", e);
                        None
                    }
                }
            }
        };
        privacy::secure_clear_audio_buffer(&mut filtered_audio);

        // Inject transcribed text
        if let Some(result) = transcription_result {
            let processed_text = post_process_text(&result.text, sentence_case, auto_punctuation);

            if processed_text.is_empty() {
                log_info!("model", "Transcription returned empty text");
                update_tray_status(app, "No speech detected");
            } else {
                let realtime = if result.inference_time_ms == 0 {
                    0.0
                } else {
                    result.audio_duration_s * 1000.0 / result.inference_time_ms as f32
                };
                log_info!(
                    "model",
                    "Transcribed {} chars in {}ms ({}x realtime)",
                    processed_text.len(),
                    result.inference_time_ms,
                    format!("{:.1}", realtime)
                );

                // Check if FTUE window exists - if so, emit directly instead of clipboard paste
                // (clipboard paste doesn't work reliably in our own webview windows)
                let ftue_exists = app.get_webview_window("ftue").is_some();

                if ftue_exists {
                    // Emit transcription directly to FTUE window
                    let _ = app.emit_to("ftue", "ftue:transcription", &processed_text);
                    log_info!("injection", "Text emitted to FTUE window");
                    update_tray_status(app, "Done");
                } else {
                    // Inject the text into the active application
                    match inject_text(&processed_text, restore_clipboard) {
                        Ok(()) => {
                            log_info!("injection", "Text injected successfully");
                            update_tray_status(app, "Done");
                        }
                        Err(e) => {
                            log_error!("injection", "Failed to inject text: {}", e);
                            update_tray_status(app, "Paste failed - text on clipboard");

                            // Show alert for accessibility permission issues on macOS
                            #[cfg(target_os = "macos")]
                            {
                                let error_str = e.to_string();
                                if error_str.contains("Accessibility")
                                    || error_str.contains("permission")
                                    || error_str.contains("not allowed")
                                {
                                    show_macos_alert(
                                        "Accessibility Permission Required",
                                        "dybur needs Accessibility permission to paste text automatically.\n\n\
                                        A system dialog should have appeared asking for permission.\n\
                                        If not, please go to:\n\
                                        System Settings > Privacy & Security > Accessibility\n\n\
                                        Then enable dybur in the list.\n\n\
                                        Your transcribed text is on the clipboard - press Cmd+V to paste it manually."
                                    );
                                } else {
                                    // Other injection error
                                    show_macos_alert(
                                        "Text Injection Failed",
                                        &format!(
                                            "Failed to paste text: {}\n\n\
                                            Your transcribed text is on the clipboard - press Cmd+V to paste it manually.",
                                            e
                                        )
                                    );
                                }
                            }

                            #[cfg(target_os = "windows")]
                            {
                                // On Windows, show a notification
                                show_windows_notification(
                                    "Injection Failed",
                                    &format!("Failed to paste text: {}. Text is on clipboard.", e),
                                );
                            }
                        }
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
}

/// Toggle recording state (for toggle mode)
fn toggle_recording(app: &tauri::AppHandle) {
    let state = app.state::<Mutex<AppState>>();
    let is_recording = state.inner().lock().unwrap().is_recording;

    if is_recording {
        stop_recording(app);
    } else {
        start_recording(app);
    }
}

/// Start streaming inference in background thread
fn start_streaming_inference(app: &tauri::AppHandle) {
    use crate::models::ModelArchitecture;

    // Get the sample rate for streaming
    let sample_rate = *RECORDING_SAMPLE_RATE.lock().unwrap();

    // Check if model supports streaming
    let streaming_state = {
        let mut engine = STT_ENGINE.lock().unwrap();
        if engine.get_architecture() != ModelArchitecture::StreamingTransducer {
            log_info!("streaming", "Model does not support streaming, skipping");
            return;
        }

        // Initialize streaming state with sample rate for proper resampling
        match streaming::StreamingState::from_engine(&mut engine, sample_rate) {
            Some(state) => state,
            None => {
                log_warn!("streaming", "Failed to initialize streaming state");
                return;
            }
        }
    };

    // Store streaming state globally
    {
        let mut global_streaming = STREAMING_STATE.lock().unwrap();
        *global_streaming = Some(streaming_state);
    }

    // Get buffer reference for streaming thread
    let buffer_arc = {
        let global_buffer = RECORDING_BUFFER.lock().unwrap();
        match global_buffer.as_ref() {
            Some(arc) => arc.clone(),
            None => {
                log_warn!("streaming", "No recording buffer available");
                return;
            }
        }
    };

    // Signal streaming thread to run
    STREAMING_RUNNING.store(true, Ordering::SeqCst);

    // Start streaming polling thread
    let app_handle = app.clone();
    std::thread::spawn(move || {
        log_info!("streaming", "Streaming inference thread started");

        while STREAMING_RUNNING.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(STREAMING_POLL_INTERVAL_MS));

            if !STREAMING_RUNNING.load(Ordering::SeqCst) {
                break;
            }

            // Process incremental audio
            let process_result = {
                let mut streaming_state = STREAMING_STATE.lock().unwrap();
                let mut engine = STT_ENGINE.lock().unwrap();

                if let Some(ref mut state) = *streaming_state {
                    match streaming::process_incremental_with_metrics(
                        state,
                        &buffer_arc,
                        &mut engine,
                    ) {
                        Ok(result) => result,
                        Err(e) => {
                            log_warn!("streaming", "Streaming inference error: {}", e);
                            streaming::StreamingProcessResult::default()
                        }
                    }
                } else {
                    streaming::StreamingProcessResult::default()
                }
            };

            for metrics in &process_result.metrics {
                log_debug!(
                    "streaming",
                    "Chunk {} metrics: available_audio_ms={} processed_audio_ms={} backlog_ms={} tokens_emitted={} partial_chars={} feature_ms={:.2} encoder_ms={:.2} decoder_ms={:.2} joiner_ms={:.2} total_ms={:.2}",
                    metrics.chunk_index,
                    metrics.available_audio_ms,
                    metrics.processed_audio_ms,
                    metrics.backlog_ms,
                    metrics.tokens_emitted,
                    metrics.partial_chars,
                    metrics.feature_ms,
                    metrics.encoder_ms,
                    metrics.decoder_ms,
                    metrics.joiner_ms,
                    metrics.total_ms
                );
                let _ = app_handle.emit("streaming-metrics", metrics);
            }

            // Emit partial transcription to frontend
            if let Some(text) = process_result.partial_text {
                log_debug!(
                    "streaming",
                    "Partial transcription updated ({} chars)",
                    text.chars().count()
                );
                let _ = app_handle.emit(
                    "streaming-transcription",
                    serde_json::json!({
                        "text": text,
                        "is_final": false
                    }),
                );
            }
        }

        log_info!("streaming", "Streaming inference thread stopped");
    });

    log_info!("streaming", "Streaming inference initialized");
}

fn signal_streaming_inference_stop() {
    STREAMING_RUNNING.store(false, Ordering::SeqCst);
}

fn finish_streaming_inference() -> Option<String> {
    // Give thread time to finish current iteration
    std::thread::sleep(std::time::Duration::from_millis(100));

    // Get final text from streaming state
    let final_text = {
        let streaming_state = STREAMING_STATE.lock().unwrap();
        if let Some(ref state) = *streaming_state {
            let text = state.get_partial_text();
            if !text.is_empty() {
                Some(text)
            } else {
                None
            }
        } else {
            None
        }
    };

    // Clear streaming state
    {
        let mut streaming_state = STREAMING_STATE.lock().unwrap();
        *streaming_state = None;
    }

    final_text
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

    let mut builder =
        WebviewWindowBuilder::new(app, OVERLAY_LABEL, WebviewUrl::App("index.html".into()))
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
                use windows::Win32::Foundation::HWND;
                use windows::Win32::Graphics::Dwm::{
                    DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND,
                    DWM_WINDOW_CORNER_PREFERENCE,
                };
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_LAYERED, WS_EX_NOACTIVATE,
                    WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
                };

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
    if let Err(e) = tauri_plugin_shell::ShellExt::shell(app)
        .open(&logs_dir, None::<tauri_plugin_shell::open::Program>)
    {
        log_error!("service", "Failed to open logs directory: {:?}", e);
    }
}

/// Register global hotkey
fn register_hotkey(
    app: &tauri::AppHandle,
    hotkey: &str,
    recording_mode: &str,
) -> Result<(), String> {
    use tauri_plugin_global_shortcut::Shortcut;

    let shortcut: Shortcut = hotkey.parse().map_err(|e| {
        let msg = format!("Invalid hotkey format '{}': {}", hotkey, e);
        log_error!("hotkey", "{}", msg);
        msg
    })?;

    let app_handle = app.clone();
    let is_push_to_talk = recording_mode == "push_to_talk";

    if is_push_to_talk {
        log_info!(
            "hotkey",
            "Using push-to-talk mode: hold {} to record",
            hotkey
        );
    } else {
        log_info!(
            "hotkey",
            "Using toggle mode: press {} to start/stop",
            hotkey
        );
    }

    app.global_shortcut()
        .on_shortcut(shortcut, move |_app, _shortcut, event| {
            if is_push_to_talk {
                // Push-to-talk mode: hold to record, release to stop
                match event.state {
                    ShortcutState::Pressed => start_recording(&app_handle),
                    ShortcutState::Released => stop_recording(&app_handle),
                }
            } else {
                // Toggle mode: press to start/stop
                if event.state == ShortcutState::Pressed {
                    toggle_recording(&app_handle);
                }
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

fn show_user_notification(title: &str, message: &str) {
    #[cfg(target_os = "windows")]
    show_windows_notification(title, message);
    #[cfg(target_os = "macos")]
    show_macos_notification(title, message);
}

fn show_user_alert(title: &str, message: &str) {
    #[cfg(target_os = "windows")]
    show_windows_alert(title, message);
    #[cfg(target_os = "macos")]
    show_macos_alert(title, message);
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
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};

    let title_wide: Vec<u16> = OsStr::new(title)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let message_wide: Vec<u16> = OsStr::new(message)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

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

    let _ = Command::new("osascript").args(["-e", &script]).output();

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
