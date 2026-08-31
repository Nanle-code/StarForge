//! Centralized Secret Redaction Engine
//!
//! Provides comprehensive secret redaction for tracing logs, CLI diagnostics,
//! and error messages. Redacts keys, mnemonics, tokens, signed transactions,
//! and embedded URL credentials.

use once_cell::sync::Lazy;
use regex::Regex;

/// Default redacted string replacement.
pub const REDACTED: &str = "[REDACTED]";

/// Redacts sensitive information (secret keys, mnemonics, tokens, signed transactions, credentials)
/// from the provided string input.
///
/// Handles invalid inputs, ultra-long strings, and unsupported/edge-case environments safely.
pub fn redact_secrets(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }

    // Safety guard against ridiculously large inputs causing performance degradation
    const MAX_SAFE_LENGTH: usize = 1_000_000; // 1 MB
    if input.len() > MAX_SAFE_LENGTH {
        let truncated = &input[..MAX_SAFE_LENGTH];
        let mut result = redact_secrets_impl(truncated);
        result.push_str("...[TRUNCATED FOR REDACTION]");
        return result;
    }

    redact_secrets_impl(input)
}

fn redact_secrets_impl(input: &str) -> String {
    // 1. Stellar Secret Keys (StrKey format: starts with 'S', 56 base32 chars)
    static STELLAR_SECRET_KEY_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"\bS[A-Z2-7]{55}\b").unwrap());

    // 2. Hex Private Keys (64 hex characters, optional 0x prefix, when isolated or key-bound)
    static HEX_PRIVATE_KEY_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)\b(?:0x)?[a-f0-9]{64}\b").unwrap());

    // 3. Tokens & API Keys (Bearer, ghp_, github_pat_, sk-, sec-, etc.)
    static BEARER_TOKEN_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)\bBearer\s+[A-Za-z0-9_\-\.=]+\b").unwrap());

    static KNOWN_TOKEN_PATTERNS_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\b(?:ghp_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22,}|sk-[A-Za-z0-9_\-]{20,}|sec-[A-Za-z0-9_\-]{16,})\b").unwrap()
    });

    // 4. Key-Value Secret Assignment (e.g., api_key = "...", secret: '...', passphrase = ...)
    static KEY_VALUE_SECRET_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)\b(api_key|apikey|secret|token|auth_token|private_key|secret_key|passphrase|seed_phrase)\s*([:=])\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s"';,]+)"#).unwrap()
    });

    // 5. Signed XDR Envelopes / Transactions
    static SIGNED_XDR_KEY_VALUE_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)\b(signed_xdr|tx_envelope|signed_tx|signed_envelope)\s*([:=])\s*(?:"[^"\r\n]*"|'[^'\r\n]*'|[^\s"';,]+)"#).unwrap()
    });

    // 6. Basic Auth credentials in URLs (e.g., https://user:pass@host)
    static URL_BASIC_AUTH_REGEX: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(https?://)([^:\s/]+):([^@\s/]+)@").unwrap());

    // 7. Sensitive query parameters in URLs (e.g., ?apiKey=xyz&secret=123)
    static URL_QUERY_PARAM_REGEX: Lazy<Regex> = Lazy::new(|| {
        Regex::new(
            r"([?&](?:apiKey|api_key|secret|token|passphrase|access_token|private_key)=)([^&\s]+)",
        )
        .unwrap()
    });

    // Apply regex replacements sequentially
    let s = URL_BASIC_AUTH_REGEX.replace_all(input, "$1[REDACTED]:[REDACTED]@");
    let s = URL_QUERY_PARAM_REGEX.replace_all(&s, "$1[REDACTED]");
    let s = STELLAR_SECRET_KEY_REGEX.replace_all(&s, REDACTED);
    let s = BEARER_TOKEN_REGEX.replace_all(&s, "Bearer [REDACTED]");
    let s = KNOWN_TOKEN_PATTERNS_REGEX.replace_all(&s, REDACTED);
    let s = KEY_VALUE_SECRET_REGEX.replace_all(&s, "$1$2[REDACTED]");
    let s = SIGNED_XDR_KEY_VALUE_REGEX.replace_all(&s, "$1$2[REDACTED]");
    let s = HEX_PRIVATE_KEY_REGEX.replace_all(&s, REDACTED);

    // 8. BIP-39 Mnemonics (12, 15, 18, 21, 24 space-separated words)
    redact_mnemonics(&s)
}

