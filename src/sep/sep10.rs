//! SEP-10: Stellar Web Authentication.
//!
//! Authenticates a local wallet against a SEP-10 anchor by performing the
//! challenge/response handshake defined in
//! [SEP-10](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0010.md):
//!
//! 1. read `https://<home_domain>/.well-known/stellar.toml` for the anchor's
//!    `WEB_AUTH_ENDPOINT` and `SIGNING_KEY`;
//! 2. fetch the challenge transaction the anchor builds for our account;
//! 3. validate it strictly (envelope type, `WEB_AUTH_DOMAIN`, source account,
//!    sequence number, time bounds, the single `manage_data` operation, the
//!    memo, and the anchor's own signature);
//! 4. sign it with the wallet's secret key and submit it;
//! 5. return the JWT the anchor issues.
//!
//! The client performs real XDR parsing and ed25519 work through the crates
//! already in this workspace (`stellar-xdr`, `stellar-strkey`,
//! `ed25519-dalek`), so a malformed or replayed challenge fails locally with a
//! specific error instead of being forwarded to the anchor.

use base64::{engine::general_purpose, Engine as _};
use ed25519_dalek::{Signature as Ed25519Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use stellar_strkey::ed25519 as strkey;
use stellar_xdr::curr::{
    BytesM, DataValue, DecoratedSignature, Hash, Limits, ManageDataOp, Memo, MuxedAccount,
    Operation, OperationBody, Preconditions, ReadXdr, SequenceNumber, Signature, SignatureHint,
    String64, TimeBounds, TimePoint, Transaction, TransactionEnvelope, TransactionExt,
    TransactionSignaturePayload, TransactionSignaturePayloadTaggedTransaction,
    TransactionV1Envelope, Uint256, VecM, WriteXdr,
};

/// Default stellar.toml location for a home domain (SEP-1).
pub const STELLAR_TOML_PATH: &str = "/.well-known/stellar.toml";

/// Suffix SEP-10 appends to the home domain in the challenge's `manage_data`.
pub const AUTH_DATA_SUFFIX: &str = " auth";

/// A SEP-10 challenge is time-bounded and short-lived; anything longer than this
/// is treated as a malformed challenge rather than accepted on trust.
pub const MAX_CHALLENGE_TIMEOUT_SECS: u64 = 15 * 60;

/// Every way a SEP-10 handshake can fail before the anchor is trusted.
#[derive(Debug, thiserror::Error)]
pub enum Sep10Error {
    #[error("request to {url} failed: {source}")]
    Http {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("{url} returned HTTP {status}: {body}")]
    HttpStatus {
        url: String,
        status: u16,
        body: String,
    },
    #[error("could not parse {url} as stellar.toml: {message}")]
    Toml { url: String, message: String },
    #[error("stellar.toml for '{home_domain}' has no WEB_AUTH_ENDPOINT")]
    MissingWebAuthEndpoint { home_domain: String },
    #[error("stellar.toml for '{home_domain}' has no SIGNING_KEY, which SEP-10 requires")]
    MissingSigningKey { home_domain: String },
    #[error("SIGNING_KEY '{signing_key}' is not a valid Stellar account id")]
    InvalidSigningKey { signing_key: String },
    #[error("WEB_AUTH_DOMAIN '{domain}' does not match the home domain '{home_domain}'")]
    WebAuthDomainMismatch { domain: String, home_domain: String },
    #[error("the anchor response was not valid JSON: {message}")]
    MalformedResponse { message: String },
    #[error("the challenge response contained no transaction")]
    MissingTransaction,
    #[error("challenge is not valid base64 XDR: {message}")]
    MalformedEnvelope { message: String },
    #[error(
        "challenge envelope type {kind} is not supported; SEP-10 requires a transaction (v1) envelope"
    )]
    UnsupportedEnvelope { kind: &'static str },
    #[error("challenge source account is a muxed account; SEP-10 requires the plain G... account")]
    MuxedSourceAccount,
    #[error("challenge source account '{actual}' does not match the authenticating account '{expected}'")]
    SourceAccountMismatch { actual: String, expected: String },
    #[error("challenge sequence number must be 0, got {sequence}")]
    NonZeroSequence { sequence: i64 },
    #[error("challenge has no time bounds; SEP-10 requires a max_time")]
    MissingTimeBounds,
    #[error("challenge preconditions are {kind}; SEP-10 requires time bounds")]
    UnsupportedPreconditions { kind: &'static str },
    #[error("challenge max_time must not be 0")]
    ZeroMaxTime,
    #[error("challenge is too long-lived: max_time gives {timeout} seconds (limit {limit})")]
    ChallengeTimeoutTooLong { timeout: u64, limit: u64 },
    #[error("challenge has expired: max_time {max_time} is in the past (now {now})")]
    ExpiredChallenge { max_time: u64, now: u64 },
    #[error("challenge is not valid yet: min_time {min_time} is in the future (now {now})")]
    ChallengeNotYetValid { min_time: u64, now: u64 },
    #[error("challenge must contain exactly 1 operation, found {operations}")]
    WrongOperationCount { operations: usize },
    #[error("challenge operation {index} is {kind}, expected manage_data")]
    WrongOperation { index: usize, kind: &'static str },
    #[error("challenge manage_data name is '{actual}', expected '{expected}'")]
    WrongHomeDomain { actual: String, expected: String },
    #[error("challenge manage_data operation carries no nonce")]
    MissingNonce,
    #[error("challenge memo must be none, found {kind}")]
    UnexpectedMemo { kind: &'static str },
    #[error("challenge is unsigned; the anchor must sign it with its SIGNING_KEY")]
    MissingAnchorSignature,
    #[error("challenge signature is not valid for SIGNING_KEY {signing_key}")]
    InvalidAnchorSignature { signing_key: String },
    #[error("challenge carries {count} signatures; expected exactly 1 (the anchor's)")]
    UnexpectedSignatures { count: usize },
    #[error("the challenge declares network passphrase '{server}' but '{expected}' was expected")]
    NetworkPassphraseMismatch { server: String, expected: String },
    #[error("the wallet's secret key is not a valid Stellar secret key (expected S...)")]
    InvalidSecretKey,
    #[error("could not build or sign the challenge transaction: {message}")]
    Signing { message: String },
    #[error("the anchor rejected the authentication request: {message}")]
    AuthenticationRejected { message: String },
    #[error("the authentication response contained no token")]
    MissingToken,
}

/// The subset of SEP-1 `stellar.toml` that SEP-10 needs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct StellarToml {
    #[serde(rename = "VERSION", default)]
    pub version: Option<String>,
    #[serde(rename = "NETWORK_PASSPHRASE", default)]
    pub network_passphrase: Option<String>,
    #[serde(rename = "SIGNING_KEY", default)]
    pub signing_key: Option<String>,
    #[serde(rename = "WEB_AUTH_ENDPOINT", default)]
    pub web_auth_endpoint: Option<String>,
    #[serde(rename = "WEB_AUTH_DOMAIN", default)]
    pub web_auth_domain: Option<String>,
}

impl StellarToml {
    /// `WEB_AUTH_ENDPOINT`, or a specific error naming what is missing.
    pub fn web_auth_endpoint(&self, home_domain: &str) -> Result<&str, Sep10Error> {
        self.web_auth_endpoint
            .as_deref()
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .ok_or_else(|| Sep10Error::MissingWebAuthEndpoint {
                home_domain: home_domain.to_string(),
            })
    }

    /// `SIGNING_KEY`, or a specific error naming what is missing.
    pub fn signing_key(&self, home_domain: &str) -> Result<&str, Sep10Error> {
        self.signing_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .ok_or_else(|| Sep10Error::MissingSigningKey {
                home_domain: home_domain.to_string(),
            })
    }
}

/// The raw challenge the anchor served, before validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FetchedChallenge {
    pub transaction: String,
    #[serde(default)]
    pub network_passphrase: Option<String>,
}

/// What the strict validation proved about the challenge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ValidatedChallenge {
    pub source_account: String,
    pub home_domain: String,
    pub data_name: String,
    pub nonce: String,
    pub min_time: u64,
    pub max_time: u64,
    pub seconds_remaining: u64,
    pub anchor_signing_key: String,
}

