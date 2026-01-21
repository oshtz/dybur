//! CLI UI utilities for dybur
//! Provides styled, branded output for a polished terminal experience

use std::io::{self, Write};

// ANSI color codes
const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

// Standard colors
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";

// Brand colors (using 24-bit true color for exact match)
// Accent: #5effc0 (94, 255, 192) - bright mint green
const ACCENT: &str = "\x1b[38;2;94;255;192m";
// Text Primary: #c9ffe8 (201, 255, 232) - light mint
const TEXT_PRIMARY: &str = "\x1b[38;2;201;255;232m";
// Muted/dim version of accent
const MUTED: &str = "\x1b[38;2;100;140;120m";

/// Check if terminal supports colors
fn supports_color() -> bool {
    std::env::var("NO_COLOR").is_err() && atty::is(atty::Stream::Stdout)
}

/// Simple atty check without dependency
mod atty {
    pub enum Stream {
        Stdout,
    }

    pub fn is(_stream: Stream) -> bool {
        #[cfg(windows)]
        {
            // On Windows, assume color support in modern terminals
            true
        }
        #[cfg(not(windows))]
        {
            // SAFETY: isatty is a standard POSIX function that checks if fd is a terminal
            unsafe { libc::isatty(libc::STDOUT_FILENO) != 0 }
        }
    }

    #[cfg(not(windows))]
    mod libc {
        extern "C" {
            pub fn isatty(fd: i32) -> i32;
        }
        pub const STDOUT_FILENO: i32 = 1;
    }
}

// Color functions
pub fn bold(text: &str) -> String {
    if supports_color() {
        format!("{BOLD}{text}{RESET}")
    } else {
        text.to_string()
    }
}

pub fn dim(text: &str) -> String {
    if supports_color() {
        format!("{MUTED}{text}{RESET}")
    } else {
        text.to_string()
    }
}

pub fn red(text: &str) -> String {
    if supports_color() {
        format!("{RED}{text}{RESET}")
    } else {
        text.to_string()
    }
}

/// Success/positive color - uses brand accent (#5effc0)
pub fn green(text: &str) -> String {
    if supports_color() {
        format!("{ACCENT}{text}{RESET}")
    } else {
        text.to_string()
    }
}

pub fn yellow(text: &str) -> String {
    if supports_color() {
        format!("{YELLOW}{text}{RESET}")
    } else {
        text.to_string()
    }
}

/// Cyan/info color - uses brand text primary (#c9ffe8)
pub fn cyan(text: &str) -> String {
    if supports_color() {
        format!("{TEXT_PRIMARY}{text}{RESET}")
    } else {
        text.to_string()
    }
}

/// Brand primary color - uses accent (#5effc0)
pub fn primary(text: &str) -> String {
    if supports_color() {
        format!("{ACCENT}{text}{RESET}")
    } else {
        text.to_string()
    }
}

/// Brand accent color - uses accent (#5effc0)
pub fn accent(text: &str) -> String {
    if supports_color() {
        format!("{ACCENT}{text}{RESET}")
    } else {
        text.to_string()
    }
}

// Icons
pub mod icons {
    use super::*;

    pub fn success() -> String {
        green("✓")
    }

    pub fn error() -> String {
        red("✗")
    }

    pub fn warning() -> String {
        yellow("⚠")
    }

    pub fn info() -> String {
        cyan("ℹ")
    }

    pub fn arrow() -> String {
        cyan("→")
    }

    pub fn bullet() -> String {
        dim("•")
    }

    pub fn recording() -> String {
        red("●")
    }

    pub fn idle() -> String {
        dim("○")
    }

    pub fn status_on() -> String {
        green("●")
    }

    pub fn status_off() -> String {
        red("○")
    }
}

/// ASCII art logo
pub fn logo() -> String {
    format!(
        r#"
{}
{}  {}{}     {}
{}
"#,
        primary("┌─────────────────────────────────────┐"),
        primary("│"),
        accent("dybur"),
        dim(" - local voice dictation"),
        primary("│"),
        primary("└─────────────────────────────────────┘")
    )
}

/// Print the welcome banner
pub fn banner() {
    println!("{}", logo());
}

/// Print a styled header
pub fn header(title: &str) {
    println!();
    println!("  {} {}", accent("▸"), bold(title));
    println!();
}

/// Print a section divider
pub fn divider() {
    println!("{}", dim(&"─".repeat(40)));
}

/// Print a key-value pair
pub fn key_value(key: &str, value: &str) {
    println!("  {}: {}", dim(key), value);
}

/// Print a list item
pub fn list_item(text: &str) {
    println!("  {} {}", icons::bullet(), text);
}

/// Print a success message
pub fn success(message: &str) {
    println!("  {} {}", icons::success(), message);
}

/// Print an error message
pub fn error(message: &str) {
    println!("  {} {}", icons::error(), red(message));
}

/// Print a warning message
pub fn warning(message: &str) {
    println!("  {} {}", icons::warning(), yellow(message));
}

/// Print an info message
pub fn info(message: &str) {
    println!("  {} {}", icons::info(), message);
}

/// Print a command example
pub fn command(cmd: &str, description: &str) {
    println!("  {}  {}", cyan(cmd), dim(description));
}

/// Format file size
pub fn format_size(bytes: u64) -> String {
    if bytes == 0 {
        return dim("0 B");
    }
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let k: f64 = 1024.0;
    let i = (bytes as f64).log(k).floor() as usize;
    let value = bytes as f64 / k.powi(i as i32);
    format!("{:.1} {}", value, dim(UNITS.get(i).unwrap_or(&"GB")))
}

