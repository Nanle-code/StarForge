use anyhow::Result;
use std::io::{self, Write};
use tracing::Level;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

/// Output format for log messages.
#[derive(Debug, Clone, PartialEq)]
pub enum LogFormat {
    /// Human-readable coloured output (default for terminals)
    Human,
    /// Newline-delimited JSON (useful for CI/CD and log aggregators)
    Json,
}

/// Configuration for the logging subsystem.
pub struct LogConfig {
    /// Minimum log level to emit (default: `warn` for normal use, `debug` with `RUST_LOG`)
    pub level: Level,
    /// Output format
    pub format: LogFormat,
    /// Optional directory to write rolling log files into
    pub log_dir: Option<std::path::PathBuf>,
    /// Log file prefix (e.g. "starforge")
    pub file_prefix: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            level: Level::WARN,
            format: LogFormat::Human,
            log_dir: None,
            file_prefix: "starforge".to_string(),
        }
    }
}

/// Initialise the global tracing subscriber.
///
/// Call this once at the start of `main()` before any commands run.
/// The `RUST_LOG` environment variable overrides `config.level` when set.
///
/// # Examples
/// ```no_run
/// use starforge::utils::logging::{LogConfig, LogFormat, init};
/// init(LogConfig { format: LogFormat::Json, ..Default::default() }).unwrap();
/// ```
pub fn init(config: LogConfig) -> Result<()> {
    // RUST_LOG takes precedence; fall back to the configured level.
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(config.level.as_str()));

    match (config.format, config.log_dir) {
        // ── JSON + file rotation ──────────────────────────────────────────
        (LogFormat::Json, Some(dir)) => {
            let file_appender = rolling::daily(&dir, &config.file_prefix);
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

            let file_layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(RedactingMakeWriter::new(move || {
                    RedactingWriter::new(non_blocking.clone())
                }))
                .with_filter(env_filter.clone());

            let stderr_layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(RedactingMakeWriter::new(|| {
                    RedactingWriter::new(std::io::stderr())
                }))
                .with_filter(env_filter);

            tracing_subscriber::registry()
                .with(file_layer)
                .with(stderr_layer)
                .try_init()
                .map_err(|e| anyhow::anyhow!("Failed to init logger: {}", e))?;
        }

        // ── JSON, stderr only ─────────────────────────────────────────────
        (LogFormat::Json, None) => {
            let layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(RedactingMakeWriter::new(|| {
                    RedactingWriter::new(std::io::stderr())
                }))
                .with_filter(env_filter);

            tracing_subscriber::registry()
                .with(layer)
                .try_init()
                .map_err(|e| anyhow::anyhow!("Failed to init logger: {}", e))?;
        }

        // ── Human + file rotation ─────────────────────────────────────────
        (LogFormat::Human, Some(dir)) => {
            let file_appender = rolling::daily(&dir, &config.file_prefix);
            let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

            let file_layer = fmt::layer()
                .with_ansi(false)
                .with_writer(RedactingMakeWriter::new(move || {
                    RedactingWriter::new(non_blocking.clone())
                }))
                .with_filter(env_filter.clone());

            let stderr_layer = fmt::layer()
                .with_writer(RedactingMakeWriter::new(|| {
                    RedactingWriter::new(std::io::stderr())
                }))
                .with_filter(env_filter);

            tracing_subscriber::registry()
                .with(file_layer)
                .with(stderr_layer)
                .try_init()
                .map_err(|e| anyhow::anyhow!("Failed to init logger: {}", e))?;
        }

        // ── Human, stderr only (default) ──────────────────────────────────
        (LogFormat::Human, None) => {
            let layer = fmt::layer()
                .with_writer(RedactingMakeWriter::new(|| {
                    RedactingWriter::new(std::io::stderr())
                }))
                .with_filter(env_filter);

            tracing_subscriber::registry()
                .with(layer)
                .try_init()
                .map_err(|e| anyhow::anyhow!("Failed to init logger: {}", e))?;
        }
    }

    Ok(())
}

/// Redact a public Stellar key unless the current log level is debug or trace.
///
/// Public keys are safe to display to users, but log streams should avoid
/// including raw account IDs in info-level logs unless the log level is explicitly
/// opted into debug.
pub fn redact_public_key(public_key: &str, level: Level) -> String {
    if matches!(level, Level::DEBUG | Level::TRACE) {
        public_key.to_string()
    } else if public_key.len() > 8 {
        let prefix = &public_key[..4];
        let suffix = &public_key[public_key.len().saturating_sub(4)..];
        format!("{}...{}", prefix, suffix)
    } else {
        "[REDACTED]".to_string()
    }
}

