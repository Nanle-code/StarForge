use crate::utils::event_stream as es;
use crate::utils::print as p;
use crate::utils::stream::{EventStreamFilters, SorobanEventStream};
use crate::utils::{config, notifications, soroban};
use anyhow::Result;
use clap::Subcommand;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Subcommand)]
pub enum EventsCommands {
    /// Stream live contract events, persisting and routing them in real time
    Stream(StreamArgs),
    /// List persisted events for a contract
    List(ListArgs),
    /// Replay persisted events through the routing + alert pipeline
    Replay(ReplayArgs),
    /// Show event analytics for a contract
    Analytics(AnalyticsArgs),
    /// Manage routing rules / event-based triggers
    #[command(subcommand)]
    Route(RouteCommands),
    /// Manage alert patterns
    #[command(subcommand)]
    Alert(AlertCommands),
}

#[derive(clap::Args)]
pub struct StreamArgs {
    /// Contract ID to stream events from
    #[arg(long)]
    pub contract: String,
    /// Network (testnet/mainnet)
    #[arg(long, default_value = "testnet")]
    pub network: String,
    /// Event type filter (contract/system/diagnostic)
    #[arg(long)]
    pub event_type: Option<String>,
    /// Comma-separated topic segments (use * to wildcard a segment)
    #[arg(long)]
    pub topic: Option<String>,
    /// Persist matched events to ~/.starforge/events/
    #[arg(long)]
    pub persist: bool,
    /// Keep streaming until interrupted (Ctrl+C)
    #[arg(long)]
    pub follow: bool,
    /// Poll interval in seconds
    #[arg(long, default_value = "2")]
    pub interval: u64,
}

#[derive(clap::Args)]
pub struct ListArgs {
    #[arg(long)]
    pub contract: String,
    #[arg(long, default_value = "20")]
    pub limit: usize,
    #[arg(long)]
    pub json: bool,
}

#[derive(clap::Args)]
pub struct ReplayArgs {
    #[arg(long)]
    pub contract: String,
    /// Only replay events at or after this ledger
    #[arg(long)]
    pub from_ledger: Option<u32>,
}

#[derive(clap::Args)]
pub struct AnalyticsArgs {
    #[arg(long)]
    pub contract: String,
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand)]
pub enum RouteCommands {
    /// Add or replace a routing rule
    Add(RouteAddArgs),
    /// List routing rules
    List,
}

#[derive(clap::Args)]
pub struct RouteAddArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub event_type: Option<String>,
    #[arg(long)]
    pub topic: Option<String>,
    #[arg(long)]
    pub value_contains: Option<String>,
    /// Action: log | notify | webhook
    #[arg(long, default_value = "log")]
    pub action: String,
    /// Webhook URL (for --action webhook)
    #[arg(long)]
    pub url: Option<String>,
    /// Notification template (for --action notify)
    #[arg(long, default_value = "event")]
    pub template: String,
    /// Notification severity (for --action notify)
    #[arg(long, default_value = "info")]
    pub severity: String,
}

#[derive(Subcommand)]
pub enum AlertCommands {
    /// Add or replace an alert pattern
    Add(AlertAddArgs),
    /// List alert patterns
    List,
}

#[derive(clap::Args)]
pub struct AlertAddArgs {
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub event_type: Option<String>,
    #[arg(long)]
    pub topic: Option<String>,
    #[arg(long)]
    pub value_contains: Option<String>,
    /// Trailing window in seconds
    #[arg(long, default_value = "60")]
    pub window: i64,
    /// Number of matching events that triggers the alert
    #[arg(long, default_value = "5")]
    pub threshold: usize,
    #[arg(long, default_value = "warning")]
    pub severity: String,
}

pub fn handle(cmd: EventsCommands) -> Result<()> {
    match cmd {
        EventsCommands::Stream(args) => handle_stream(args),
        EventsCommands::List(args) => handle_list(args),
        EventsCommands::Replay(args) => handle_replay(args),
        EventsCommands::Analytics(args) => handle_analytics(args),
        EventsCommands::Route(cmd) => handle_route(cmd),
        EventsCommands::Alert(cmd) => handle_alert(cmd),
    }
}