/// Format a path (truncate if too long)
pub fn format_path(path: &str, max_len: usize) -> String {
    if path.len() <= max_len {
        dim(path)
    } else {
        dim(&format!("...{}", &path[path.len() - (max_len - 3)..]))
    }
}

/// Progress bar
pub fn progress_bar(current: u64, total: u64, width: usize) -> String {
    let percent = if total > 0 {
        (current as f64 / total as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let filled = (width as f64 * percent).round() as usize;
    let empty = width - filled;

    let bar = format!(
        "{}{}",
        primary(&"█".repeat(filled)),
        dim(&"░".repeat(empty))
    );
    let percent_str = format!("{:>3}%", (percent * 100.0).round() as u32);

    format!("{} {}", bar, percent_str)
}

/// Simple spinner frames
pub const SPINNER_FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Print a spinner frame
pub fn spinner_frame(frame: usize, message: &str) {
    let f = SPINNER_FRAMES[frame % SPINNER_FRAMES.len()];
    print!("\r  {} {}", cyan(f), message);
    let _ = io::stdout().flush();
}

/// Clear the current line
pub fn clear_line() {
    print!("\r{}\r", " ".repeat(60));
    let _ = io::stdout().flush();
}

/// Print a boxed message
pub fn box_message(lines: &[&str], title: Option<&str>) {
    let max_len = lines
        .iter()
        .map(|l| l.chars().count())
        .chain(title.map(|t| t.chars().count()))
        .max()
        .unwrap_or(0);
    let width = max_len + 4;

    println!();
    println!("  {}", primary(&format!("┌{}┐", "─".repeat(width))));

    if let Some(t) = title {
        println!(
            "  {} {:<width$} {}",
            primary("│"),
            bold(t),
            primary("│"),
            width = max_len + 2
        );
        println!("  {}", primary(&format!("├{}┤", "─".repeat(width))));
    }

    for line in lines {
        println!(
            "  {} {:<width$} {}",
            primary("│"),
            line,
            primary("│"),
            width = max_len + 2
        );
    }

    println!("  {}", primary(&format!("└{}┘", "─".repeat(width))));
    println!();
}

/// Interactive select menu
pub fn select<T: Clone>(
    message: &str,
    choices: &[(String, T, Option<String>)],
    initial: usize,
) -> Option<T> {
    use crossterm::{
        cursor,
        event::{self, Event, KeyCode, KeyEvent, KeyEventKind},
        execute,
        terminal::{self, Clear, ClearType},
    };

    if choices.is_empty() {
        return None;
    }

    let mut selected = initial.min(choices.len() - 1);
    let mut stdout = io::stdout();

    // Total lines we'll print (header + choices)
    let total_lines = choices.len() + 1;

    // Enable raw mode for keypress detection
    let _ = terminal::enable_raw_mode();

    // Small delay to let any pending key events (Enter from command) arrive
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Flush any pending input events (e.g., the Enter key from running the command)
    while event::poll(std::time::Duration::from_millis(10)).unwrap_or(false) {
        let _ = event::read();
    }

    // Build display lines
    let build_lines = |sel: usize| -> Vec<String> {
        let mut lines = Vec::new();

        // Header line
        lines.push(format!("  {} {}", accent("?"), bold(message)));

        // Choice lines
        for (i, (label, _, hint)) in choices.iter().enumerate() {
            let cursor_char = if i == sel { accent(">") } else { " ".to_string() };
            let label_str = if i == sel {
                cyan(label)
            } else {
                label.clone()
            };
            let hint_str = hint
                .as_ref()
                .map(|h| format!(" {}", dim(&format!("({})", h))))
                .unwrap_or_default();
            lines.push(format!("  {} {}{}", cursor_char, label_str, hint_str));
        }

        lines
    };

    let render = |stdout: &mut io::Stdout, sel: usize, first: bool| {
        if !first {
            // Move cursor up to the start of our menu and clear
            let _ = execute!(
                stdout,
                cursor::MoveUp(total_lines as u16),
                cursor::MoveToColumn(0),
                Clear(ClearType::FromCursorDown)
            );
        }

        let lines = build_lines(sel);
        for line in &lines {
            // Use \r\n explicitly to ensure cursor returns to column 0 in raw mode
            print!("{}\r\n", line);
        }
        let _ = stdout.flush();
    };

    // Initial render
    render(&mut stdout, selected, true);

    loop {
        if event::poll(std::time::Duration::from_millis(100)).unwrap_or(false) {
            if let Ok(Event::Key(KeyEvent { code, kind, .. })) = event::read() {
                // Only handle key press events, not release or repeat
                if kind != KeyEventKind::Press {
                    continue;
                }

                match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        selected = if selected > 0 {
                            selected - 1
                        } else {
                            choices.len() - 1
                        };
                        render(&mut stdout, selected, false);
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        selected = if selected < choices.len() - 1 {
                            selected + 1
                        } else {
                            0
                        };
                        render(&mut stdout, selected, false);
                    }
                    KeyCode::Enter => {
                        let _ = terminal::disable_raw_mode();
                        println!();
                        return Some(choices[selected].1.clone());
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {
                        let _ = terminal::disable_raw_mode();
                        println!();
                        return None;
                    }
                    KeyCode::Char('c') if event::poll(std::time::Duration::ZERO).is_ok() => {
                        // Ctrl+C
                        let _ = terminal::disable_raw_mode();
                        std::process::exit(0);
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Strip ANSI escape codes from a string for accurate length calculation
fn strip_ansi_codes(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Skip escape sequence
            if chars.peek() == Some(&'[') {
                chars.next(); // consume '['
                // Skip until we hit a letter (the command terminator)
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}
