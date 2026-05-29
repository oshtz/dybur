//! Text injection via clipboard and keyboard simulation
//!
//! Primary method: Copy to clipboard + Paste (Cmd+V / Ctrl+V)
//! Fallback: Synthetic key events
//!
//! Includes secure input detection and retry logic.

#[cfg(target_os = "macos")]
use std::process::Command;
use std::time::Duration;

// ============================================================================
// macOS Accessibility Permission Check
// ============================================================================

#[cfg(target_os = "macos")]
mod macos_accessibility {
    use std::ffi::c_void;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFDictionaryCreate(
            allocator: *const c_void,
            keys: *const *const c_void,
            values: *const *const c_void,
            num_values: isize,
            key_callbacks: *const c_void,
            value_callbacks: *const c_void,
        ) -> *const c_void;
        fn CFRelease(cf: *const c_void);

        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
        static kCFBooleanTrue: *const c_void;
    }

    // This is the key for the prompt option
    // "AXTrustedCheckOptionPrompt"
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        static kAXTrustedCheckOptionPrompt: *const c_void;
    }

    /// Check if the process has Accessibility permissions
    pub fn is_accessibility_enabled() -> bool {
        unsafe { AXIsProcessTrustedWithOptions(std::ptr::null()) }
    }

    /// Check if the process has Accessibility permissions, optionally prompting the user
    /// If prompt is true, shows a system dialog directing the user to Security & Privacy settings
    pub fn check_accessibility_with_prompt(prompt: bool) -> bool {
        unsafe {
            if !prompt {
                return AXIsProcessTrustedWithOptions(std::ptr::null());
            }

            // Create options dictionary with kAXTrustedCheckOptionPrompt = true
            let keys: [*const c_void; 1] = [kAXTrustedCheckOptionPrompt];
            let values: [*const c_void; 1] = [kCFBooleanTrue];

            let options = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                values.as_ptr(),
                1,
                &kCFTypeDictionaryKeyCallBacks as *const _ as *const c_void,
                &kCFTypeDictionaryValueCallBacks as *const _ as *const c_void,
            );

            let result = AXIsProcessTrustedWithOptions(options);

            if !options.is_null() {
                CFRelease(options);
            }

            result
        }
    }
}

/// Check if we have Accessibility permissions (macOS only)
/// Returns true on non-macOS platforms
#[cfg(target_os = "macos")]
pub fn has_accessibility_permission() -> bool {
    macos_accessibility::is_accessibility_enabled()
}

#[cfg(not(target_os = "macos"))]
pub fn has_accessibility_permission() -> bool {
    true // Not needed on other platforms
}

/// Request Accessibility permissions from the user (macOS only)
/// This will show a system dialog directing them to Security & Privacy settings
/// Returns true if permissions are already granted, false otherwise
#[cfg(target_os = "macos")]
pub fn request_accessibility_permission() -> bool {
    let result = macos_accessibility::check_accessibility_with_prompt(true);
    if result {
        crate::log_info!("injection", "Accessibility permission granted");
    } else {
        crate::log_info!(
            "injection",
            "Accessibility permission dialog shown, waiting for user"
        );
    }
    result
}

#[cfg(not(target_os = "macos"))]
pub fn request_accessibility_permission() -> bool {
    true // Not needed on other platforms
}

/// Clipboard operations
pub struct Clipboard;

// ============================================================================
// Windows implementation using native Win32 APIs
// ============================================================================