/// Claims decoded from the JWT the anchor issues.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct JwtClaims {
    #[serde(default)]
    pub sub: Option<String>,
    #[serde(default)]
    pub iss: Option<String>,
    #[serde(default)]
    pub iat: Option<u64>,
    #[serde(default)]
    pub exp: Option<u64>,
    #[serde(default)]
    pub jti: Option<String>,
}

/// Decode (not verify) a JWT payload so the CLI can show who authenticated and
/// until when. Signature verification is the resource server's job.
pub fn decode_jwt_claims(token: &str) -> Option<JwtClaims> {
    let payload = token.split('.').nth(1)?;
    let bytes = general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?;
    serde_json::from_slice(&bytes).ok()
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
struct ChallengeResponse {
    transaction: Option<String>,
    #[serde(default)]
    network_passphrase: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    token: Option<String>,
    #[serde(default)]
    error: Option<String>,
}

/// SEP-10 client for one anchor.
#[derive(Debug, Clone)]
pub struct Sep10Client {
    home_domain: String,
    network_passphrase: String,
    toml_url_override: Option<String>,
    http: reqwest::Client,
}

impl Sep10Client {
    pub fn new(home_domain: &str, network_passphrase: &str) -> Result<Self, Sep10Error> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|source| Sep10Error::Http {
                url: "client".to_string(),
                source,
            })?;
        Ok(Self {
            home_domain: home_domain.trim().trim_end_matches('/').to_string(),
            network_passphrase: network_passphrase.to_string(),
            toml_url_override: None,
            http,
        })
    }

    /// Point the client at a specific stellar.toml. Used by the local
    /// reference-server tests, and available for anchors that do not serve the
    /// SEP-1 well-known path.
    pub fn with_toml_url(mut self, url: impl Into<String>) -> Self {
        self.toml_url_override = Some(url.into());
        self
    }

    /// Override the HTTP timeout, mostly so tests do not wait 15s to fail.
    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self, Sep10Error> {
        self.http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|source| Sep10Error::Http {
                url: "client".to_string(),
                source,
            })?;
        Ok(self)
    }

    pub fn home_domain(&self) -> &str {
        &self.home_domain
    }

    pub fn toml_url(&self) -> String {
        match &self.toml_url_override {
            Some(url) => url.clone(),
            None => format!("https://{}{}", self.home_domain, STELLAR_TOML_PATH),
        }
    }

    /// The passphrase the anchor operates on: its stellar.toml wins, since the
    /// challenge must be hashed with the anchor's network.
    pub fn network_passphrase_for(&self, toml: &StellarToml) -> String {
        toml.network_passphrase
            .as_deref()
            .map(str::trim)
            .filter(|passphrase| !passphrase.is_empty())
            .unwrap_or(&self.network_passphrase)
            .to_string()
    }

    pub async fn load_stellar_toml(&self) -> Result<StellarToml, Sep10Error> {
        let url = self.toml_url();
        let response = self
            .http
            .get(&url)
            .header("Accept", "text/plain, application/toml, */*")
            .send()
            .await
            .map_err(|source| Sep10Error::Http {
                url: url.clone(),
                source,
            })?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(Sep10Error::HttpStatus {
                url,
                status: status.as_u16(),
                body: body.chars().take(200).collect(),
            });
        }

        let body = response.text().await.map_err(|source| Sep10Error::Http {
            url: url.clone(),
            source,
        })?;
        let toml: StellarToml = toml::from_str(&body).map_err(|error| Sep10Error::Toml {
            url,
            message: error.to_string(),
        })?;

        toml.web_auth_endpoint(&self.home_domain)?;
        let signing_key = toml.signing_key(&self.home_domain)?;
        strkey::PublicKey::from_string(signing_key).map_err(|_| Sep10Error::InvalidSigningKey {
            signing_key: signing_key.to_string(),
        })?;

        Ok(toml)
    }

    /// Ask the anchor for a challenge transaction for `account`.
    pub async fn fetch_challenge(
        &self,
        toml: &StellarToml,
        account: &str,
    ) -> Result<FetchedChallenge, Sep10Error> {
        let endpoint = toml.web_auth_endpoint(&self.home_domain)?;
        let url = format!(
            "{}?account={}&home_domain={}",
            endpoint.trim_end_matches('?'),
            urlencoding::encode(account),
            urlencoding::encode(&self.home_domain)
        );

        let response = self
            .http
            .get(&url)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|source| Sep10Error::Http {
                url: url.clone(),
                source,
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|source| Sep10Error::Http {
            url: url.clone(),
            source,
        })?;

        if !status.is_success() {
            return Err(Sep10Error::AuthenticationRejected {
                message: error_message(&body)
                    .unwrap_or_else(|| format!("HTTP {}", status.as_u16())),
            });
        }

        let parsed: ChallengeResponse =
            serde_json::from_str(&body).map_err(|error| Sep10Error::MalformedResponse {
                message: error.to_string(),
            })?;

        let transaction = parsed.transaction.ok_or(Sep10Error::MissingTransaction)?;
        if let Some(server_passphrase) = parsed.network_passphrase.as_deref() {
            let expected = self.network_passphrase_for(toml);
            if server_passphrase != expected {
                return Err(Sep10Error::NetworkPassphraseMismatch {
                    server: server_passphrase.to_string(),
                    expected,
                });
            }
        }

        Ok(FetchedChallenge {
            transaction,
            network_passphrase: parsed.network_passphrase,
        })
    }

    /// Validate a challenge against SEP-10 and return the parsed envelope plus
    /// a summary of what was checked. `now` is injectable so time-bound rules
    /// can be tested without waiting.
    pub fn validate_challenge(
        &self,
        toml: &StellarToml,
        challenge_xdr: &str,
        account: &str,
        now: u64,
    ) -> Result<(TransactionV1Envelope, ValidatedChallenge), Sep10Error> {
        // An anchor must not claim another anchor's web auth domain: the signed
        // challenge is bound to the domain we asked.
        if let Some(domain) = toml
            .web_auth_domain
            .as_deref()
            .map(str::trim)
            .filter(|domain| !domain.is_empty())
        {
            if !domain.eq_ignore_ascii_case(&self.home_domain) {
                return Err(Sep10Error::WebAuthDomainMismatch {
                    domain: domain.to_string(),
                    home_domain: self.home_domain.clone(),
                });
            }
        }

        let envelope = envelope_from_base64(challenge_xdr)?;
        let envelope = match envelope {
            TransactionEnvelope::Tx(envelope) => envelope,
            TransactionEnvelope::TxV0(_) => {
                return Err(Sep10Error::UnsupportedEnvelope {
                    kind: "transaction (v0)",
                })
            }
            TransactionEnvelope::TxFeeBump(_) => {
                return Err(Sep10Error::UnsupportedEnvelope { kind: "fee bump" })
            }
        };

        // Source account must be the plain account that is authenticating.
        let source_account = match &envelope.tx.source_account {
            MuxedAccount::Ed25519(Uint256(key)) => strkey::PublicKey(*key).to_string(),
            MuxedAccount::MuxedEd25519(_) => return Err(Sep10Error::MuxedSourceAccount),
        };
        if source_account != account {
            return Err(Sep10Error::SourceAccountMismatch {
                actual: source_account,
                expected: account.to_string(),
            });
        }

        if envelope.tx.seq_num.0 != 0 {
            return Err(Sep10Error::NonZeroSequence {
                sequence: envelope.tx.seq_num.0,
            });
        }

        let (min_time, max_time) = match &envelope.tx.cond {
            Preconditions::Time(bounds) => (bounds.min_time.0, bounds.max_time.0),
            Preconditions::None => return Err(Sep10Error::MissingTimeBounds),
            Preconditions::V2(_) => {
                return Err(Sep10Error::UnsupportedPreconditions {
                    kind: "preconditions v2",
                })
            }
        };

        if max_time == 0 {
            return Err(Sep10Error::ZeroMaxTime);
        }
        if max_time <= now {
            return Err(Sep10Error::ExpiredChallenge { max_time, now });
        }
        if min_time > now {
            return Err(Sep10Error::ChallengeNotYetValid { min_time, now });
        }
        // A challenge that outlives the SEP-10 window is a replay risk.
        let timeout = max_time - now;
        if timeout > MAX_CHALLENGE_TIMEOUT_SECS {
            return Err(Sep10Error::ChallengeTimeoutTooLong {
                timeout,
                limit: MAX_CHALLENGE_TIMEOUT_SECS,
            });
        }

        if envelope.tx.operations.len() != 1 {
            return Err(Sep10Error::WrongOperationCount {
                operations: envelope.tx.operations.len(),
            });
        }
        let operation = envelope
            .tx
            .operations
            .first()
            .expect("length checked above");
        let data_op = match &operation.body {
            OperationBody::ManageData(op) => op,
            other => {
                return Err(Sep10Error::WrongOperation {
                    index: 0,
                    kind: operation_kind(other),
                })
            }
        };

        // SEP-10 home domains are ASCII, so the plain string is enough to compare.
        let data_name = data_op.data_name.to_string();
        let expected_name = format!("{}{}", self.home_domain.to_lowercase(), AUTH_DATA_SUFFIX);
        if data_name.to_lowercase() != expected_name {
            return Err(Sep10Error::WrongHomeDomain {
                actual: data_name,
                expected: expected_name,
            });
        }

        // The nonce is opaque to the client; hex is the unambiguous way to show
        // it in the report without assuming it is printable text.
        let nonce_bytes = data_op
            .data_value
            .as_ref()
            .map(|value| value.to_vec())
            .unwrap_or_default();
        if nonce_bytes.is_empty() {
            return Err(Sep10Error::MissingNonce);
        }
        let nonce = hex::encode(&nonce_bytes);

        match &envelope.tx.memo {
            Memo::None => {}
            other => {
                return Err(Sep10Error::UnexpectedMemo {
                    kind: memo_kind(other),
                })
            }
        }

        // The anchor signs the challenge so the client can trust what it signs.
        let signing_key = toml.signing_key(&self.home_domain)?.to_string();
        if envelope.signatures.is_empty() {
            return Err(Sep10Error::MissingAnchorSignature);
        }
        if envelope.signatures.len() > 1 {
            return Err(Sep10Error::UnexpectedSignatures {
                count: envelope.signatures.len(),
            });
        }
        let passphrase = self.network_passphrase_for(toml);
        verify_anchor_signature(&envelope, &signing_key, &passphrase)?;

        let validated = ValidatedChallenge {
            source_account,
            home_domain: self.home_domain.clone(),
            data_name,
            nonce,
            min_time,
            max_time,
            seconds_remaining: max_time.saturating_sub(now),
            anchor_signing_key: signing_key,
        };

        Ok((envelope, validated))
    }

    /// Sign the validated challenge with the wallet's secret key and return the
    /// base64 XDR the anchor expects.
    pub fn sign_challenge(
        &self,
        toml: &StellarToml,
        envelope: &TransactionV1Envelope,
        secret_key: &str,
    ) -> Result<String, Sep10Error> {
        let passphrase = self.network_passphrase_for(toml);
        let signed = sign_envelope(envelope, secret_key, &passphrase)?;
        envelope_to_base64(&signed)
    }

    /// Exchange the signed challenge for a JWT.
    pub async fn submit_challenge(
        &self,
        toml: &StellarToml,
        signed_xdr: &str,
    ) -> Result<String, Sep10Error> {
        let endpoint = toml.web_auth_endpoint(&self.home_domain)?.to_string();
        let response = self
            .http
            .post(&endpoint)
            .header("Accept", "application/json")
            .form(&[("transaction", signed_xdr)])
            .send()
            .await
            .map_err(|source| Sep10Error::Http {
                url: endpoint.clone(),
                source,
            })?;

        let status = response.status();
        let body = response.text().await.map_err(|source| Sep10Error::Http {
            url: endpoint.clone(),
            source,
        })?;

        let parsed: TokenResponse = serde_json::from_str(&body).unwrap_or(TokenResponse {
            token: None,
            error: Some(body.chars().take(200).collect()),
        });

        if !status.is_success() {
            return Err(Sep10Error::AuthenticationRejected {
                message: parsed
                    .error
                    .or_else(|| error_message(&body))
                    .unwrap_or_else(|| format!("HTTP {}", status.as_u16())),
            });
        }

        parsed.token.ok_or(Sep10Error::MissingToken)
    }

    /// Run the whole handshake in one call. The CLI drives the individual steps
    /// instead so it can print them; this is the programmatic entry point.
    pub async fn authenticate(
        &self,
        account: &str,
        secret_key: &str,
    ) -> Result<Authentication, Sep10Error> {
        let toml = self.load_stellar_toml().await?;
        let challenge = self.fetch_challenge(&toml, account).await?;
        let (envelope, validated) =
            self.validate_challenge(&toml, &challenge.transaction, account, unix_now())?;
        let signed_xdr = self.sign_challenge(&toml, &envelope, secret_key)?;
        let token = self.submit_challenge(&toml, &signed_xdr).await?;

        Ok(Authentication {
            token,
            toml,
            validated,
            signed_xdr,
        })
    }
}

