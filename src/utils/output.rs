use anyhow::Result;
use serde::Serialize;
use std::env;

pub fn set_json_mode(enabled: bool) {
    if enabled {
        env::set_var("STARFORGE_OUTPUT_JSON", "1");
    } else {
        env::remove_var("STARFORGE_OUTPUT_JSON");
    }
}

pub fn is_json_mode_enabled() -> bool {
    env::var("STARFORGE_OUTPUT_JSON")
        .ok()
        .map(|value| {
            let normalized = value.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false)
}

pub fn print_json<T: Serialize>(value: &T) -> Result<()> {
    let rendered = serde_json::to_string_pretty(value)?;
    let redacted = crate::utils::redaction::redact_secrets(&rendered);
    println!("{redacted}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truthy_values_enable_json_mode() {
        for value in ["1", "true", "TRUE", "yes", "on"] {
            env::set_var("STARFORGE_OUTPUT_JSON", value);
            assert!(is_json_mode_enabled());
        }
        env::remove_var("STARFORGE_OUTPUT_JSON");
    }

    #[test]
    fn falsy_values_disable_json_mode() {
        for value in ["0", "false", "no", "off", ""] {
            env::set_var("STARFORGE_OUTPUT_JSON", value);
            assert!(!is_json_mode_enabled());
        }
        env::remove_var("STARFORGE_OUTPUT_JSON");
    }
}
