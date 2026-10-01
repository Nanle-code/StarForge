/// StarForge signing agent — ssh-agent–style key caching daemon.
///
/// Architecture overview
/// ---------------------
/// ```text
///  ┌─────────────────────────────────────┐
///  │  starforge agent start [--timeout N] │  ← starts server, blocks (or forks)
///  └──────────────────────────┬──────────┘
///                             │ Unix socket / Windows named-pipe
///                             ▼
///  ┌─────────────────────────────────────┐
///  │            AgentServer              │
///  │  KeyStore                           │
///  │  ┌───────────────────────────────┐  │
///  │  │  wallet_name → LockedKey      │  │  ← key bytes mlocked, zeroized on
///  │  │  (Ed25519 SigningKey,  expiry) │  │    timeout / stop / drop
///  │  └───────────────────────────────┘  │
///  └──────────────────────────┬──────────┘
///                             │ newline-delimited JSON
///  ┌──────────────────────────▼──────────┐
///  │  AgentClient  (used by sign path)   │
///  │  sign_transaction_xdr_via_agent()   │
///  └─────────────────────────────────────┘
/// ```
///
/// Modules
/// -------
/// - `proto`     — wire types (request / response / error enums)
/// - `keystore`  — locked in-memory key map with zeroization and timeout
/// - `server`    — tokio listener that drives the keystore
/// - `client`    — lightweight client used by the signing path
/// - `socket`    — platform-specific socket path helpers
pub mod client;
pub mod keystore;
pub mod proto;
pub mod server;
pub mod socket;
