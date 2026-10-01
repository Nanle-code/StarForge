//! SEP-10 (Stellar Web Authentication) client and challenge validator.
//!
//! Implements the client half of SEP-10: discover a server's `WEB_AUTH_ENDPOINT`
//! and `SIGNING_KEY` from its `stellar.toml`, request a challenge, validate that
//! challenge strictly, sign it with a local account, and exchange the signed
//! challenge for a session JWT.
//!
//! The validation rules live in [`validate_challenge`] as a pure function over
//! decoded XDR, so a malformed challenge is rejected with a specific
//! [`ChallengeError`] — and can be asserted on in tests — without a server in
//! the loop. The two network calls ([`Sep10Client::fetch_challenge`] and
//! [`Sep10Client::submit_challenge`]) are thin `reqwest` wrappers around them.
//!
//! Reference: <https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0010.md>

use anyhow::{Context, Result};
use base64::engine::general_purpose::{
    STANDARD as BASE64_STANDARD, URL_SAFE_NO_PAD as BASE64_URL_SAFE,
};
use base64::Engine as _;
use ed25519_dalek::{Signature as Ed25519Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::str::FromStr;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use stellar_strkey::ed25519::PrivateKey;
use stellar_xdr::curr::{
    BytesM, DataValue, DecoratedSignature, Hash, Limits, ManageDataOp, Memo, MuxedAccount,
    Operation, OperationBody, Preconditions, PublicKey as XdrPublicKey, ReadXdr, SequenceNumber,
    Signature as XdrSignature, SignatureHint, String64, StringM, TimeBounds, TimePoint,
    Transaction, TransactionEnvelope, TransactionSignaturePayload,
    TransactionSignaturePayloadTaggedTransaction, TransactionV1Envelope, Uint256, VecM, WriteXdr,
};
use thiserror::Error;

/// Network passphrase used when a server neither publishes `NETWORK_PASSPHRASE`
/// in its `stellar.toml` nor returns one with the challenge.
pub const PUBLIC_NETWORK_PASSPHRASE: &str = "Public Global Stellar Network ; September 2015";

/// Number of raw random bytes the challenge nonce must decode to.
pub const NONCE_BYTES: usize = 48;

/// Number of base64 characters a [`NONCE_BYTES`] nonce occupies.
pub const NONCE_ENCODED_LEN: usize = 64;

/// Suffix of the challenge's first Manage Data operation key (`<home domain> auth`).
pub const AUTH_DATA_NAME_SUFFIX: &str = " auth";

/// Data name of the optional operation carrying the server's own web auth domain.
pub const WEB_AUTH_DOMAIN_DATA_NAME: &str = "web_auth_domain";

/// Data name of the optional operation pinning the client's domain.
pub const CLIENT_DOMAIN_DATA_NAME: &str = "client_domain";

/// Default HTTP timeout for the challenge and token endpoints.
const HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Reasons a SEP-10 challenge is rejected by [`validate_challenge`].
///
/// Every variant names the single rule that failed so a caller can tell a
/// malformed challenge from an expired one without decoding the XDR by hand.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChallengeError {
    /// The challenge is not valid base64.
    #[error("challenge is not valid base64: {0}")]
    InvalidBase64(String),
    /// The decoded bytes are not a Stellar transaction envelope.
    #[error("challenge is not a valid XDR transaction envelope: {0}")]
    MalformedXdr(String),
    /// Only v1 (`ENVELOPE_TYPE_TX`) envelopes carry a SEP-10 challenge.
    #[error("challenge must be a v1 transaction envelope, got {0}")]
    UnsupportedEnvelope(&'static str),
    /// The transaction's source account must be the server's `SIGNING_KEY`.
    #[error(
        "challenge source account '{found}' does not match the server signing key '{expected}'"
    )]
    WrongSourceAccount { expected: String, found: String },
    /// A challenge must be unsubmittable, so its sequence number is 0.
    #[error("challenge sequence number must be 0, got {0}")]
    NonZeroSequence(i64),
    /// Time bounds are mandatory: they bound the useful life of the nonce.
    #[error("challenge must carry time bounds, got {0}")]
    MissingTimeBounds(&'static str),
    /// The challenge window is already over.
    #[error("challenge expired at {expired_at} (now {now})")]
    Expired { expired_at: u64, now: u64 },
    /// The challenge is not valid yet.
    #[error("challenge is not valid until {min_time} (now {now})")]
    NotYetValid { min_time: u64, now: u64 },
    /// The time bound window is inverted.
    #[error("challenge time bounds are inverted: min {min_time} > max {max_time}")]
    InvertedTimeBounds { min_time: u64, max_time: u64 },
    /// A challenge carries at least the client's Manage Data operation.
    #[error("challenge must contain at least one operation, found {0}")]
    WrongOperationCount(usize),
    /// The first operation must be the `<home domain> auth` Manage Data.
    #[error("challenge first operation must be ManageData, found {0}")]
    WrongOperationType(&'static str),
    /// The first operation names a different home domain.
    #[error("challenge data name '{found}' does not match the expected '{expected}'")]
    WrongDataName { expected: String, found: String },
    /// The `<home domain> auth` operation must carry a nonce.
    #[error("challenge ManageData operation '{0}' has no value")]
    MissingNonce(String),
    /// The nonce is not the 64 base64 characters (48 raw bytes) SEP-10 requires.
    #[error("challenge nonce must be {NONCE_ENCODED_LEN} base64 characters decoding to {NONCE_BYTES} bytes, got {0} characters")]
    InvalidNonceLength(usize),
    /// The nonce is 64 characters but does not decode to 48 bytes.
    #[error("challenge nonce is not valid base64: {0}")]
    InvalidNonceEncoding(String),
    /// The first operation must be signed for by the account being authenticated.
    #[error(
        "challenge first operation source '{found}' does not match the client account '{expected}'"
    )]
    WrongOperationSource { expected: String, found: String },
    /// An operation other than the first must be a server or client-domain operation.
    #[error("challenge operation {index} must be ManageData, found {operation_type}")]
    UnexpectedOperation {
        index: usize,
        operation_type: &'static str,
    },
    /// Any operation after the first must be signed for by a known account.
    #[error("challenge operation {index} ('{data_name}') has source account '{found}', expected one of {expected}")]
    WrongOperationAccount {
        index: usize,
        data_name: String,
        expected: String,
        found: String,
    },
    /// The `web_auth_domain` operation must name a domain the client contacted.
    #[error(
        "challenge web_auth_domain value '{found}' is not one of the server's domains: {expected}"
    )]
    WrongWebAuthDomain { expected: String, found: String },
    /// The memo must be absent, or the id the client asked for.
    #[error("challenge memo type {0} is not supported by SEP-10 (MEMO_ID only)")]
    UnsupportedMemo(&'static str),
    /// The memo does not match the one the client requested.
    #[error("challenge memo {found} does not match the requested memo {expected}")]
    WrongMemo { expected: u64, found: u64 },
    /// The server's signature is absent or does not verify.
    #[error("challenge is not signed by the server signing key '{0}'")]
    InvalidServerSignature(String),
    /// The signing key published in `stellar.toml` is unusable.
    #[error("invalid server signing key '{key}': {reason}")]
    InvalidSigningKey { key: String, reason: String },
    /// The client's secret key is unusable.
    #[error("invalid client secret key: {0}")]
    InvalidSecretKey(String),
    /// The signed envelope could not be re-encoded.
    #[error("failed to encode the signed challenge: {0}")]
    Encoding(String),
}

/// SEP-10 discovery data for one server, as published in its `stellar.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sep10Server {
    /// Domain that hosts the `stellar.toml` (`example.com`, no scheme).
    pub home_domain: String,
    /// `SIGNING_KEY` — the account that signs challenges.
    pub signing_key: String,
    /// `WEB_AUTH_ENDPOINT` — where challenges are fetched and submitted.
    pub web_auth_endpoint: String,
    /// `NETWORK_PASSPHRASE` — defaults to the public network.
    pub network_passphrase: String,
    /// `WEB_AUTH_DOMAIN`, when the endpoint lives on another domain.
    pub web_auth_domain: Option<String>,
}