/// Everything one successful SEP-10 handshake produced.
#[derive(Debug, Clone)]
pub struct Authentication {
    pub token: String,
    pub toml: StellarToml,
    pub validated: ValidatedChallenge,
    pub signed_xdr: String,
}

/// Whether `public_key` signed the transaction in `xdr`.
///
/// `Err` means the XDR or the public key could not be read; `Ok(false)` means
/// the envelope parsed but carries no valid signature from that key. Together
/// with [`signature_count`] this is what an anchor uses to accept a submission,
/// and what a developer uses to check who really signed a challenge.
pub fn verify_signature(
    xdr: &str,
    public_key: &str,
    network_passphrase: &str,
) -> Result<bool, Sep10Error> {
    match envelope_from_base64(xdr)? {
        TransactionEnvelope::Tx(envelope) => {
            Ok(verify_anchor_signature(&envelope, public_key, network_passphrase).is_ok())
        }
        other => Err(Sep10Error::UnsupportedEnvelope {
            kind: envelope_kind(&other),
        }),
    }
}

/// Number of signatures carried by a signed challenge transaction.
///
/// After a successful handshake this is the anchor's signature plus our own,
/// which the CLI reports so the user can see the signature actually landed.
pub fn signature_count(xdr: &str) -> Result<usize, Sep10Error> {
    let envelope = envelope_from_base64(xdr)?;
    let signatures = match envelope {
        TransactionEnvelope::Tx(envelope) => envelope.signatures,
        TransactionEnvelope::TxV0(envelope) => envelope.signatures,
        TransactionEnvelope::TxFeeBump(envelope) => envelope.signatures,
    };
    Ok(signatures.len())
}

