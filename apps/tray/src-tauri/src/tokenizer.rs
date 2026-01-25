//! BPE Tokenizer for Whisper and other models using tokenizer.json
//!
//! This module provides BPE (Byte-Pair Encoding) tokenization for models
//! that use HuggingFace's tokenizer.json format (Whisper, Canary, etc.)

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// Special token IDs for Whisper
pub mod whisper_tokens {
    pub const SOT: i64 = 50258; // Start of transcript
    pub const EOT: i64 = 50257; // End of transcript
    pub const TRANSCRIBE: i64 = 50359; // Transcribe task
    pub const TRANSLATE: i64 = 50358; // Translate task
    pub const NO_TIMESTAMPS: i64 = 50363; // No timestamps
    pub const EN: i64 = 50259; // English language token
    pub const BLANK: i64 = 50256; // Blank/padding token
}

/// BPE Tokenizer for decoding token IDs to text
#[derive(Debug, Clone)]
pub struct BpeTokenizer {
    /// Token ID to text mapping
    vocab: HashMap<i64, String>,
    /// Text to token ID mapping (for encoding)
    vocab_reverse: HashMap<String, i64>,
    /// Special tokens that should be filtered from output
    special_tokens: Vec<i64>,
    /// End of text token ID
    pub eot_token_id: i64,
    /// Start of transcript token ID
    pub sot_token_id: i64,
}

/// HuggingFace tokenizer.json format structures
#[derive(Debug, Deserialize)]
struct TokenizerJson {
    model: TokenizerModel,
    #[serde(default)]
    added_tokens: Vec<AddedToken>,
}

#[derive(Debug, Deserialize)]
struct TokenizerModel {
    vocab: HashMap<String, i64>,
    // Merges can be Vec<String> (Whisper style: "a b") or Vec<[String; 2]> (Qwen style: ["a", "b"])
    // We don't use merges for decoding, so just skip parsing them
    #[serde(default, skip_deserializing)]
    merges: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AddedToken {
    id: i64,
    content: String,
    special: bool,
}

impl BpeTokenizer {
    /// Load tokenizer from tokenizer.json file
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read tokenizer.json: {}", e))?;

        let tokenizer_json: TokenizerJson = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse tokenizer.json: {}", e))?;

        // Build vocab mappings
        let mut vocab = HashMap::new();
        let mut vocab_reverse = HashMap::new();

        for (token_str, token_id) in &tokenizer_json.model.vocab {
            vocab.insert(*token_id, token_str.clone());
            vocab_reverse.insert(token_str.clone(), *token_id);
        }

        // Add special tokens
        let mut special_tokens = Vec::new();
        for added_token in &tokenizer_json.added_tokens {
            vocab.insert(added_token.id, added_token.content.clone());
            vocab_reverse.insert(added_token.content.clone(), added_token.id);
            if added_token.special {
                special_tokens.push(added_token.id);
            }
        }

        crate::log_info!(
            "tokenizer",
            "Loaded BPE tokenizer with {} vocab entries, {} special tokens",
            vocab.len(),
            special_tokens.len()
        );

        // Detect EOT and SOT tokens (support Whisper, Qwen, and other model formats)
        let eot_token_id = vocab_reverse.get("<|endoftext|>")
            .or_else(|| vocab_reverse.get("<|im_end|>"))  // Qwen-style
            .or_else(|| vocab_reverse.get("</s>"))
            .copied()
            .unwrap_or(whisper_tokens::EOT);

        let sot_token_id = vocab_reverse.get("<|startoftranscript|>")
            .or_else(|| vocab_reverse.get("<|im_start|>"))  // Qwen-style
            .or_else(|| vocab_reverse.get("<s>"))
            .copied()
            .unwrap_or(whisper_tokens::SOT);

        Ok(Self {
            vocab,
            vocab_reverse,
            special_tokens,
            eot_token_id,
            sot_token_id,
        })
    }

    /// Decode a single token ID to text
    pub fn decode_token(&self, token_id: i64) -> Option<String> {
        self.vocab.get(&token_id).cloned()
    }

