use crate::utils::{
    config,
    event_monitoring::{
        AlertEngine, EventAnalytics, EventRouter, EventStore, EventTrigger, PersistedEvent,
    },
    horizon, notifications, print as p, soroban,
    stream::{EventStreamFilters, EventStreamTransport, SorobanEvent, SorobanEventStream},
};
use anyhow::Result;
use clap::Args;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const DEFAULT_PERSIST_SENTINEL: &str = "__starforge_default_event_store__";

#[derive(Args)]
pub struct MonitorArgs {
    /// Contract ID (starts with 'C...') to monitor via Soroban RPC getEvents
    #[arg(long)]
    pub contract: Option<String>,

    /// Comma-separated list of event names to filter (best-effort; matches topic strings)
    #[arg(long)]
    pub events: Option<String>,

    /// Stream events continuously until Ctrl+C (contract mode)
    #[arg(long)]
    pub follow: bool,

    /// Soroban event type filter (contract, system, diagnostic)
    #[arg(long = "type")]
    pub event_type: Option<String>,

    /// Topic filter: comma-separated segment matchers (* wildcards supported)
    #[arg(long)]
    pub topic: Option<String>,

    /// Match emitted event value (substring match on JSON payload)
    #[arg(long)]
    pub value: Option<String>,

    /// Event transport: auto prefers WebSocket and falls back to HTTP; websocket requires WS support; http preserves polling
    #[arg(long, default_value = "auto", value_parser = ["auto", "websocket", "http"])]
    pub transport: String,

    /// Override the derived WebSocket URL (for RPCs that expose WS on a different path)
    #[arg(long)]
    pub websocket_url: Option<String>,

    /// Route matching events into named lanes using name=pattern (repeatable)
    #[arg(long = "route")]
    pub routes: Vec<String>,

    /// Alert rule as pattern, severity:pattern, or severity:pattern:message (repeatable)
    #[arg(long = "alert")]
    pub alerts: Vec<String>,

    /// Persist matching events to JSONL; omit PATH to use ~/.starforge/events/<network>-<contract>.jsonl
    #[arg(
        long,
        value_name = "PATH",
        num_args = 0..=1,
        default_missing_value = DEFAULT_PERSIST_SENTINEL
    )]
    pub persist: Option<PathBuf>,

    /// Replay a JSONL event store instead of connecting to RPC
    #[arg(long)]
    pub replay: Option<PathBuf>,

    /// Render a live/replay analytics dashboard
    #[arg(long)]
    pub dashboard: bool,

    /// Execute a shell command when pattern matches, using pattern=command (repeatable)
    #[arg(long = "trigger")]
    pub triggers: Vec<String>,

    /// Explicitly allow configured event triggers to execute shell commands
    #[arg(long)]
    pub allow_triggers: bool,

    /// Wallet name from starforge config to monitor
    #[arg(long)]
    pub wallet: Option<String>,

    /// Threshold amount in XLM to trigger a notification (wallet mode)
    #[arg(long)]
    pub threshold: Option<f64>,

    /// Alert when wallet XLM balance drops below this amount (watchman)
    #[arg(long)]
    pub balance_alert: Option<f64>,

    /// Network to use (overrides config)
    #[arg(long)]
    pub network: Option<String>,

    /// Poll interval in seconds (also controls WebSocket getEvents cadence)
    #[arg(long, default_value = "2")]
    pub interval: u64,
}