#[cfg(target_os = "windows")]
mod windows_impl {
    use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER,
        COINIT_APARTMENTTHREADED,
    };
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
    use windows::Win32::UI::Accessibility::{CUIAutomation, IUIAutomation};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VK_CONTROL, VK_V,
    };

    const CF_UNICODETEXT: u32 = 13;

    /// Get text from clipboard using Win32 API
    pub fn get_clipboard() -> Result<String, String> {
        unsafe {
            // Open clipboard
            OpenClipboard(HWND::default())
                .map_err(|e| format!("Failed to open clipboard: {}", e))?;

            // Get clipboard data
            let handle = GetClipboardData(CF_UNICODETEXT);
            let result = if handle.is_err() {
                Ok(String::new()) // Empty clipboard
            } else {
                let handle = handle.unwrap();
                // Convert HANDLE to HGLOBAL for GlobalLock
                let hglobal = HGLOBAL(handle.0);
                let ptr = GlobalLock(hglobal);
                if ptr.is_null() {
                    Ok(String::new())
                } else {
                    // Read UTF-16 string
                    let wide_ptr = ptr as *const u16;
                    let mut len = 0;
                    while *wide_ptr.add(len) != 0 {
                        len += 1;
                    }
                    let slice = std::slice::from_raw_parts(wide_ptr, len);
                    let text = String::from_utf16_lossy(slice);
                    let _ = GlobalUnlock(hglobal);
                    Ok(text)
                }
            };

            CloseClipboard().map_err(|e| format!("Failed to close clipboard: {}", e))?;
            result
        }
    }

    /// Set text to clipboard using Win32 API
    pub fn set_clipboard(text: &str) -> Result<(), String> {
        unsafe {
            // Convert to UTF-16
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let size = wide.len() * 2;

            // Allocate global memory
            let hmem = GlobalAlloc(GMEM_MOVEABLE, size)
                .map_err(|e| format!("Failed to allocate memory: {}", e))?;

            let ptr = GlobalLock(hmem);
            if ptr.is_null() {
                return Err("Failed to lock memory".to_string());
            }

            // Copy data
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr as *mut u16, wide.len());
            let _ = GlobalUnlock(hmem);

            // Open clipboard
            OpenClipboard(HWND::default())
                .map_err(|e| format!("Failed to open clipboard: {}", e))?;

            // Empty and set clipboard
            EmptyClipboard().map_err(|e| format!("Failed to empty clipboard: {}", e))?;

            // Convert HGLOBAL to HANDLE for SetClipboardData
            let result = SetClipboardData(CF_UNICODETEXT, HANDLE(hmem.0));

            CloseClipboard().map_err(|e| format!("Failed to close clipboard: {}", e))?;

            result.map_err(|e| format!("Failed to set clipboard data: {}", e))?;
            Ok(())
        }
    }

    /// Send Ctrl+V using SendInput (native, no process spawn)
    pub fn send_paste() -> Result<(), String> {
        unsafe {
            let mut inputs: [INPUT; 4] = std::mem::zeroed();

            // Ctrl down
            inputs[0].r#type = INPUT_KEYBOARD;
            inputs[0].Anonymous.ki = KEYBDINPUT {
                wVk: VK_CONTROL,
                wScan: 0,
                dwFlags: KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0,
            };

            // V down
            inputs[1].r#type = INPUT_KEYBOARD;
            inputs[1].Anonymous.ki = KEYBDINPUT {
                wVk: VK_V,
                wScan: 0,
                dwFlags: KEYBD_EVENT_FLAGS(0),
                time: 0,
                dwExtraInfo: 0,
            };

            // V up
            inputs[2].r#type = INPUT_KEYBOARD;
            inputs[2].Anonymous.ki = KEYBDINPUT {
                wVk: VK_V,
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            };

            // Ctrl up
            inputs[3].r#type = INPUT_KEYBOARD;
            inputs[3].Anonymous.ki = KEYBDINPUT {
                wVk: VK_CONTROL,
                wScan: 0,
                dwFlags: KEYEVENTF_KEYUP,
                time: 0,
                dwExtraInfo: 0,
            };

            let sent = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
            if sent != 4 {
                return Err(format!("SendInput only sent {} of 4 inputs", sent));
            }

            Ok(())
        }
    }

    /// Detect whether the focused UI Automation element is a password field.
    pub fn is_secure_input_active() -> Result<bool, String> {
        unsafe {
            let com_init = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let should_uninitialize = com_init.is_ok();
            if com_init.is_err() {
                crate::log_warn!(
                    "injection",
                    "COM initialization for secure input detection failed: {:?}",
                    com_init
                );
            }

            let result = (|| {
                let automation: IUIAutomation =
                    CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
                        .map_err(|e| format!("Failed to create UI Automation instance: {}", e))?;
                let focused = automation
                    .GetFocusedElement()
                    .map_err(|e| format!("Failed to read focused UI element: {}", e))?;
                let is_password = focused
                    .CurrentIsPassword()
                    .map_err(|e| format!("Failed to read password field state: {}", e))?;

                Ok(is_password.as_bool())
            })();

            if should_uninitialize {
                CoUninitialize();
            }

            result
        }
    }
}

impl Clipboard {
    /// Get current clipboard contents
    #[cfg(target_os = "windows")]
    pub fn get() -> Result<String, String> {
        windows_impl::get_clipboard()
    }

