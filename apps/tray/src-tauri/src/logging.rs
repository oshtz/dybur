//! Logging utilities for dybur tray app
//!
//! Local-only logging with no speech content.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Log levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// Get the logs directory path
fn get_logs_dir() -> PathBuf {
    super::config::get_data_dir().join("logs")
}

/// Get the current log file path
pub fn get_log_file_path() -> PathBuf {
    let date = chrono::Local::now().format("%Y-%m-%d").to_string();
    get_logs_dir().join(format!("dybur-{}.log", date))
}

/// Ensure logs directory exists
fn ensure_logs_dir() -> Result<(), String> {
    let dir = get_logs_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| format!("Failed to create logs directory: {}", e))?;
    }
    Ok(())
}

/// Format a log entry
fn format_log_entry(level: LogLevel, category: &str, message: &str) -> String {
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    format!("{} {:5} [{}] {}", timestamp, level, category, message)
}

/// Write a log entry to file
fn write_log_to_file(entry: &str) {
    if let Err(e) = ensure_logs_dir() {
        eprintln!("Logging error: {}", e);
        return;
    }

    let log_file = get_log_file_path();
    match OpenOptions::new().create(true).append(true).open(&log_file) {
        Ok(mut file) => {
            if let Err(e) = writeln!(file, "{}", entry) {
                eprintln!("Failed to write log: {}", e);
            }
        }
        Err(e) => {
            eprintln!("Failed to open log file: {}", e);
        }
    }
}

/// Log a message at the specified level
pub fn log(level: LogLevel, category: &str, message: &str) {
    let entry = format_log_entry(level, category, message);

    // Console output
    match level {
        LogLevel::Debug => println!("{}", entry),
        LogLevel::Info => println!("{}", entry),
        LogLevel::Warn => eprintln!("{}", entry),
        LogLevel::Error => eprintln!("{}", entry),
    }

    // File output
    write_log_to_file(&entry);
}

/// Convenience macros for logging
#[macro_export]
macro_rules! log_debug {
    ($category:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::LogLevel::Debug, $category, &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_info {
    ($category:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::LogLevel::Info, $category, &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_warn {
    ($category:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::LogLevel::Warn, $category, &format!($($arg)*))
    };
}

#[macro_export]
macro_rules! log_error {
    ($category:expr, $($arg:tt)*) => {
        $crate::logging::log($crate::logging::LogLevel::Error, $category, &format!($($arg)*))
    };
}

/// Logger for a specific category
pub struct Logger {
    category: String,
}

impl Logger {
    pub fn new(category: &str) -> Self {
        Self {
            category: category.to_string(),
        }
    }

    pub fn debug(&self, message: &str) {
        log(LogLevel::Debug, &self.category, message);
    }

    pub fn info(&self, message: &str) {
        log(LogLevel::Info, &self.category, message);
    }

    pub fn warn(&self, message: &str) {
        log(LogLevel::Warn, &self.category, message);
    }

    pub fn error(&self, message: &str) {
        log(LogLevel::Error, &self.category, message);
    }
}

/// Pre-defined loggers for common categories
pub mod loggers {
    use super::Logger;

    lazy_static::lazy_static! {
        pub static ref SERVICE: Logger = Logger::new("service");
        pub static ref HOTKEY: Logger = Logger::new("hotkey");
        pub static ref AUDIO: Logger = Logger::new("audio");
        pub static ref INJECTION: Logger = Logger::new("injection");
        pub static ref CONFIG: Logger = Logger::new("config");
        pub static ref MODEL: Logger = Logger::new("model");
    }
}
