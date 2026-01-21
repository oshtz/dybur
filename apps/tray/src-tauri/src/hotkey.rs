//! Global hotkey handling
//!
//! This module provides hotkey registration using the Tauri global shortcut plugin.
//! The actual registration is done in main.rs using tauri-plugin-global-shortcut.

/// Parse a hotkey string into components
/// Format: "Modifier+Modifier+Key" (e.g., "Ctrl+Shift+Space")
pub fn parse_hotkey(hotkey: &str) -> Result<(Vec<String>, String), String> {
    let parts: Vec<&str> = hotkey.split('+').map(|s| s.trim()).collect();

    if parts.len() < 2 {
        return Err("Hotkey must have at least one modifier and a key".to_string());
    }

    let modifiers: Vec<String> = parts[..parts.len() - 1]
        .iter()
        .map(|s| s.to_string())
        .collect();

    let key = parts[parts.len() - 1].to_string();

    // Validate modifiers
    let valid_modifiers = ["Ctrl", "Alt", "Shift", "Meta", "Cmd", "Win", "Super"];
    for modifier in &modifiers {
        if !valid_modifiers.contains(&modifier.as_str()) {
            return Err(format!("Invalid modifier: {}", modifier));
        }
    }

    Ok((modifiers, key))
}

/// Convert our hotkey format to Tauri shortcut format
/// Our format: "Ctrl+Shift+Space"
/// Tauri format: "ctrl+shift+space" (lowercase)
pub fn to_tauri_shortcut(hotkey: &str) -> String {
    hotkey
        .split('+')
        .map(|part| {
            let part = part.trim().to_lowercase();
            // Map some common aliases
            match part.as_str() {
                "cmd" => "super".to_string(),
                "win" => "super".to_string(),
                "meta" => "super".to_string(),
                other => other.to_string(),
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_hotkey() {
        let (mods, key) = parse_hotkey("Ctrl+Shift+Space").unwrap();
        assert_eq!(mods, vec!["Ctrl", "Shift"]);
        assert_eq!(key, "Space");
    }

    #[test]
    fn test_parse_hotkey_single_modifier() {
        let (mods, key) = parse_hotkey("Alt+A").unwrap();
        assert_eq!(mods, vec!["Alt"]);
        assert_eq!(key, "A");
    }

    #[test]
    fn test_parse_hotkey_invalid() {
        assert!(parse_hotkey("Space").is_err());
        assert!(parse_hotkey("Invalid+Space").is_err());
    }

    #[test]
    fn test_to_tauri_shortcut() {
        assert_eq!(to_tauri_shortcut("Ctrl+Shift+Space"), "ctrl+shift+space");
        assert_eq!(to_tauri_shortcut("Cmd+D"), "super+d");
    }
}