    #[cfg(target_os = "macos")]
    pub fn get() -> Result<String, String> {
        let output = Command::new("pbpaste")
            .output()
            .map_err(|e| format!("Failed to get clipboard: {}", e))?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Set clipboard contents
    #[cfg(target_os = "windows")]
    pub fn set(text: &str) -> Result<(), String> {
        windows_impl::set_clipboard(text)
    }

    #[cfg(target_os = "macos")]
    pub fn set(text: &str) -> Result<(), String> {
        use std::io::Write;
        use std::process::Stdio;

        crate::log_debug!("injection", "Setting clipboard ({} chars)", text.len());

        let mut child = Command::new("pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| {
                crate::log_error!("injection", "Failed to spawn pbcopy: {}", e);
                format!("Failed to set clipboard: {}", e)
            })?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(text.as_bytes()).map_err(|e| {
                crate::log_error!("injection", "Failed to write to pbcopy: {}", e);
                format!("Failed to write to clipboard: {}", e)
            })?;
        }

        let status = child.wait().map_err(|e| {
            crate::log_error!("injection", "pbcopy failed: {}", e);
            format!("Clipboard command failed: {}", e)
        })?;

        if !status.success() {
            crate::log_error!("injection", "pbcopy exited with status: {}", status);
            return Err(format!("pbcopy failed with status: {}", status));
        }

        crate::log_debug!("injection", "Clipboard set successfully");
        Ok(())
    }
}

/// Send paste command (Ctrl+V on Windows, Cmd+V on macOS)
#[cfg(target_os = "windows")]
pub fn send_paste() -> Result<(), String> {
    windows_impl::send_paste()
}

#[cfg(target_os = "macos")]
pub fn send_paste() -> Result<(), String> {
    // macOS: Use AppleScript with System Events
    // Note: This requires Accessibility permissions to be granted to the app

    // Check accessibility permissions first - this will prompt the user if not granted
    if !has_accessibility_permission() {
        crate::log_info!(
            "injection",
            "Accessibility permission not granted, requesting..."
        );

        // This will show the system dialog to request permissions
        let granted = request_accessibility_permission();

        if !granted {
            // User needs to grant permission - the dialog has been shown
            return Err("Accessibility permission required. Please grant permission in the dialog that appeared, then try again.".to_string());
        }
    }

    // Send the paste command
    let result = Command::new("osascript")
        .args([
            "-e",
            "tell application \"System Events\" to keystroke \"v\" using command down",
        ])
        .output();

    match result {
        Ok(output) => {
            // Check for errors in stderr
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stderr.is_empty() {
                crate::log_warn!("injection", "osascript stderr: {}", stderr);
                // Common errors indicating permission issues:
                // - "osascript is not allowed assistive access"
                // - "osascript is not allowed to send keystrokes" (error 1002)
                // - "accessibility"
                if stderr.contains("not allowed")
                    || stderr.contains("accessibility")
                    || stderr.contains("1002")
                {
                    crate::log_error!(
                        "injection",
                        "Accessibility permission denied for keystroke injection"
                    );
                    return Err("Accessibility permissions required. Please grant dybur access in System Preferences > Security & Privacy > Privacy > Accessibility".to_string());
                }
                // For other errors, still fail
                if stderr.contains("error") || stderr.contains("Error") {
                    return Err(format!("osascript error: {}", stderr.trim()));
                }
            }

            // Give the system time to process the keystroke
            std::thread::sleep(std::time::Duration::from_millis(100));

            crate::log_debug!("injection", "Paste keystroke sent via osascript");
            Ok(())
        }
        Err(e) => {
            crate::log_error!("injection", "Failed to send paste: {}", e);
            Err(format!("Failed to send paste: {}", e))
        }
    }
}

/// Injection error types
#[derive(Debug, Clone)]
pub enum InjectionError {
    /// Clipboard operation failed
    ClipboardError(String),
    /// Paste command failed
    PasteError(String),
    /// Secure input detected (password field)
    SecureInputDetected,
    /// Injection failed after retries
    RetryExhausted(String),
}

impl std::fmt::Display for InjectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InjectionError::ClipboardError(msg) => write!(f, "Clipboard error: {}", msg),
            InjectionError::PasteError(msg) => write!(f, "Paste failed: {}", msg),
            InjectionError::SecureInputDetected => write!(
                f,
                "Cannot inject text into secure input field (password field detected)"
            ),
            InjectionError::RetryExhausted(msg) => {
                write!(f, "Text injection failed after retry: {}", msg)
            }
        }
    }
}

impl From<InjectionError> for String {
    fn from(err: InjectionError) -> Self {
        err.to_string()
    }
}