/// Sign an envelope's transaction with a Stellar secret key, leaving any
/// existing signatures in place, exactly as SEP-10 clients and anchors do.
fn sign_envelope(
    envelope: &TransactionV1Envelope,
    secret_key: &str,
    network_passphrase: &str,
) -> Result<TransactionV1Envelope, Sep10Error> {
    let secret = strkey::PrivateKey::from_string(secret_key.trim())
        .map_err(|_| Sep10Error::InvalidSecretKey)?;
    let signing_key = SigningKey::from_bytes(&secret.0);
    let public_key = signing_key.verifying_key().to_bytes();

    let hash = transaction_hash(envelope, network_passphrase)?;
    let signature = signing_key.sign(&hash).to_bytes();

    let mut signatures: Vec<DecoratedSignature> = envelope.signatures.to_vec();
    signatures.push(DecoratedSignature {
        hint: SignatureHint([
            public_key[28],
            public_key[29],
            public_key[30],
            public_key[31],
        ]),
        signature: Signature(BytesM::try_from(signature.to_vec()).map_err(signing_error)?),
    });

    Ok(TransactionV1Envelope {
        tx: envelope.tx.clone(),
        signatures: VecM::try_from(signatures).map_err(signing_error)?,
    })
}

/// Build the *unsigned* challenge transaction for `client_account`, the way a
/// reference SEP-10 server does: sequence number 0, time bounds, no memo, and a
/// single `manage_data` operation named `<home_domain> auth`.
///
/// Kept separate from [`build_reference_challenge`] so tests can build a
/// deliberately malformed challenge and sign it like an anchor would.
fn build_challenge_transaction(
    client_account: &str,
    home_domain: &str,
    nonce: &[u8],
    min_time: u64,
    max_time: u64,
) -> Result<TransactionV1Envelope, Sep10Error> {
    let client_public = strkey::PublicKey::from_string(client_account).map_err(|_| {
        Sep10Error::InvalidSigningKey {
            signing_key: client_account.to_string(),
        }
    })?;

    let data_name = String64::try_from(format!("{}{}", home_domain, AUTH_DATA_SUFFIX).into_bytes())
        .map_err(signing_error)?;

    Ok(TransactionV1Envelope {
        tx: Transaction {
            source_account: MuxedAccount::Ed25519(Uint256(client_public.0)),
            fee: 100,
            seq_num: SequenceNumber(0),
            cond: Preconditions::Time(TimeBounds {
                min_time: TimePoint(min_time),
                max_time: TimePoint(max_time),
            }),
            memo: Memo::None,
            operations: VecM::try_from(vec![Operation {
                source_account: None,
                body: OperationBody::ManageData(ManageDataOp {
                    data_name,
                    data_value: Some(DataValue(
                        BytesM::try_from(nonce.to_vec()).map_err(signing_error)?,
                    )),
                }),
            }])
            .map_err(signing_error)?,
            ext: TransactionExt::V0,
        },
        signatures: VecM::try_from(Vec::new()).map_err(signing_error)?,
    })
}

/// Build and sign a challenge transaction the way a reference SEP-10 server
/// does. Used by the local reference server in the tests, and useful to
/// reproduce a peer's output when debugging.
#[allow(clippy::too_many_arguments)]
pub fn build_reference_challenge(
    server_signing_key: &str,
    client_account: &str,
    home_domain: &str,
    nonce: &[u8],
    min_time: u64,
    max_time: u64,
    network_passphrase: &str,
) -> Result<String, Sep10Error> {
    let envelope =
        build_challenge_transaction(client_account, home_domain, nonce, min_time, max_time)?;
    let signed = sign_envelope(&envelope, server_signing_key, network_passphrase)?;
    envelope_to_base64(&signed)
}

fn signing_error(error: impl std::fmt::Display) -> Sep10Error {
    Sep10Error::Signing {
        message: error.to_string(),
    }
}

fn error_message(body: &str) -> Option<String> {
    serde_json::from_str::<TokenResponse>(body)
        .ok()
        .and_then(|parsed| parsed.error)
        .or_else(|| {
            let trimmed = body.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.chars().take(200).collect())
            }
        })
}

/// stellar.toml and challenge transactions carry base64 XDR. The workspace
/// pins `stellar-xdr` without its `base64` feature, so the encode/decode step
/// is explicit here.
fn envelope_to_base64(envelope: &TransactionV1Envelope) -> Result<String, Sep10Error> {
    // Encode through the union: a bare `TransactionV1Envelope` serialises only
    // its fields, so the bytes would be missing the `EnvelopeType` discriminant
    // the wire format starts with.
    let bytes = TransactionEnvelope::Tx(envelope.clone())
        .to_xdr(Limits::none())
        .map_err(signing_error)?;
    Ok(general_purpose::STANDARD.encode(bytes))
}

