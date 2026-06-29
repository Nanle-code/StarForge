//! Real-time contract event processing: filtering, routing, alerting,
//! persistence/replay, analytics and event-based triggers.
//!
//! Live delivery is handled by [`crate::utils::stream::SorobanEventStream`],
//! which streams Soroban RPC `getEvents` with cursor pagination and exponential
//! backoff (the canonical event transport — Soroban RPC exposes events over
//! JSON-RPC rather than a socket). This module sits on top of that transport
//! and turns raw events into a processing pipeline:
//!
//! ```text
//!   SorobanEvent -> EventRecord -> [filter] -> route(s) / alert pattern(s) / triggers
//!                                      |                                          |
//!                                   persist  <----------- replay ---------------- +
//! ```
//!
//! Everything here is sync and persisted as JSON under `~/.starforge/events/`.

use crate::utils::config;
use crate::utils::stream::SorobanEvent;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Normalized, persistable event
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: String,
    pub contract_id: String,
    pub event_type: String,
    pub ledger: u32,
    pub topics: Vec<String>,
    pub value: String,
    /// RFC3339 timestamp the event was received/recorded.
    pub received_at: String,
}

impl EventRecord {
    pub fn from_soroban(ev: &SorobanEvent, contract_id: &str) -> Self {
        Self {
            id: ev.id.clone(),
            contract_id: contract_id.to_string(),
            event_type: ev.event_type.clone(),
            ledger: ev.ledger,
            topics: ev.topic.clone(),
            value: ev.value.to_string(),
            received_at: now_rfc3339(),
        }
    }
}

// ---------------------------------------------------------------------------
// Filtering
// ---------------------------------------------------------------------------

/// A declarative filter matched against an [`EventRecord`]. All set fields must
/// match (logical AND). `topic_pattern` is a comma-separated list of segments
/// where `*` matches any single segment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventFilter {
    #[serde(default)]
    pub contract_id: Option<String>,
    #[serde(default)]
    pub event_type: Option<String>,
    #[serde(default)]
    pub topic_pattern: Option<String>,
    #[serde(default)]
    pub value_contains: Option<String>,
}

impl EventFilter {
    pub fn matches(&self, rec: &EventRecord) -> bool {
        if let Some(c) = &self.contract_id {
            if &rec.contract_id != c {
                return false;
            }
        }
        if let Some(t) = &self.event_type {
            if !rec.event_type.eq_ignore_ascii_case(t) {
                return false;
            }
        }
        if let Some(v) = &self.value_contains {
            if !rec.value.to_lowercase().contains(&v.to_lowercase()) {
                return false;
            }
        }
        if let Some(pattern) = &self.topic_pattern {
            if !topic_matches(pattern, &rec.topics) {
                return false;
            }
        }
        true
    }
}

/// Segment-wise wildcard match: each comma-separated pattern segment must equal
/// the corresponding topic segment, or be `*`. A pattern with fewer segments
/// than the topic matches as a prefix.
fn topic_matches(pattern: &str, topics: &[String]) -> bool {
    let segments: Vec<&str> = pattern.split(',').map(|s| s.trim()).collect();
    if segments.len() > topics.len() {
        return false;
    }
    segments
        .iter()
        .zip(topics.iter())
        .all(|(pat, topic)| *pat == "*" || pat == topic)
}

// ---------------------------------------------------------------------------
// Routing & triggers
// ---------------------------------------------------------------------------

/// The side effect to perform when a route matches an event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RouteAction {
    /// Print the event to the console.
    Log,
    /// Queue a notification through the configured channels.
    Notify { template: String, severity: String },
    /// POST the event payload to a webhook URL.
    Webhook { url: String },
}

/// A named routing rule: when `filter` matches, perform `action`. Routes double
/// as event-based triggers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventRoute {
    pub name: String,
    pub filter: EventFilter,
    pub action: RouteAction,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Return the routes (by reference) whose filter matches the record.
pub fn match_routes<'a>(rec: &EventRecord, routes: &'a [EventRoute]) -> Vec<&'a EventRoute> {
    routes
        .iter()
        .filter(|r| r.enabled && r.filter.matches(rec))
        .collect()
}

