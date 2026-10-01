/// Wire protocol for the StarForge signing agent.
///
/// Every message is a single JSON object terminated by `\n` (newline-delimited
/// JSON, NDJSON). The client sends one [`AgentRequest`] and reads one
/// [`AgentResponse`] per connection — the connection is closed after the
/// response is sent.
///
/// Version field
/// -------------
/// Both request and response carry `"version": 1`. If a future version of the
/// agent introduces breaking changes it will increment this number and the old
/// client can surface a helpful "upgrade the agent" message instead of a parse
/// error.
use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Request
// ---------------------------------------------------------------------------

/// A request sent by the CLI client to the running agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRequest {
    /// Protocol version — must equal [`PROTOCOL_VERSION`].
    #[serde(default = "default_version")]
    pub version: u32,
    /// The operation the client wants the agent to perform.
    #[serde(flatten)]
    pub op: AgentOp,
}

/// All operations the agent understands.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum AgentOp {
    /// Add (or refresh the TTL of) a wallet's key in the agent.
    ///
    /// The client sends the *already-decrypted* Stellar secret key (S…)
    /// together with the public key so the agent can verify integrity.
    /// The plaintext secret is only ever transmitted over the local
    /// socket, which is protected by `0600` permissions.
    AddKey {
        wallet_name: String,
        public_key: String,
        /// Plaintext Stellar secret key (S… 56-char StrKey).
        secret_key: String,
        /// How long (in seconds) this key should stay loaded.
        /// Overrides the agent's default timeout for this key only.
        #[serde(skip_serializing_if = "Option::is_none")]
        ttl_secs: Option<u64>,
    },

    /// Remove a specific wallet's key from the agent immediately.
    RemoveKey { wallet_name: String },

    /// Remove all loaded keys immediately (equivalent to `agent stop` for the
    /// key material, but keeps the agent process running).
    RemoveAll,

    /// Sign `transaction_xdr` (base64 TransactionEnvelope) with the key for
    /// `wallet_name` and return the signed envelope.
    Sign {
        wallet_name: String,
        /// Base64-encoded Stellar `TransactionEnvelope` XDR.
        transaction_xdr: String,
        /// Network passphrase (used in the signature payload hash).
        network_passphrase: String,
        /// Human-readable preview shown to the user before signing when the
        /// agent is running interactively. Empty string = no preview.
        preview: String,
        /// When `true`, require interactive confirmation before signing.
        require_confirmation: bool,
    },

    /// Return agent status: uptime, loaded keys, per-key TTL remaining.
    Status,

    /// Ping — used by `agent status` to check if the agent is alive.
    Ping,

    /// Ask the agent to shut down gracefully (zeroizes all keys first).
    Shutdown,
}

// ---------------------------------------------------------------------------
// Response
// ---------------------------------------------------------------------------

/// Response sent by the agent back to the client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub version: u32,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<AgentResponseData>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<AgentError>,
}

impl AgentResponse {
    pub fn ok(data: AgentResponseData) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            ok: true,
            data: Some(data),
            error: None,
        }
    }

    pub fn err(error: AgentError) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            ok: false,
            data: None,
            error: Some(error),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AgentResponseData {
    Pong,
    KeyAdded { wallet_name: String },
    KeyRemoved { wallet_name: String },
    AllRemoved,
    Signed { signed_xdr: String },
    Status(AgentStatus),
    ShuttingDown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStatus {
    pub version: u32,
    pub pid: u32,
    /// Seconds the agent has been running.
    pub uptime_secs: u64,
    /// Default TTL in seconds for newly loaded keys.
    pub default_timeout_secs: u64,
    /// Currently loaded keys (one entry per wallet).
    pub loaded_keys: Vec<LoadedKeyInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedKeyInfo {
    pub wallet_name: String,
    pub public_key: String,
    /// Seconds remaining before this key expires. `None` = never expires
    /// (only possible if the key was loaded with `ttl_secs = 0`).
    pub ttl_remaining_secs: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: {message}")]
pub struct AgentError {
    pub code: AgentErrorCode,
    pub message: String,
}

impl AgentError {
    pub fn new(code: AgentErrorCode, msg: impl Into<String>) -> Self {
        Self {
            code,
            message: msg.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum AgentErrorCode {
    /// The requested wallet is not currently loaded in the agent.
    KeyNotLoaded,
    /// The provided secret key is not a valid Stellar StrKey.
    InvalidSecretKey,
    /// The provided public key does not match the secret key.
    PublicKeyMismatch,
    /// The agent rejected the signing request (e.g. user declined confirmation).
    SigningRejected,
    /// XDR signing failed (bad envelope or signature error).
    SigningFailed,
    /// Protocol version mismatch between client and agent.
    VersionMismatch,
    /// The agent is shutting down.
    ShuttingDown,
    /// Generic internal error.
    Internal,
}

fn default_version() -> u32 {
    PROTOCOL_VERSION
}