fn envelope_from_base64(xdr: &str) -> Result<TransactionEnvelope, Sep10Error> {
    let bytes = general_purpose::STANDARD
        .decode(xdr.trim())
        .map_err(|error| Sep10Error::MalformedEnvelope {
            message: error.to_string(),
        })?;
    TransactionEnvelope::from_xdr(bytes, Limits::none()).map_err(|error| {
        Sep10Error::MalformedEnvelope {
            message: error.to_string(),
        }
    })
}

fn envelope_kind(envelope: &TransactionEnvelope) -> &'static str {
    match envelope {
        TransactionEnvelope::Tx(_) => "transaction",
        TransactionEnvelope::TxV0(_) => "transaction (v0)",
        TransactionEnvelope::TxFeeBump(_) => "fee bump",
    }
}

fn memo_kind(memo: &Memo) -> &'static str {
    match memo {
        Memo::None => "none",
        Memo::Text(_) => "text",
        Memo::Id(_) => "id",
        Memo::Hash(_) => "hash",
        Memo::Return(_) => "return",
    }
}

fn operation_kind(operation: &OperationBody) -> &'static str {
    match operation {
        OperationBody::CreateAccount(_) => "create_account",
        OperationBody::Payment(_) => "payment",
        OperationBody::PathPaymentStrictReceive(_) => "path_payment_strict_receive",
        OperationBody::ManageSellOffer(_) => "manage_sell_offer",
        OperationBody::CreatePassiveSellOffer(_) => "create_passive_sell_offer",
        OperationBody::SetOptions(_) => "set_options",
        OperationBody::ChangeTrust(_) => "change_trust",
        OperationBody::AllowTrust(_) => "allow_trust",
        OperationBody::AccountMerge(_) => "account_merge",
        OperationBody::Inflation => "inflation",
        OperationBody::ManageData(_) => "manage_data",
        OperationBody::BumpSequence(_) => "bump_sequence",
        OperationBody::ManageBuyOffer(_) => "manage_buy_offer",
        OperationBody::PathPaymentStrictSend(_) => "path_payment_strict_send",
        OperationBody::CreateClaimableBalance(_) => "create_claimable_balance",
        OperationBody::ClaimClaimableBalance(_) => "claim_claimable_balance",
        OperationBody::BeginSponsoringFutureReserves(_) => "begin_sponsoring_future_reserves",
        OperationBody::EndSponsoringFutureReserves => "end_sponsoring_future_reserves",
        OperationBody::RevokeSponsorship(_) => "revoke_sponsorship",
        OperationBody::Clawback(_) => "clawback",
        OperationBody::ClawbackClaimableBalance(_) => "clawback_claimable_balance",
        OperationBody::SetTrustLineFlags(_) => "set_trust_line_flags",
        OperationBody::LiquidityPoolDeposit(_) => "liquidity_pool_deposit",
        OperationBody::LiquidityPoolWithdraw(_) => "liquidity_pool_withdraw",
        OperationBody::InvokeHostFunction(_) => "invoke_host_function",
        OperationBody::ExtendFootprintTtl(_) => "extend_footprint_ttl",
        OperationBody::RestoreFootprint(_) => "restore_footprint",
    }
}

/// `sha256(network_id || envelope_type || transaction)`, the byte string every
/// Stellar signature covers.
fn transaction_hash(
    envelope: &TransactionV1Envelope,
    network_passphrase: &str,
) -> Result<[u8; 32], Sep10Error> {
    let digest = Sha256::digest(network_passphrase.as_bytes());
    let mut network_id = [0u8; 32];
    network_id.copy_from_slice(&digest);

    let payload = TransactionSignaturePayload {
        network_id: Hash(network_id),
        tagged_transaction: TransactionSignaturePayloadTaggedTransaction::Tx(envelope.tx.clone()),
    };
    let xdr = payload.to_xdr(Limits::none()).map_err(signing_error)?;
    Ok(Sha256::digest(xdr).into())
}