/// Execute a route's action against an event (performs side effects).
pub fn execute_route(route: &EventRoute, rec: &EventRecord) -> Result<()> {
    match &route.action {
        RouteAction::Log => {
            println!(
                "  [{}] ledger {} {} {}",
                route.name, rec.ledger, rec.event_type, rec.value
            );
            Ok(())
        }
        RouteAction::Notify { template, severity } => {
            let mut data = std::collections::HashMap::new();
            data.insert(
                "message".to_string(),
                format!("Event {} on {}", rec.id, rec.contract_id),
            );
            data.insert("event_id".to_string(), rec.id.clone());
            data.insert("ledger".to_string(), rec.ledger.to_string());
            crate::utils::notifications::send_notification(template, &data, severity)
        }
        RouteAction::Webhook { url } => {
            let payload = serde_json::to_value(rec)?;
            ureq::post(url)
                .set("Content-Type", "application/json")
                .send_json(payload)
                .map(|_| ())
                .with_context(|| format!("Webhook POST to {} failed", url))
        }
    }
}

// ---------------------------------------------------------------------------
// Alert patterns (sliding-window thresholds)
// ---------------------------------------------------------------------------

/// Fire an alert when at least `threshold` events match `filter` within a
/// trailing window of `window_secs`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertPattern {
    pub name: String,
    pub filter: EventFilter,
    pub window_secs: i64,
    pub threshold: usize,
    #[serde(default = "default_severity")]
    pub severity: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_severity() -> String {
    "warning".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Alert {
    pub pattern: String,
    pub severity: String,
    pub count: usize,
    pub threshold: usize,
    pub window_secs: i64,
}

/// Count how many of `events` match the pattern within `window_secs` of
/// `now_epoch` (seconds since the Unix epoch).
pub fn count_in_window(pattern: &AlertPattern, events: &[EventRecord], now_epoch: i64) -> usize {
    events
        .iter()
        .filter(|e| pattern.filter.matches(e))
        .filter(|e| {
            parse_epoch(&e.received_at)
                .map(|t| now_epoch - t <= pattern.window_secs && now_epoch - t >= 0)
                .unwrap_or(false)
        })
        .count()
}

/// Evaluate a single pattern, returning an `Alert` if the threshold is met.
pub fn evaluate_pattern(
    pattern: &AlertPattern,
    events: &[EventRecord],
    now_epoch: i64,
) -> Option<Alert> {
    if !pattern.enabled {
        return None;
    }
    let count = count_in_window(pattern, events, now_epoch);
    if count >= pattern.threshold {
        Some(Alert {
            pattern: pattern.name.clone(),
            severity: pattern.severity.clone(),
            count,
            threshold: pattern.threshold,
            window_secs: pattern.window_secs,
        })
    } else {
        None
    }
}

/// Evaluate every pattern against the event history.
pub fn evaluate_alerts(
    patterns: &[AlertPattern],
    events: &[EventRecord],
    now_epoch: i64,
) -> Vec<Alert> {
    patterns
        .iter()
        .filter_map(|p| evaluate_pattern(p, events, now_epoch))
        .collect()
}

// ---------------------------------------------------------------------------
// Analytics
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventAnalytics {
    pub total: usize,
    pub by_type: BTreeMap<String, usize>,
    pub by_contract: BTreeMap<String, usize>,
    pub unique_ledgers: usize,
    pub first_ledger: Option<u32>,
    pub last_ledger: Option<u32>,
    /// Top topic segments by frequency, descending.
    pub top_topics: Vec<(String, usize)>,
}

impl EventAnalytics {
    pub fn from_events(events: &[EventRecord]) -> Self {
        let mut by_type: BTreeMap<String, usize> = BTreeMap::new();
        let mut by_contract: BTreeMap<String, usize> = BTreeMap::new();
        let mut topic_counts: BTreeMap<String, usize> = BTreeMap::new();
        let mut ledgers = std::collections::BTreeSet::new();

        for e in events {
            *by_type.entry(e.event_type.clone()).or_default() += 1;
            *by_contract.entry(e.contract_id.clone()).or_default() += 1;
            ledgers.insert(e.ledger);
            for t in &e.topics {
                *topic_counts.entry(t.clone()).or_default() += 1;
            }
        }

        let mut top_topics: Vec<(String, usize)> = topic_counts.into_iter().collect();
        top_topics.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        top_topics.truncate(10);

        Self {
            total: events.len(),
            by_type,
            by_contract,
            unique_ledgers: ledgers.len(),
            first_ledger: ledgers.iter().next().copied(),
            last_ledger: ledgers.iter().next_back().copied(),
            top_topics,
        }
    }
}

// ---------------------------------------------------------------------------
// Replay
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReplaySummary {
    pub processed: usize,
    pub matched_routes: usize,
    pub alerts: Vec<Alert>,
}

/// Replay persisted events through the routing + alerting pipeline without
/// performing side effects. Useful for testing rules against history.
pub fn replay(
    events: &[EventRecord],
    routes: &[EventRoute],
    patterns: &[AlertPattern],
    now_epoch: i64,
) -> ReplaySummary {
    let mut matched_routes = 0;
    for e in events {
        matched_routes += match_routes(e, routes).len();
    }
    ReplaySummary {
        processed: events.len(),
        matched_routes,
        alerts: evaluate_alerts(patterns, events, now_epoch),
    }
}

// ---------------------------------------------------------------------------
// Persistence (~/.starforge/events/)
// ---------------------------------------------------------------------------

const MAX_PERSISTED_EVENTS: usize = 5000;

fn events_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("events");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

fn events_path(contract: &str) -> Result<PathBuf> {
    Ok(events_dir()?.join(format!("{}.json", sanitize(contract))))
}

/// Load all persisted events for a contract (oldest first).
pub fn load_events(contract: &str) -> Result<Vec<EventRecord>> {
    let path = events_path(contract)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let contents = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&contents).unwrap_or_default())
}

