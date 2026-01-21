//! Single instance guard
//!
//! Ensures only one instance of the tray app runs at a time.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process;

#[cfg(target_os = "windows")]
use std::os::windows::fs::OpenOptionsExt;

/// Lock file name
const LOCK_FILE_NAME: &str = "dybur.lock";

/// Get the lock file path
fn get_lock_path() -> PathBuf {
    super::config::get_data_dir().join(LOCK_FILE_NAME)
}

/// Result of attempting to acquire the single instance lock
pub enum LockResult {
    /// Lock acquired successfully
    Acquired(InstanceGuard),
    /// Another instance is already running
    AlreadyRunning(u32),
}

/// Guard that releases the lock when dropped
pub struct InstanceGuard {
    #[allow(dead_code)]
    lock_file: File,
    lock_path: PathBuf,
}

impl Drop for InstanceGuard {
    fn drop(&mut self) {
        // Remove lock file on exit
        let _ = fs::remove_file(&self.lock_path);
    }
}

/// Try to acquire the single instance lock
///
/// Returns `LockResult::Acquired` if this is the first instance,
/// or `LockResult::AlreadyRunning` with the PID of the existing instance.
pub fn try_acquire_lock() -> Result<LockResult, String> {
    let lock_path = get_lock_path();

    // Ensure data directory exists
    if let Some(parent) = lock_path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create data directory: {}", e))?;
    }

    // Check if lock file exists and contains a valid PID
    if lock_path.exists() {
        if let Ok(existing_pid) = read_pid_from_lock(&lock_path) {
            if is_process_running(existing_pid) {
                return Ok(LockResult::AlreadyRunning(existing_pid));
            }
            // Process not running, clean up stale lock
            let _ = fs::remove_file(&lock_path);
        }
    }

    // Try to create lock file with exclusive access
    let lock_file = create_lock_file(&lock_path)?;

    // Write our PID to the lock file
    write_pid_to_lock(&lock_file)?;

    Ok(LockResult::Acquired(InstanceGuard {
        lock_file,
        lock_path,
    }))
}

/// Read PID from lock file
fn read_pid_from_lock(path: &PathBuf) -> Result<u32, String> {
    let mut file = File::open(path).map_err(|e| format!("Failed to open lock file: {}", e))?;

    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| format!("Failed to read lock file: {}", e))?;

    contents
        .trim()
        .parse::<u32>()
        .map_err(|e| format!("Failed to parse PID: {}", e))
}

/// Create lock file with exclusive access
#[cfg(target_os = "windows")]
fn create_lock_file(path: &PathBuf) -> Result<File, String> {
    // FILE_FLAG_DELETE_ON_CLOSE = 0x04000000 (not using this as we want manual cleanup)
    // FILE_SHARE_NONE = 0 (exclusive access)
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .share_mode(0) // Exclusive access
        .open(path)
        .map_err(|e| format!("Failed to create lock file: {}", e))
}

#[cfg(not(target_os = "windows"))]
fn create_lock_file(path: &PathBuf) -> Result<File, String> {
    use std::os::unix::fs::OpenOptionsExt;

    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o644)
        .open(path)
        .map_err(|e| format!("Failed to create lock file: {}", e))
}

/// Write current PID to lock file
fn write_pid_to_lock(mut file: &File) -> Result<(), String> {
    let pid = process::id();
    write!(file, "{}", pid).map_err(|e| format!("Failed to write PID to lock file: {}", e))
}

/// Check if a process with the given PID is running
#[cfg(target_os = "windows")]
fn is_process_running(pid: u32) -> bool {
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(
            dwDesiredAccess: u32,
            bInheritHandle: i32,
            dwProcessId: u32,
        ) -> *mut std::ffi::c_void;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
        fn GetExitCodeProcess(hProcess: *mut std::ffi::c_void, lpExitCode: *mut u32) -> i32;
    }

    const STILL_ACTIVE: u32 = 259;

    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }

        let mut exit_code: u32 = 0;
        let result = GetExitCodeProcess(handle, &mut exit_code);
        CloseHandle(handle);

        result != 0 && exit_code == STILL_ACTIVE
    }
}

#[cfg(target_os = "macos")]
fn is_process_running(pid: u32) -> bool {
    use std::process::Command;

    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

#[cfg(target_os = "linux")]
fn is_process_running(pid: u32) -> bool {
    std::path::Path::new(&format!("/proc/{}", pid)).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_current_process_is_running() {
        let pid = process::id();
        assert!(is_process_running(pid));
    }

    #[test]
    fn test_invalid_process_not_running() {
        // Very high PID unlikely to exist
        assert!(!is_process_running(999999999));
    }
}