pub async fn handle(args: MonitorArgs) -> Result<()> {
    let cfg = config::load()?;
    let network = args.network.as_deref().unwrap_or(&cfg.network);
    config::validate_network(network)?;

    p::header("Real-time Monitoring");
    p::separator();
    p::kv("Network", network);
    p::separator();
    println!();

    match (&args.contract, &args.wallet) {
        (Some(contract_id), None) => {
            monitor_contract(
                contract_id,
                args.events.as_deref(),
                args.event_type.as_deref(),
                args.topic.as_deref(),
                args.value.as_deref(),
                network,
                args.interval,
                args.follow,
                &args.transport,
                args.websocket_url.as_deref(),
                &args.routes,
                &args.alerts,
                args.persist.as_ref(),
                args.replay.as_ref(),
                args.dashboard,
                &args.triggers,
                args.allow_triggers,
            )
            .await
        }
        (None, Some(wallet_name)) => {
            if args.replay.is_some()
                || args.persist.is_some()
                || !args.routes.is_empty()
                || !args.alerts.is_empty()
                || !args.triggers.is_empty()
            {
                anyhow::bail!(
                    "event stream options (--replay/--persist/--route/--alert/--trigger) are only supported with --contract"
                );
            }
            monitor_wallet(
                wallet_name,
                args.threshold,
                args.balance_alert,
                network,
                args.interval,
            )
            .await
        }
        _ => anyhow::bail!("Specify either --contract or --wallet (but not both)"),
    }
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
async fn monitor_contract(
    contract_id: &str,
    events_filter: Option<&str>,
    event_type: Option<&str>,
    topic: Option<&str>,
    value: Option<&str>,
    network: &str,
    interval: u64,
    follow: bool,
    transport: &str,
    websocket_url: Option<&str>,
    routes: &[String],
    alerts: &[String],
    persist: Option<&PathBuf>,
    replay: Option<&PathBuf>,
    dashboard: bool,
    trigger_specs: &[String],
    allow_triggers: bool,
) -> Result<()> {
    config::validate_contract_id(contract_id)?;

    let legacy_filter_set = parse_legacy_filter(events_filter);
    let stream_filters = build_stream_filters(event_type, topic, value);
    let router = EventRouter::from_specs(routes)?;
    let alert_engine = AlertEngine::from_specs(alerts)?;
    let triggers = EventTrigger::from_specs(trigger_specs)?;
    if !triggers.is_empty() && !allow_triggers {
        anyhow::bail!(
            "event triggers execute shell commands; rerun with --allow-triggers to enable them"
        );
    }

    if let Some(replay_path) = replay {
        return replay_contract_events(
            contract_id,
            network,
            replay_path,
            &legacy_filter_set,
            &stream_filters,
            &router,
            &alert_engine,
            &triggers,
            dashboard,
        );
    }

    let rpc_url = soroban::rpc_url(network)?;
    let transport = EventStreamTransport::parse(transport)?;
    let mut stream = SorobanEventStream::new(rpc_url.clone(), contract_id.to_string())
        .with_poll_interval(interval)
        .with_transport(transport)
        .with_filters(stream_filters.clone());
    if let Some(url) = websocket_url {
        stream = stream.with_websocket_url(url.to_string());
    }

    notifications::info(&format!("Streaming contract events from {}.", rpc_url));
    p::kv(
        "Transport",
        &format!("{:?}", stream.transport()).to_lowercase(),
    );
    if matches!(
        stream.transport(),
        EventStreamTransport::Auto | EventStreamTransport::WebSocket
    ) {
        p::kv("WebSocket", stream.websocket_url());
    }

    let event_store = match persist {
        Some(path) if path == &PathBuf::from(DEFAULT_PERSIST_SENTINEL) => Some(EventStore::new(
            EventStore::default_path(network, contract_id)?,
        )),
        Some(path) => Some(EventStore::new(path.clone())),
        None => None,
    };
    if let Some(store) = &event_store {
        p::kv("Event store", &store.path().display().to_string());
    }
    if !router.is_empty() {
        p::kv("Routing", &format!("{} route(s)", routes.len()));
    }
    if !alerts.is_empty() {
        p::kv("Alerts", &format!("{} rule(s)", alerts.len()));
    }
    if !triggers.is_empty() {
        p::kv("Triggers", &format!("{} trigger(s)", triggers.len()));
    }

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })?;
    }

    let mut printed_any = false;
    let mut analytics = EventAnalytics::default();

    while running.load(Ordering::SeqCst) {
        match stream.next_batch().await {
            Ok(batch) => {
                for event in batch {
                    if matches_monitor_filters(&event, &legacy_filter_set, &stream_filters) {
                        printed_any = true;
                        process_contract_event(
                            network,
                            contract_id,
                            &event,
                            &router,
                            &alert_engine,
                            event_store.as_ref(),
                            &mut analytics,
                            &triggers,
                        )?;
                    }
                }

                if dashboard {
                    println!("{}", analytics.render_dashboard());
                }

                if !follow {
                    if !printed_any {
                        notifications::warn("No matching events in the latest batch.");
                    }
                    break;
                }
                stream.sleep().await;
            }
            Err(err) => {
                if !follow && !printed_any {
                    return Err(err);
                }
                notifications::warn(&format!(
                    "Event stream error: {}. Reconnecting with backoff…",
                    err
                ));
                stream.sleep_backoff().await;
            }
        }
    }

    if dashboard && printed_any {
        p::success("Final analytics dashboard rendered above");
    }

    Ok(())
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
fn replay_contract_events(
    contract_id: &str,
    network: &str,
    replay_path: &Path,
    legacy_filter_set: &Option<Vec<String>>,
    stream_filters: &EventStreamFilters,
    router: &EventRouter,
    alert_engine: &AlertEngine,
    triggers: &[EventTrigger],
    dashboard: bool,
) -> Result<()> {
    let store = EventStore::new(replay_path.to_path_buf());
    let events = store.replay()?;
    notifications::info(&format!(
        "Replaying {} persisted event(s) from {}.",
        events.len(),
        replay_path.display()
    ));

    let mut analytics = EventAnalytics::default();
    let mut matched = 0usize;

    for persisted in events {
        if persisted.contract_id != contract_id || persisted.network != network {
            continue;
        }
        if !matches_monitor_filters(&persisted.event, legacy_filter_set, stream_filters) {
            continue;
        }
        matched += 1;
        process_contract_event(
            network,
            contract_id,
            &persisted.event,
            router,
            alert_engine,
            None,
            &mut analytics,
            triggers,
        )?;
    }

    if dashboard {
        println!("{}", analytics.render_dashboard());
    }
    if matched == 0 {
        notifications::warn(
            "No matching persisted events found for this contract/network/filter set.",
        );
    }
    Ok(())
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
fn process_contract_event(
    network: &str,
    contract_id: &str,
    event: &SorobanEvent,
    router: &EventRouter,
    alert_engine: &AlertEngine,
    event_store: Option<&EventStore>,
    analytics: &mut EventAnalytics,
    triggers: &[EventTrigger],
) -> Result<()> {
    let routes = router.route(event);
    let alerts = alert_engine.evaluate(event);

    notifications::success(&format!(
        "Ledger {} event {} [{}]: {}",
        event.ledger, event.id, event.event_type, event.value
    ));

    if !routes.is_empty() {
        notifications::info(&format!("Event routed to: {}", routes.join(", ")));
    }

    for alert in &alerts {
        notifications::alert(&format!(
            "[{}] {} (rule {}) on event {}",
            alert.severity, alert.message, alert.rule_id, event.id
        ));
    }

    for trigger in triggers.iter().filter(|trigger| trigger.matches(event)) {
        notifications::info(&format!(
            "Executing trigger for pattern '{}': {}",
            trigger.pattern, trigger.command
        ));
        if let Err(err) = trigger.execute(network, contract_id, event) {
            notifications::warn(&format!("Trigger failed: {}", err));
        }
    }

    let persisted =
        PersistedEvent::new(network, contract_id, event.clone(), routes, alerts.clone());
    if let Some(store) = event_store {
        store.persist(&persisted)?;
    }
    analytics.record(&persisted);

    Ok(())
}

fn parse_legacy_filter(events_filter: Option<&str>) -> Option<Vec<String>> {
    events_filter.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_lowercase())
            .filter(|x| !x.is_empty())
            .collect()
    })
}