impl Sep10Server {
    /// Build the `<home domain> auth` data name the challenge must carry.
    #[must_use]
    pub fn auth_data_name(&self) -> String {
        format!("{}{}", self.home_domain, AUTH_DATA_NAME_SUFFIX)
    }

    /// Domains this client knows the server by, used to validate the
    /// `web_auth_domain` operation's value.
    ///
    /// SEP-10 asks the client to check that value against "the server's domain
    /// that the client requested the challenge from". Depending on how the
    /// anchor publishes itself that is the home domain, the host of
    /// `WEB_AUTH_ENDPOINT`, or the explicit `WEB_AUTH_DOMAIN`; accepting any of
    /// the three still rejects a value naming a domain the client never talked
    /// to.
    #[must_use]
    pub fn known_domains(&self) -> Vec<String> {
        let mut domains = vec![self.home_domain.clone()];
        if let Some(host) = endpoint_host(&self.web_auth_endpoint) {
            if !domains.contains(&host) {
                domains.push(host);
            }
        }
        if let Some(domain) = &self.web_auth_domain {
            if !domains.contains(domain) {
                domains.push(domain.clone());
            }
        }
        domains
    }
}

/// The subset of a `stellar.toml` this client reads.
///
/// SEP-1 writes these keys in upper case; older anchors and every local test
/// fixture in the wild also use lower case, so both are accepted.
#[derive(Debug, Deserialize)]
struct StellarToml {
    #[serde(rename = "SIGNING_KEY", alias = "signing_key")]
    signing_key: Option<String>,
    #[serde(rename = "NETWORK_PASSPHRASE", alias = "network_passphrase")]
    network_passphrase: Option<String>,
    #[serde(rename = "WEB_AUTH_ENDPOINT", alias = "web_auth_endpoint")]
    web_auth_endpoint: Option<String>,
    #[serde(rename = "WEB_AUTH_DOMAIN", alias = "web_auth_domain")]
    web_auth_domain: Option<String>,
}

/// Parse the SEP-10 keys out of a `stellar.toml` body.
///
/// `home_domain` is the domain the document was fetched from; it is *not* read
/// from the document, because a challenge's `<home domain> auth` key is only
/// trustworthy if it matches the domain the client actually queried.
pub fn parse_stellar_toml(home_domain: &str, raw: &str) -> Result<Sep10Server> {
    if home_domain.trim().is_empty() {
        anyhow::bail!("home domain cannot be empty");
    }
    let doc: StellarToml = toml::from_str(raw).context("failed to parse stellar.toml as TOML")?;

    let signing_key = doc
        .signing_key
        .filter(|key| !key.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("stellar.toml has no SIGNING_KEY — SEP-10 is not supported")
        })?;
    // Reject an unusable signing key here rather than mid-flow, so `auth` fails
    // before it starts signing.
    parse_account(&signing_key).map_err(|reason| {
        anyhow::anyhow!(
            "stellar.toml SIGNING_KEY '{}' is invalid: {}",
            signing_key,
            reason
        )
    })?;

    let web_auth_endpoint = doc
        .web_auth_endpoint
        .filter(|url| !url.trim().is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("stellar.toml has no WEB_AUTH_ENDPOINT — SEP-10 is not supported")
        })?;
    let parsed = reqwest::Url::parse(&web_auth_endpoint).map_err(|e| {
        anyhow::anyhow!(
            "stellar.toml WEB_AUTH_ENDPOINT '{}' is not a URL: {}",
            web_auth_endpoint,
            e
        )
    })?;
    let host = parsed.host_str().ok_or_else(|| {
        anyhow::anyhow!(
            "stellar.toml WEB_AUTH_ENDPOINT '{}' has no host",
            web_auth_endpoint
        )
    })?;
    let loopback = is_loopback_host(host);
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && loopback) {
        anyhow::bail!(
            "stellar.toml WEB_AUTH_ENDPOINT '{}' must use https (http is only accepted for loopback hosts)",
            web_auth_endpoint
        );
    }

    let network_passphrase = doc
        .network_passphrase
        .filter(|passphrase| !passphrase.trim().is_empty())
        .unwrap_or_else(|| PUBLIC_NETWORK_PASSPHRASE.to_string());

    Ok(Sep10Server {
        home_domain: home_domain.trim().to_string(),
        signing_key: signing_key.trim().to_string(),
        web_auth_endpoint: web_auth_endpoint.trim().to_string(),
        network_passphrase,
        web_auth_domain: doc
            .web_auth_domain
            .filter(|domain| !domain.trim().is_empty())
            .map(|domain| domain.trim().to_string()),
    })
}

/// SEP-1 well-known URL a `home_domain` publishes its `stellar.toml` at.
///
/// `home_domain` may include a port (`127.0.0.1:8000`) so a local reference
/// server can be exercised. TLS is dropped only for loopback hosts, whose
/// traffic never leaves the machine.
#[must_use]
pub fn stellar_toml_url(home_domain: &str) -> String {
    let domain = home_domain.trim();
    let scheme = if is_loopback_host(host_of(domain)) {
        "http"
    } else {
        "https"
    };
    format!("{scheme}://{domain}/.well-known/stellar.toml")
}

/// Fetch and parse `home_domain`'s `stellar.toml` (SEP-1 discovery).
///
/// This is the first step of `sep10 auth`: the document names the server's
/// `SIGNING_KEY` and `WEB_AUTH_ENDPOINT`, so an anchor that does not publish
/// them is reported as "SEP-10 is not supported" before any challenge is
/// requested.
pub async fn fetch_stellar_toml(home_domain: &str) -> Result<Sep10Server> {
    // #902: the shared factory owns proxy, custom CA bundle and user agent;
    // redirects stay un-followed because the challenge response is signed with
    // a credential the server must not be able to re-route.
    let client = crate::utils::http_client::client_for(
        crate::utils::http_client::HttpClientSettings::from_env()
            .with_timeout(HTTP_TIMEOUT)
            .without_redirects(),
    )
    .context("failed to build the SEP-1 discovery HTTP client")?;
    fetch_stellar_toml_with_client(&client, home_domain).await
}