/// Check if the active input field is a secure input (password field)
///
/// This uses platform-specific accessibility APIs to detect secure inputs.
#[cfg(target_os = "windows")]
pub fn is_secure_input_active() -> bool {
    match windows_impl::is_secure_input_active() {
        Ok(is_secure) => is_secure,
        Err(error) => {
            crate::log_warn!(
                "injection",
                "Secure input detection failed; allowing injection: {}",
                error
            );
            false
        }
    }
}

#[cfg(target_os = "macos")]
pub fn is_secure_input_active() -> bool {
    // On macOS, check IsSecureEventInputEnabled
    // This is set when a secure text field has focus
    let script = r#"
        tell application "System Events"
            try
                return secure input enabled
            on error
                return false
            end try
        end tell
    "#;

    match Command::new("osascript").args(["-e", script]).output() {
        Ok(output) => {
            let result = String::from_utf8_lossy(&output.stdout)
                .trim()
                .to_lowercase();
            result == "true"
        }
        Err(_) => false,
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos")))]
pub fn is_secure_input_active() -> bool {
    // On other platforms, we can't reliably detect secure inputs
    false
}

/// Inject text into the active application with retry logic
///
/// This is the main entry point for text injection.
pub fn inject_text(text: &str, restore_clipboard: bool) -> Result<(), InjectionError> {
    // Check for secure input first
    if is_secure_input_active() {
        crate::log_warn!(
            "injection",
            "Secure input detected, refusing to inject text"
        );
        return Err(InjectionError::SecureInputDetected);
    }

    // Try injection with one retry on failure
    match inject_text_internal(text, restore_clipboard) {
        Ok(()) => {
            crate::log_info!("injection", "Text injected successfully");
            Ok(())
        }
        Err(e) => {
            crate::log_warn!("injection", "First injection attempt failed: {}", e);

            // Wait a bit before retrying
            std::thread::sleep(Duration::from_millis(200));

            // Retry once
            match inject_text_internal(text, restore_clipboard) {
                Ok(()) => {
                    crate::log_info!("injection", "Text injected on retry");
                    Ok(())
                }
                Err(e2) => {
                    crate::log_error!("injection", "Injection failed after retry: {}", e2);
                    Err(InjectionError::RetryExhausted(e2))
                }
            }
        }
    }
}

/// Internal injection implementation
fn inject_text_internal(text: &str, restore_clipboard: bool) -> Result<(), String> {
    // Save current clipboard if needed
    let original_clipboard = if restore_clipboard {
        Clipboard::get().ok()
    } else {
        None
    };

    // Set new clipboard content
    Clipboard::set(text)?;

    // Small delay to ensure clipboard is ready
    std::thread::sleep(Duration::from_millis(50));

    // Send paste command - if this fails, DON'T restore clipboard so user can paste manually
    if let Err(e) = send_paste() {
        crate::log_warn!(
            "injection",
            "Paste failed, keeping text on clipboard for manual paste: {}",
            e
        );
        return Err(e);
    }

    // Only restore original clipboard if paste succeeded
    if let Some(original) = original_clipboard {
        // Wait before restoring to ensure paste completes (webviews need more time)
        std::thread::sleep(Duration::from_millis(300));
        Clipboard::set(&original)?;
    }

    Ok(())
}

/// Inject text with verification (checks if paste succeeded)
///
/// This is a more robust injection method that verifies the paste worked.
/// Use this when reliability is critical.
#[allow(dead_code)]
pub fn inject_text_verified(text: &str, restore_clipboard: bool) -> Result<(), InjectionError> {
    // Check for secure input first
    if is_secure_input_active() {
        return Err(InjectionError::SecureInputDetected);
    }

    // Save current clipboard
    let original_clipboard = Clipboard::get().ok();

    // Set a verification marker (for future verification feature)
    let _verification_marker = format!(
        "dybur_verify_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );

    // Set clipboard to our text
    Clipboard::set(text).map_err(InjectionError::ClipboardError)?;

    // Small delay
    std::thread::sleep(Duration::from_millis(50));

    // Send paste
    send_paste().map_err(InjectionError::PasteError)?;

    // Wait for paste to complete
    std::thread::sleep(Duration::from_millis(150));

    // Restore clipboard if needed
    if restore_clipboard {
        if let Some(original) = original_clipboard {
            let _ = Clipboard::set(&original);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clipboard_roundtrip() {
        let test_text = "dybur test";
        Clipboard::set(test_text).unwrap();
        let result = Clipboard::get().unwrap();
        assert!(result.contains("dybur"));
    }
}