fn build_stream_filters(
    event_type: Option<&str>,
    topic: Option<&str>,
    value: Option<&str>,
) -> EventStreamFilters {
    let mut stream_filters = EventStreamFilters::default();
    if let Some(t) = event_type {
        let normalized = t.trim().to_lowercase();
        if !normalized.is_empty() {
            stream_filters.event_type = Some(normalized);
        }
    }
    if let Some(topic_filter) = topic {
        let segments: Vec<String> = topic_filter
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !segments.is_empty() {
            stream_filters.topic_segments = Some(segments);
        }
    }
    if let Some(value_match) = value {
        let trimmed = value_match.trim();
        if !trimmed.is_empty() {
            stream_filters.value_match = Some(trimmed.to_string());
        }
    }
    stream_filters
}

fn matches_monitor_filters(
    event: &SorobanEvent,
    legacy_filter_set: &Option<Vec<String>>,
    stream_filters: &EventStreamFilters,
) -> bool {
    if let Some(filters) = legacy_filter_set {
        let as_text = event.value.to_string().to_lowercase();
        let topic_text = event.topic.join(",").to_lowercase();
        if !filters
            .iter()
            .any(|f| as_text.contains(f) || topic_text.contains(f))
        {
            return false;
        }
    }

    if let Some(expected_type) = &stream_filters.event_type {
        if event.event_type.to_lowercase() != expected_type.to_lowercase() {
            return false;
        }
    }

    if let Some(value_match) = &stream_filters.value_match {
        if !event
            .value
            .to_string()
            .to_lowercase()
            .contains(&value_match.to_lowercase())
        {
            return false;
        }
    }

    if let Some(segments) = &stream_filters.topic_segments {
        for segment in segments.iter().filter(|segment| segment.as_str() != "*") {
            let needle = segment.to_lowercase();
            let matched = event
                .topic
                .iter()
                .any(|topic| topic.to_lowercase().contains(&needle));
            if !matched {
                return false;
            }
        }
    }

    true
}