/// [`fetch_stellar_toml`] against a caller-supplied client, so tests can point
/// discovery at a local reference server.
pub async fn fetch_stellar_toml_with_client(
    client: &reqwest::Client,
    home_domain: &str,
) -> Result<Sep10Server> {
    let domain = home_domain.trim();
    if domain.is_empty() {
        anyhow::bail!("home domain cannot be empty");
    }
    let url = stellar_toml_url(domain);
    let response = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("failed to fetch {url}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .with_context(|| format!("failed to read {url}"))?;
    if !status.is_success() {
        anyhow::bail!("{url} returned HTTP {}", status.as_u16());
    }
    parse_stellar_toml(domain, &body)
}

/// Host portion of a `home_domain`, with any port and IPv6 brackets removed.
fn host_of(home_domain: &str) -> &str {
    let authority = home_domain.rsplit('@').next().unwrap_or(home_domain);
    match authority.strip_prefix('[') {
        Some(rest) => rest.split(']').next().unwrap_or(rest),
        None => authority.split(':').next().unwrap_or(authority),
    }
}

/// A challenge that passed every validation rule in [`validate_challenge`].
#[derive(Debug, Clone)]
pub struct ValidatedChallenge {
    /// The parsed envelope, with the server's signature still attached.
    pub envelope: TransactionEnvelope,
    /// Home domain the challenge was validated against.
    pub home_domain: String,
    /// The `<home domain> auth` data name that was checked.
    pub data_name: String,
    /// Nonce from that operation, still base64 encoded as the server sent it.
    pub nonce: String,
    /// `minTime` of the challenge's time bounds.
    pub min_time: u64,
    /// `maxTime` of the challenge's time bounds.
    pub max_time: u64,
    /// Memo id, when the server echoed the one the client asked for.
    pub memo: Option<u64>,
    /// Account the challenge authenticates.
    pub client_account: String,
    /// Passphrase the challenge will be signed for.
    pub network_passphrase: String,
    /// Account whose key signed the challenge.
    pub server_signing_key: String,
    /// Value of the optional `client_domain` operation.
    pub client_domain: Option<String>,
    /// Value of the optional `web_auth_domain` operation.
    pub web_auth_domain: Option<String>,
    /// Number of operations in the challenge.
    pub operation_count: usize,
}

impl ValidatedChallenge {
    /// One `(label, value)` pair per checked field, for `--verbose` output.
    #[must_use]
    pub fn describe(&self) -> Vec<(String, String)> {
        let mut rows = vec![
            ("home domain".to_string(), self.home_domain.clone()),
            ("client account".to_string(), self.client_account.clone()),
            (
                "server account".to_string(),
                self.server_signing_key.clone(),
            ),
            ("data name".to_string(), self.data_name.clone()),
            ("nonce".to_string(), self.nonce.clone()),
            (
                "time bounds".to_string(),
                format!("{} – {}", self.min_time, self.max_time),
            ),
            ("network".to_string(), self.network_passphrase.clone()),
            ("operations".to_string(), self.operation_count.to_string()),
        ];
        if let Some(memo) = self.memo {
            rows.push(("memo".to_string(), memo.to_string()));
        }
        if let Some(domain) = &self.client_domain {
            rows.push(("client domain".to_string(), domain.clone()));
        }
        if let Some(domain) = &self.web_auth_domain {
            rows.push(("web auth domain".to_string(), domain.clone()));
        }
        rows
    }
}

/// The JSON body the `challenge` endpoint returns.
#[derive(Debug, Clone, Deserialize)]
pub struct ChallengeResponse {
    /// Base64-encoded challenge transaction envelope.
    pub transaction: String,
    /// Passphrase the server will verify the signed challenge against.
    #[serde(default)]
    pub network_passphrase: Option<String>,
}

/// The JSON body the `token` endpoint returns.
#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    token: String,
}

/// The `error` field SEP-10 servers return on failure.
#[derive(Debug, Clone, Deserialize, Default)]
struct ErrorResponse {
    #[serde(default)]
    error: Option<String>,
}

/// One step of the authentication flow, recorded so `--verbose` can replay it.
#[derive(Debug, Clone, Serialize)]
pub struct AuthStep {
    /// Short name of the step (`fetch`, `validate`, `sign`, `submit`).
    pub step: String,
    /// Human-readable detail for that step.
    pub detail: String,
}

/// Everything the flow learned, including the session token.
#[derive(Debug, Clone, Serialize)]
pub struct AuthOutcome {
    /// Account that was authenticated.
    pub account: String,
    /// Home domain the challenge was requested from and validated against.
    pub home_domain: String,
    /// Endpoint the challenge was fetched from and submitted to.
    pub web_auth_endpoint: String,
    /// Network passphrase the challenge was signed for.
    pub network_passphrase: String,
    /// The session JWT.
    pub jwt: String,
    /// Ordered record of the flow.
    pub steps: Vec<AuthStep>,
    /// One `(label, value)` pair per rule the validated challenge satisfied,
    /// from [`ValidatedChallenge::describe`].
    pub checks: Vec<(String, String)>,
}

/// Client for one server's SEP-10 endpoints.
pub struct Sep10Client {
    server: Sep10Server,
    http: reqwest::Client,
}

impl Sep10Client {
    /// Build a client for `server` with the default HTTP timeouts.
    ///
    /// Redirects are *not* followed: the signed challenge and the session token
    /// are bearer credentials, and a `3xx` would re-send them to whatever host
    /// the server names. SEP-10 endpoints are required to answer directly, so a
    /// redirect is reported as an error instead of being chased.
    pub fn new(server: Sep10Server) -> Result<Self> {
        // #902: shared factory (proxy, custom CA bundle, user agent), no
        // redirects.
        let http = crate::utils::http_client::client_for(
            crate::utils::http_client::HttpClientSettings::from_env()
                .with_timeout(HTTP_TIMEOUT)
                .without_redirects(),
        )
        .context("failed to build the SEP-10 HTTP client")?;
        Ok(Self { server, http })
    }

    /// Build a client around an existing `reqwest::Client`.
    ///
    /// Used by tests to point the client at a local reference server.
    #[must_use]
    pub fn with_client(server: Sep10Server, http: reqwest::Client) -> Self {
        Self { server, http }
    }

    /// The discovery data this client was built from.
    #[must_use]
    pub fn server(&self) -> &Sep10Server {
        &self.server
    }

    /// `GET <WEB_AUTH_ENDPOINT>` — request a challenge for `account`.
    pub async fn fetch_challenge(
        &self,
        account: &str,
        memo: Option<u64>,
        client_domain: Option<&str>,
    ) -> Result<ChallengeResponse> {
        parse_account(account).map_err(|reason| {
            anyhow::anyhow!("invalid client account '{}': {}", account, reason)
        })?;
        if memo.is_some() && account.starts_with('M') {
            anyhow::bail!("a memo can only be attached to a G... account, not a muxed account");
        }

        let mut query: Vec<(&str, String)> = vec![
            ("account", account.to_string()),
            ("home_domain", self.server.home_domain.clone()),
        ];
        if let Some(memo) = memo {
            query.push(("memo", memo.to_string()));
        }
        if let Some(domain) = client_domain {
            query.push(("client_domain", domain.to_string()));
        }

        let response = self
            .http
            .get(&self.server.web_auth_endpoint)
            .query(&query)
            .send()
            .await
            .with_context(|| {
                format!(
                    "failed to request a challenge from {}",
                    self.server.web_auth_endpoint
                )
            })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read the challenge response body")?;
        if !status.is_success() {
            anyhow::bail!(
                "challenge endpoint returned {}: {}",
                status.as_u16(),
                server_error_message(&body)
            );
        }

        let parsed: ChallengeResponse =
            serde_json::from_str(&body).context("challenge response is not valid JSON")?;
        if parsed.transaction.trim().is_empty() {
            anyhow::bail!("challenge response did not include a transaction");
        }
        Ok(parsed)
    }

    /// `POST <WEB_AUTH_ENDPOINT>` — exchange a signed challenge for a JWT.
    pub async fn submit_challenge(&self, signed_transaction: &str) -> Result<String> {
        let response = self
            .http
            .post(&self.server.web_auth_endpoint)
            .form(&[("transaction", signed_transaction)])
            .send()
            .await
            .with_context(|| {
                format!(
                    "failed to submit the signed challenge to {}",
                    self.server.web_auth_endpoint
                )
            })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .context("failed to read the token response body")?;
        if !status.is_success() {
            anyhow::bail!(
                "token endpoint returned {}: {}",
                status.as_u16(),
                server_error_message(&body)
            );
        }

        let parsed: TokenResponse =
            serde_json::from_str(&body).context("token response is not valid JSON")?;
        if parsed.token.trim().is_empty() {
            anyhow::bail!("token response did not include a token");
        }
        Ok(parsed.token)
    }

