/// `starforge agent` sub-commands.
///
/// | Sub-command | What it does |
/// |-------------|--------------|
/// | `start`     | Starts the signing agent in the foreground (or as a background daemon with `--daemon`). Prompts for the wallet passphrase once, loads the decrypted key, then serves signing requests until the timeout or `Ctrl-C`. |
/// | `stop`      | Sends a `Shutdown` message to the running agent. |
/// | `status`    | Shows uptime, loaded keys, and per-key TTL. |
/// | `add`       | Loads an additional wallet into a running agent. |
/// | `remove`    | Removes a single wallet key from a running agent. |

use crate::agent::client;
use crate::agent::keystore::DEFAULT_TIMEOUT_SECS;
use crate::agent::server;
use crate::agent::socket::{agent_pid_path, agent_socket_path, read_pid_file, write_pid_file};
use crate::utils::{config, crypto, output, print as p};
use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use colored::Colorize;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

// ---------------------------------------------------------------------------
// CLI types
// ---------------------------------------------------------------------------

#[derive(Subcommand)]
pub enum AgentCommands {
    /// Start the signing agent (keeps keys in locked memory with a timeout).
    Start(StartArgs),
    /// Stop the running signing agent and zeroize all loaded keys.
    Stop,
    /// Show agent status: uptime, loaded wallets, and remaining TTL for each key.
    Status,
    /// Load an additional wallet key into the running agent.
    Add(AddArgs),
    /// Remove a wallet key from the running agent (zeroizes immediately).
    Remove(RemoveArgs),
}

#[derive(Args)]
pub struct StartArgs {
    /// Wallet(s) to load on start. Omit to load the default wallet.
    #[arg(long = "wallet", short = 'w', num_args = 0..)]
    pub wallets: Vec<String>,

    /// Key lifetime in seconds (default: 900 = 15 minutes).
    /// Set to 0 for no expiry (keys live until `agent stop`).
    #[arg(long, default_value_t = DEFAULT_TIMEOUT_SECS)]
    pub timeout: u64,

    /// Fork into the background and return immediately.
    /// The child writes its PID to `~/.starforge/agent.pid`.
    #[arg(long)]
    pub daemon: bool,

    /// Require interactive confirmation before every signing request.
    /// Ignored when `--daemon` is active (no terminal).
    #[arg(long)]
    pub confirm: bool,

    /// Do not load any wallets on start — wait for explicit `agent add` calls.
    #[arg(long, conflicts_with = "wallets")]
    pub empty: bool,
}

#[derive(Args)]
pub struct AddArgs {
    /// Wallet name to load into the running agent.
    #[arg(long = "wallet", short = 'w')]
    pub wallet: String,

    /// Override the default TTL for this specific wallet (seconds).
    #[arg(long)]
    pub ttl: Option<u64>,
}

#[derive(Args)]
pub struct RemoveArgs {
    /// Wallet name to remove from the running agent.
    #[arg(long = "wallet", short = 'w')]
    pub wallet: String,
}

// ---------------------------------------------------------------------------
// Dispatch
// ---------------------------------------------------------------------------

pub async fn handle(cmd: AgentCommands) -> Result<()> {
    match cmd {
        AgentCommands::Start(args) => handle_start(args).await,
        AgentCommands::Stop => handle_stop().await,
        AgentCommands::Status => handle_status().await,
        AgentCommands::Add(args) => handle_add(args).await,
        AgentCommands::Remove(args) => handle_remove(args).await,
    }
}

// ---------------------------------------------------------------------------
// start
// ---------------------------------------------------------------------------