fn handle_stream(args: StreamArgs) -> Result<()> {
    config::validate_contract_id(&args.contract)?;
    config::validate_network(&args.network)?;

    let mut filters = EventStreamFilters::default();
    if let Some(t) = &args.event_type {
        filters.event_type = Some(t.trim().to_lowercase());
    }
    if let Some(topic) = &args.topic {
        let segments: Vec<String> = topic
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !segments.is_empty() {
            filters.topic_segments = Some(segments);
        }
    }

    let rpc_url = soroban::rpc_url(&args.network)?;
    let routes = es::load_routes()?;
    let patterns = es::load_alert_patterns()?;

    p::header("Real-Time Event Stream");
    p::kv("Contract", &args.contract);
    p::kv("RPC", &rpc_url);
    p::kv("Routes", &routes.len().to_string());
    p::kv("Alert patterns", &patterns.len().to_string());
    p::kv("Persist", if args.persist { "yes" } else { "no" });
    p::separator();

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || running.store(false, Ordering::SeqCst))?;
    }

    let mut stream = SorobanEventStream::new(rpc_url, args.contract.clone())
        .with_poll_interval(args.interval)
        .with_filters(filters);

    let mut session: Vec<es::EventRecord> = Vec::new();
    let mut fired: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut seen_any = false;

    while running.load(Ordering::SeqCst) {
        match stream.next_batch() {
            Ok(batch) => {
                let records: Vec<es::EventRecord> = batch
                    .iter()
                    .map(|e| es::EventRecord::from_soroban(e, &args.contract))
                    .collect();

                for rec in &records {
                    seen_any = true;
                    p::success(&format!(
                        "Ledger {} · {} · {}",
                        rec.ledger, rec.event_type, rec.value
                    ));
                    // Routing / triggers.
                    for route in es::match_routes(rec, &routes) {
                        if let Err(e) = es::execute_route(route, rec) {
                            p::warn(&format!("route '{}' failed: {}", route.name, e));
                        }
                    }
                }

                session.extend(records.iter().cloned());

                if args.persist && !records.is_empty() {
                    let added = es::append_events(&args.contract, &records)?;
                    if added > 0 {
                        p::info(&format!("Persisted {} event(s)", added));
                    }
                }

                // Alert evaluation over the live session window.
                for alert in es::evaluate_alerts(&patterns, &session, es::now_epoch()) {
                    if fired.insert(alert.pattern.clone()) {
                        notifications::alert(&format!(
                            "[{}] alert '{}' fired: {} events in {}s (threshold {})",
                            alert.severity,
                            alert.pattern,
                            alert.count,
                            alert.window_secs,
                            alert.threshold
                        ));
                    }
                }

                if !args.follow {
                    if !seen_any {
                        p::warn("No matching events in the latest batch.");
                    }
                    break;
                }
                stream.sleep();
            }
            Err(err) => {
                if !args.follow && !seen_any {
                    return Err(err);
                }
                p::warn(&format!(
                    "Stream error: {}. Reconnecting with backoff…",
                    err
                ));
                stream.sleep_backoff();
            }
        }
    }

    p::separator();
    p::success(&format!(
        "Stream ended — {} event(s) this session",
        session.len()
    ));
    Ok(())
}

fn handle_list(args: ListArgs) -> Result<()> {
    let mut events = es::load_events(&args.contract)?;
    events.reverse(); // newest first
    events.truncate(args.limit);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&events)?);
        return Ok(());
    }

    p::header("Persisted Events");
    if events.is_empty() {
        p::info("No events stored. Stream with: starforge events stream --contract <id> --persist --follow");
        return Ok(());
    }
    let rows: Vec<Vec<String>> = events
        .iter()
        .map(|e| {
            vec![
                e.ledger.to_string(),
                e.event_type.clone(),
                truncate(&e.topics.join(","), 24),
                truncate(&e.value, 32),
                e.received_at.chars().take(19).collect(),
            ]
        })
        .collect();
    p::table(&["Ledger", "Type", "Topics", "Value", "Received"], &rows);
    Ok(())
}

fn handle_replay(args: ReplayArgs) -> Result<()> {
    let mut events = es::load_events(&args.contract)?;
    if let Some(from) = args.from_ledger {
        events.retain(|e| e.ledger >= from);
    }
    let routes = es::load_routes()?;
    let patterns = es::load_alert_patterns()?;
    let summary = es::replay(&events, &routes, &patterns, es::now_epoch());

    p::header("Event Replay");
    p::kv("Events processed", &summary.processed.to_string());
    p::kv("Route matches", &summary.matched_routes.to_string());
    p::kv("Alerts", &summary.alerts.len().to_string());
    p::separator();
    for a in &summary.alerts {
        p::warn(&format!(
            "[{}] {}: {} events (threshold {})",
            a.severity, a.pattern, a.count, a.threshold
        ));
    }
    if summary.alerts.is_empty() {
        p::success("No alert patterns triggered during replay");
    }
    Ok(())
}

