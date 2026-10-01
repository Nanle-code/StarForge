/// Signing agent client — used by the CLI signing path.
///
/// Connects to the running agent over the Unix socket / Windows named pipe,
/// sends one request, reads one response, and closes the connection.

use anyhow::{Context, Result};
use std::time::Duration;

use crate::agent::proto::{
    AgentError, AgentErrorCode, AgentOp, AgentRequest, AgentResponse, AgentResponseData,
    PROTOCOL_VERSION,
};
use crate::agent::socket::agent_socket_path;

/// Connection timeout for the agent client.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Return `true` if the agent is running and responsive.
pub fn is_running() -> bool {
    let rt = tokio::runtime::Handle::try_current();
    if let Ok(handle) = rt {
        // We're already inside a tokio runtime — use block_in_place.
        tokio::task::block_in_place(|| {
            handle.block_on(ping())
        }).is_ok()
    } else {
        // Spin up a tiny runtime for the ping.
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map(|rt| rt.block_on(ping()).is_ok())
            .unwrap_or(false)
    }
}

/// Ask the running agent to sign `transaction_xdr`.
///
/// Returns `Err` if the agent is not running, the key is not loaded, or the
/// user rejected the confirmation prompt.
pub async fn sign_via_agent(
    wallet_name: &str,
    transaction_xdr: &str,
    network_passphrase: &str,
    preview: &str,
    require_confirmation: bool,
) -> Result<String> {
    let req = AgentRequest {
        version: PROTOCOL_VERSION,
        op: AgentOp::Sign {
            wallet_name: wallet_name.to_string(),
            transaction_xdr: transaction_xdr.to_string(),
            network_passphrase: network_passphrase.to_string(),
            preview: preview.to_string(),
            require_confirmation,
        },
    };

    let resp = send(req).await?;
    match resp.data {
        Some(AgentResponseData::Signed { signed_xdr }) => Ok(signed_xdr),
        _ => Err(resp
            .error
            .map(|e| anyhow::anyhow!("{}", e))
            .unwrap_or_else(|| anyhow::anyhow!("Unexpected agent response"))),
    }
}

/// Tell the agent to load (or refresh) a wallet's key.
pub async fn add_key(
    wallet_name: &str,
    public_key: &str,
    secret_key: &str,
    ttl_secs: Option<u64>,
) -> Result<()> {
    let req = AgentRequest {
        version: PROTOCOL_VERSION,
        op: AgentOp::AddKey {
            wallet_name: wallet_name.to_string(),
            public_key: public_key.to_string(),
            secret_key: secret_key.to_string(),
            ttl_secs,
        },
    };
    let resp = send(req).await?;
    if resp.ok {
        Ok(())
    } else {
        Err(resp
            .error
            .map(|e| anyhow::anyhow!("{}", e))
            .unwrap_or_else(|| anyhow::anyhow!("add_key failed")))
    }
}

/// Tell the agent to remove a single key.
pub async fn remove_key(wallet_name: &str) -> Result<()> {
    let req = AgentRequest {
        version: PROTOCOL_VERSION,
        op: AgentOp::RemoveKey {
            wallet_name: wallet_name.to_string(),
        },
    };
    let resp = send(req).await?;
    if resp.ok {
        Ok(())
    } else {
        Err(resp
            .error
            .map(|e| anyhow::anyhow!("{}", e))
            .unwrap_or_else(|| anyhow::anyhow!("remove_key failed")))
    }
}

/// Fetch the agent's status.
pub async fn get_status() -> Result<crate::agent::proto::AgentStatus> {
    let req = AgentRequest {
        version: PROTOCOL_VERSION,
        op: AgentOp::Status,
    };
    let resp = send(req).await?;
    match resp.data {
        Some(AgentResponseData::Status(s)) => Ok(s),
        _ => Err(resp
            .error
            .map(|e| anyhow::anyhow!("{}", e))
            .unwrap_or_else(|| anyhow::anyhow!("Unexpected status response"))),
    }
}