async fn handle_start(args: StartArgs) -> Result<()> {
    let emit_json = output::is_json_mode_enabled();

    // Refuse to start a second agent.
    if client::is_running() {
        if emit_json {
            #[derive(serde::Serialize)]
            struct R {
                status: &'static str,
                message: &'static str,
            }
            return output::print_json(&R {
                status: "already_running",
                message: "Signing agent is already running",
            });
        }
        anyhow::bail!(
            "A signing agent is already running.\n\
             Check its status with: starforge agent status\n\
             Stop it with:          starforge agent stop"
        );
    }

    let cfg = config::load()?;
    let socket_path = agent_socket_path()?;
    let pid_path = agent_pid_path()?;

    // Collect the wallets we should preload.
    let wallet_names: Vec<String> = if args.empty {
        vec![]
    } else if args.wallets.is_empty() {
        // Default: load the first configured wallet.
        cfg.wallets
            .first()
            .map(|w| vec![w.name.clone()])
            .unwrap_or_default()
    } else {
        args.wallets.clone()
    };

    // Decrypt each requested wallet's secret key *before* daemonizing so we
    // can prompt on the original terminal.
    let mut loaded: Vec<(String, String, String)> = Vec::new(); // (name, pubkey, secret)
    for name in &wallet_names {
        let wallet = cfg
            .wallets
            .iter()
            .find(|w| &w.name == name)
            .with_context(|| format!("Wallet '{}' not found in config", name))?;

        p::info(&format!("Unlocking wallet '{}'…", name.cyan()));
        let secret = crate::utils::wallet_signer::resolve_local_secret(wallet, name)?;
        loaded.push((name.clone(), wallet.public_key.clone(), secret.to_string()));
    }

    // Daemonize on Unix if requested.
    #[cfg(unix)]
    if args.daemon {
        daemonize()?;
    }

    write_pid_file(&pid_path)?;

    if !emit_json {
        p::header("StarForge Signing Agent");
        p::kv("Socket", &socket_path.display());
        p::kv("Timeout", &format!("{} s", args.timeout));
        p::kv(
            "Wallets",
            &if loaded.is_empty() {
                "(none — waiting for `agent add`)".to_string()
            } else {
                loaded
                    .iter()
                    .map(|(n, _, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            },
        );
        println!();
        p::info("Press Ctrl-C to stop the agent and zeroize all keys.");
        println!();
    }

    // Set up the shutdown channel wired to Ctrl-C.
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let ctrlc_tx = std::sync::Mutex::new(Some(shutdown_tx));
    ctrlc::set_handler(move || {
        if let Ok(mut guard) = ctrlc_tx.lock() {
            if let Some(tx) = guard.take() {
                let _ = tx.send(());
            }
        }
    })
    .context("Failed to set Ctrl-C handler")?;

    // Start the server.
    let server_task = tokio::spawn(server::run(socket_path.clone(), args.timeout, shutdown_rx));

    // Give the server a moment to bind the socket, then add the preloaded keys.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    for (name, pubkey, secret) in &loaded {
        let ttl = if args.timeout == 0 {
            None
        } else {
            Some(args.timeout)
        };
        if let Err(e) = client::add_key(name, pubkey, secret, ttl).await {
            p::warn(&format!("Failed to load wallet '{}': {}", name, e));
        } else if !emit_json {
            p::success(&format!("Loaded wallet '{}' (TTL: {} s)", name.cyan(), args.timeout));
        }
    }

    if emit_json {
        #[derive(serde::Serialize)]
        struct R<'a> {
            status: &'static str,
            socket: String,
            timeout_secs: u64,
            loaded_wallets: Vec<&'a str>,
        }
        output::print_json(&R {
            status: "running",
            socket: socket_path.display(),
            timeout_secs: args.timeout,
            loaded_wallets: loaded.iter().map(|(n, _, _)| n.as_str()).collect(),
        })?;
    }

    // Block until the server exits.
    server_task.await??;

    // Clean up PID file.
    let _ = std::fs::remove_file(&pid_path);
    if !emit_json {
        println!();
        p::success("Signing agent stopped — all keys zeroized.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// stop
// ---------------------------------------------------------------------------

async fn handle_stop() -> Result<()> {
    let emit_json = output::is_json_mode_enabled();

    if !client::is_running() {
        if emit_json {
            #[derive(serde::Serialize)]
            struct R { status: &'static str }
            return output::print_json(&R { status: "not_running" });
        }
        p::warn("No signing agent is running.");
        return Ok(());
    }

    client::shutdown().await?;
    let _ = std::fs::remove_file(agent_pid_path()?);

    if emit_json {
        #[derive(serde::Serialize)]
        struct R { status: &'static str }
        output::print_json(&R { status: "stopped" })?;
    } else {
        p::success("Signing agent stopped — all keys zeroized.");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

async fn handle_status() -> Result<()> {
    let emit_json = output::is_json_mode_enabled();

    if !client::is_running() {
        if emit_json {
            #[derive(serde::Serialize)]
            struct R { running: bool }
            return output::print_json(&R { running: false });
        }
        p::info("Signing agent: not running");
        p::info("Start it with: starforge agent start");
        return Ok(());
    }

    let status = client::get_status().await?;

    if emit_json {
        return output::print_json(&status);
    }

    p::header("StarForge Signing Agent — Status");
    p::kv("PID", &status.pid.to_string());
    p::kv("Uptime", &format_duration(status.uptime_secs));
    p::kv("Default timeout", &format!("{} s", status.default_timeout_secs));
    p::kv(
        "Loaded wallets",
        &status.loaded_keys.len().to_string(),
    );

    if status.loaded_keys.is_empty() {
        println!("  (no wallets loaded)");
    } else {
        println!();
        for key in &status.loaded_keys {
            let ttl = key
                .ttl_remaining_secs
                .map(|s| format!("{} s remaining", s))
                .unwrap_or_else(|| "no expiry".to_string());
            println!(
                "  {} {} ({})",
                "•".cyan(),
                key.wallet_name.bold(),
                ttl.dimmed()
            );
            println!("    Public key: {}", key.public_key.dimmed());
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// add
// ---------------------------------------------------------------------------

async fn handle_add(args: AddArgs) -> Result<()> {
    let emit_json = output::is_json_mode_enabled();

    if !client::is_running() {
        anyhow::bail!(
            "No signing agent is running. Start one with: starforge agent start"
        );
    }

    let cfg = config::load()?;
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == args.wallet)
        .with_context(|| format!("Wallet '{}' not found", args.wallet))?;

    p::info(&format!("Unlocking wallet '{}'…", args.wallet.cyan()));
    let secret = crate::utils::wallet_signer::resolve_local_secret(wallet, &args.wallet)?;

    client::add_key(
        &args.wallet,
        &wallet.public_key,
        &secret,
        args.ttl,
    )
    .await?;

    if emit_json {
        #[derive(serde::Serialize)]
        struct R<'a> { wallet: &'a str }
        output::print_json(&R { wallet: &args.wallet })?;
    } else {
        p::success(&format!(
            "Wallet '{}' loaded into agent{}",
            args.wallet.cyan(),
            args.ttl
                .map(|t| format!(" (TTL: {} s)", t))
                .unwrap_or_default()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// remove
// ---------------------------------------------------------------------------

async fn handle_remove(args: RemoveArgs) -> Result<()> {
    let emit_json = output::is_json_mode_enabled();

    if !client::is_running() {
        anyhow::bail!(
            "No signing agent is running. Start one with: starforge agent start"
        );
    }

    client::remove_key(&args.wallet).await?;

    if emit_json {
        #[derive(serde::Serialize)]
        struct R<'a> { wallet: &'a str }
        output::print_json(&R { wallet: &args.wallet })?;
    } else {
        p::success(&format!(
            "Wallet '{}' removed from agent and zeroized.",
            args.wallet.cyan()
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Unix daemonize
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn daemonize() -> Result<()> {
    use std::os::unix::process::CommandExt;

    // Double-fork pattern:
    // 1. First fork — the parent exits, the child becomes a session leader.
    // 2. Second fork — the grandchild is no longer a session leader and can
    //    never re-acquire a controlling terminal.
    let child = unsafe { libc::fork() };
    if child < 0 {
        anyhow::bail!("fork(1) failed");
    }
    if child > 0 {
        // First parent: print the child PID and exit.
        println!("Signing agent started (PID {})", child);
        std::process::exit(0);
    }

    // First child: become session leader.
    if unsafe { libc::setsid() } < 0 {
        anyhow::bail!("setsid() failed");
    }

    // Second fork.
    let grandchild = unsafe { libc::fork() };
    if grandchild < 0 {
        anyhow::bail!("fork(2) failed");
    }
    if grandchild > 0 {
        std::process::exit(0);
    }

    // Grandchild: redirect stdio to /dev/null.
    use std::fs::OpenOptions;
    let devnull = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")?;
    unsafe {
        libc::dup2(devnull.as_raw_fd(), libc::STDIN_FILENO);
        libc::dup2(devnull.as_raw_fd(), libc::STDOUT_FILENO);
        libc::dup2(devnull.as_raw_fd(), libc::STDERR_FILENO);
    }
    Ok(())
}

#[cfg(unix)]
trait AsRawFd {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd;
}

#[cfg(unix)]
impl AsRawFd for std::fs::File {
    fn as_raw_fd(&self) -> std::os::unix::io::RawFd {
        std::os::unix::io::AsRawFd::as_raw_fd(self)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}
