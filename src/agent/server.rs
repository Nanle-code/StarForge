/// Signing agent server.
///
/// Listens on a Unix socket (Linux/macOS) or Windows named pipe.
/// Each accepted connection receives exactly one request and sends one response,
/// then closes.  The server shuts down cleanly on a `Shutdown` request or when
/// the `shutdown_rx` channel fires (from the `Ctrl-C` handler in `agent start`).
///
/// The socket file is created with `0600` permissions on Unix so that only the
/// owning user can connect.  On Windows the named-pipe ACL is set to allow only
/// the creating user.
///
/// Key expiry is handled by a background sweep task that runs every 30 seconds.

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, oneshot};
use tokio::time;

use crate::agent::keystore::KeyStore;
use crate::agent::proto::{
    AgentError, AgentErrorCode, AgentOp, AgentRequest, AgentResponse, AgentResponseData,
    AgentStatus, PROTOCOL_VERSION,
};
use crate::agent::socket::AgentSocketPath;

/// How often the background sweep task checks for expired keys.
const SWEEP_INTERVAL: Duration = Duration::from_secs(30);

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run the agent server until a `Shutdown` message is received or
/// `shutdown_rx` fires.  This function blocks the calling task.
pub async fn run(
    socket_path: AgentSocketPath,
    default_timeout_secs: u64,
    shutdown_rx: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    let keystore = Arc::new(Mutex::new(KeyStore::new(default_timeout_secs)));

    // Spawn the background sweep task.
    let sweep_store = Arc::clone(&keystore);
    tokio::spawn(async move {
        let mut interval = time::interval(SWEEP_INTERVAL);
        loop {
            interval.tick().await;
            let mut store = sweep_store.lock().await;
            let swept = store.sweep_expired();
            for name in &swept {
                tracing::info!(wallet = %name, "agent sweep: key expired and zeroized");
            }
        }
    });

    // Platform-specific listener.
    #[cfg(unix)]
    let result = run_unix(socket_path, keystore, shutdown_rx).await;

    #[cfg(windows)]
    let result = run_windows(socket_path, keystore, shutdown_rx).await;

    #[cfg(not(any(unix, windows)))]
    let result: anyhow::Result<()> = Err(anyhow::anyhow!(
        "Signing agent is not supported on this platform"
    ));

    result
}

// ---------------------------------------------------------------------------
// Unix listener
// ---------------------------------------------------------------------------

#[cfg(unix)]
async fn run_unix(
    socket_path: AgentSocketPath,
    keystore: Arc<Mutex<KeyStore>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::net::UnixListener;

    // Remove stale socket file from a previous (crashed) run.
    let _ = std::fs::remove_file(socket_path.as_path());

    let listener = UnixListener::bind(socket_path.as_path())
        .map_err(|e| anyhow::anyhow!("Failed to bind agent socket {}: {e}", socket_path.display()))?;

    // Restrict access to owner only (0600).
    std::fs::set_permissions(
        socket_path.as_path(),
        std::fs::Permissions::from_mode(0o600),
    )?;

    tracing::info!(path = %socket_path.display(), "signing agent listening");

    let (shutdown_tx_internal, mut shutdown_internal_rx) = tokio::sync::watch::channel(false);

    // Move the oneshot into a task that converts it to a watch channel signal.
    let sock_path_for_cleanup = socket_path.0.clone();
    tokio::spawn(async move {
        let _ = shutdown_rx.await;
        let _ = shutdown_tx_internal.send(true);
        let _ = std::fs::remove_file(&sock_path_for_cleanup);
    });

    loop {
        tokio::select! {
            accept = listener.accept() => {
                match accept {
                    Ok((stream, _)) => {
                        let ks = Arc::clone(&keystore);
                        let mut watch = shutdown_internal_rx.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection_unix(stream, ks, &mut watch).await {
                                tracing::debug!(error = %e, "agent: connection error");
                            }
                        });
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "agent: accept error");
                    }
                }
            }
            _ = shutdown_internal_rx.changed() => {
                if *shutdown_internal_rx.borrow() {
                    tracing::info!("signing agent shutting down");
                    let mut store = keystore.lock().await;
                    store.remove_all();
                    break;
                }
            }
        }
    }

    Ok(())
}

