use crate::utils::{config, privacy};
use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct TelemetryData {
    pub timestamp: DateTime<Utc>,
    pub event: String,
    pub properties: serde_json::Value,
    pub anonymous_id: String,
}

pub fn telemetry_log_path() -> Result<PathBuf> {
    Ok(config::get_data_dir()?.join("telemetry.log"))
}

pub fn is_telemetry_enabled() -> bool {
    if let Ok(env_val) = std::env::var("STARFORGE_TELEMETRY") {
        let enabled = !matches!(
            env_val.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "disabled" | "no"
        );
        if env_val.trim() == "" {
            return false;
        }
        return enabled;
    }

    let cfg = match config::load() {
        Ok(cfg) => cfg,
        Err(_) => return false,
    };

    cfg.telemetry_enabled.unwrap_or(false)
}

pub fn read_events() -> Result<Vec<TelemetryData>> {
    let path = telemetry_log_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }

    let contents = fs::read_to_string(path)?;
    let mut events = Vec::new();
    for line in contents.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(event) = serde_json::from_str::<TelemetryData>(line) {
            events.push(event);
        }
    }
    Ok(events)
}

pub fn show_payload() -> Result<Option<String>> {
    let events = read_events()?;
    let last = events.last().cloned();
    match last {
        Some(event) => Ok(Some(serde_json::to_string_pretty(&event)?)),
        None => Ok(None),
    }
}

pub fn reset() -> Result<()> {
    let data_dir = config::get_data_dir()?;
    let telemetry_log = data_dir.join("telemetry.log");
    let anonymous_id = data_dir.join("anonymous_id");

    if telemetry_log.exists() {
        fs::remove_file(telemetry_log)?;
    }
    if anonymous_id.exists() {
        fs::remove_file(anonymous_id)?;
    }

    Ok(())
}

pub fn track_event(event: &str, properties: serde_json::Value) -> Result<()> {
    if !is_telemetry_enabled() {
        return Ok(());
    }

    let anonymous_id = get_or_create_anonymous_id()?;
    let minimized_properties =
        privacy::minimize_payload(&properties, &["event", "success", "duration_ms"]);
    let sanitized_properties = privacy::sanitize_payload(&minimized_properties);
    let assessment = privacy::assess_privacy_impact(&sanitized_properties, "telemetry", true);
    let consent = privacy::ConsentRecord::new("telemetry", true);
    let report = privacy::build_privacy_report(&assessment, &consent);
    let _ = privacy::persist_privacy_report(&report);

    let data = TelemetryData {
        timestamp: Utc::now(),
        event: event.to_string(),
        properties: sanitized_properties,
        anonymous_id,
    };

    save_telemetry_locally(&data)?;

    Ok(())
}

fn get_or_create_anonymous_id() -> Result<String> {
    let data_dir = config::get_data_dir()?;
    let id_file = data_dir.join("anonymous_id");

    if id_file.exists() {
        Ok(fs::read_to_string(id_file)?.trim().to_string())
    } else {
        let id = Uuid::new_v4().to_string();
        fs::write(id_file, &id)?;
        Ok(id)
    }
}

fn save_telemetry_locally(data: &TelemetryData) -> Result<()> {
    let data_dir = config::get_data_dir()?;
    let telemetry_log = data_dir.join("telemetry.log");

    let json = serde_json::to_string(data)?;

    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(telemetry_log)?;

    writeln!(file, "{}", json)?;

    Ok(())
}

#[allow(dead_code)]
pub fn set_telemetry_enabled(enabled: bool) -> Result<()> {
    let mut cfg = config::load()?;
    cfg.telemetry_enabled = Some(enabled);
    config::save(&cfg)?;
    Ok(())
}