/// Always redact secret values when they are written to logs.
///
/// Secret keys and passphrases should never appear in info-level or debug-level
/// logs.
pub fn redact_secret_value(_value: &str) -> &'static str {
    "[REDACTED]"
}

/// Always redact signed XDR payloads when they are written to logs.
///
/// XDR envelopes containing signatures are secret and must not be emitted at
/// info level.
pub fn redact_signed_xdr(_xdr: &str) -> &'static str {
    "[REDACTED]"
}

const SENSITIVE_FIELD_NAMES: &[&str] = &[
    "secret_key",
    "private_key",
    "passphrase",
    "api_key",
    "mnemonic",
    "auth_token",
    "access_token",
    "refresh_token",
    "signed_xdr",
    "transaction_xdr",
];

/// Redact sensitive values embedded in human-readable or structured output.
///
/// This is applied at the output boundary so new log or error calls cannot
/// accidentally bypass the redaction helpers. Unknown values are left intact
/// unless they match a known sensitive shape.
pub fn redact_text(input: &str) -> String {
    let mut output = redact_mnemonics(input);
    for field in SENSITIVE_FIELD_NAMES {
        output = redact_field_value(&output, field);
    }

    let mut redacted = String::with_capacity(output.len());
    let mut start = 0;
    for (index, character) in output.char_indices() {
        if !is_candidate_character(character) {
            if start < index {
                let candidate = &output[start..index];
                if is_sensitive_value(candidate) {
                    redacted.push_str("[REDACTED]");
                } else {
                    redacted.push_str(candidate);
                }
            }
            redacted.push(character);
            start = index + character.len_utf8();
        }
    }
    if start < output.len() {
        let candidate = &output[start..];
        if is_sensitive_value(candidate) {
            redacted.push_str("[REDACTED]");
        } else {
            redacted.push_str(candidate);
        }
    }
    redacted
}

fn redact_field_value(input: &str, field: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut output = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(relative) = lower[cursor..].find(field) {
        let start = cursor + relative;
        let boundary = start == 0
            || !input[..start]
                .chars()
                .next_back()
                .is_some_and(|character| character.is_ascii_alphanumeric() || character == '_');
        let after = start + field.len();
        if !boundary {
            cursor = after;
            continue;
        }

        let mut value_start = after;
        while input[value_start..].starts_with(char::is_whitespace) {
            value_start += input[value_start..].chars().next().unwrap().len_utf8();
        }
        if !input[value_start..].starts_with([':', '=']) {
            cursor = after;
            continue;
        }
        value_start += 1;
        while input[value_start..].starts_with(char::is_whitespace) {
            value_start += input[value_start..].chars().next().unwrap().len_utf8();
        }

        let (value_end, replacement) = if input[value_start..].starts_with('"') {
            let end = input[value_start + 1..]
                .find('"')
                .map(|offset| value_start + 1 + offset)
                .unwrap_or(input.len());
            (
                end + 1,
                format!("\"{}\"", crate::utils::correlation::REDACTED),
            )
        } else {
            let end = input[value_start..]
                .find(|character: char| {
                    character == ',' || character == '}' || character.is_whitespace()
                })
                .map(|offset| value_start + offset)
                .unwrap_or(input.len());
            (end, crate::utils::correlation::REDACTED.to_string())
        };

        output.push_str(&input[cursor..value_start]);
        output.push_str(&replacement);
        cursor = value_end;
    }
    output.push_str(&input[cursor..]);
    output
}

fn redact_mnemonics(input: &str) -> String {
    let words: Vec<&str> = input.split_whitespace().collect();
    for start in 0..words.len() {
        for count in [24, 12] {
            if start + count <= words.len() {
                let phrase = words[start..start + count].join(" ");
                if bip39::Mnemonic::parse_in(bip39::Language::English, &phrase).is_ok() {
                    return input.replace(&phrase, crate::utils::correlation::REDACTED);
                }
                for prefix in ["mnemonic=", "mnemonic:"] {
                    let labeled_phrase = format!("{}{}", prefix, phrase);
                    if input.contains(&labeled_phrase) {
                        return input.replace(
                            &labeled_phrase,
                            &format!("{}{}", prefix, crate::utils::correlation::REDACTED),
                        );
                    }
                }
            }
        }
    }
    input.to_string()
}

fn is_candidate_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=' | '.' | '_' | '-')
}