/// Append events for a contract, de-duplicating by id and capping history.
pub fn append_events(contract: &str, new_events: &[EventRecord]) -> Result<usize> {
    if new_events.is_empty() {
        return Ok(0);
    }
    let mut existing = load_events(contract)?;
    let mut seen: std::collections::HashSet<String> =
        existing.iter().map(|e| e.id.clone()).collect();

    let mut added = 0;
    for e in new_events {
        if seen.insert(e.id.clone()) {
            existing.push(e.clone());
            added += 1;
        }
    }

    if existing.len() > MAX_PERSISTED_EVENTS {
        let overflow = existing.len() - MAX_PERSISTED_EVENTS;
        existing.drain(0..overflow);
    }

    let path = events_path(contract)?;
    fs::write(&path, serde_json::to_string_pretty(&existing)?)?;
    Ok(added)
}

// --- routes config ---

fn routes_path() -> Result<PathBuf> {
    Ok(events_dir()?.join("routes.json"))
}

pub fn load_routes() -> Result<Vec<EventRoute>> {
    let path = routes_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default())
}

pub fn save_routes(routes: &[EventRoute]) -> Result<()> {
    fs::write(routes_path()?, serde_json::to_string_pretty(routes)?)?;
    Ok(())
}

pub fn add_route(route: EventRoute) -> Result<()> {
    let mut routes = load_routes()?;
    routes.retain(|r| r.name != route.name);
    routes.push(route);
    save_routes(&routes)
}

// --- alert patterns config ---

fn alerts_path() -> Result<PathBuf> {
    Ok(events_dir()?.join("alert_patterns.json"))
}

pub fn load_alert_patterns() -> Result<Vec<AlertPattern>> {
    let path = alerts_path()?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(serde_json::from_str(&fs::read_to_string(&path)?).unwrap_or_default())
}

pub fn save_alert_patterns(patterns: &[AlertPattern]) -> Result<()> {
    fs::write(alerts_path()?, serde_json::to_string_pretty(patterns)?)?;
    Ok(())
}