    /// Decode a sequence of token IDs to text
    pub fn decode(&self, token_ids: &[i64], skip_special: bool) -> String {
        let mut text = String::new();

        for &token_id in token_ids {
            // Skip special tokens if requested
            if skip_special && self.special_tokens.contains(&token_id) {
                continue;
            }

            // Skip EOT and similar end tokens
            if token_id == self.eot_token_id {
                break;
            }

            if let Some(token_str) = self.vocab.get(&token_id) {
                // Handle BPE byte encoding (e.g., "Ġ" for space)
                let decoded = self.decode_bpe_token(token_str);
                text.push_str(&decoded);
            }
        }

        text
    }

    /// Decode BPE-specific encodings to regular text
    fn decode_bpe_token(&self, token: &str) -> String {
        // GPT-2 style BPE uses special characters:
        // "Ġ" (U+0120) represents a space before the word
        // "Ċ" (U+010A) represents a newline
        // Bytes are encoded as single Unicode chars in a special range

        let mut result = String::new();
        let chars: Vec<char> = token.chars().collect();

        for ch in chars {
            match ch {
                '\u{0120}' => result.push(' '), // Ġ -> space
                '\u{010A}' => result.push('\n'), // Ċ -> newline
                '\u{010D}' => result.push('\r'), // Special carriage return
                // Handle other byte encodings (characters in range U+0100 to U+0200)
                c if ('\u{0100}'..='\u{01FF}').contains(&c) => {
                    // These map to bytes 0-255
                    let byte_val = (c as u32 - 0x100) as u8;
                    if byte_val.is_ascii() {
                        result.push(byte_val as char);
                    } else {
                        // For non-ASCII bytes, keep the replacement
                        result.push(c);
                    }
                }
                c => result.push(c),
            }
        }

        result
    }

    /// Check if a token ID is a special token
    pub fn is_special_token(&self, token_id: i64) -> bool {
        self.special_tokens.contains(&token_id)
    }

    /// Get the vocabulary size
    pub fn vocab_size(&self) -> usize {
        self.vocab.len()
    }

    /// Encode text to token IDs (basic implementation)
    pub fn encode(&self, text: &str) -> Vec<i64> {
        // For now, just do character-level lookup
        // A full BPE encoder would need the merge rules
        let mut tokens = Vec::new();

        // Simple word-level lookup as fallback
        for word in text.split_whitespace() {
            let word_with_space = format!("Ġ{}", word);
            if let Some(&id) = self.vocab_reverse.get(&word_with_space) {
                tokens.push(id);
            } else if let Some(&id) = self.vocab_reverse.get(word) {
                tokens.push(id);
            }
        }

        tokens
    }

    /// Get language token ID for Whisper
    pub fn get_language_token(&self, lang_code: &str) -> Option<i64> {
        let token = format!("<|{}|>", lang_code);
        self.vocab_reverse.get(&token).copied()
    }

    /// Get task token ID for Whisper
    pub fn get_task_token(&self, task: &str) -> Option<i64> {
        let token = format!("<|{}|>", task);
        self.vocab_reverse.get(&token).copied()
    }
}

/// Filter Whisper special tokens from output
pub fn filter_whisper_special_tokens(token_ids: &[i64]) -> Vec<i64> {
    token_ids
        .iter()
        .filter(|&&id| {
            // Filter timestamp tokens (50364 and above typically)
            if id >= 50364 {
                return false;
            }
            // Filter known special tokens
            !matches!(id,
                whisper_tokens::SOT |
                whisper_tokens::EOT |
                whisper_tokens::TRANSCRIBE |
                whisper_tokens::TRANSLATE |
                whisper_tokens::NO_TIMESTAMPS |
                whisper_tokens::BLANK
            )
        })
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bpe_decode() {
        // Test the BPE byte decoding
        let tokenizer = BpeTokenizer {
            vocab: HashMap::from([
                (0, "hello".to_string()),
                (1, "Ġworld".to_string()), // space + world
            ]),
            vocab_reverse: HashMap::new(),
            special_tokens: vec![],
            eot_token_id: 50257,
            sot_token_id: 50258,
        };

        assert_eq!(tokenizer.decode_bpe_token("Ġworld"), " world");
        assert_eq!(tokenizer.decode_bpe_token("hello"), "hello");
    }
}