async fn monitor_wallet(
    wallet_name: &str,
    threshold: Option<f64>,
    balance_alert: Option<f64>,
    network: &str,
    interval: u64,
) -> Result<()> {
    let cfg = config::load()?;
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == wallet_name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", wallet_name))?;

    let threshold = threshold.unwrap_or(0.0);
    if threshold <= 0.0 && balance_alert.is_none() {
        notifications::warn(
            "No --threshold or --balance-alert provided; will print balance changes only.",
        );
    }

    if let Some(alert_level) = balance_alert {
        if alert_level <= 0.0 {
            anyhow::bail!("--balance-alert must be greater than zero");
        }
        notifications::info(&format!(
            "Watchman enabled: alert when balance drops below {:.7} XLM.",
            alert_level
        ));
    }

    notifications::info(&format!(
        "Monitoring wallet {} ({}) on {}.",
        wallet.name, wallet.public_key, network
    ));

    let mut last_balance: Option<f64> = None;
    let mut low_balance_alerted = false;

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })?;
    }

    while running.load(Ordering::SeqCst) {
        let account = horizon::fetch_account(&wallet.public_key, network).await?;
        let native = account
            .balances
            .iter()
            .find(|b| b.asset_type == "native")
            .and_then(|b| b.balance.parse::<f64>().ok())
            .unwrap_or(0.0);

        if last_balance
            .map(|b| (b - native).abs() > f64::EPSILON)
            .unwrap_or(true)
        {
            notifications::info(&format!("XLM balance: {:.7}", native));
            last_balance = Some(native);
        }

        if threshold > 0.0 && native >= threshold {
            notifications::success(&format!(
                "Threshold met: {:.7} XLM (>= {:.7})",
                native, threshold
            ));
        }

        if let Some(alert_level) = balance_alert {
            if native < alert_level {
                if !low_balance_alerted {
                    notifications::alert(&format!(
                        "Balance {:.7} XLM dropped below watchman threshold {:.7} XLM",
                        native, alert_level
                    ));
                    low_balance_alerted = true;
                }
            } else {
                low_balance_alerted = false;
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(interval.max(1))).await;
    }

    Ok(())
}