pub fn add_alert_pattern(pattern: AlertPattern) -> Result<()> {
    let mut patterns = load_alert_patterns()?;
    patterns.retain(|p| p.name != pattern.name);
    patterns.push(pattern);
    save_alert_patterns(&patterns)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// Current Unix time in seconds (used for alert windows).
pub fn now_epoch() -> i64 {
    chrono::Utc::now().timestamp()
}

fn parse_epoch(rfc3339: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .ok()
        .map(|dt| dt.timestamp())
}

fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(
        id: &str,
        etype: &str,
        ledger: u32,
        topics: &[&str],
        value: &str,
        ts: &str,
    ) -> EventRecord {
        EventRecord {
            id: id.to_string(),
            contract_id: "CTEST".to_string(),
            event_type: etype.to_string(),
            ledger,
            topics: topics.iter().map(|s| s.to_string()).collect(),
            value: value.to_string(),
            received_at: ts.to_string(),
        }
    }

    #[test]
    fn filter_matches_type_and_value() {
        let r = rec(
            "1",
            "contract",
            10,
            &["transfer", "alice"],
            "100",
            "2026-01-01T00:00:00Z",
        );
        let f = EventFilter {
            event_type: Some("contract".into()),
            value_contains: Some("100".into()),
            ..Default::default()
        };
        assert!(f.matches(&r));

        let f2 = EventFilter {
            value_contains: Some("999".into()),
            ..Default::default()
        };
        assert!(!f2.matches(&r));
    }

    #[test]
    fn topic_wildcard_matches() {
        let r = rec(
            "1",
            "contract",
            10,
            &["transfer", "alice"],
            "x",
            "2026-01-01T00:00:00Z",
        );
        assert!(topic_matches("transfer,*", &r.topics));
        assert!(topic_matches("transfer", &r.topics)); // prefix
        assert!(!topic_matches("mint,*", &r.topics));
        assert!(!topic_matches("transfer,alice,extra", &r.topics)); // too long
    }

    #[test]
    fn routes_match_only_enabled() {
        let r = rec(
            "1",
            "contract",
            10,
            &["transfer"],
            "x",
            "2026-01-01T00:00:00Z",
        );
        let routes = vec![
            EventRoute {
                name: "on".into(),
                filter: EventFilter {
                    topic_pattern: Some("transfer".into()),
                    ..Default::default()
                },
                action: RouteAction::Log,
                enabled: true,
            },
            EventRoute {
                name: "off".into(),
                filter: EventFilter::default(),
                action: RouteAction::Log,
                enabled: false,
            },
        ];
        let matched = match_routes(&r, &routes);
        assert_eq!(matched.len(), 1);
        assert_eq!(matched[0].name, "on");
    }

    #[test]
    fn alert_fires_when_threshold_met_in_window() {
        let events = vec![
            rec(
                "1",
                "contract",
                1,
                &["err"],
                "x",
                "2026-01-01T00:00:00+00:00",
            ),
            rec(
                "2",
                "contract",
                2,
                &["err"],
                "x",
                "2026-01-01T00:00:30+00:00",
            ),
            rec(
                "3",
                "contract",
                3,
                &["err"],
                "x",
                "2026-01-01T00:00:50+00:00",
            ),
        ];
        let now = parse_epoch("2026-01-01T00:01:00+00:00").unwrap();
        let pattern = AlertPattern {
            name: "errors".into(),
            filter: EventFilter {
                topic_pattern: Some("err".into()),
                ..Default::default()
            },
            window_secs: 120,
            threshold: 3,
            severity: "critical".into(),
            enabled: true,
        };
        let alert = evaluate_pattern(&pattern, &events, now).unwrap();
        assert_eq!(alert.count, 3);
    }

    #[test]
    fn alert_respects_window() {
        let events = vec![
            rec(
                "1",
                "contract",
                1,
                &["err"],
                "x",
                "2026-01-01T00:00:00+00:00",
            ),
            rec(
                "2",
                "contract",
                2,
                &["err"],
                "x",
                "2026-01-01T00:00:01+00:00",
            ),
        ];
        // 'now' is far in the future; both events fall outside a 10s window.
        let now = parse_epoch("2026-01-01T01:00:00+00:00").unwrap();
        let pattern = AlertPattern {
            name: "errors".into(),
            filter: EventFilter::default(),
            window_secs: 10,
            threshold: 1,
            severity: "warning".into(),
            enabled: true,
        };
        assert!(evaluate_pattern(&pattern, &events, now).is_none());
    }

    #[test]
    fn analytics_aggregates_counts() {
        let events = vec![
            rec(
                "1",
                "contract",
                10,
                &["transfer"],
                "x",
                "2026-01-01T00:00:00Z",
            ),
            rec(
                "2",
                "contract",
                11,
                &["transfer"],
                "y",
                "2026-01-01T00:00:01Z",
            ),
            rec("3", "system", 11, &["fee"], "z", "2026-01-01T00:00:02Z"),
        ];
        let a = EventAnalytics::from_events(&events);
        assert_eq!(a.total, 3);
        assert_eq!(a.by_type["contract"], 2);
        assert_eq!(a.unique_ledgers, 2);
        assert_eq!(a.first_ledger, Some(10));
        assert_eq!(a.last_ledger, Some(11));
        assert_eq!(a.top_topics[0].0, "transfer");
    }

    #[test]
    fn replay_counts_routes_and_alerts() {
        let events = vec![
            rec(
                "1",
                "contract",
                1,
                &["err"],
                "x",
                "2026-01-01T00:00:00+00:00",
            ),
            rec(
                "2",
                "contract",
                2,
                &["err"],
                "x",
                "2026-01-01T00:00:01+00:00",
            ),
        ];
        let routes = vec![EventRoute {
            name: "log-all".into(),
            filter: EventFilter::default(),
            action: RouteAction::Log,
            enabled: true,
        }];
        let patterns = vec![AlertPattern {
            name: "any".into(),
            filter: EventFilter::default(),
            window_secs: 3600,
            threshold: 2,
            severity: "info".into(),
            enabled: true,
        }];
        let now = parse_epoch("2026-01-01T00:01:00+00:00").unwrap();
        let summary = replay(&events, &routes, &patterns, now);
        assert_eq!(summary.processed, 2);
        assert_eq!(summary.matched_routes, 2);
        assert_eq!(summary.alerts.len(), 1);
    }
}
