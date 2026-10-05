/// Platform-specific agent socket path helpers.
///
/// Unix  : `~/.starforge/agent.sock`  (0600 permissions, cleaned up on exit)
/// Windows: `\\.\pipe\starforge-agent-<username>`
use anyhow::{Context, Result};
use std::path::PathBuf;

/// Environment variable that can override the default socket path.
pub const ENV_AGENT_SOCK: &str = "STARFORGE_AGENT_SOCK";

/// Return the path of the agent socket / named-pipe endpoint.
///
/// Check `STARFORGE_AGENT_SOCK` first so tests can redirect to a temp dir.
pub fn agent_socket_path() -> Result<AgentSocketPath> {
    if let Ok(val) = std::env::var(ENV_AGENT_SOCK) {
        return Ok(AgentSocketPath(val.into()));
    }

    #[cfg(unix)]
    {
        let base = dirs::home_dir()
            .context("Cannot determine home directory")?
            .join(".starforge");
        std::fs::create_dir_all(&base)
            .with_context(|| format!("Cannot create {}", base.display()))?;
        Ok(AgentSocketPath(base.join("agent.sock")))
    }

    #[cfg(windows)]
    {
        // Named-pipe path is fixed — multiple users get different pipes because
        // the username is embedded (Windows named pipes are per-machine but
        // ACLs restrict access to the creating user).
        let username = std::env::var("USERNAME").unwrap_or_else(|_| "user".to_string());
        Ok(AgentSocketPath(
            format!(r"\\.\pipe\starforge-agent-{username}").into(),
        ))
    }

    #[cfg(not(any(unix, windows)))]
    {
        anyhow::bail!("Signing agent is not supported on this platform")
    }
}

/// Newtype so the caller can call `.as_path()` or `.as_str()` uniformly.
#[derive(Debug, Clone)]
pub struct AgentSocketPath(pub PathBuf);

impl AgentSocketPath {
    pub fn as_path(&self) -> &std::path::Path {
        &self.0
    }

    /// String representation used for display and named-pipe paths on Windows.
    pub fn display(&self) -> String {
        self.0.display().to_string()
    }
}

/// Return the path where the agent stores its PID file.
pub fn agent_pid_path() -> Result<PathBuf> {
    if let Ok(val) = std::env::var("STARFORGE_AGENT_PID") {
        return Ok(val.into());
    }
    let base = dirs::home_dir()
        .context("Cannot determine home directory")?
        .join(".starforge");
    std::fs::create_dir_all(&base).with_context(|| format!("Cannot create {}", base.display()))?;
    Ok(base.join("agent.pid"))
}

/// Write the current PID to the PID file.
pub fn write_pid_file(path: &std::path::Path) -> Result<()> {
    let pid = std::process::id();
    std::fs::write(path, format!("{pid}\n"))
        .with_context(|| format!("Failed to write PID file {}", path.display()))
}

/// Read PID from the PID file. Returns `None` if the file does not exist.
pub fn read_pid_file(path: &std::path::Path) -> Result<Option<u32>> {
    match std::fs::read_to_string(path) {
        Ok(s) => {
            let pid = s
                .trim()
                .parse::<u32>()
                .with_context(|| format!("Malformed PID file {}", path.display()))?;
            Ok(Some(pid))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("Failed to read PID file {}", path.display())),
    }
}
