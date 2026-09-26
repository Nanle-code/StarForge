use crate::utils::output::is_plain_mode_enabled;
use crate::utils::redaction::redact_secrets;
use colored::*;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

/// ASCII, screen-reader-friendly labels used in place of the decorative
/// Unicode symbols (`✓`/`✗`/`⚠`/`→`) when [`is_plain_mode_enabled`] is true.
/// The Unicode glyphs are announced inconsistently (or not at all) across
/// screen readers, and clutter braille-display and log-file output even
/// though this crate never relies on color alone to distinguish them.
mod symbol {
    pub const SUCCESS_PLAIN: &str = "[OK]";
    pub const ERROR_PLAIN: &str = "[ERROR]";
    pub const WARN_PLAIN: &str = "[WARN]";
    pub const INFO_PLAIN: &str = "[INFO]";
}

/// Builds the `success`/`error`/`warn`/`info` line, without printing it, so
/// the plain/non-plain choice is unit-testable independent of stdout/stderr.
/// `plain` is threaded in explicitly (`success`/etc. pass
/// `is_plain_mode_enabled()`) rather than read again in here, so a test can
/// exercise both branches without mutating global env state.
fn format_line(
    plain: bool,
    plain_symbol: &str,
    colored_symbol: colored::ColoredString,
    msg: &str,
) -> String {
    if plain {
        format!("{plain_symbol} {msg}")
    } else {
        format!("{colored_symbol} {msg}")
    }
}

pub fn success(msg: &str) {
    println!(
        "{}",
        format_line(
            is_plain_mode_enabled(),
            symbol::SUCCESS_PLAIN,
            "✓".green().bold(),
            msg
        )
    );
}

#[allow(dead_code)]
pub fn error(msg: &str) {
    let redacted = redact_secrets(msg);
    eprintln!(
        "{}",
        format_line(
            is_plain_mode_enabled(),
            symbol::ERROR_PLAIN,
            "✗".red().bold(),
            &redacted
        )
    );
}

pub fn info(msg: &str) {
    let redacted = redact_secrets(msg);
    println!(
        "{}",
        format_line(
            is_plain_mode_enabled(),
            symbol::INFO_PLAIN,
            "→".cyan(),
            &redacted
        )
    );
}

pub fn warn(msg: &str) {
    let redacted = redact_secrets(msg);
    println!(
        "{}",
        format_line(
            is_plain_mode_enabled(),
            symbol::WARN_PLAIN,
            "⚠".yellow().bold(),
            &redacted
        )
    );
}

/// Print a structured CLI error to stderr with context and recovery hints.
///
/// Output format:
///
/// ```text
///
///   ✗  Error: <message>
///      Context: <context>      (optional, from anyhow chain)
///
///   What to try:
///     → hint one
///     → hint two
///
/// ```
///
/// # Arguments
/// * `err`   – The `anyhow::Error` returned from a command.
/// * `hints` – Zero or more recovery hint strings shown under "What to try:".
///
/// If `hints` is empty, a generic fallback is printed instead.
pub fn cli_error(err: &anyhow::Error, hints: &[&str]) {
    let plain = is_plain_mode_enabled();
    let hint_marker = if plain { "-" } else { "→" };

    // Primary message (the outermost error in the anyhow chain)
    let err_msg = redact_secrets(&err.to_string());
    if plain {
        eprintln!("\n  {} {}\n", symbol::ERROR_PLAIN, err_msg);
    } else {
        eprintln!("\n  {} {}\n", "✗  Error:".red().bold(), err_msg);
    }

    // Walk the anyhow cause chain and print each context layer (skipping the
    // root which was already printed above).
    let chain: Vec<_> = err.chain().skip(1).collect();
    if !chain.is_empty() {
        for cause in &chain {
            let cause_msg = redact_secrets(&cause.to_string());
            if plain {
                eprintln!("     Context: {}", cause_msg);
            } else {
                eprintln!("     {} {}", "Context:".dimmed(), cause_msg.dimmed());
            }
        }
        eprintln!();
    }

    // Recovery hints
    if plain {
        eprintln!("  What to try:");
    } else {
        eprintln!("  {}", "What to try:".yellow().bold());
    }
    if hints.is_empty() {
        if plain {
            eprintln!(
                "   {} Run the command again with --verbose for more detail",
                hint_marker
            );
            eprintln!(
                "   {} Check https://github.com/Nanle-code/StarForge/issues for known issues",
                hint_marker
            );
        } else {
            eprintln!(
                "   {} Run the command again with {} for more detail",
                hint_marker,
                "--verbose".bright_white()
            );
            eprintln!(
                "   {} Check {} for known issues",
                hint_marker,
                "https://github.com/Nanle-code/StarForge/issues".bright_white()
            );
        }
    } else {
        for hint in hints {
            let redacted = redact_secrets(hint);
            if plain {
                eprintln!("   {} {}", hint_marker, redacted);
            } else {
                eprintln!("   {} {}", hint_marker.cyan(), redacted);
            }
        }
    }
    eprintln!();
}

