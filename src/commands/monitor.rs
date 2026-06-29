use crate::utils::{
    config, horizon, notifications, print as p, soroban,
    stream::{EventStreamFilters, SorobanEventStream},
};
use anyhow::Result;
use clap::Args;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

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

    /// Poll interval in seconds
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
            )
            .await
        }
        (None, Some(wallet_name)) => {
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

async fn monitor_contract(
    contract_id: &str,
    events_filter: Option<&str>,
    event_type: Option<&str>,
    topic: Option<&str>,
    value: Option<&str>,
    network: &str,
    interval: u64,
    follow: bool,
) -> Result<()> {
    config::validate_contract_id(contract_id)?;

    let legacy_filter_set: Option<Vec<String>> = events_filter.map(|s| {
        s.split(',')
            .map(|x| x.trim().to_lowercase())
            .filter(|x| !x.is_empty())
            .collect()
    });

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

    let rpc_url = soroban::rpc_url(network)?;

    notifications::info(&format!("Streaming contract events from {}.", rpc_url));

    let running = Arc::new(AtomicBool::new(true));
    {
        let running = Arc::clone(&running);
        ctrlc::set_handler(move || {
            running.store(false, Ordering::SeqCst);
        })?;
    }

    let mut stream = SorobanEventStream::new(rpc_url, contract_id.to_string())
        .with_poll_interval(interval)
        .with_filters(stream_filters);

    let mut printed_any = false;

    while running.load(Ordering::SeqCst) {
        match stream.next_batch() {
            Ok(batch) => {
                for event in batch {
                    let as_text = event.value.to_string();
                    let topic_text = event.topic.join(",");
                    let matches_legacy = legacy_filter_set.as_ref().is_none_or(|filters| {
                        filters.iter().any(|f| {
                            as_text.to_lowercase().contains(f)
                                || topic_text.to_lowercase().contains(f)
                        })
                    });

                    if matches_legacy {
                        printed_any = true;
                        notifications::success(&format!(
                            "Ledger {} event {}: {}",
                            event.ledger, event.id, as_text
                        ));
                    }
                }

                if !follow {
                    if !printed_any {
                        notifications::warn("No matching events in the latest batch.");
                    }
                    break;
                }
                stream.sleep();
            }
            Err(err) => {
                if !follow && !printed_any {
                    return Err(err);
                }
                notifications::warn(&format!(
                    "Event stream error: {}. Reconnecting with backoff…",
                    err
                ));
                stream.sleep_backoff();
            }
        }
    }

    Ok(())
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

        std::thread::sleep(std::time::Duration::from_secs(interval.max(1)));
    }

    Ok(())
}