fn is_sensitive_value(value: &str) -> bool {
    if crate::utils::correlation::looks_like_secret(value) {
        return true;
    }
    if value.len() == 56
        && value.starts_with('G')
        && value
            .chars()
            .all(|character| matches!(character, 'A'..='Z' | '2'..='7'))
    {
        return true;
    }
    if value.split('.').count() == 3 && value.split('.').all(|part| !part.is_empty()) {
        return true;
    }
    matches!(value.split_whitespace().count(), 12 | 24)
        && bip39::Mnemonic::parse_in(bip39::Language::English, value).is_ok()
}

/// A formatter writer that redacts complete log lines before they reach the
/// terminal or rotating file.
pub struct RedactingWriter<W> {
    inner: W,
    pending: Vec<u8>,
}

impl<W> RedactingWriter<W> {
    fn new(inner: W) -> Self {
        Self {
            inner,
            pending: Vec::new(),
        }
    }
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(bytes);
        while let Some(newline) = self.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.pending.drain(..=newline).collect();
            let text = String::from_utf8_lossy(&line);
            self.inner.write_all(redact_text(&text).as_bytes())?;
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            let text = String::from_utf8_lossy(&self.pending);
            self.inner.write_all(redact_text(&text).as_bytes())?;
            self.pending.clear();
        }
        self.inner.flush()
    }
}

struct RedactingMakeWriter<F> {
    factory: F,
}

impl<F> RedactingMakeWriter<F> {
    fn new(factory: F) -> Self {
        Self { factory }
    }
}

impl<'a, F, W> tracing_subscriber::fmt::MakeWriter<'a> for RedactingMakeWriter<F>
where
    F: Fn() -> W,
    W: Write,
{
    type Writer = RedactingWriter<W>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter::new((self.factory)())
    }
}

/// Build a `LogConfig` from CLI flags / environment.
///
/// - `--log-format json` → `LogFormat::Json`
/// - `--log-dir <path>`  → file rotation into that directory
/// - `RUST_LOG`          → overrides level at the filter level
pub fn config_from_env(format: Option<&str>, log_dir: Option<std::path::PathBuf>) -> LogConfig {
    let format = match format {
        Some("json") => LogFormat::Json,
        _ => LogFormat::Human,
    };

    LogConfig {
        format,
        log_dir,
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing::Level;

    #[test]
    fn redact_public_key_hides_value_at_info_level() {
        let key = "GDRXMZDQW34QHX6F5U6FFWJZZZDQ4KYWJO65HS4CUT62X7Y7RXYWXE4T";
        let redacted = redact_public_key(key, Level::INFO);

        assert!(redacted.starts_with("GDRX"));
        assert!(redacted.ends_with("4T"));
        assert!(redacted.contains("..."));
        assert_ne!(redacted, key);
    }

    #[test]
    fn redact_public_key_returns_full_value_at_debug_level() {
        let key = "GDRXMZDQW34QHX6F5U6FFWJZZZDQ4KYWJO65HS4CUT62X7Y7RXYWXE4T";
        assert_eq!(redact_public_key(key, Level::DEBUG), key);
        assert_eq!(redact_public_key(key, Level::TRACE), key);
    }

    #[test]
    fn redact_secret_value_always_redacts() {
        assert_eq!(redact_secret_value("super-secret"), "[REDACTED]");
    }

    #[test]
    fn redact_signed_xdr_always_redacts() {
        assert_eq!(redact_signed_xdr("signed-xdr-payload"), "[REDACTED]");
    }

    #[test]
    fn redact_text_covers_keys_mnemonics_tokens_and_signed_payloads() {
        let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let signed_xdr = "AbCdEf0123456789AbCdEf0123456789AbCdEf0123456789";
        let text = format!(
            "secret_key=SABC123 api_key=jwt.header.signature mnemonic={mnemonic} transaction_xdr={signed_xdr}"
        );
        let redacted = redact_text(&text);

        assert!(!redacted.contains("SABC123"));
        assert!(!redacted.contains(mnemonic));
        assert!(!redacted.contains("jwt.header.signature"));
        assert!(!redacted.contains(signed_xdr));
        assert!(redacted.matches("[REDACTED]").count() >= 4);
    }

    #[test]
    fn redact_text_preserves_invalid_and_ordinary_values() {
        let input = "invalid secret_key= host:port ordinary-value";
        assert_eq!(redact_text(input), input);
    }

    #[test]
    fn redacting_writer_redacts_on_flush() {
        let mut output = Vec::new();
        {
            let mut writer = RedactingWriter::new(&mut output);
            writer.write_all(b"api_key=jwt.header.signature").unwrap();
            writer.flush().unwrap();
        }
        assert!(!String::from_utf8(output)
            .unwrap()
            .contains("jwt.header.signature"));
    }
}
