//! Shared progress reporting for multi-step commands.
//!
//! The reporter deliberately owns output formatting so orchestration and
//! pipeline commands cannot drift into incompatible progress behaviour.

use crate::utils::{output, redaction};
use indicatif::{ProgressBar, ProgressStyle};
use serde::Serialize;
use std::io::IsTerminal;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProgressEventKind {
    Started,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProgressEvent {
    pub event: ProgressEventKind,
    pub step: String,
    pub index: usize,
    pub total: usize,
    pub message: Option<String>,
    pub elapsed_ms: u128,
}

/// Reports one event per lifecycle transition for a multi-step operation.
///
/// TTY users get a spinner; redirected output gets one stable line per event;
/// JSON mode gets one JSON object per event. Messages are redacted before they
/// reach any output sink so credentials cannot be copied into progress logs.
pub struct ProgressReporter {
    total: usize,
    started_at: Instant,
    spinner: Option<ProgressBar>,
}

impl ProgressReporter {
    pub fn new(total: usize) -> Self {
        let spinner = if !output::is_json_mode_enabled() && std::io::stdout().is_terminal() {
            let pb = ProgressBar::new_spinner();
            pb.enable_steady_tick(std::time::Duration::from_millis(100));
            pb.set_style(
                ProgressStyle::with_template("{spinner:.cyan} {msg}")
                    .expect("static progress template is valid"),
            );
            Some(pb)
        } else {
            None
        };

        Self {
            total,
            started_at: Instant::now(),
            spinner,
        }
    }

    pub fn started(&self, index: usize, step: impl Into<String>) {
        self.emit(ProgressEventKind::Started, index, step.into(), None);
    }

    pub fn completed(&self, index: usize, step: impl Into<String>, message: impl Into<String>) {
        self.emit(
            ProgressEventKind::Completed,
            index,
            step.into(),
            Some(message.into()),
        );
    }

    pub fn failed(&self, index: usize, step: impl Into<String>, message: impl Into<String>) {
        self.emit(
            ProgressEventKind::Failed,
            index,
            step.into(),
            Some(message.into()),
        );
    }

    fn emit(&self, event: ProgressEventKind, index: usize, step: String, message: Option<String>) {
        let step = redaction::redact_secrets(&step);
        let message = message.map(|value| redaction::redact_secrets(&value));
        let elapsed_ms = self.started_at.elapsed().as_millis();

        if output::is_json_mode_enabled() {
            let event = ProgressEvent {
                event,
                step,
                index,
                total: self.total,
                message,
                elapsed_ms,
            };
            // Serialization of this in-memory event cannot fail for the types
            // above. Keep the fallback defensive in case that changes later.
            if let Ok(rendered) = serde_json::to_string(&event) {
                println!("{rendered}");
            }
            return;
        }

        let label = format!("[{}/{}] {}", index, self.total, step);
        match event {
            ProgressEventKind::Started => {
                if let Some(spinner) = &self.spinner {
                    spinner.set_message(label);
                } else {
                    println!("[INFO] {label}");
                }
            }
            ProgressEventKind::Completed => {
                if let Some(spinner) = &self.spinner {
                    spinner.println(format!("[OK] {label}{}", suffix(message.as_deref())));
                } else {
                    println!("[OK] {label}{}", suffix(message.as_deref()));
                }
            }
            ProgressEventKind::Failed => {
                if let Some(spinner) = &self.spinner {
                    spinner.println(format!("[ERROR] {label}{}", suffix(message.as_deref())));
                } else {
                    println!("[ERROR] {label}{}", suffix(message.as_deref()));
                }
            }
        }
    }
}

impl Drop for ProgressReporter {
    fn drop(&mut self) {
        if let Some(spinner) = self.spinner.take() {
            spinner.finish_and_clear();
        }
    }
}

fn suffix(message: Option<&str>) -> String {
    message.map(|value| format!(": {value}")).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_kind_serializes_as_snake_case() {
        let event = ProgressEvent {
            event: ProgressEventKind::Completed,
            step: "build".into(),
            index: 1,
            total: 2,
            message: Some("done".into()),
            elapsed_ms: 3,
        };
        let json = serde_json::to_string(&event).unwrap();
        assert!(json.contains("\"event\":\"completed\""));
        assert!(json.contains("\"total\":2"));
    }

    #[test]
    fn suffix_is_optional() {
        assert_eq!(suffix(None), "");
        assert_eq!(suffix(Some("done")), ": done");
    }
}