    /// Run the whole flow: fetch, validate, sign, submit.
    ///
    /// `secret_key` is the `S...` key of the account being authenticated. The
    /// challenge is validated against `self.server` before it is signed, so a
    /// server that answers with someone else's challenge cannot get this key to
    /// sign it.
    pub async fn authenticate(
        &self,
        secret_key: &str,
        memo: Option<u64>,
        client_domain: Option<&str>,
    ) -> Result<AuthOutcome> {
        let signing_key = signing_key_from_secret(secret_key)?;
        let account = account_from_signing_key(&signing_key);
        let mut steps = vec![AuthStep {
            step: "discover".to_string(),
            detail: format!(
                "{} via {}",
                self.server.signing_key, self.server.web_auth_endpoint
            ),
        }];

        let challenge = self
            .fetch_challenge(&account, memo, client_domain)
            .await
            .context("failed to fetch a SEP-10 challenge")?;
        steps.push(AuthStep {
            step: "fetch".to_string(),
            detail: format!(
                "challenge for {} ({} bytes of XDR)",
                account,
                challenge.transaction.len()
            ),
        });

        let network_passphrase = match &challenge.network_passphrase {
            Some(passphrase) if passphrase != &self.server.network_passphrase => {
                anyhow::bail!(
                    "server sent network passphrase '{}' but stellar.toml declares '{}'",
                    passphrase,
                    self.server.network_passphrase
                );
            }
            Some(passphrase) => passphrase.clone(),
            None => self.server.network_passphrase.clone(),
        };
        let server = Sep10Server {
            network_passphrase: network_passphrase.clone(),
            ..self.server.clone()
        };

        let validated =
            validate_challenge(&challenge.transaction, &server, &account, memo, unix_now())
                .map_err(|error| anyhow::anyhow!("challenge rejected: {}", error))?;
        steps.push(AuthStep {
            step: "validate".to_string(),
            detail: format!(
                "{} operation(s), nonce {}…, expires {}",
                validated.operation_count,
                truncate(&validated.nonce, 12),
                validated.max_time
            ),
        });

        let signed = sign_challenge(&validated, &signing_key)
            .map_err(|error| anyhow::anyhow!("failed to sign the challenge: {}", error))?;
        steps.push(AuthStep {
            step: "sign".to_string(),
            detail: format!("signed with {}", account),
        });

        let jwt = self
            .submit_challenge(&signed)
            .await
            .context("failed to submit the signed challenge")?;
        steps.push(AuthStep {
            step: "submit".to_string(),
            detail: format!("session token received ({} characters)", jwt.len()),
        });

        Ok(AuthOutcome {
            account,
            home_domain: server.home_domain.clone(),
            web_auth_endpoint: server.web_auth_endpoint.clone(),
            network_passphrase,
            jwt,
            steps,
            checks: validated.describe(),
        })
    }
}

/// Validate a challenge against `server` for `client_account`.
///
/// Implements the client-side checks of SEP-10: v1 envelope, source account
/// equal to the server's `SIGNING_KEY`, sequence number 0, usable time bounds, a
/// single `<home domain> auth` Manage Data operation owned by the client account
/// with a 48-byte nonce, only server/client-domain Manage Data operations
/// afterwards, no non-`MEMO_ID` memo, and a verifying server signature.
///
/// `memo` is the memo the caller asked the server for, when it asked for one.
/// `now` is unix seconds, injected so the time bound checks are testable.
pub fn validate_challenge(
    challenge_xdr: &str,
    server: &Sep10Server,
    client_account: &str,
    memo: Option<u64>,
    now: u64,
) -> Result<ValidatedChallenge, ChallengeError> {
    let envelope = decode_envelope(challenge_xdr)?;

    let TransactionEnvelope::Tx(v1) = &envelope else {
        return Err(ChallengeError::UnsupportedEnvelope(match &envelope {
            TransactionEnvelope::TxV0(_) => "TxV0",
            TransactionEnvelope::TxFeeBump(_) => "TxFeeBump",
            TransactionEnvelope::Tx(_) => unreachable!("checked by the let-else above"),
        }));
    };
    let tx = &v1.tx;

    let expected_source =
        parse_account(&server.signing_key).map_err(|reason| ChallengeError::InvalidSigningKey {
            key: server.signing_key.clone(),
            reason: reason.to_string(),
        })?;
    let expected_source_str = stellar_strkey::ed25519::PublicKey(expected_source).to_string();
    let found_source = account_of(&tx.source_account);
    if found_source != expected_source_str {
        return Err(ChallengeError::WrongSourceAccount {
            expected: server.signing_key.clone(),
            found: found_source,
        });
    }

    if tx.seq_num.0 != 0 {
        return Err(ChallengeError::NonZeroSequence(tx.seq_num.0));
    }

    let time_bounds = match &tx.cond {
        Preconditions::Time(bounds) => bounds.clone(),
        Preconditions::None => {
            return Err(ChallengeError::MissingTimeBounds("Preconditions::None"));
        }
        Preconditions::V2(_) => {
            return Err(ChallengeError::MissingTimeBounds("Preconditions::V2"));
        }
    };
    let (min_time, max_time) = (time_bounds.min_time.0, time_bounds.max_time.0);
    if min_time > max_time {
        return Err(ChallengeError::InvertedTimeBounds { min_time, max_time });
    }
    if max_time <= now {
        return Err(ChallengeError::Expired {
            expired_at: max_time,
            now,
        });
    }
    if min_time > now {
        return Err(ChallengeError::NotYetValid { min_time, now });
    }

    let challenge_memo = match &tx.memo {
        Memo::None => None,
        Memo::Id(id) => Some(*id),
        Memo::Text(_) => return Err(ChallengeError::UnsupportedMemo("MEMO_TEXT")),
        Memo::Hash(_) => return Err(ChallengeError::UnsupportedMemo("MEMO_HASH")),
        Memo::Return(_) => return Err(ChallengeError::UnsupportedMemo("MEMO_RETURN")),
    };
    match (memo, challenge_memo) {
        (Some(expected), Some(found)) if expected != found => {
            return Err(ChallengeError::WrongMemo { expected, found });
        }
        (Some(expected), None) => {
            return Err(ChallengeError::WrongMemo { expected, found: 0 });
        }
        _ => {}
    }

    let operations: Vec<Operation> = tx.operations.to_vec();
    if operations.is_empty() {
        return Err(ChallengeError::WrongOperationCount(0));
    }

    let expected_client =
        parse_account(client_account).map_err(|reason| ChallengeError::WrongOperationSource {
            expected: client_account.to_string(),
            found: reason.to_string(),
        })?;
    let expected_client_str = stellar_strkey::ed25519::PublicKey(expected_client).to_string();

    let expected_data_name = server.auth_data_name();
    let first = &operations[0];
    let (data_name, data_value) = match &first.body {
        OperationBody::ManageData(ManageDataOp {
            data_name,
            data_value,
        }) => (data_name.clone(), data_value.clone()),
        other => {
            return Err(ChallengeError::WrongOperationType(operation_type_name(
                other,
            )));
        }
    };

    let found_data_name = string64_to_string(&data_name);
    if found_data_name != expected_data_name {
        return Err(ChallengeError::WrongDataName {
            expected: expected_data_name,
            found: found_data_name,
        });
    }

    let first_source = operation_account(first);
    if first_source.as_deref() != Some(expected_client_str.as_str()) {
        return Err(ChallengeError::WrongOperationSource {
            expected: client_account.to_string(),
            found: first_source.unwrap_or_else(|| "(missing)".to_string()),
        });
    }

    let nonce = match data_value {
        Some(value) => value,
        None => {
            return Err(ChallengeError::MissingNonce(string64_to_string(&data_name)));
        }
    };
    let nonce_bytes = nonce_bytes(&nonce)?;
    let nonce_encoded = BASE64_STANDARD.encode(&nonce_bytes);

    let known_domains = server.known_domains();
    let mut additional_operations = Vec::new();

    for (index, operation) in operations.iter().enumerate().skip(1) {
        let body = match &operation.body {
            OperationBody::ManageData(body) => body,
            other => {
                return Err(ChallengeError::UnexpectedOperation {
                    index,
                    operation_type: operation_type_name(other),
                });
            }
        };
        let name = string64_to_string(&body.data_name);
        let source = operation_account(operation);
        let source_display = source.clone().unwrap_or_else(|| "(missing)".to_string());

        if name == WEB_AUTH_DOMAIN_DATA_NAME {
            let value = body
                .data_value
                .as_ref()
                .map(string_to_lossy)
                .unwrap_or_default();
            if source.as_deref() != Some(expected_source_str.as_str()) {
                return Err(ChallengeError::WrongOperationAccount {
                    index,
                    data_name: name,
                    expected: server.signing_key.clone(),
                    found: source_display,
                });
            }
            if !known_domains.contains(&value) {
                return Err(ChallengeError::WrongWebAuthDomain {
                    expected: known_domains.join(", "),
                    found: value,
                });
            }
            additional_operations.push(format!("{name} = {value}"));
            continue;
        }

        if name == CLIENT_DOMAIN_DATA_NAME {
            // The value is a client domain account (a `G...` address); the
            // domain host it belongs to is resolved by the server, not here.
            if source.is_none() {
                return Err(ChallengeError::WrongOperationAccount {
                    index,
                    data_name: name,
                    expected: "a client domain account".to_string(),
                    found: source_display,
                });
            }
            additional_operations.push(format!("{name} = {source_display}"));
            continue;
        }

        // Any other operation is reserved for future use and must belong to the
        // server account.
        if source.as_deref() != Some(expected_source_str.as_str()) {
            return Err(ChallengeError::WrongOperationAccount {
                index,
                data_name: name,
                expected: server.signing_key.clone(),
                found: source_display,
            });
        }
        additional_operations.push(format!("{name} (server)"));
    }

    verify_server_signature(&envelope, &server.network_passphrase, &expected_source)
        .map_err(|_| ChallengeError::InvalidServerSignature(server.signing_key.clone()))?;

    let (client_domain, web_auth_domain) =
        additional_operations
            .iter()
            .fold((None, None), |(client, web), entry| {
                if let Some(value) = entry.strip_prefix(&format!("{CLIENT_DOMAIN_DATA_NAME} = ")) {
                    (Some(value.to_string()), web)
                } else if let Some(value) =
                    entry.strip_prefix(&format!("{WEB_AUTH_DOMAIN_DATA_NAME} = "))
                {
                    (client, Some(value.to_string()))
                } else {
                    (client, web)
                }
            });

    Ok(ValidatedChallenge {
        envelope,
        home_domain: server.home_domain.clone(),
        data_name: expected_data_name,
        nonce: nonce_encoded,
        min_time,
        max_time,
        memo: challenge_memo,
        client_account: client_account.to_string(),
        network_passphrase: server.network_passphrase.clone(),
        server_signing_key: server.signing_key.clone(),
        client_domain,
        web_auth_domain,
        operation_count: operations.len(),
    })
}

