use crate::utils::redaction::redact_secrets;
use colored::*;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use std::time::Duration;

pub fn success(msg: &str) {
    println!("{} {}", "✓".green().bold(), msg);
}

#[allow(dead_code)]
pub fn error(msg: &str) {
    eprintln!("{} {}", "✗".red().bold(), redact_secrets(msg));
}

pub fn info(msg: &str) {
    println!("{} {}", "→".cyan(), redact_secrets(msg));
}

pub fn warn(msg: &str) {
    println!("{} {}", "⚠".yellow().bold(), redact_secrets(msg));
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
    // Primary message (the outermost error in the anyhow chain)
    let err_msg = redact_secrets(&err.to_string());
    eprintln!("\n  {} {}\n", "✗  Error:".red().bold(), err_msg);

    // Walk the anyhow cause chain and print each context layer (skipping the
    // root which was already printed above).
    let chain: Vec<_> = err.chain().skip(1).collect();
    if !chain.is_empty() {
        for cause in &chain {
            let cause_msg = redact_secrets(&cause.to_string());
            eprintln!("     {} {}", "Context:".dimmed(), cause_msg.dimmed());
        }
        eprintln!();
    }

    // Recovery hints
    eprintln!("  {}", "What to try:".yellow().bold());
    if hints.is_empty() {
        eprintln!(
            "   {} Run the command again with {} for more detail",
            "→".cyan(),
            "--verbose".bright_white()
        );
        eprintln!(
            "   {} Check {} for known issues",
            "→".cyan(),
            "https://github.com/Nanle-code/StarForge/issues".bright_white()
        );
    } else {
        for hint in hints {
            eprintln!("   {} {}", "→".cyan(), redact_secrets(hint));
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
}
