use crate::utils::config;
use crate::utils::stream::SorobanEvent;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventRoute {
    pub name: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Default)]
pub struct EventRouter {
    routes: Vec<EventRoute>,
}

impl EventRouter {
    pub fn new(routes: Vec<EventRoute>) -> Self {
        Self { routes }
    }

    pub fn from_specs(specs: &[String]) -> Result<Self> {
        let mut routes = Vec::new();
        for spec in specs {
            routes.push(parse_route(spec)?);
        }
        Ok(Self::new(routes))
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    pub fn route(&self, event: &SorobanEvent) -> Vec<String> {
        self.routes
            .iter()
            .filter(|route| event_matches_pattern(event, &route.pattern))
            .map(|route| route.name.clone())
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventAlertRule {
    pub id: String,
    pub severity: String,
    pub pattern: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventAlert {
    pub rule_id: String,
    pub severity: String,
    pub message: String,
}

#[derive(Debug, Clone, Default)]
pub struct AlertEngine {
    rules: Vec<EventAlertRule>,
}

impl AlertEngine {
    pub fn new(rules: Vec<EventAlertRule>) -> Self {
        Self { rules }
    }

    pub fn from_specs(specs: &[String]) -> Result<Self> {
        let mut rules = Vec::new();
        for (index, spec) in specs.iter().enumerate() {
            rules.push(parse_alert_rule(spec, index)?);
        }
        Ok(Self::new(rules))
    }

    pub fn evaluate(&self, event: &SorobanEvent) -> Vec<EventAlert> {
        self.rules
            .iter()
            .filter(|rule| event_matches_pattern(event, &rule.pattern))
            .map(|rule| EventAlert {
                rule_id: rule.id.clone(),
                severity: rule.severity.clone(),
                message: rule.message.clone(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventTrigger {
    pub pattern: String,
    pub command: String,
}

impl EventTrigger {
    pub fn from_specs(specs: &[String]) -> Result<Vec<Self>> {
        specs.iter().map(|spec| parse_trigger(spec)).collect()
    }

    pub fn matches(&self, event: &SorobanEvent) -> bool {
        event_matches_pattern(event, &self.pattern)
    }

    pub fn execute(&self, network: &str, contract_id: &str, event: &SorobanEvent) -> Result<()> {
        let topic = event.topic.join(",");
        let value = event.value.to_string();

        #[cfg(target_os = "windows")]
        let mut command = {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", &self.command]);
            cmd
        };

        #[cfg(not(target_os = "windows"))]
        let mut command = {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", &self.command]);
            cmd
        };

        let status = command
            .env("STARFORGE_NETWORK", network)
            .env("STARFORGE_CONTRACT_ID", contract_id)
            .env("STARFORGE_EVENT_ID", &event.id)
            .env("STARFORGE_EVENT_LEDGER", event.ledger.to_string())
            .env("STARFORGE_EVENT_TYPE", &event.event_type)
            .env("STARFORGE_EVENT_TOPIC", topic)
            .env("STARFORGE_EVENT_VALUE", value)
            .status()
            .with_context(|| format!("failed to execute trigger command '{}'", self.command))?;

        if !status.success() {
            anyhow::bail!(
                "trigger command '{}' exited with status {}",
                self.command,
                status
            );
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedEvent {
    pub observed_at: String,
    pub network: String,
    pub contract_id: String,
    pub routes: Vec<String>,
    pub alerts: Vec<EventAlert>,
    pub event: SorobanEvent,
}

impl PersistedEvent {
    pub fn new(
        network: &str,
        contract_id: &str,
        event: SorobanEvent,
        routes: Vec<String>,
        alerts: Vec<EventAlert>,
    ) -> Self {
        Self {
            observed_at: Utc::now().to_rfc3339(),
            network: network.to_string(),
            contract_id: contract_id.to_string(),
            routes,
            alerts,
            event,
        }
    }

    pub fn identity(&self) -> String {
        format!("{}:{}:{}", self.network, self.contract_id, self.event.id)
    }
}

#[derive(Debug, Clone)]
pub struct EventStore {
    path: PathBuf,
}

impl EventStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path(network: &str, contract_id: &str) -> Result<PathBuf> {
        let dir = config::config_dir().join("events");
        fs::create_dir_all(&dir)
            .with_context(|| format!("failed to create event store directory {}", dir.display()))?;
        Ok(dir.join(format!(
            "{}-{}.jsonl",
            sanitize_component(network),
            sanitize_component(contract_id)
        )))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn persist(&self, event: &PersistedEvent) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create event store directory {}",
                    parent.display()
                )
            })?;
        }

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .with_context(|| format!("failed to open event store {}", self.path.display()))?;
        serde_json::to_writer(&mut file, event)
            .with_context(|| format!("failed to serialize event into {}", self.path.display()))?;
        file.write_all(b"\n")?;
        Ok(())
    }

    pub fn replay(&self) -> Result<Vec<PersistedEvent>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let file = fs::File::open(&self.path)
            .with_context(|| format!("failed to open replay file {}", self.path.display()))?;
        let reader = BufReader::new(file);
        let mut events = Vec::new();

        let mut identities = HashSet::new();
        for (index, line) in reader.lines().enumerate() {
            let line = line.with_context(|| {
                format!(
                    "failed to read line {} from {}",
                    index + 1,
                    self.path.display()
                )
            })?;
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let event: PersistedEvent = match serde_json::from_str(trimmed) {
                Ok(event) => event,
                Err(error) => {
                    eprintln!(
                        "warning: skipping corrupt persisted event on line {} of {}: {}",
                        index + 1,
                        self.path.display(),
                        error
                    );
                    continue;
                }
            };
            if identities.insert(event.identity()) {
                events.push(event);
            }
        }

        Ok(events)
    }
}

#[derive(Debug, Clone, Default)]
pub struct EventAnalytics {
    pub total_events: usize,
    pub alert_count: usize,
    pub first_ledger: Option<u32>,
    pub last_ledger: Option<u32>,
    pub by_type: HashMap<String, usize>,
    pub by_route: HashMap<String, usize>,
    pub by_alert_severity: HashMap<String, usize>,
    recent: Vec<String>,
}

impl EventAnalytics {
    pub fn record(&mut self, event: &PersistedEvent) {
        self.total_events += 1;
        self.alert_count += event.alerts.len();
        self.first_ledger = Some(
            self.first_ledger
                .map(|ledger| ledger.min(event.event.ledger))
                .unwrap_or(event.event.ledger),
        );
        self.last_ledger = Some(
            self.last_ledger
                .map(|ledger| ledger.max(event.event.ledger))
                .unwrap_or(event.event.ledger),
        );
        *self
            .by_type
            .entry(event.event.event_type.clone())
            .or_insert(0) += 1;

        for route in &event.routes {
            *self.by_route.entry(route.clone()).or_insert(0) += 1;
        }
        for alert in &event.alerts {
            *self
                .by_alert_severity
                .entry(alert.severity.clone())
                .or_insert(0) += 1;
        }

        self.recent.push(format!(
            "ledger={} id={} type={} alerts={} routes={}",
            event.event.ledger,
            event.event.id,
            event.event.event_type,
            event.alerts.len(),
            if event.routes.is_empty() {
                "-".to_string()
            } else {
                event.routes.join(",")
            }
        ));
        if self.recent.len() > 10 {
            let excess = self.recent.len() - 10;
            self.recent.drain(0..excess);
        }
    }

    pub fn from_events(events: &[PersistedEvent]) -> Self {
        let mut analytics = Self::default();
        for event in events {
            analytics.record(event);
        }
        analytics
    }

    pub fn render_dashboard(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "\nEvent Analytics Dashboard");
        let _ = writeln!(out, "-------------------------");
        let _ = writeln!(out, "Total events : {}", self.total_events);
        let _ = writeln!(out, "Alerts fired : {}", self.alert_count);
        let ledger_range = match (self.first_ledger, self.last_ledger) {
            (Some(first), Some(last)) => format!("{}..{}", first, last),
            _ => "n/a".to_string(),
        };
        let _ = writeln!(out, "Ledger range : {}", ledger_range);
        write_counts(&mut out, "By type", &self.by_type);
        write_counts(&mut out, "By route", &self.by_route);
        write_counts(&mut out, "Alerts by severity", &self.by_alert_severity);
        let _ = writeln!(out, "Recent events:");
        if self.recent.is_empty() {
            let _ = writeln!(out, "  - none");
        } else {
            for event in &self.recent {
                let _ = writeln!(out, "  - {}", event);
            }
        }
        out
    }
}

pub fn event_matches_pattern(event: &SorobanEvent, pattern: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    if pattern.is_empty() || pattern == "*" {
        return true;
    }
    event_search_text(event).to_lowercase().contains(&pattern)
}

pub fn event_search_text(event: &SorobanEvent) -> String {
    format!(
        "{} {} {} {} {}",
        event.event_type,
        event.ledger,
        event.id,
        event.topic.join(" "),
        event.value
    )
}

fn parse_route(spec: &str) -> Result<EventRoute> {
    let (name, pattern) = spec.split_once('=').ok_or_else(|| {
        anyhow::anyhow!(
            "invalid route '{}'; expected name=pattern (example: swaps=swap)",
            spec
        )
    })?;
    let name = name.trim();
    let pattern = pattern.trim();
    if name.is_empty() || pattern.is_empty() {
        anyhow::bail!("invalid route '{}'; name and pattern cannot be empty", spec);
    }
    Ok(EventRoute {
        name: name.to_string(),
        pattern: pattern.to_string(),
    })
}

fn parse_alert_rule(spec: &str, index: usize) -> Result<EventAlertRule> {
    let parts: Vec<&str> = spec.splitn(3, ':').collect();
    let (severity, pattern, message) = match parts.as_slice() {
        [pattern] => ("high", pattern.trim(), None),
        [severity, pattern] if is_severity(severity.trim()) => {
            (severity.trim(), pattern.trim(), None)
        }
        [severity, pattern, message] if is_severity(severity.trim()) => {
            (severity.trim(), pattern.trim(), Some(message.trim()))
        }
        _ => ("high", spec.trim(), None),
    };

    if pattern.is_empty() {
        anyhow::bail!("invalid alert rule '{}'; pattern cannot be empty", spec);
    }

    Ok(EventAlertRule {
        id: format!("alert-{}", index + 1),
        severity: severity.to_lowercase(),
        pattern: pattern.to_string(),
        message: message
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("matched event pattern '{}'", pattern)),
    })
}

fn parse_trigger(spec: &str) -> Result<EventTrigger> {
    let (pattern, command) = spec.split_once('=').ok_or_else(|| {
        anyhow::anyhow!(
            "invalid trigger '{}'; expected pattern=command (example: mint=./on-mint.sh)",
            spec
        )
    })?;
    let pattern = pattern.trim();
    let command = command.trim();
    if pattern.is_empty() || command.is_empty() {
        anyhow::bail!(
            "invalid trigger '{}'; pattern and command cannot be empty",
            spec
        );
    }
    Ok(EventTrigger {
        pattern: pattern.to_string(),
        command: command.to_string(),
    })
}

fn is_severity(value: &str) -> bool {
    matches!(
        value.to_lowercase().as_str(),
        "info" | "low" | "medium" | "high" | "critical"
    )
}

fn sanitize_component(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn write_counts(out: &mut String, title: &str, counts: &HashMap<String, usize>) {
    let _ = writeln!(out, "{}:", title);
    if counts.is_empty() {
        let _ = writeln!(out, "  - none");
        return;
    }

    let mut items: Vec<_> = counts.iter().collect();
    items.sort_by_key(|(left, _)| *left);
    for (key, count) in items {
        let _ = writeln!(out, "  - {}: {}", key, count);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    fn sample_event() -> SorobanEvent {
        SorobanEvent {
            event_type: "contract".to_string(),
            ledger: 42,
            id: "0000000042-0000000001".to_string(),
            topic: vec!["swap".to_string(), "admin".to_string()],
            value: json!({ "amount": 100, "asset": "XLM" }),
        }
    }

    #[test]
    fn patterns_match_topics_and_values() {
        let event = sample_event();
        assert!(event_matches_pattern(&event, "swap"));
        assert!(event_matches_pattern(&event, "xlm"));
        assert!(!event_matches_pattern(&event, "missing"));
    }

    #[test]
    fn routes_are_parsed_and_applied() {
        let router = EventRouter::from_specs(&["dex=swap".to_string()]).unwrap();
        assert_eq!(router.route(&sample_event()), vec!["dex".to_string()]);
    }

    #[test]
    fn alerts_are_parsed_and_applied() {
        let engine = AlertEngine::from_specs(&["critical:admin:admin event".to_string()]).unwrap();
        let alerts = engine.evaluate(&sample_event());
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, "critical");
        assert_eq!(alerts[0].message, "admin event");
    }

    #[test]
    fn event_store_round_trips_persisted_events() {
        let dir = TempDir::new().unwrap();
        let store = EventStore::new(dir.path().join("events.jsonl"));
        let event = sample_event();
        let persisted = PersistedEvent::new(
            "testnet",
            "C123",
            event,
            vec!["dex".to_string()],
            vec![EventAlert {
                rule_id: "alert-1".to_string(),
                severity: "high".to_string(),
                message: "matched event pattern 'swap'".to_string(),
            }],
        );

        store.persist(&persisted).unwrap();

        let replayed = store.replay().unwrap();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].network, "testnet");
        assert_eq!(replayed[0].contract_id, "C123");
        assert_eq!(replayed[0].routes, vec!["dex".to_string()]);
        assert_eq!(replayed[0].alerts.len(), 1);
        assert_eq!(replayed[0].event.id, "0000000042-0000000001");
    }

    #[test]
    fn analytics_dashboard_includes_counts_and_recent_events() {
        let event = sample_event();
        let persisted = PersistedEvent::new(
            "testnet",
            "C123",
            event,
            vec!["dex".to_string()],
            vec![EventAlert {
                rule_id: "alert-1".to_string(),
                severity: "critical".to_string(),
                message: "admin event".to_string(),
            }],
        );

        let analytics = EventAnalytics::from_events(&[persisted]);
        let dashboard = analytics.render_dashboard();

        assert!(dashboard.contains("Event Analytics Dashboard"));
        assert!(dashboard.contains("Total events : 1"));
        assert!(dashboard.contains("Alerts fired : 1"));
        assert!(dashboard.contains("By route:"));
        assert!(dashboard.contains("Recent events:"));
    }

    #[test]
    fn replay_skips_corrupt_and_duplicate_records() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("events.jsonl");
        let store = EventStore::new(path.clone());
        let persisted =
            PersistedEvent::new("testnet", "C123", sample_event(), Vec::new(), Vec::new());
        store.persist(&persisted).unwrap();
        store.persist(&persisted).unwrap();
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap()
            .write_all(b"not-json\n")
            .unwrap();

        let replayed = store.replay().unwrap();
        assert_eq!(replayed.len(), 1);
        assert_eq!(replayed[0].identity(), "testnet:C123:0000000042-0000000001");
    }
}