/// Sign a validated challenge, preserving the server's signature.
///
/// Returns the base64 XDR of the envelope to POST to the token endpoint.
pub fn sign_challenge(
    challenge: &ValidatedChallenge,
    signing_key: &SigningKey,
) -> Result<String, ChallengeError> {
    let TransactionEnvelope::Tx(v1) = &challenge.envelope else {
        return Err(ChallengeError::UnsupportedEnvelope("not Tx"));
    };

    let payload = signature_payload(&v1.tx, &challenge.network_passphrase)?;
    let mut signatures = v1.signatures.to_vec();
    signatures.push(decorate(signing_key, &payload)?);

    let signed = TransactionEnvelope::Tx(TransactionV1Envelope {
        tx: v1.tx.clone(),
        signatures: signature_vec(signatures)?,
    });
    let bytes = signed
        .to_xdr(Limits::none())
        .map_err(|e| ChallengeError::Encoding(e.to_string()))?;
    Ok(BASE64_STANDARD.encode(bytes))
}

/// Sign a challenge with the `S...` secret key of the account being authenticated.
pub fn sign_challenge_with_secret(
    challenge: &ValidatedChallenge,
    secret_key: &str,
) -> Result<String, ChallengeError> {
    let signing_key = signing_key_from_secret(secret_key)?;
    sign_challenge(challenge, &signing_key)
}

/// Derive the `G...` account of an `S...` secret key.
pub fn account_from_secret(secret_key: &str) -> Result<String> {
    Ok(account_from_signing_key(&signing_key_from_secret(
        secret_key,
    )?))
}

/// Current unix time in seconds.
#[must_use]
pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default()
}

// ── internals ────────────────────────────────────────────────────────────────

fn signing_key_from_secret(secret_key: &str) -> Result<SigningKey, ChallengeError> {
    let private = PrivateKey::from_string(secret_key.trim()).map_err(|e| {
        ChallengeError::InvalidSecretKey(format!(
            "'{}' is not a valid S... strkey: {}",
            truncate(secret_key, 6),
            e
        ))
    })?;
    Ok(SigningKey::from_bytes(&private.0))
}

fn account_from_signing_key(signing_key: &SigningKey) -> String {
    stellar_strkey::ed25519::PublicKey(signing_key.verifying_key().to_bytes()).to_string()
}

/// Parse a `G...` account into its raw 32-byte key.
fn parse_account(account: &str) -> Result<[u8; 32], &'static str> {
    let trimmed = account.trim();
    if trimmed.starts_with('M') {
        return Err("muxed accounts (M...) are not supported here");
    }
    let public = stellar_strkey::ed25519::PublicKey::from_string(trimmed)
        .map_err(|_| "expected a G... ed25519 public key")?;
    Ok(public.0)
}

fn is_loopback_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

fn endpoint_host(endpoint: &str) -> Option<String> {
    reqwest::Url::parse(endpoint)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
}

fn decode_envelope(challenge_xdr: &str) -> Result<TransactionEnvelope, ChallengeError> {
    let trimmed = challenge_xdr.trim();
    let bytes = BASE64_STANDARD
        .decode(trimmed)
        .or_else(|_| BASE64_URL_SAFE.decode(trimmed))
        .map_err(|e| ChallengeError::InvalidBase64(e.to_string()))?;
    TransactionEnvelope::from_xdr(bytes, Limits::none())
        .map_err(|e| ChallengeError::MalformedXdr(e.to_string()))
}

fn nonce_bytes(value: &DataValue) -> Result<Vec<u8>, ChallengeError> {
    let raw: Vec<u8> = value.0.as_vec().clone();
    if raw.len() != NONCE_ENCODED_LEN {
        return Err(ChallengeError::InvalidNonceLength(raw.len()));
    }
    let encoded = String::from_utf8_lossy(&raw).to_string();
    let decoded = BASE64_STANDARD
        .decode(encoded.as_bytes())
        .map_err(|e| ChallengeError::InvalidNonceEncoding(e.to_string()))?;
    if decoded.len() != NONCE_BYTES {
        return Err(ChallengeError::InvalidNonceEncoding(format!(
            "decoded to {} bytes, expected {NONCE_BYTES}",
            decoded.len()
        )));
    }
    Ok(decoded)
}

/// Signature payload for a transaction: network id plus the envelope's transaction.
fn signature_payload(
    tx: &Transaction,
    network_passphrase: &str,
) -> Result<Vec<u8>, ChallengeError> {
    let payload = TransactionSignaturePayload {
        network_id: network_id(network_passphrase),
        tagged_transaction: TransactionSignaturePayloadTaggedTransaction::Tx(tx.clone()),
    };
    payload
        .to_xdr(Limits::none())
        .map_err(|e| ChallengeError::Encoding(e.to_string()))
}

fn network_id(network_passphrase: &str) -> Hash {
    Hash(Sha256::digest(network_passphrase.as_bytes()).into())
}

fn decorate(
    signing_key: &SigningKey,
    payload: &[u8],
) -> Result<DecoratedSignature, ChallengeError> {
    let signature = signing_key.sign(payload);
    let hint = hint_of(&signing_key.verifying_key().to_bytes());
    Ok(DecoratedSignature {
        hint: SignatureHint(hint),
        signature: XdrSignature(
            BytesM::try_from(signature.to_bytes().to_vec())
                .map_err(|e| ChallengeError::Encoding(e.to_string()))?,
        ),
    })
}

fn hint_of(public_key: &[u8; 32]) -> [u8; 4] {
    [
        public_key[28],
        public_key[29],
        public_key[30],
        public_key[31],
    ]
}

fn signature_vec(
    signatures: Vec<DecoratedSignature>,
) -> Result<VecM<DecoratedSignature, 20>, ChallengeError> {
    VecM::try_from(signatures).map_err(|e| ChallengeError::Encoding(e.to_string()))
}