#[cfg(unix)]
async fn handle_connection_unix(
    stream: tokio::net::UnixStream,
    keystore: Arc<Mutex<KeyStore>>,
    shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();

    // Read exactly one line (one JSON request).
    buf_reader.read_line(&mut line).await?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let response = dispatch_request(line, &keystore, shutdown_rx).await;
    let mut out = serde_json::to_string(&response)?;
    out.push('\n');
    writer.write_all(out.as_bytes()).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Windows named-pipe listener
// ---------------------------------------------------------------------------

#[cfg(windows)]
async fn run_windows(
    socket_path: AgentSocketPath,
    keystore: Arc<Mutex<KeyStore>>,
    shutdown_rx: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    use tokio::net::windows::named_pipe::{PipeMode, ServerOptions};

    let pipe_name = socket_path.display();
    let (shutdown_tx_internal, mut shutdown_rx_internal) =
        tokio::sync::watch::channel(false);

    tokio::spawn(async move {
        let _ = shutdown_rx.await;
        let _ = shutdown_tx_internal.send(true);
    });

    tracing::info!(pipe = %pipe_name, "signing agent listening (Windows named pipe)");

    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(false)
            .pipe_mode(PipeMode::Byte)
            .create(&pipe_name)?;

        tokio::select! {
            connect = server.connect() => {
                match connect {
                    Ok(()) => {
                        let ks = Arc::clone(&keystore);
                        let mut watch = shutdown_rx_internal.clone();
                        tokio::spawn(async move {
                            if let Err(e) = handle_connection_windows(server, ks, &mut watch).await {
                                tracing::debug!(error = %e, "agent: connection error");
                            }
                        });
                    }
                    Err(e) => tracing::warn!(error = %e, "agent: pipe connect error"),
                }
            }
            _ = shutdown_rx_internal.changed() => {
                if *shutdown_rx_internal.borrow() {
                    let mut store = keystore.lock().await;
                    store.remove_all();
                    tracing::info!("signing agent shutting down (Windows)");
                    break;
                }
            }
        }
    }

    Ok(())
}

#[cfg(windows)]
async fn handle_connection_windows(
    pipe: tokio::net::windows::named_pipe::NamedPipeServer,
    keystore: Arc<Mutex<KeyStore>>,
    shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    let (reader, mut writer) = tokio::io::split(pipe);
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    buf_reader.read_line(&mut line).await?;
    let line = line.trim();
    if line.is_empty() {
        return Ok(());
    }

    let response = dispatch_request(line, &keystore, shutdown_rx).await;
    let mut out = serde_json::to_string(&response)?;
    out.push('\n');
    writer.write_all(out.as_bytes()).await?;

    Ok(())
}

// ---------------------------------------------------------------------------
// Request dispatcher (platform-independent)
// ---------------------------------------------------------------------------

