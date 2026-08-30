use anyhow::Result;
use std::io::{self, Write};
use tracing::Level;
use tracing_appender::rolling;
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Layer};

use crate::utils::redaction::redact_secrets;

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

/// A custom writer wrapper that automatically redacts secrets from tracing log streams.
pub struct RedactingWriter<W: Write> {
    inner: W,
    buffer: Vec<u8>,
}

impl<W: Write> RedactingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            buffer: Vec::new(),
        }
    }
}

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend_from_slice(buf);

        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let line_bytes: Vec<u8> = self.buffer.drain(..=pos).collect();
            if let Ok(line_str) = std::str::from_utf8(&line_bytes) {
                let redacted = redact_secrets(line_str);
                self.inner.write_all(redacted.as_bytes())?;
            } else {
                self.inner.write_all(&line_bytes)?;
            }
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if !self.buffer.is_empty() {
            let remaining: Vec<u8> = self.buffer.drain(..).collect();
            if let Ok(line_str) = std::str::from_utf8(&remaining) {
                let redacted = redact_secrets(line_str);
                self.inner.write_all(redacted.as_bytes())?;
            } else {
                self.inner.write_all(&remaining)?;
            }
        }
        self.inner.flush()
    }
}

/// Helper constructor to wrap a writer target in a redacting writer.
pub fn redacting_writer<W: Write>(writer: W) -> RedactingWriter<W> {
    RedactingWriter::new(writer)
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

            let non_blocking_clone = non_blocking.clone();
            let file_layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(move || redacting_writer(non_blocking_clone.clone()))
                .with_filter(env_filter.clone());

            let stderr_layer = fmt::layer()
                .json()
                .with_current_span(true)
                .with_span_list(true)
                .with_writer(|| redacting_writer(std::io::stderr()))
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
                .with_writer(|| redacting_writer(std::io::stderr()))
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

            let non_blocking_clone = non_blocking.clone();
            let file_layer = fmt::layer()
                .with_ansi(false)
                .with_writer(move || redacting_writer(non_blocking_clone.clone()))
                .with_filter(env_filter.clone());

            let stderr_layer = fmt::layer()
                .with_writer(|| redacting_writer(std::io::stderr()))
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
                .with_writer(|| redacting_writer(std::io::stderr()))
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
pub fn redact_secret_value(_value: &str) -> String {
    "[REDACTED]".to_string()
}

/// Always redact signed XDR payloads when they are written to logs.
///
/// XDR envelopes containing signatures are secret and must not be emitted at
/// info level.
pub fn redact_signed_xdr(_xdr: &str) -> String {
    "[REDACTED]".to_string()
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
    fn redacting_writer_redacts_stream() {
        let mut buf = Vec::new();
        {
            let mut writer = RedactingWriter::new(&mut buf);
            writer
                .write_all(b"Authorization: Bearer mytoken123\n")
                .unwrap();
            writer.flush().unwrap();
        }
        let output = String::from_utf8(buf).unwrap();
        assert!(!output.contains("mytoken123"));
        assert!(output.contains("Bearer [REDACTED]"));
    }
}