/// Check that at least one signature on the envelope verifies against `account`.
fn verify_server_signature(
    envelope: &TransactionEnvelope,
    network_passphrase: &str,
    account: &[u8; 32],
) -> Result<(), ChallengeError> {
    let TransactionEnvelope::Tx(v1) = envelope else {
        return Err(ChallengeError::UnsupportedEnvelope("not Tx"));
    };
    let payload = signature_payload(&v1.tx, network_passphrase)?;
    let verifying_key =
        VerifyingKey::from_bytes(account).map_err(|e| ChallengeError::InvalidSigningKey {
            key: hex::encode(account),
            reason: e.to_string(),
        })?;

    for signature in v1.signatures.iter() {
        let bytes: Vec<u8> = signature.signature.0.as_vec().clone();
        let Ok(raw) = <[u8; 64]>::try_from(bytes.as_slice()) else {
            continue;
        };
        if verifying_key
            .verify_strict(&payload, &Ed25519Signature::from_bytes(&raw))
            .is_ok()
        {
            return Ok(());
        }
    }
    Err(ChallengeError::InvalidServerSignature(hex::encode(account)))
}

fn account_of(account: &MuxedAccount) -> String {
    match account {
        MuxedAccount::Ed25519(Uint256(bytes)) => {
            stellar_strkey::ed25519::PublicKey(*bytes).to_string()
        }
        MuxedAccount::MuxedEd25519(muxed) => stellar_strkey::ed25519::MuxedAccount {
            ed25519: muxed.ed25519.0,
            id: muxed.id,
        }
        .to_string(),
    }
}

/// The `G...`/`M...` account an operation is signed for, when it sets one.
fn operation_account(operation: &Operation) -> Option<String> {
    operation.source_account.as_ref().map(account_of)
}

fn string64_to_string(value: &String64) -> String {
    String::from_utf8_lossy(&value.0).to_string()
}

fn string_to_lossy(value: &DataValue) -> String {
    String::from_utf8_lossy(&value.0).to_string()
}

fn operation_type_name(body: &OperationBody) -> &'static str {
    // Only the shapes a SEP-10 challenge could plausibly carry are named;
    // everything else is reported as a generic operation.
    match body {
        OperationBody::ManageData(_) => "ManageData",
        OperationBody::Payment(_) => "Payment",
        OperationBody::CreateAccount(_) => "CreateAccount",
        OperationBody::SetOptions(_) => "SetOptions",
        OperationBody::ChangeTrust(_) => "ChangeTrust",
        OperationBody::InvokeHostFunction(_) => "InvokeHostFunction",
        _ => "another operation type",
    }
}