pub fn header(msg: &str) {
    println!("\n{}", msg.bright_white().bold().underline());
}

pub fn kv(key: &str, value: &str) {
    println!(
        "  {:<20} {}",
        key.dimmed(),
        redact_secrets(value).bright_white()
    );
}

pub fn kv_accent(key: &str, value: &str) {
    println!(
        "  {:<20} {}",
        key.dimmed(),
        redact_secrets(value).cyan().bold()
    );
}

pub fn separator() {
    println!("{}", "─".repeat(60).dimmed());
}

pub fn step(n: usize, total: usize, msg: &str) {
    println!(
        "{} {}",
        format!("[{}/{}]", n, total).dimmed(),
        msg.bright_white()
    );
}

#[allow(dead_code)]
pub fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.enable_steady_tick(Duration::from_millis(120));
    pb.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.set_message(msg.to_string());
    pb
}

pub fn progress_bar(total: u64, msg: &str) -> ProgressBar {
    let pb = ProgressBar::new(total);
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos}/{len} ({eta}) {msg}",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb.set_message(msg.to_string());
    pb
}

#[allow(dead_code)]
pub fn multi_progress() -> MultiProgress {
    MultiProgress::new()
}

pub fn verified_badge(verified: bool) -> colored::ColoredString {
    if verified {
        " ✓ verified".green()
    } else {
        "".normal()
    }
}

/// Print an aligned table with dimmed headers and bright row values.
pub fn table(headers: &[&str], rows: &[Vec<String>]) {
    if headers.is_empty() {
        return;
    }

    let ncol = headers.len();
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate().take(ncol) {
            widths[i] = widths[i].max(cell.len());
        }
    }

    let header_line = headers
        .iter()
        .enumerate()
        .map(|(i, h)| format!("{:<width$}", h, width = widths[i]))
        .collect::<Vec<_>>()
        .join("  ");
    println!("  {}", header_line.dimmed());

    for row in rows {
        let line = (0..ncol)
            .map(|i| {
                let val = row.get(i).map(String::as_str).unwrap_or("");
                format!("{:<width$}", val, width = widths[i])
            })
            .collect::<Vec<_>>()
            .join("  ");
        println!("  {}", line.bright_white());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
        let ncol = headers.len();
        let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
        for row in rows {
            for (i, cell) in row.iter().enumerate().take(ncol) {
                widths[i] = widths[i].max(cell.len());
            }
        }
        widths
    }

    #[test]
    fn table_widths_use_header_and_cell_maxima() {
        let widths = column_widths(
            &["Name", "Description"],
            &[vec![
                "trusted".into(),
                "Lifecycle integration test plugin".into(),
            ]],
        );
        assert_eq!(widths[0], "trusted".len());
        assert_eq!(widths[1], "Lifecycle integration test plugin".len());
    }

    #[test]
    fn table_widths_handle_empty_rows() {
        let widths = column_widths(&["Name", "Version"], &[]);
        assert_eq!(widths, vec![4, 7]);
    }

    #[test]
    fn plain_line_uses_the_ascii_label_and_no_ansi_escapes() {
        let line = format_line(true, symbol::SUCCESS_PLAIN, "✓".green().bold(), "done");
        assert_eq!(line, "[OK] done");
        assert!(
            !line.contains('\u{1b}'),
            "plain output must carry no ANSI escapes: {line:?}"
        );
        assert!(
            !line.contains('✓'),
            "plain output must not carry the decorative symbol: {line:?}"
        );
    }

    #[test]
    fn non_plain_line_uses_the_symbol() {
        // `colored` only emits ANSI escapes when it believes the output
        // stream supports them, which is not guaranteed in a test harness;
        // the symbol itself is the part this crate's non-color-only
        // guarantee actually depends on; deterministic evidence of no
        // color-alone reliance, not proof that ANSI codes were emitted.
        let line = format_line(false, symbol::SUCCESS_PLAIN, "✓".green().bold(), "done");
        assert!(
            line.contains('✓'),
            "non-plain output must carry the symbol: {line:?}"
        );
        assert!(line.contains("done"));
        assert!(!line.contains(symbol::SUCCESS_PLAIN));
    }

    #[test]
    fn every_message_kind_has_a_distinct_plain_label() {
        let labels = [
            symbol::SUCCESS_PLAIN,
            symbol::ERROR_PLAIN,
            symbol::WARN_PLAIN,
            symbol::INFO_PLAIN,
        ];
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(
            unique.len(),
            labels.len(),
            "every message kind must remain distinguishable by its ASCII label alone in plain mode"
        );
    }
}