/// Helper function to detect and redact BIP-39 mnemonic seed phrases (12 to 24 words).
fn redact_mnemonics(input: &str) -> String {
    static MNEMONIC_PATTERN: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"\b(?:[a-z]{3,8}\s+){11,23}[a-z]{3,8}\b").unwrap());

    MNEMONIC_PATTERN
        .replace_all(input, |caps: &regex::Captures| {
            let phrase = caps.get(0).unwrap().as_str();
            let word_count = phrase.split_whitespace().count();
            // Validate if word count corresponds to BIP-39 standard (12, 15, 18, 21, 24 words)
            if matches!(word_count, 12 | 15 | 18 | 21 | 24) {
                // Verify all words are valid BIP-39 English words
                let all_bip39 = phrase
                    .split_whitespace()
                    .all(|w| bip39::Language::English.find_word(w).is_some());
                if all_bip39 {
                    return REDACTED.to_string();
                }
            }
            phrase.to_string()
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stellar_secret_key_redaction() {
        let valid_sk = "SDJ34K5N6P7Q2R3S4T5U2V3W4X5Y6Z7A2B3C4D5E2F3G4H5I6J7K2L3M";
        let text = format!("Account created with secret key {}", valid_sk);
        let redacted = redact_secrets(&text);
        assert!(!redacted.contains(valid_sk));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_mnemonic_redaction() {
        let mnemonic_12 =
            "army vanish defense carry reward write custom cargo adult melt verify polar";
        let text = format!("Seed phrase is: {}", mnemonic_12);
        let redacted = redact_secrets(&text);
        assert!(!redacted.contains("army vanish"));
        assert!(redacted.contains("Seed phrase is: [REDACTED]"));
    }

    #[test]
    fn test_bearer_and_api_tokens() {
        let text = "Header Authorization: Bearer eyJhbGciOiJIUzI1NiJ9.test.sig and ghp_1234567890abcdef1234567890abcdef1234";
        let redacted = redact_secrets(text);
        assert!(!redacted.contains("eyJhbGciOiJIUzI1NiJ9"));
        assert!(!redacted.contains("ghp_1234567890abcdef1234567890abcdef1234"));
        assert!(redacted.contains("Bearer [REDACTED]"));
    }

    #[test]
    fn test_url_credentials_redaction() {
        let url = "https://user:hunter2@rpc.example.com/soroban?apiKey=abcdef123456";
        let redacted = redact_secrets(url);
        assert!(!redacted.contains("hunter2"));
        assert!(!redacted.contains("abcdef123456"));
        assert!(redacted
            .contains("https://[REDACTED]:[REDACTED]@rpc.example.com/soroban?apiKey=[REDACTED]"));
    }

    #[test]
    fn test_hex_private_key_redaction() {
        let hex_key = "1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
        let text = format!("private_key = {}", hex_key);
        let redacted = redact_secrets(&text);
        assert!(!redacted.contains(hex_key));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_signed_xdr_redaction() {
        let text = "signed_xdr: AAAAAgAAAAD1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890";
        let redacted = redact_secrets(text);
        assert!(!redacted.contains("AAAAAgAAAAD1234567890"));
        assert!(redacted.contains("[REDACTED]"));
    }

    #[test]
    fn test_boundary_and_invalid_inputs() {
        // Empty string
        assert_eq!(redact_secrets(""), "");

        // Normal text without secrets remains intact
        let normal = "Deploying contract to testnet network with public account GDRX...";
        assert_eq!(redact_secrets(normal), normal);

        // Invalid/non-matching 11 words (not a 12-word mnemonic)
        let short_phrase = "army vanish defense carry reward write custom cargo adult melt verify";
        assert_eq!(redact_secrets(short_phrase), short_phrase);

        // Non-bip39 words in 12-word length sentence
        let regular_sentence =
            "this is just a regular sentence with twelve words that are not mnemonic";
        assert_eq!(redact_secrets(regular_sentence), regular_sentence);

        // String with null bytes and special chars
        let null_byte_input = "error\0with\0api_key = secret123\0data";
        let redacted_null = redact_secrets(null_byte_input);
        assert!(!redacted_null.contains("secret123"));
    }
}