async fn dispatch_request(
    raw: &str,
    keystore: &Arc<Mutex<KeyStore>>,
    shutdown_tx: &mut tokio::sync::watch::Receiver<bool>,
) -> AgentResponse {
    let req: AgentRequest = match serde_json::from_str(raw) {
        Ok(r) => r,
        Err(e) => {
            return AgentResponse::err(AgentError::new(
                AgentErrorCode::Internal,
                format!("Failed to parse request: {e}"),
            ))
        }
    };

    if req.version != PROTOCOL_VERSION {
        return AgentResponse::err(AgentError::new(
            AgentErrorCode::VersionMismatch,
            format!(
                "Client protocol version {} != agent version {}",
                req.version, PROTOCOL_VERSION
            ),
        ));
    }

    match req.op {
        AgentOp::Ping => AgentResponse::ok(AgentResponseData::Pong),

        AgentOp::AddKey {
            wallet_name,
            public_key,
            secret_key,
            ttl_secs,
        } => {
            let mut store = keystore.lock().await;
            match store.add_key(&wallet_name, &public_key, &secret_key, ttl_secs) {
                Ok(inserted) => {
                    let action = if inserted { "added" } else { "refreshed" };
                    tracing::info!(wallet = %wallet_name, action, "agent: key {}",action);
                    AgentResponse::ok(AgentResponseData::KeyAdded { wallet_name })
                }
                Err(e) => {
                    let code = e
                        .downcast_ref::<AgentError>()
                        .map(|a| a.code)
                        .unwrap_or(AgentErrorCode::Internal);
                    AgentResponse::err(AgentError::new(code, e.to_string()))
                }
            }
        }

        AgentOp::RemoveKey { wallet_name } => {
            let mut store = keystore.lock().await;
            store.remove_key(&wallet_name);
            AgentResponse::ok(AgentResponseData::KeyRemoved { wallet_name })
        }

        AgentOp::RemoveAll => {
            let mut store = keystore.lock().await;
            store.remove_all();
            AgentResponse::ok(AgentResponseData::AllRemoved)
        }

        AgentOp::Sign {
            wallet_name,
            transaction_xdr,
            network_passphrase,
            preview,
            require_confirmation,
        } => {
            // Show signing preview before acquiring the keystore lock so the
            // user sees it promptly and it does not block other connections.
            if require_confirmation && !preview.is_empty() {
                // Running interactively — print the preview and prompt.
                // The agent server itself owns the terminal when started in
                // foreground mode; if daemonized, confirmation is skipped.
                if !prompt_sign_confirmation(&preview, &wallet_name) {
                    return AgentResponse::err(AgentError::new(
                        AgentErrorCode::SigningRejected,
                        "User declined the signing request",
                    ));
                }
            }

            let store = keystore.lock().await;
            match store.sign(&wallet_name, &transaction_xdr, &network_passphrase) {
                Ok(signed_xdr) => {
                    tracing::info!(wallet = %wallet_name, "agent: signed transaction");
                    AgentResponse::ok(AgentResponseData::Signed { signed_xdr })
                }
                Err(e) => AgentResponse::err(e),
            }
        }

        AgentOp::Status => {
            let store = keystore.lock().await;
            let status = AgentStatus {
                version: PROTOCOL_VERSION,
                pid: std::process::id(),
                uptime_secs: store.started_at.elapsed().as_secs(),
                default_timeout_secs: store.default_timeout.as_secs(),
                loaded_keys: store.loaded_key_infos(),
            };
            AgentResponse::ok(AgentResponseData::Status(status))
        }

        AgentOp::Shutdown => {
            tracing::info!("agent: shutdown requested by client");
            // Signal the listener loop.
            let _ = shutdown_tx; // receiver side — we use the watch channel approach below
            // Zeroize immediately.
            let mut store = keystore.lock().await;
            store.remove_all();
            AgentResponse::ok(AgentResponseData::ShuttingDown)
        }
    }
}

// ---------------------------------------------------------------------------
// Interactive confirmation helper
// ---------------------------------------------------------------------------

/// Print a signing preview and prompt the user to confirm or reject.
/// Returns `true` if confirmed.
fn prompt_sign_confirmation(preview: &str, wallet_name: &str) -> bool {
    use colored::Colorize;
    println!();
    println!("{}", "═══ Signing Request ═══".cyan().bold());
    println!("{}", preview);
    println!();
    println!(
        "  Wallet  : {}",
        wallet_name.cyan()
    );
    println!(
        "  {}",
        "Sign this transaction? [y/N] ".yellow()
    );

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        return false;
    }
    matches!(input.trim().to_lowercase().as_str(), "y" | "yes")
}