/// Ask the agent to shut down.
pub async fn shutdown() -> Result<()> {
    let req = AgentRequest {
        version: PROTOCOL_VERSION,
        op: AgentOp::Shutdown,
    };
    // Ignore "connection reset" errors — the agent may close before it replies.
    let _ = send(req).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Low-level send / receive
// ---------------------------------------------------------------------------

/// Ping the agent.  Returns `Ok(())` if the agent is alive.
async fn ping() -> Result<()> {
    let req = AgentRequest {
        version: PROTOCOL_VERSION,
        op: AgentOp::Ping,
    };
    let resp = send(req).await?;
    if resp.ok {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Agent ping failed"))
    }
}

/// Send a single request and read a single response.
async fn send(req: AgentRequest) -> Result<AgentResponse> {
    let socket_path = agent_socket_path()?;

    #[cfg(unix)]
    return send_unix(socket_path, req).await;

    #[cfg(windows)]
    return send_windows(socket_path, req).await;

    #[cfg(not(any(unix, windows)))]
    return Err(anyhow::anyhow!(
        "Signing agent is not supported on this platform"
    ));
}

// ---------------------------------------------------------------------------
// Unix send
// ---------------------------------------------------------------------------

#[cfg(unix)]
async fn send_unix(
    socket_path: crate::agent::socket::AgentSocketPath,
    req: AgentRequest,
) -> Result<AgentResponse> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let stream = tokio::time::timeout(
        CONNECT_TIMEOUT,
        UnixStream::connect(socket_path.as_path()),
    )
    .await
    .context("Timed out connecting to signing agent")?
    .with_context(|| {
        format!(
            "Cannot connect to signing agent at {}\n\
             Is the agent running? Start it with: starforge agent start",
            socket_path.display()
        )
    })?;

    let (reader, mut writer) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);

    let mut msg = serde_json::to_string(&req).context("Serialization failed")?;
    msg.push('\n');
    writer.write_all(msg.as_bytes()).await?;
    writer.flush().await?;

    let mut response_line = String::new();
    buf_reader
        .read_line(&mut response_line)
        .await
        .context("Failed to read agent response")?;

    serde_json::from_str::<AgentResponse>(response_line.trim())
        .context("Failed to parse agent response")
}

// ---------------------------------------------------------------------------
// Windows send
// ---------------------------------------------------------------------------

#[cfg(windows)]
async fn send_windows(
    socket_path: crate::agent::socket::AgentSocketPath,
    req: AgentRequest,
) -> Result<AgentResponse> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::windows::named_pipe::ClientOptions;

    let pipe_name = socket_path.display();

    let stream = tokio::time::timeout(CONNECT_TIMEOUT, async {
        // Retry until the pipe is available (typical Windows named-pipe pattern).
        loop {
            match ClientOptions::new().open(&pipe_name) {
                Ok(s) => break Ok(s),
                Err(e)
                    if e.raw_os_error()
                        == Some(windows_sys::Win32::Foundation::ERROR_PIPE_BUSY as i32) =>
                {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) => break Err(e),
            }
        }
    })
    .await
    .context("Timed out connecting to signing agent")?
    .with_context(|| {
        format!(
            "Cannot connect to signing agent at {}\n\
             Is the agent running? Start it with: starforge agent start",
            pipe_name
        )
    })?;

    let (reader, mut writer) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);

    let mut msg = serde_json::to_string(&req).context("Serialization failed")?;
    msg.push('\n');
    writer.write_all(msg.as_bytes()).await?;
    writer.flush().await?;

    let mut response_line = String::new();
    buf_reader
        .read_line(&mut response_line)
        .await
        .context("Failed to read agent response")?;

    serde_json::from_str::<AgentResponse>(response_line.trim())
        .context("Failed to parse agent response")
}

// ---------------------------------------------------------------------------
// Convenience: sign via agent if running, else return None
// ---------------------------------------------------------------------------

/// Try to sign through the agent.  Returns `None` if the agent is not running
/// or does not have the key loaded.  Returns `Err` only for hard errors
/// (e.g. the user rejected the signing request).
pub async fn try_sign_via_agent(
    wallet_name: &str,
    public_key: &str,
    transaction_xdr: &str,
    network_passphrase: &str,
    preview: &str,
    require_confirmation: bool,
) -> Result<Option<String>> {
    // Fast path: check if the agent socket exists before trying to connect.
    let socket_path = match agent_socket_path() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };

    #[cfg(unix)]
    {
        if !socket_path.as_path().exists() {
            return Ok(None);
        }
    }
    #[cfg(not(unix))]
    let _ = socket_path;

    // Attempt the sign.
    match sign_via_agent(
        wallet_name,
        transaction_xdr,
        network_passphrase,
        preview,
        require_confirmation,
    )
    .await
    {
        Ok(signed) => Ok(Some(signed)),
        Err(e) => {
            // Key not loaded → treat as "agent not available for this wallet"
            let is_key_not_loaded = e
                .downcast_ref::<AgentError>()
                .map(|a| a.code == AgentErrorCode::KeyNotLoaded)
                .unwrap_or(false);
            let msg = e.to_string();
            let is_connect_err = msg.contains("Cannot connect") || msg.contains("Timed out");
            if is_key_not_loaded || is_connect_err {
                Ok(None)
            } else {
                Err(e)
            }
        }
    }
}