fn server_error_message(body: &str) -> String {
    match serde_json::from_str::<ErrorResponse>(body) {
        Ok(parsed) => parsed.error.unwrap_or_else(|| body.trim().to_string()),
        Err(_) => body.trim().to_string(),
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::curr::{BumpSequenceOp, TransactionExt, Uint256};

    const TEST_PASSPHRASE: &str = "Test SDF Network ; September 2015";

    fn signing_key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn strkey(signing_key: &SigningKey) -> String {
        account_from_signing_key(signing_key)
    }

    fn secret(signing_key: &SigningKey) -> String {
        PrivateKey(signing_key.to_bytes()).to_string()
    }

    fn server_for(signing_key: &SigningKey, endpoint: &str) -> Sep10Server {
        Sep10Server {
            home_domain: "testanchor.stellar.org".to_string(),
            signing_key: strkey(signing_key),
            web_auth_endpoint: endpoint.to_string(),
            network_passphrase: TEST_PASSPHRASE.to_string(),
            web_auth_domain: None,
        }
    }

    /// Everything a challenge builder needs; `Default` yields a valid challenge.
    struct ChallengeSpec {
        server_key: SigningKey,
        client_account: String,
        seq_num: i64,
        min_time: u64,
        max_time: u64,
        data_name: String,
        data_value: Option<Vec<u8>>,
        memo: Option<u64>,
        extra_operations: Vec<(String, String, String)>,
        raw_operations: Vec<Operation>,
        sign_with_server: bool,
    }

    impl ChallengeSpec {
        fn new(server_key: SigningKey, client_account: &str) -> Self {
            let now = unix_now();
            Self {
                server_key,
                client_account: client_account.to_string(),
                seq_num: 0,
                min_time: now.saturating_sub(60),
                max_time: now + 900,
                data_name: "testanchor.stellar.org auth".to_string(),
                data_value: Some(BASE64_STANDARD.encode([7u8; NONCE_BYTES]).into_bytes()),
                memo: None,
                extra_operations: Vec::new(),
                raw_operations: Vec::new(),
                sign_with_server: true,
            }
        }

        fn build(&self) -> String {
            let mut operations = Vec::new();
            operations.push(manage_data(
                Some(self.client_account.clone()),
                &self.data_name,
                self.data_value.clone(),
            ));
            for (name, value, source) in &self.extra_operations {
                operations.push(manage_data(
                    Some(source.clone()),
                    name,
                    Some(value.as_bytes().to_vec()),
                ));
            }
            operations.extend(self.raw_operations.iter().cloned());

            let tx = Transaction {
                source_account: MuxedAccount::Ed25519(Uint256(
                    self.server_key.verifying_key().to_bytes(),
                )),
                fee: 100,
                seq_num: SequenceNumber(self.seq_num),
                cond: Preconditions::Time(TimeBounds {
                    min_time: TimePoint(self.min_time),
                    max_time: TimePoint(self.max_time),
                }),
                memo: match self.memo {
                    Some(id) => Memo::Id(id),
                    None => Memo::None,
                },
                operations: VecM::try_from(operations).unwrap(),
                ext: TransactionExt::V0,
            };

            let mut signatures = Vec::new();
            if self.sign_with_server {
                let payload = signature_payload(&tx, TEST_PASSPHRASE).unwrap();
                signatures.push(decorate(&self.server_key, &payload).unwrap());
            }

            let envelope = TransactionEnvelope::Tx(TransactionV1Envelope {
                tx,
                signatures: VecM::try_from(signatures).unwrap(),
            });
            BASE64_STANDARD.encode(envelope.to_xdr(Limits::none()).unwrap())
        }
    }

    fn manage_data(source: Option<String>, name: &str, value: Option<Vec<u8>>) -> Operation {
        Operation {
            source_account: source
                .map(|account| MuxedAccount::Ed25519(Uint256(parse_account(&account).unwrap()))),
            body: OperationBody::ManageData(ManageDataOp {
                data_name: String64(StringM::<64>::from_str(name).unwrap()),
                data_value: value.map(|raw| DataValue(BytesM::try_from(raw).unwrap())),
            }),
        }
    }

    fn valid_challenge(server_key: &SigningKey, client: &str) -> (Sep10Server, String) {
        let server = server_for(server_key, "https://testanchor.stellar.org/auth");
        let challenge = ChallengeSpec::new(server_key.clone(), client).build();
        (server, challenge)
    }

    #[test]
    fn parses_stellar_toml_with_upper_case_keys() {
        let raw = format!(
            "SIGNING_KEY = \"{}\"\nWEB_AUTH_ENDPOINT = \"https://anchor.example.com/auth\"\nNETWORK_PASSPHRASE = \"Test SDF Network ; September 2015\"\n",
            strkey(&signing_key(1))
        );
        let server = parse_stellar_toml("anchor.example.com", &raw).unwrap();
        assert_eq!(server.home_domain, "anchor.example.com");
        assert_eq!(server.web_auth_endpoint, "https://anchor.example.com/auth");
        assert_eq!(server.auth_data_name(), "anchor.example.com auth");
        assert!(server
            .known_domains()
            .contains(&"anchor.example.com".to_string()));
    }

    #[test]
    fn parses_stellar_toml_with_lower_case_keys_and_defaults_passphrase() {
        let key = signing_key(1);
        let raw = format!(
            "signing_key = \"{}\"\nweb_auth_endpoint = \"https://anchor.example.com/auth\"\n",
            strkey(&key)
        );
        let server = parse_stellar_toml("anchor.example.com", &raw).unwrap();
        assert_eq!(server.network_passphrase, PUBLIC_NETWORK_PASSPHRASE);
    }

    #[test]
    fn rejects_stellar_toml_without_signing_key() {
        let err = parse_stellar_toml(
            "anchor.example.com",
            "WEB_AUTH_ENDPOINT = \"https://anchor.example.com/auth\"\n",
        )
        .unwrap_err();
        assert!(err.to_string().contains("SIGNING_KEY"), "got: {err}");
    }

    #[test]
    fn rejects_insecure_web_auth_endpoint_outside_loopback() {
        let key = signing_key(1);
        let raw = format!(
            "SIGNING_KEY = \"{}\"\nWEB_AUTH_ENDPOINT = \"http://anchor.example.com/auth\"\n",
            strkey(&key)
        );
        let err = parse_stellar_toml("anchor.example.com", &raw).unwrap_err();
        assert!(err.to_string().contains("https"), "got: {err}");
    }

    #[test]
    fn allows_loopback_web_auth_endpoint_for_local_testing() {
        let key = signing_key(1);
        let raw = format!(
            "SIGNING_KEY = \"{}\"\nWEB_AUTH_ENDPOINT = \"http://127.0.0.1:8000/auth\"\n",
            strkey(&key)
        );
        let server = parse_stellar_toml("127.0.0.1", &raw).unwrap();
        assert!(server.known_domains().contains(&"127.0.0.1".to_string()));
    }

    #[test]
    fn builds_the_well_known_url_over_https_for_public_domains() {
        assert_eq!(
            stellar_toml_url("anchor.example.com"),
            "https://anchor.example.com/.well-known/stellar.toml"
        );
        // A port is preserved; only the scheme changes for loopback.
        assert_eq!(
            stellar_toml_url("127.0.0.1:8000"),
            "http://127.0.0.1:8000/.well-known/stellar.toml"
        );
        assert_eq!(
            stellar_toml_url("localhost:3000"),
            "http://localhost:3000/.well-known/stellar.toml"
        );
    }

    #[tokio::test]
    async fn fetch_stellar_toml_reads_a_local_reference_document() {
        use mockito::Server;

        let mut mock_server = Server::new_async().await;
        let key = signing_key(1);
        let body = format!(
            "SIGNING_KEY = \"{}\"\nWEB_AUTH_ENDPOINT = \"{}/auth\"\n",
            strkey(&key),
            mock_server.url()
        );
        let mock = mock_server
            .mock("GET", "/.well-known/stellar.toml")
            .with_status(200)
            .with_header("content-type", "text/plain")
            .with_body(body)
            .create_async()
            .await;

        let home_domain = mock_server.url().trim_start_matches("http://").to_string();
        let server = fetch_stellar_toml(&home_domain).await.unwrap();

        assert_eq!(server.home_domain, home_domain);
        assert_eq!(server.signing_key, strkey(&key));
        assert_eq!(
            server.web_auth_endpoint,
            format!("{}/auth", mock_server.url())
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_stellar_toml_reports_a_missing_document() {
        use mockito::Server;

        let mut mock_server = Server::new_async().await;
        let mock = mock_server
            .mock("GET", "/.well-known/stellar.toml")
            .with_status(404)
            .create_async()
            .await;

        let home_domain = mock_server.url().trim_start_matches("http://").to_string();
        let err = fetch_stellar_toml(&home_domain).await.unwrap_err();

        assert!(err.to_string().contains("404"), "got: {err}");
        mock.assert_async().await;
    }

    #[test]
    fn validates_a_well_formed_challenge() {
        let server_key = signing_key(1);
        let client_key = signing_key(2);
        let client = strkey(&client_key);
        let (server, challenge) = valid_challenge(&server_key, &client);

        let validated = validate_challenge(&challenge, &server, &client, None, unix_now()).unwrap();
        assert_eq!(validated.data_name, "testanchor.stellar.org auth");
        assert_eq!(validated.nonce, BASE64_STANDARD.encode([7u8; NONCE_BYTES]));
        assert_eq!(validated.operation_count, 1);
        assert_eq!(validated.client_account, client);
        assert!(validated.describe().iter().any(|(row, _)| row == "nonce"));
    }

    #[test]
    fn rejects_non_zero_sequence_numbers() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.seq_num = 42;

        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert_eq!(err, ChallengeError::NonZeroSequence(42));
    }

    #[test]
    fn rejects_a_different_home_domain() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.data_name = "evil.example.com auth".to_string();

        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert_eq!(
            err,
            ChallengeError::WrongDataName {
                expected: "testanchor.stellar.org auth".to_string(),
                found: "evil.example.com auth".to_string(),
            }
        );
    }

    #[test]
    fn rejects_expired_and_inverted_time_bounds() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");

        let mut spec = ChallengeSpec::new(server_key.clone(), &client);
        spec.max_time = unix_now() - 1;
        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert!(matches!(err, ChallengeError::Expired { .. }), "got {err:?}");

        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.min_time = unix_now() + 600;
        spec.max_time = unix_now() - 600;
        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert!(
            matches!(err, ChallengeError::InvertedTimeBounds { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_a_short_nonce() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.data_value = Some(b"too-short".to_vec());

        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert_eq!(err, ChallengeError::InvalidNonceLength(9));
    }

    #[test]
    fn rejects_a_nonce_that_is_not_base64() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.data_value = Some(vec![b'*'; NONCE_ENCODED_LEN]);

        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert!(
            matches!(err, ChallengeError::InvalidNonceEncoding(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_a_missing_server_signature() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.sign_with_server = false;

        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert_eq!(
            err,
            ChallengeError::InvalidServerSignature(server.signing_key.clone())
        );
    }

    #[test]
    fn rejects_a_challenge_signed_by_the_wrong_server() {
        let client = strkey(&signing_key(2));
        // The challenge is signed by key 9, but the toml advertises key 1.
        let server = server_for(&signing_key(1), "https://testanchor.stellar.org/auth");
        let spec = ChallengeSpec::new(signing_key(9), &client);

        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert!(
            matches!(err, ChallengeError::WrongSourceAccount { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_a_wrong_client_operation_source() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.client_account = strkey(&signing_key(3));

        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert!(
            matches!(err, ChallengeError::WrongOperationSource { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn accepts_a_web_auth_domain_operation_and_rejects_a_foreign_one() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");

        let mut spec = ChallengeSpec::new(server_key.clone(), &client);
        spec.extra_operations.push((
            WEB_AUTH_DOMAIN_DATA_NAME.to_string(),
            "testanchor.stellar.org".to_string(),
            strkey(&server_key),
        ));
        let validated =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap();
        assert_eq!(
            validated.web_auth_domain.as_deref(),
            Some("testanchor.stellar.org")
        );

        let server_key_str = strkey(&server_key);
        let mut spec = ChallengeSpec::new(server_key.clone(), &client);
        spec.extra_operations.push((
            WEB_AUTH_DOMAIN_DATA_NAME.to_string(),
            "evil.example.com".to_string(),
            server_key_str,
        ));
        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert!(
            matches!(err, ChallengeError::WrongWebAuthDomain { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn accepts_reserved_server_manage_data_operations() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let server_key_str = strkey(&server_key);
        let mut spec = ChallengeSpec::new(server_key.clone(), &client);
        // SEP-10 reserves unnamed Manage Data operations owned by the server
        // account for future use; they must be accepted.
        spec.extra_operations.push((
            "unexpected".to_string(),
            "value".to_string(),
            server_key_str,
        ));
        let validated =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap();
        assert_eq!(validated.operation_count, 2);
    }

    #[test]
    fn rejects_a_reserved_operation_owned_by_a_third_party() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.extra_operations.push((
            "reserved".to_string(),
            "value".to_string(),
            strkey(&signing_key(3)),
        ));
        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert!(
            matches!(err, ChallengeError::WrongOperationAccount { .. }),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_a_non_manage_data_operation_in_the_challenge() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.raw_operations.push(Operation {
            source_account: None,
            body: OperationBody::BumpSequence(BumpSequenceOp {
                bump_to: SequenceNumber(1),
            }),
        });
        let err =
            validate_challenge(&spec.build(), &server, &client, None, unix_now()).unwrap_err();
        assert_eq!(
            err,
            ChallengeError::UnexpectedOperation {
                index: 1,
                operation_type: "another operation type",
            }
        );
    }

    #[test]
    fn rejects_a_memo_that_does_not_match_the_request() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        let mut spec = ChallengeSpec::new(server_key, &client);
        spec.memo = Some(99);

        let err =
            validate_challenge(&spec.build(), &server, &client, Some(42), unix_now()).unwrap_err();
        assert_eq!(
            err,
            ChallengeError::WrongMemo {
                expected: 42,
                found: 99
            }
        );
    }

    #[test]
    fn rejects_a_fee_bump_envelope() {
        let server_key = signing_key(1);
        let client = strkey(&signing_key(2));
        let server = server_for(&server_key, "https://testanchor.stellar.org/auth");
        // Envelope type 5 (ENVELOPE_TYPE_TX_FEE_BUMP) with an empty body is
        // enough to prove the client refuses anything but a v1 envelope.
        let fake = BASE64_STANDARD.encode([0, 0, 0, 5, 0, 0, 0, 0]);
        let err = validate_challenge(&fake, &server, &client, None, unix_now()).unwrap_err();
        assert!(
            matches!(err, ChallengeError::MalformedXdr(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn rejects_xdr_that_is_not_base64() {
        let server = server_for(&signing_key(1), "https://testanchor.stellar.org/auth");
        let err = validate_challenge("not base64!!", &server, "G", None, unix_now()).unwrap_err();
        assert!(
            matches!(err, ChallengeError::InvalidBase64(_)),
            "got {err:?}"
        );
    }

    #[test]
    fn signing_adds_a_client_signature_that_verifies() {
        let server_key = signing_key(1);
        let client_key = signing_key(2);
        let client = strkey(&client_key);
        let (server, challenge) = valid_challenge(&server_key, &client);
        let validated = validate_challenge(&challenge, &server, &client, None, unix_now()).unwrap();

        let signed = sign_challenge(&validated, &client_key).unwrap();
        let decoded = decode_envelope(&signed).unwrap();
        let TransactionEnvelope::Tx(v1) = decoded else {
            panic!("signed challenge is not a v1 envelope");
        };
        assert_eq!(v1.signatures.len(), 2, "server + client signatures");

        let payload = signature_payload(&v1.tx, TEST_PASSPHRASE).unwrap();
        let client_signature = v1.signatures.last().unwrap();
        let raw: [u8; 64] = client_signature
            .signature
            .0
            .as_vec()
            .clone()
            .try_into()
            .unwrap();
        client_key
            .verifying_key()
            .verify_strict(&payload, &Ed25519Signature::from_bytes(&raw))
            .expect("client signature must verify");
        // And the server's signature is still valid on the signed envelope.
        assert!(verify_server_signature(
            &TransactionEnvelope::Tx(v1),
            TEST_PASSPHRASE,
            &server_key.verifying_key().to_bytes()
        )
        .is_ok());
    }

    #[test]
    fn derive_account_from_secret_key_matches_the_public_key() {
        let key = signing_key(4);
        assert_eq!(account_from_secret(&secret(&key)).unwrap(), strkey(&key));
    }

    #[tokio::test]
    async fn authenticate_against_a_local_reference_server() {
        use mockito::Server;

        let server_key = signing_key(1);
        let client_key = signing_key(2);
        let client_account = strkey(&client_key);

        let mut mock_server = Server::new_async().await;
        let mut challenge_spec = ChallengeSpec::new(server_key.clone(), &client_account);
        challenge_spec.data_name = "127.0.0.1 auth".to_string();
        challenge_spec.extra_operations.push((
            WEB_AUTH_DOMAIN_DATA_NAME.to_string(),
            "127.0.0.1".to_string(),
            strkey(&server_key),
        ));
        let challenge = challenge_spec.build();

        let challenge_mock = mock_server
            .mock("GET", mockito::Matcher::Any)
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("account".into(), client_account.clone()),
                mockito::Matcher::UrlEncoded("home_domain".into(), "127.0.0.1".to_string()),
            ]))
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "transaction": challenge,
                    "network_passphrase": TEST_PASSPHRASE,
                })
                .to_string(),
            )
            .create_async()
            .await;

        let token_mock = mock_server
            .mock("POST", mockito::Matcher::Any)
            .match_header("content-type", "application/x-www-form-urlencoded")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"token":"header.payload.signature"}"#)
            .create_async()
            .await;

        let server = Sep10Server {
            home_domain: "127.0.0.1".to_string(),
            signing_key: strkey(&server_key),
            web_auth_endpoint: format!("{}/auth", mock_server.url()),
            network_passphrase: TEST_PASSPHRASE.to_string(),
            web_auth_domain: None,
        };

        let outcome = Sep10Client::new(server)
            .unwrap()
            .authenticate(&secret(&client_key), None, None)
            .await
            .unwrap();

        assert_eq!(outcome.account, client_account);
        assert_eq!(outcome.jwt, "header.payload.signature");
        assert_eq!(
            outcome
                .steps
                .iter()
                .map(|step| step.step.as_str())
                .collect::<Vec<_>>(),
            vec!["discover", "fetch", "validate", "sign", "submit"]
        );
        assert!(
            outcome
                .checks
                .iter()
                .any(|(label, value)| label == "home domain" && value == "127.0.0.1"),
            "validated checks must be reported for --verbose: {:?}",
            outcome.checks
        );
        challenge_mock.assert_async().await;
        token_mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_challenge_surfaces_the_server_error_message() {
        use mockito::Server;

        let mut mock_server = Server::new_async().await;
        let mock = mock_server
            .mock("GET", mockito::Matcher::Any)
            .with_status(400)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error":"The provided account is malformed"}"#)
            .create_async()
            .await;

        let server = Sep10Server {
            home_domain: "127.0.0.1".to_string(),
            signing_key: strkey(&signing_key(1)),
            web_auth_endpoint: format!("{}/auth", mock_server.url()),
            network_passphrase: TEST_PASSPHRASE.to_string(),
            web_auth_domain: None,
        };

        let err = Sep10Client::new(server)
            .unwrap()
            .fetch_challenge(&strkey(&signing_key(2)), None, None)
            .await
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("The provided account is malformed"),
            "got: {err}"
        );
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn authenticate_rejects_a_challenge_for_another_account_before_signing() {
        use mockito::Server;

        let server_key = signing_key(1);
        let client_key = signing_key(2);
        let other_client = strkey(&signing_key(5));

        let mut mock_server = Server::new_async().await;
        let challenge = ChallengeSpec::new(server_key.clone(), &other_client).build();
        mock_server
            .mock("GET", mockito::Matcher::Any)
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                serde_json::json!({
                    "transaction": challenge,
                    "network_passphrase": TEST_PASSPHRASE,
                })
                .to_string(),
            )
            .create_async()
            .await;
        let token_mock = mock_server
            .mock("POST", mockito::Matcher::Any)
            .with_status(200)
            .with_body(r#"{"token":"should-not-be-used"}"#)
            .expect(0)
            .create_async()
            .await;

        let server = Sep10Server {
            home_domain: "127.0.0.1".to_string(),
            signing_key: strkey(&server_key),
            web_auth_endpoint: format!("{}/auth", mock_server.url()),
            network_passphrase: TEST_PASSPHRASE.to_string(),
            web_auth_domain: None,
        };

        let err = Sep10Client::new(server)
            .unwrap()
            .authenticate(&secret(&client_key), None, None)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("challenge rejected"), "got: {err}");
        token_mock.assert_async().await;
    }
}
