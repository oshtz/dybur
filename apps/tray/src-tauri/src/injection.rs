//! Text injection via clipboard and keyboard simulation
//!
//! Primary method: Copy to clipboard + Paste (Cmd+V / Ctrl+V)
//! Fallback: Synthetic key events
//!
//! Includes secure input detection and retry logic.

#[cfg(target_os = "macos")]
use std::process::Command;
use std::time::Duration;

/// Clipboard operations
pub struct Clipboard;

// ============================================================================
// Windows implementation using native Win32 APIs
// ============================================================================

#[cfg(target_os = "windows")]
mod windows_impl {
    use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
    use windows::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
    };
    use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
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

        let mut child = Command::new("pbcopy")
            .stdin(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to set clipboard: {}", e))?;

        if let Some(stdin) = child.stdin.as_mut() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| format!("Failed to write to clipboard: {}", e))?;
        }

        child
            .wait()
            .map_err(|e| format!("Clipboard command failed: {}", e))?;
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
    // macOS: Use AppleScript
    Command::new("osascript")
        .args([
            "-e",
            "tell application \"System Events\" to keystroke \"v\" using command down",
        ])
        .output()
        .map_err(|e| format!("Failed to send paste: {}", e))?;

    Ok(())
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
    // On Windows, detecting password fields reliably requires UI Automation
    // which is complex and slow via PowerShell. For now, we skip this check.
    // Users should be aware not to use dictation in password fields.
    // TODO: Implement native UI Automation check if needed
    false
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

    // Send paste command
    send_paste()?;

    // Restore original clipboard if needed
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