fn handle_analytics(args: AnalyticsArgs) -> Result<()> {
    let events = es::load_events(&args.contract)?;
    let analytics = es::EventAnalytics::from_events(&events);

    if args.json {
        println!("{}", serde_json::to_string_pretty(&analytics)?);
        return Ok(());
    }

    p::header("Event Analytics");
    p::kv("Contract", &args.contract);
    p::kv_accent("Total events", &analytics.total.to_string());
    p::kv("Unique ledgers", &analytics.unique_ledgers.to_string());
    if let (Some(first), Some(last)) = (analytics.first_ledger, analytics.last_ledger) {
        p::kv("Ledger range", &format!("{} → {}", first, last));
    }
    p::separator();

    if analytics.total == 0 {
        p::info("No events recorded yet.");
        return Ok(());
    }

    p::header("By Type");
    let max = analytics
        .by_type
        .values()
        .copied()
        .max()
        .unwrap_or(1)
        .max(1);
    for (ty, count) in &analytics.by_type {
        let bar = "█".repeat(((*count * 30) / max).max(1));
        println!("  {:<14} {} {}", ty, bar, count);
    }

    if !analytics.top_topics.is_empty() {
        p::header("Top Topics");
        let rows: Vec<Vec<String>> = analytics
            .top_topics
            .iter()
            .map(|(t, c)| vec![truncate(t, 36), c.to_string()])
            .collect();
        p::table(&["Topic", "Count"], &rows);
    }
    Ok(())
}

fn handle_route(cmd: RouteCommands) -> Result<()> {
    match cmd {
        RouteCommands::List => {
            p::header("Event Routes");
            let routes = es::load_routes()?;
            if routes.is_empty() {
                p::info("No routes configured. Add one with: starforge events route add ...");
                return Ok(());
            }
            for r in &routes {
                let action = match &r.action {
                    es::RouteAction::Log => "log".to_string(),
                    es::RouteAction::Notify { template, severity } => {
                        format!("notify({}, {})", template, severity)
                    }
                    es::RouteAction::Webhook { url } => format!("webhook({})", url),
                };
                p::kv(
                    &r.name,
                    &format!("{} [{}]", action, if r.enabled { "on" } else { "off" }),
                );
            }
            Ok(())
        }
        RouteCommands::Add(args) => {
            let action = match args.action.as_str() {
                "log" => es::RouteAction::Log,
                "notify" => es::RouteAction::Notify {
                    template: args.template.clone(),
                    severity: args.severity.clone(),
                },
                "webhook" => {
                    let url = args
                        .url
                        .clone()
                        .ok_or_else(|| anyhow::anyhow!("--url is required for --action webhook"))?;
                    es::RouteAction::Webhook { url }
                }
                other => anyhow::bail!("Unknown action '{}' (use log, notify, or webhook)", other),
            };
            let route = es::EventRoute {
                name: args.name.clone(),
                filter: es::EventFilter {
                    contract_id: None,
                    event_type: args.event_type.clone(),
                    topic_pattern: args.topic.clone(),
                    value_contains: args.value_contains.clone(),
                },
                action,
                enabled: true,
            };
            es::add_route(route)?;
            p::success(&format!("Route '{}' saved", args.name));
            Ok(())
        }
    }
}

fn handle_alert(cmd: AlertCommands) -> Result<()> {
    match cmd {
        AlertCommands::List => {
            p::header("Alert Patterns");
            let patterns = es::load_alert_patterns()?;
            if patterns.is_empty() {
                p::info("No alert patterns. Add one with: starforge events alert add ...");
                return Ok(());
            }
            let rows: Vec<Vec<String>> = patterns
                .iter()
                .map(|p| {
                    vec![
                        p.name.clone(),
                        p.severity.clone(),
                        format!("{}s", p.window_secs),
                        p.threshold.to_string(),
                        if p.enabled { "on".into() } else { "off".into() },
                    ]
                })
                .collect();
            p::table(
                &["Name", "Severity", "Window", "Threshold", "Enabled"],
                &rows,
            );
            Ok(())
        }
        AlertCommands::Add(args) => {
            let pattern = es::AlertPattern {
                name: args.name.clone(),
                filter: es::EventFilter {
                    contract_id: None,
                    event_type: args.event_type.clone(),
                    topic_pattern: args.topic.clone(),
                    value_contains: args.value_contains.clone(),
                },
                window_secs: args.window,
                threshold: args.threshold,
                severity: args.severity.clone(),
                enabled: true,
            };
            es::add_alert_pattern(pattern)?;
            p::success(&format!("Alert pattern '{}' saved", args.name));
            Ok(())
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}…", &s[..max])
    } else {
        s.to_string()
    }
}