fn verify_anchor_signature(
    envelope: &TransactionV1Envelope,
    signing_key: &str,
    network_passphrase: &str,
) -> Result<(), Sep10Error> {
    let public =
        strkey::PublicKey::from_string(signing_key).map_err(|_| Sep10Error::InvalidSigningKey {
            signing_key: signing_key.to_string(),
        })?;
    let verifier =
        VerifyingKey::from_bytes(&public.0).map_err(|_| Sep10Error::InvalidSigningKey {
            signing_key: signing_key.to_string(),
        })?;

    let hash = transaction_hash(envelope, network_passphrase)?;
    for decorated in envelope.signatures.iter() {
        let Ok(bytes): Result<[u8; 64], _> = decorated.signature.to_vec().try_into() else {
            continue;
        };
        if verifier
            .verify_strict(&hash, &Ed25519Signature::from_bytes(&bytes))
            .is_ok()
        {
            return Ok(());
        }
    }

    Err(Sep10Error::InvalidAnchorSignature {
        signing_key: signing_key.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";
    const HOME_DOMAIN: &str = "anchor.example.com";
    const SERVER_SEED: [u8; 32] = [7u8; 32];
    const CLIENT_SEED: [u8; 32] = [9u8; 32];
    const IMPOSTOR_SEED: [u8; 32] = [11u8; 32];

    fn secret_for(seed: [u8; 32]) -> String {
        strkey::PrivateKey(seed).to_string()
    }

    fn public_for(seed: [u8; 32]) -> String {
        let signing = SigningKey::from_bytes(&seed);
        strkey::PublicKey(signing.verifying_key().to_bytes()).to_string()
    }

    fn client() -> Sep10Client {
        Sep10Client::new(HOME_DOMAIN, TESTNET_PASSPHRASE).expect("client")
    }

    fn toml_with(endpoint: &str, signing_key: &str) -> StellarToml {
        StellarToml {
            version: Some("1.0.0".to_string()),
            network_passphrase: Some(TESTNET_PASSPHRASE.to_string()),
            signing_key: Some(signing_key.to_string()),
            web_auth_endpoint: Some(endpoint.to_string()),
            web_auth_domain: Some(HOME_DOMAIN.to_string()),
        }
    }

    /// A challenge as a well-behaved anchor would build it (`now`-relative).
    fn challenge_for(account: &str, now: u64) -> String {
        build_reference_challenge(
            &secret_for(SERVER_SEED),
            account,
            HOME_DOMAIN,
            &[3u8; 48],
            now.saturating_sub(60),
            now + 300,
            TESTNET_PASSPHRASE,
        )
        .expect("build challenge")
    }

    fn parse(xdr: &str) -> TransactionV1Envelope {
        match envelope_from_base64(xdr).expect("parse envelope") {
            TransactionEnvelope::Tx(envelope) => envelope,
            other => panic!("expected a v1 transaction envelope, got {other:?}"),
        }
    }

    fn encode(envelope: &TransactionV1Envelope) -> String {
        envelope_to_base64(envelope).expect("encode envelope")
    }

    /// Re-sign a (possibly tampered) challenge the way a real anchor would.
    fn resign(envelope: &TransactionV1Envelope, seed: [u8; 32]) -> String {
        encode(&sign_envelope(envelope, &secret_for(seed), TESTNET_PASSPHRASE).expect("sign"))
    }

    fn expect_rejected(toml: &StellarToml, xdr: &str, now: u64) -> Sep10Error {
        client()
            .validate_challenge(toml, xdr, &public_for(CLIENT_SEED), now)
            .expect_err("challenge should have been rejected")
    }

    #[test]
    fn validates_a_well_formed_challenge() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let (_envelope, validated) = client()
            .validate_challenge(
                &toml,
                &challenge_for(&public_for(CLIENT_SEED), now),
                &public_for(CLIENT_SEED),
                now,
            )
            .expect("challenge should validate");

        assert_eq!(validated.source_account, public_for(CLIENT_SEED));
        assert_eq!(validated.data_name, format!("{HOME_DOMAIN} auth"));
        assert_eq!(validated.seconds_remaining, 300);
        assert_eq!(validated.min_time, now - 60);
        assert_eq!(validated.max_time, now + 300);
        assert_eq!(validated.nonce, hex::encode([3u8; 48]));
        assert_eq!(validated.anchor_signing_key, public_for(SERVER_SEED));
    }

    #[test]
    fn signing_a_challenge_produces_a_verifiable_signature() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let (envelope, _validated) = client()
            .validate_challenge(
                &toml,
                &challenge_for(&public_for(CLIENT_SEED), now),
                &public_for(CLIENT_SEED),
                now,
            )
            .expect("challenge should validate");

        let signed = client()
            .sign_challenge(&toml, &envelope, &secret_for(CLIENT_SEED))
            .expect("sign");
        assert_eq!(signature_count(&signed).expect("count"), 2);

        // The wallet's own signature must be valid over the challenge hash.
        let signed_envelope = parse(&signed);
        let client_public = public_for(CLIENT_SEED);
        assert!(
            verify_anchor_signature(&signed_envelope, &client_public, TESTNET_PASSPHRASE).is_ok(),
            "the wallet signature must verify against its own public key"
        );
    }

    #[test]
    fn signs_only_its_own_accounts_challenge() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let err = expect_rejected(&toml, &challenge_for(&public_for(IMPOSTOR_SEED), now), now);

        assert!(
            matches!(err, Sep10Error::SourceAccountMismatch { .. }),
            "expected a source account mismatch, got {err:?}"
        );
        assert!(err
            .to_string()
            .contains("does not match the authenticating account"));
    }

    #[test]
    fn rejects_challenges_signed_by_the_wrong_key() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let forged = challenge_for_signed_with(&public_for(CLIENT_SEED), now, IMPOSTOR_SEED);
        let err = expect_rejected(&toml, &forged, now);

        assert!(
            matches!(err, Sep10Error::InvalidAnchorSignature { .. }),
            "expected an anchor signature failure, got {err:?}"
        );
    }

    #[test]
    fn rejects_unsigned_challenges() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let unsigned = encode(
            &build_challenge_transaction(
                &public_for(CLIENT_SEED),
                HOME_DOMAIN,
                &[4u8; 48],
                now - 60,
                now + 300,
            )
            .expect("build unsigned challenge"),
        );
        let err = expect_rejected(&toml, &unsigned, now);

        assert!(
            matches!(err, Sep10Error::MissingAnchorSignature),
            "expected a missing signature error, got {err:?}"
        );
    }

    #[test]
    fn rejects_challenges_with_a_second_signature() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let mut envelope = parse(&challenge_for(&public_for(CLIENT_SEED), now));
        envelope = sign_envelope(&envelope, &secret_for(CLIENT_SEED), TESTNET_PASSPHRASE)
            .expect("add a second signature");
        let err = expect_rejected(&toml, &encode(&envelope), now);

        assert!(
            matches!(err, Sep10Error::UnexpectedSignatures { count: 2 }),
            "expected an unexpected signatures error, got {err:?}"
        );
    }

    #[test]
    fn rejects_expired_challenges() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let challenge = challenge_for(&public_for(CLIENT_SEED), now);
        let err = expect_rejected(&toml, &challenge, now + 301);

        assert!(
            matches!(err, Sep10Error::ExpiredChallenge { .. }),
            "expected an expiry error, got {err:?}"
        );
    }

    #[test]
    fn rejects_challenges_that_outlive_the_sep10_window() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let long_lived = build_reference_challenge(
            &secret_for(SERVER_SEED),
            &public_for(CLIENT_SEED),
            HOME_DOMAIN,
            &[5u8; 48],
            now,
            now + MAX_CHALLENGE_TIMEOUT_SECS + 60,
            TESTNET_PASSPHRASE,
        )
        .expect("build challenge");
        let err = expect_rejected(&toml, &long_lived, now);

        assert!(
            matches!(err, Sep10Error::ChallengeTimeoutTooLong { .. }),
            "expected a timeout error, got {err:?}"
        );
    }

    #[test]
    fn rejects_challenges_with_a_non_zero_sequence_number() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let mut envelope = parse(&challenge_for(&public_for(CLIENT_SEED), now));
        envelope.tx.seq_num = SequenceNumber(42);
        let err = expect_rejected(&toml, &resign(&envelope, SERVER_SEED), now);

        assert!(
            matches!(err, Sep10Error::NonZeroSequence { sequence: 42 }),
            "expected a sequence number error, got {err:?}"
        );
    }

    #[test]
    fn rejects_challenges_carrying_a_memo() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let mut envelope = parse(&challenge_for(&public_for(CLIENT_SEED), now));
        envelope.tx.memo = Memo::Id(7);
        let err = expect_rejected(&toml, &resign(&envelope, SERVER_SEED), now);

        assert!(
            matches!(err, Sep10Error::UnexpectedMemo { kind: "id" }),
            "expected a memo error, got {err:?}"
        );
    }

    #[test]
    fn rejects_challenges_without_manage_data() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let mut envelope = parse(&challenge_for(&public_for(CLIENT_SEED), now));
        envelope.tx.operations = VecM::try_from(vec![Operation {
            source_account: None,
            body: OperationBody::AccountMerge(MuxedAccount::Ed25519(Uint256([1u8; 32]))),
        }])
        .expect("operations");
        let err = expect_rejected(&toml, &resign(&envelope, SERVER_SEED), now);

        assert!(
            matches!(
                err,
                Sep10Error::WrongOperation {
                    index: 0,
                    kind: "account_merge"
                }
            ),
            "expected an operation type error, got {err:?}"
        );
    }

    #[test]
    fn rejects_challenges_for_another_home_domain() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let other_domain = build_reference_challenge(
            &secret_for(SERVER_SEED),
            &public_for(CLIENT_SEED),
            "evil.example.com",
            &[6u8; 48],
            now - 60,
            now + 300,
            TESTNET_PASSPHRASE,
        )
        .expect("build challenge");
        let err = expect_rejected(&toml, &other_domain, now);

        match err {
            Sep10Error::WrongHomeDomain { actual, expected } => {
                assert_eq!(actual, "evil.example.com auth");
                assert_eq!(expected, format!("{HOME_DOMAIN} auth"));
            }
            other => panic!("expected a home domain error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_a_mismatched_web_auth_domain() {
        let now = 1_700_000_000;
        let mut toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        toml.web_auth_domain = Some("impostor.example.com".to_string());
        let err = expect_rejected(&toml, &challenge_for(&public_for(CLIENT_SEED), now), now);

        assert!(
            matches!(err, Sep10Error::WebAuthDomainMismatch { .. }),
            "expected a web auth domain error, got {err:?}"
        );
    }

    #[test]
    fn rejects_a_network_passphrase_the_anchor_does_not_use() {
        let now = 1_700_000_000;
        let mut toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        toml.network_passphrase =
            Some("Public Global Stellar Network ; September 2015".to_string());
        let err = expect_rejected(&toml, &challenge_for(&public_for(CLIENT_SEED), now), now);

        assert!(
            matches!(err, Sep10Error::InvalidAnchorSignature { .. }),
            "a challenge hashed for another network must not verify, got {err:?}"
        );
    }

    #[test]
    fn rejects_malformed_challenge_xdr() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let err = expect_rejected(&toml, "not-xdr-at-all", now);

        assert!(
            matches!(err, Sep10Error::MalformedEnvelope { .. }),
            "expected a malformed envelope error, got {err:?}"
        );
    }

    #[test]
    fn rejects_stellar_toml_without_the_sep10_fields() {
        let mut no_endpoint =
            toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        no_endpoint.web_auth_endpoint = None;
        let err = no_endpoint
            .web_auth_endpoint(HOME_DOMAIN)
            .expect_err("missing endpoint should fail");
        assert!(matches!(err, Sep10Error::MissingWebAuthEndpoint { .. }));

        let mut no_signing_key =
            toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        no_signing_key.signing_key = None;
        let err = no_signing_key
            .signing_key(HOME_DOMAIN)
            .expect_err("missing signing key should fail");
        assert!(matches!(err, Sep10Error::MissingSigningKey { .. }));
    }

    #[test]
    fn rejects_secret_keys_that_are_not_strkeys() {
        let now = 1_700_000_000;
        let toml = toml_with("https://anchor.example.com/auth", &public_for(SERVER_SEED));
        let (envelope, _validated) = client()
            .validate_challenge(
                &toml,
                &challenge_for(&public_for(CLIENT_SEED), now),
                &public_for(CLIENT_SEED),
                now,
            )
            .expect("challenge should validate");

        let err = client()
            .sign_challenge(&toml, &envelope, "not-a-secret-key")
            .expect_err("an invalid secret key must be rejected");
        assert!(matches!(err, Sep10Error::InvalidSecretKey));
    }

    #[test]
    fn decodes_jwt_claims_without_verifying_them() {
        let payload = general_purpose::URL_SAFE_NO_PAD
            .encode(r#"{"sub":"GABC","iss":"anchor.example.com","exp":123}"#);
        let token = format!("header.{payload}.signature");

        let claims = decode_jwt_claims(&token).expect("claims");
        assert_eq!(claims.sub.as_deref(), Some("GABC"));
        assert_eq!(claims.iss.as_deref(), Some("anchor.example.com"));
        assert_eq!(claims.exp, Some(123));
        assert!(decode_jwt_claims("not-a-jwt").is_none());
    }

    /// A challenge built by a key that is *not* the anchor's advertised
    /// `SIGNING_KEY`: the client must refuse it even though it is otherwise
    /// perfectly formed.
    fn challenge_for_signed_with(account: &str, now: u64, seed: [u8; 32]) -> String {
        build_reference_challenge(
            &secret_for(seed),
            account,
            HOME_DOMAIN,
            &[8u8; 48],
            now - 60,
            now + 300,
            TESTNET_PASSPHRASE,
        )
        .expect("build challenge")
    }

    // ── Local reference server ──────────────────────────────────────────────
    //
    // A minimal SEP-10 anchor over plain HTTP on an ephemeral port, used to
    // prove the whole handshake works against a live server rather than a
    // hand-built challenge. It serves the SEP-1 stellar.toml, issues a real
    // challenge, and only issues a JWT once it has verified the wallet's
    // signature over that challenge.

    struct ReferenceAnchor {
        home_domain: String,
        toml_url: String,
        authorized: Arc<AtomicBool>,
    }

    impl ReferenceAnchor {
        async fn start(client_account: &str) -> Self {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind anchor");
            let home_domain = format!(
                "127.0.0.1:{}",
                listener.local_addr().expect("anchor address").port()
            );
            let authorized = Arc::new(AtomicBool::new(false));

            let context = Arc::new(AnchorContext {
                home_domain: home_domain.clone(),
                client_account: client_account.to_string(),
                authorized: authorized.clone(),
                challenges: AtomicU64::new(0),
            });

            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    let context = context.clone();
                    tokio::spawn(async move {
                        let _ = serve(stream, context).await;
                    });
                }
            });

            Self {
                toml_url: format!("http://{home_domain}{STELLAR_TOML_PATH}"),
                home_domain,
                authorized,
            }
        }

        fn authorized(&self) -> bool {
            self.authorized.load(Ordering::SeqCst)
        }
    }

    struct AnchorContext {
        home_domain: String,
        client_account: String,
        authorized: Arc<AtomicBool>,
        challenges: AtomicU64,
    }

    async fn serve(mut stream: TcpStream, context: Arc<AnchorContext>) -> std::io::Result<()> {
        let (method, target, body) = read_request(&mut stream).await?;
        let (path, query) = match target.split_once('?') {
            Some((path, query)) => (path, query),
            None => (target.as_str(), ""),
        };

        match (method.as_str(), path) {
            ("GET", "/.well-known/stellar.toml") => {
                let toml = format!(
                    "VERSION = \"1.0.0\"\n\
                     NETWORK_PASSPHRASE = \"{TESTNET_PASSPHRASE}\"\n\
                     SIGNING_KEY = \"{}\"\n\
                     WEB_AUTH_ENDPOINT = \"http://{}/auth\"\n\
                     WEB_AUTH_DOMAIN = \"{}\"\n",
                    public_for(SERVER_SEED),
                    context.home_domain,
                    context.home_domain
                );
                write_response(&mut stream, 200, "text/plain", toml.as_bytes()).await
            }
            ("GET", "/auth") => {
                let account =
                    query_param(query, "account").unwrap_or_else(|| context.client_account.clone());
                let requested_domain = query_param(query, "home_domain");
                if requested_domain.as_deref() != Some(context.home_domain.as_str()) {
                    return write_response(
                        &mut stream,
                        400,
                        "application/json",
                        br#"{"error":"unknown home_domain"}"#,
                    )
                    .await;
                }

                let now = unix_now();
                let nonce = nonce_for(context.challenges.fetch_add(1, Ordering::SeqCst), now);
                let challenge = build_reference_challenge(
                    &secret_for(SERVER_SEED),
                    &account,
                    &context.home_domain,
                    &nonce,
                    now.saturating_sub(1),
                    now + 300,
                    TESTNET_PASSPHRASE,
                )
                .expect("build challenge");

                let body = serde_json::json!({
                    "transaction": challenge,
                    "network_passphrase": TESTNET_PASSPHRASE,
                })
                .to_string();
                write_response(&mut stream, 200, "application/json", body.as_bytes()).await
            }
            ("POST", "/auth") => {
                let submitted = form_field(&String::from_utf8_lossy(&body), "transaction");
                let Some(submitted) = submitted else {
                    return write_response(
                        &mut stream,
                        400,
                        "application/json",
                        br#"{"error":"missing transaction"}"#,
                    )
                    .await;
                };

                let envelope = match envelope_from_base64(&submitted) {
                    Ok(TransactionEnvelope::Tx(envelope)) => envelope,
                    _ => {
                        return write_response(
                            &mut stream,
                            400,
                            "application/json",
                            br#"{"error":"malformed transaction"}"#,
                        )
                        .await
                    }
                };

                // The anchor accepts only a challenge it issued and that the
                // wallet actually signed: two signatures, one of them valid
                // for the client account.
                let signed_by_client =
                    verify_anchor_signature(&envelope, &context.client_account, TESTNET_PASSPHRASE)
                        .is_ok();
                if !signed_by_client || envelope.signatures.len() != 2 {
                    return write_response(
                        &mut stream,
                        400,
                        "application/json",
                        br#"{"error":"invalid challenge signature"}"#,
                    )
                    .await;
                }

                context.authorized.store(true, Ordering::SeqCst);
                let token = reference_jwt(&context.home_domain, &context.client_account);
                let body = serde_json::json!({ "token": token }).to_string();
                write_response(&mut stream, 200, "application/json", body.as_bytes()).await
            }
            _ => write_response(&mut stream, 404, "text/plain", b"not found").await,
        }
    }

    fn nonce_for(counter: u64, now: u64) -> [u8; 48] {
        let mut nonce = [0u8; 48];
        nonce[..8].copy_from_slice(&counter.to_be_bytes());
        nonce[8..16].copy_from_slice(&now.to_be_bytes());
        nonce
    }

    fn reference_jwt(home_domain: &str, account: &str) -> String {
        let header = general_purpose::URL_SAFE_NO_PAD.encode(r#"{"alg":"EdDSA","typ":"JWT"}"#);
        let now = unix_now();
        let payload = general_purpose::URL_SAFE_NO_PAD.encode(format!(
            r#"{{"iss":"{home_domain}","sub":"{account}","iat":{now},"exp":{}}}"#,
            now + 3600
        ));
        format!("{header}.{payload}.anchor-signature")
    }

    async fn read_request(stream: &mut TcpStream) -> std::io::Result<(String, String, Vec<u8>)> {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 1024];
        let header_end = loop {
            if let Some(end) = find_headers_end(&buffer) {
                break end;
            }
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                return Ok((String::new(), String::new(), Vec::new()));
            }
            buffer.extend_from_slice(&chunk[..read]);
        };

        let head = String::from_utf8_lossy(&buffer[..header_end]).into_owned();
        let mut lines = head.split("\r\n");
        let request_line = lines.next().unwrap_or_default();
        let mut parts = request_line.split(' ');
        let method = parts.next().unwrap_or_default().to_string();
        let target = parts.next().unwrap_or_default().to_string();

        let content_length = lines
            .filter_map(|line| line.split_once(':'))
            .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<usize>().ok())
            .unwrap_or(0);

        let body_start = header_end + 4;
        while buffer.len() < body_start + content_length {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }

        let body_end = (body_start + content_length).min(buffer.len());
        Ok((method, target, buffer[body_start..body_end].to_vec()))
    }

    fn find_headers_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn write_response(
        stream: &mut TcpStream,
        status: u16,
        content_type: &str,
        body: &[u8],
    ) -> std::io::Result<()> {
        let reason = match status {
            200 => "OK",
            400 => "Bad Request",
            _ => "Not Found",
        };
        let head = format!(
            "HTTP/1.1 {status} {reason}\r\n\
             Content-Type: {content_type}\r\n\
             Content-Length: {}\r\n\
             Connection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(head.as_bytes()).await?;
        stream.write_all(body).await?;
        stream.flush().await?;
        stream.shutdown().await
    }

    fn query_param(query: &str, name: &str) -> Option<String> {
        form_field(query, name)
    }

    fn form_field(body: &str, name: &str) -> Option<String> {
        body.split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == name)
            .map(|(_, value)| {
                urlencoding::decode(value)
                    .map(|decoded| decoded.into_owned())
                    .unwrap_or_else(|_| value.to_string())
            })
    }

    #[tokio::test]
    async fn authenticates_against_a_local_reference_anchor() {
        let client_account = public_for(CLIENT_SEED);
        let anchor = ReferenceAnchor::start(&client_account).await;
        let client = Sep10Client::new(&anchor.home_domain, TESTNET_PASSPHRASE)
            .expect("client")
            .with_toml_url(anchor.toml_url.clone());

        let toml = client.load_stellar_toml().await.expect("read stellar.toml");
        assert_eq!(
            toml.signing_key.as_deref(),
            Some(public_for(SERVER_SEED).as_str())
        );

        let authentication = client
            .authenticate(&client_account, &secret_for(CLIENT_SEED))
            .await
            .expect("handshake should succeed");

        assert_eq!(
            authentication.validated.home_domain, anchor.home_domain,
            "the challenge must be bound to the requesting home domain"
        );
        assert_eq!(
            authentication.validated.data_name,
            format!("{} auth", anchor.home_domain)
        );
        assert_eq!(
            signature_count(&authentication.signed_xdr).expect("count"),
            2
        );
        assert!(
            anchor.authorized(),
            "the anchor must only issue a JWT for a validly signed challenge"
        );

        let claims = decode_jwt_claims(&authentication.token).expect("claims");
        assert_eq!(claims.sub.as_deref(), Some(client_account.as_str()));
        assert_eq!(claims.iss.as_deref(), Some(anchor.home_domain.as_str()));
        assert!(claims.exp.expect("exp") > unix_now());
    }

    #[tokio::test]
    async fn reference_anchor_rejects_a_challenge_signed_by_a_stranger() {
        let client_account = public_for(CLIENT_SEED);
        let anchor = ReferenceAnchor::start(&client_account).await;

        // A wallet that signs the challenge for an account the anchor did not
        // authenticate: the anchor must refuse to issue a token.
        let err = client()
            .validate_challenge(
                &toml_with(
                    &format!("http://{}/auth", anchor.home_domain),
                    &public_for(SERVER_SEED),
                ),
                &challenge_for(&public_for(IMPOSTOR_SEED), unix_now()),
                &client_account,
                unix_now(),
            )
            .expect_err("the client must reject a challenge for another account");
        assert!(matches!(err, Sep10Error::SourceAccountMismatch { .. }));
        assert!(!anchor.authorized());
    }
}
