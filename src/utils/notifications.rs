use anyhow::Result;
use chrono::Utc;
use colored::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
#[allow(unused_imports)]
use std::process::Command;

pub fn info(message: &str) {
    println!("  {} {}", "•".bright_blue(), message);
}

pub fn success(message: &str) {
    println!("  {} {}", "✓".green().bold(), message);
}

pub fn warn(message: &str) {
    eprintln!("  {} {}", "!".yellow().bold(), message);
}

pub fn alert(message: &str) {
    eprintln!(
        "\n  {} {}\n",
        "⚠ ALERT:".red().bold(),
        message.bright_white().bold()
    );
    print!("\x07");
    let _ = std::io::Write::flush(&mut std::io::stdout());
    try_system_notification(message);
}

fn try_system_notification(_message: &str) {
    #[allow(unused_variables)]
    let msg = _message;
    #[cfg(target_os = "macos")]
    let escaped = msg.replace('\\', "\\\\").replace('"', "\\\"");

    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification \"{}\" with title \"StarForge\"",
            escaped
        );
        let _ = Command::new("osascript").args(["-e", &script]).status();
    }

    #[cfg(target_os = "linux")]
    {
        let _ = Command::new("notify-send")
            .args(["StarForge", msg])
            .status();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationChannel {
    pub channel_type: String,
    pub destination: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationTemplate {
    pub name: String,
    pub subject: String,
    pub body: String,
    pub channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotificationEvent {
    pub id: String,
    pub template: String,
    pub severity: String,
    pub timestamp: String,
    pub data: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: String,
    pub condition: String,
    pub template: String,
    pub enabled: bool,
    pub channels: Vec<String>,
}

fn notifications_dir() -> Result<PathBuf> {
    let dir = crate::utils::config::config_dir().join("notifications");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn load_channels() -> Result<Vec<NotificationChannel>> {
    let path = notifications_dir()?.join("channels.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

pub fn save_channels(channels: &[NotificationChannel]) -> Result<()> {
    let path = notifications_dir()?.join("channels.json");
    fs::write(path, serde_json::to_string_pretty(channels)?)?;
    Ok(())
}

pub fn add_channel(channel_type: &str, destination: &str) -> Result<()> {
    let mut channels = load_channels()?;
    channels.push(NotificationChannel {
        channel_type: channel_type.to_string(),
        destination: destination.to_string(),
        enabled: true,
    });
    save_channels(&channels)?;
    Ok(())
}

pub fn send_notification(
    template_name: &str,
    data: &HashMap<String, String>,
    severity: &str,
) -> Result<()> {
    let channels = load_channels()?;

    let event = NotificationEvent {
        id: format!("notify-{}", Utc::now().timestamp()),
        template: template_name.to_string(),
        severity: severity.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        data: data.clone(),
    };

    save_notification_history(&event)?;

    for channel in channels.iter().filter(|c| c.enabled) {
        match channel.channel_type.as_str() {
            "email" => send_email(&channel.destination, template_name, data)?,
            "slack" => send_slack(&channel.destination, template_name, data)?,
            "discord" => send_discord(&channel.destination, template_name, data)?,
            "webhook" => send_webhook(&channel.destination, template_name, data)?,
            _ => {}
        }
    }
    Ok(())
}

fn send_email(destination: &str, _template: &str, _data: &HashMap<String, String>) -> Result<()> {
    info(&format!("Email notification queued to {}", destination));
    Ok(())
}

fn send_slack(destination: &str, _template: &str, data: &HashMap<String, String>) -> Result<()> {
    let default_msg = "Deployment notification".to_string();
    let msg = data.get("message").unwrap_or(&default_msg);
    info(&format!(
        "Slack notification queued to {}: {}",
        destination, msg
    ));
    Ok(())
}

fn send_discord(destination: &str, _template: &str, data: &HashMap<String, String>) -> Result<()> {
    let default_msg = "Deployment notification".to_string();
    let msg = data.get("message").unwrap_or(&default_msg);
    info(&format!(
        "Discord notification queued to {}: {}",
        destination, msg
    ));
    Ok(())
}

fn send_webhook(destination: &str, _template: &str, _data: &HashMap<String, String>) -> Result<()> {
    info(&format!("Webhook notification queued to {}", destination));
    Ok(())
}

fn save_notification_history(event: &NotificationEvent) -> Result<()> {
    let path = notifications_dir()?.join("history.json");
    let mut history: Vec<NotificationEvent> = if path.exists() {
        let data = fs::read_to_string(&path)?;
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        vec![]
    };
    history.push(event.clone());
    let limit = 1000;
    if history.len() > limit {
        let len = history.len();
        history = history.into_iter().skip(len - limit).collect();
    }
    fs::write(path, serde_json::to_string_pretty(&history)?)?;
    Ok(())
}

pub fn send_approval_notification(
    template: &str,
    request_id: &str,
    contract_id: &str,
    network: &str,
    requested_by: &str,
    level: &str,
    status: &str,
) -> Result<()> {
    let mut data = HashMap::new();
    data.insert("request_id".to_string(), request_id.to_string());
    data.insert("contract_id".to_string(), contract_id.to_string());
    data.insert("network".to_string(), network.to_string());
    data.insert("requested_by".to_string(), requested_by.to_string());
    data.insert("level".to_string(), level.to_string());
    data.insert("status".to_string(), status.to_string());
    data.insert(
        "message".to_string(),
        format!(
            "Approval {} for deployment of {} on {}",
            status, contract_id, network
        ),
    );
    send_notification(template, &data, "medium")
}

pub fn send_approval_requested_notification(
    request_id: &str,
    contract_id: &str,
    network: &str,
    requested_by: &str,
    level: &str,
) -> Result<()> {
    alert(&format!(
        "Approval request {} submitted for {} on {} by {}",
        request_id, contract_id, network, requested_by
    ));
    send_approval_notification(
        "approval_requested",
        request_id,
        contract_id,
        network,
        requested_by,
        level,
        "requested",
    )
}

pub fn send_approval_completed_notification(
    request_id: &str,
    contract_id: &str,
    network: &str,
    approved_by: &str,
    status: &str,
) -> Result<()> {
    success(&format!(
        "Approval request {} completed: {} by {}",
        request_id, status, approved_by
    ));
    send_approval_notification(
        "approval_completed",
        request_id,
        contract_id,
        network,
        approved_by,
        "",
        status,
    )
}

// ---------------------------------------------------------------------------
// Rollback notifications (#383 / D-46)
// ---------------------------------------------------------------------------
//
// Mirrors the `send_approval_*` pair above: a plain-language line printed
// immediately via `alert`/`info` (so a human watching the terminal sees it
// without needing a configured channel), fanned out to whatever channels
// are configured via `send_notification`, and — either way — recorded in
// notification history so "was anyone told about this rollback?" has an
// auditable answer even when no channel is configured yet.
//
// Called for every automatic-rollback outcome, not just the successful
// path: an operator needs to know just as urgently that a deployment
// failed with *nothing* to roll back to, or that a rollback was recorded
// but failed its consistency check, as they do that a rollback succeeded.

fn rollback_notification_data(
    network: &str,
    wallet: &str,
    rollback_id: Option<&str>,
    rolled_back_to: Option<&str>,
    contract_id: Option<&str>,
    reason: &str,
    verified: Option<bool>,
) -> HashMap<String, String> {
    let mut data = HashMap::new();
    data.insert("network".to_string(), network.to_string());
    data.insert("wallet".to_string(), wallet.to_string());
    if let Some(id) = rollback_id {
        data.insert("rollback_id".to_string(), id.to_string());
    }
    if let Some(id) = rolled_back_to {
        data.insert("rolled_back_to".to_string(), id.to_string());
    }
    if let Some(id) = contract_id {
        data.insert("contract_id".to_string(), id.to_string());
    }
    data.insert("reason".to_string(), reason.to_string());
    if let Some(v) = verified {
        data.insert("verified".to_string(), v.to_string());
    }
    data.insert(
        "message".to_string(),
        rollback_notification_message(network, rolled_back_to, reason, verified),
    );
    data
}

/// Builds the human-readable summary line shared by the terminal alert and
/// every dispatched channel. Pure and separated out from `data` construction
/// specifically so it's directly unit-testable without touching
/// `~/.starforge/notifications`.
fn rollback_notification_message(
    network: &str,
    rolled_back_to: Option<&str>,
    reason: &str,
    verified: Option<bool>,
) -> String {
    match rolled_back_to {
        Some(target) => {
            let verification = match verified {
                Some(true) => "verified",
                Some(false) => "verification FAILED",
                None => "verification pending",
            };
            format!(
                "Automatic rollback engaged on {network}: reverted to deployment {target} ({reason}) — {verification}."
            )
        }
        None => {
            format!(
                "Automatic rollback skipped on {network}: {reason}."
            )
        }
    }
}

/// Notify about an automatic-rollback outcome — engaged (with or without a
/// passing verification) or skipped, e.g. because there was nothing to roll
/// back to, or automatic rollback was disabled for this deploy.
#[allow(clippy::too_many_arguments)]
pub fn send_rollback_notification(
    network: &str,
    wallet: &str,
    rollback_id: Option<&str>,
    rolled_back_to: Option<&str>,
    contract_id: Option<&str>,
    reason: &str,
    verified: Option<bool>,
) -> Result<()> {
    let message =
        rollback_notification_message(network, rolled_back_to, reason, verified);

    match (rolled_back_to.is_some(), verified) {
        (true, Some(false)) => alert(&message),
        (true, _) => warn(&message),
        (false, _) => info(&message),
    }

    let data = rollback_notification_data(
        network,
        wallet,
        rollback_id,
        rolled_back_to,
        contract_id,
        reason,
        verified,
    );
    let severity = match (rolled_back_to.is_some(), verified) {
        (true, Some(false)) => "high",
        (true, _) => "medium",
        (false, _) => "low",
    };
    send_notification("rollback", &data, severity)
}

pub fn list_notification_history(limit: usize) -> Result<Vec<NotificationEvent>> {
    let path = notifications_dir()?.join("history.json");
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    let mut history: Vec<NotificationEvent> = serde_json::from_str(&data).unwrap_or_default();
    history.reverse();
    Ok(history.into_iter().take(limit).collect())
}

#[cfg(test)]
mod rollback_notification_tests {
    use super::*;

    #[test]
    fn message_for_engaged_rollback_with_passing_verification() {
        let msg = rollback_notification_message(
            "testnet",
            Some("abcd1234"),
            "deployment failed",
            Some(true),
        );
        assert!(msg.contains("testnet"));
        assert!(msg.contains("abcd1234"));
        assert!(msg.contains("deployment failed"));
        assert!(msg.contains("verified"));
        assert!(!msg.contains("FAILED"));
    }

    #[test]
    fn message_for_engaged_rollback_with_failing_verification() {
        let msg = rollback_notification_message(
            "mainnet",
            Some("target-1"),
            "post-deployment verification failed",
            Some(false),
        );
        assert!(msg.contains("verification FAILED"));
    }

    #[test]
    fn message_for_engaged_rollback_pending_verification() {
        let msg = rollback_notification_message("testnet", Some("target-1"), "reason", None);
        assert!(msg.contains("verification pending"));
    }

    #[test]
    fn message_for_skipped_rollback_has_no_target_language() {
        let msg = rollback_notification_message(
            "testnet",
            None,
            "no previous successful deployment on this network",
            None,
        );
        assert!(msg.contains("skipped"));
        assert!(msg.contains("no previous successful deployment"));
        // A skipped rollback must never claim a revert happened.
        assert!(!msg.contains("reverted to"));
    }

    #[test]
    fn data_includes_all_provided_fields() {
        let data = rollback_notification_data(
            "testnet",
            "alice",
            Some("rb-1"),
            Some("target-1"),
            Some("CABC"),
            "deployment failed",
            Some(true),
        );
        assert_eq!(data.get("network").map(String::as_str), Some("testnet"));
        assert_eq!(data.get("wallet").map(String::as_str), Some("alice"));
        assert_eq!(data.get("rollback_id").map(String::as_str), Some("rb-1"));
        assert_eq!(
            data.get("rolled_back_to").map(String::as_str),
            Some("target-1")
        );
        assert_eq!(data.get("contract_id").map(String::as_str), Some("CABC"));
        assert_eq!(data.get("verified").map(String::as_str), Some("true"));
        assert!(data.contains_key("message"));
    }

    #[test]
    fn data_omits_optional_fields_that_are_none() {
        // Boundary: a skipped rollback has no rollback_id, target, contract,
        // or verification result — the data map must not fabricate any of
        // those keys.
        let data = rollback_notification_data(
            "testnet",
            "alice",
            None,
            None,
            None,
            "no previous successful deployment",
            None,
        );
        assert!(!data.contains_key("rollback_id"));
        assert!(!data.contains_key("rolled_back_to"));
        assert!(!data.contains_key("contract_id"));
        assert!(!data.contains_key("verified"));
        assert_eq!(data.get("network").map(String::as_str), Some("testnet"));
    }
}
