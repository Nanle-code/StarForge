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
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let dir = home.join(".starforge").join("notifications");
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

fn send_email(destination: &str, _template: &str, data: &HashMap<String, String>) -> Result<()> {
    info(&format!("Email notification queued to {}", destination));
    return Ok(());
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
        let skip = history.len() - limit;
        history = history.into_iter().skip(skip).collect();
    }
    fs::write(path, serde_json::to_string_pretty(&history)?)?;
    Ok(())
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
