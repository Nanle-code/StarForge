//! Browser-wallet signing via a one-time localhost handoff.
//!
//! `starforge wallet sign-tx --signer browser` never asks the user to paste a
//! secret key. Instead it binds a minimal HTTP server to `127.0.0.1` on an
//! ephemeral port and opens a page that asks a Stellar Wallets Kit compatible
//! wallet (Freighter, xBull, Rabet, …) to sign a transaction XDR. The page is
//!
//! * authorised by a single-use cryptographic nonce,
//! * served under a strict `Content-Security-Policy`,
//! * only reachable from the loopback interface, and
//! * short lived (the CLI stops listening after a configurable timeout).
//!
//! The signed envelope XDR is POSTed back to the same server, which hands it to
//! the waiting CLI process. No browser is required for tests: [`sign_with`]
//! accepts any [`WalletSigner`] implementation and [`HandoffServer`] can be
//! driven by a raw `TcpStream` client.

use anyhow::Result;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use crate::utils::print as p;

/// Default number of seconds the handoff waits for the wallet before aborting.
pub const DEFAULT_HANDOFF_TIMEOUT_SECS: u64 = 120;

/// Upper bound on a single HTTP request the handoff will read. This keeps a
/// local attacker from exhausting memory with a forged `Content-Length`.
pub const MAX_REQUEST_BYTES: usize = 128 * 1024;

/// Number of random bytes in a handoff nonce.
pub const NONCE_BYTES: usize = 32;

/// CDN origin allowed to load the Stellar Wallets Kit bundle.
pub const WALLET_KIT_ORIGIN: &str = "https://cdn.jsdelivr.net";

/// Pinned Stellar Wallets Kit version requested by the handoff page.
pub const WALLET_KIT_VERSION: &str = "1.4.0";

/// Errors raised by the localhost handoff.
#[derive(Debug)]
pub enum HandoffError {
    /// The listener could not bind, or a request could not be read/written.
    Io(std::io::Error),
    /// A request arrived without the handoff nonce.
    MissingNonce,
    /// A request presented a nonce that does not match the handoff.
    InvalidNonce,
    /// The nonce was already used once; the handoff is single-use.
    NonceAlreadyUsed,
    /// The request itself was malformed.
    BadRequest(String),
    /// The wallet returned something that is not a base64 transaction XDR.
    InvalidSignature(String),
    /// The wallet explicitly declined the request.
    WalletRejected(String),
    /// No signature arrived before the handoff deadline.
    Timeout(Duration),
}

impl std::fmt::Display for HandoffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HandoffError::Io(err) => write!(f, "browser handoff I/O error: {}", err),
            HandoffError::MissingNonce => write!(f, "browser handoff request is missing its nonce"),
            HandoffError::InvalidNonce => write!(f, "browser handoff nonce is invalid"),
            HandoffError::NonceAlreadyUsed => {
                write!(f, "browser handoff nonce has already been used")
            }
            HandoffError::BadRequest(msg) => write!(f, "browser handoff bad request: {}", msg),
            HandoffError::InvalidSignature(msg) => {
                write!(f, "browser wallet returned an invalid signature: {}", msg)
            }
            HandoffError::WalletRejected(msg) => write!(f, "browser wallet rejected: {}", msg),
            HandoffError::Timeout(timeout) => write!(
                f,
                "browser handoff timed out after {}s; no signature was returned",
                timeout.as_secs()
            ),
        }
    }
}

impl std::error::Error for HandoffError {}

impl From<std::io::Error> for HandoffError {
    fn from(err: std::io::Error) -> Self {
        HandoffError::Io(err)
    }
}

/// A request for the wallet to sign.
#[derive(Debug, Clone)]
pub struct BrowserSignRequest {
    /// Base64-encoded transaction envelope XDR.
    pub transaction_xdr: String,
    /// Network name (`testnet` / `mainnet` / a custom network).
    pub network: String,
    /// Stellar network passphrase the wallet must sign against.
    pub network_passphrase: String,
}

impl BrowserSignRequest {
    /// Build a request, resolving the network passphrase from configuration.
    pub fn new(transaction_xdr: impl Into<String>, network: impl Into<String>) -> Self {
        let transaction_xdr = transaction_xdr.into();
        let network = network.into();
        let network_passphrase = crate::utils::config::get_network_passphrase(&network);
        Self {
            transaction_xdr,
            network,
            network_passphrase,
        }
    }

    /// A dependency-free, human-readable summary of the envelope. The page shows
    /// this to the user so they can compare it against what their wallet shows
    /// before approving.
    pub fn decoded_summary(&self) -> DecodedTransaction {
        use base64::{engine::general_purpose, Engine as _};

        match general_purpose::STANDARD.decode(self.transaction_xdr.trim()) {
            Ok(bytes) => DecodedTransaction {
                base64_valid: true,
                byte_length: bytes.len(),
                envelope_type: envelope_label(&bytes).to_string(),
                sha256: hex::encode(Sha256::digest(&bytes)),
                xdr_preview: hex::encode(&bytes[..bytes.len().min(16)]),
            },
            Err(_) => DecodedTransaction {
                base64_valid: false,
                byte_length: 0,
                envelope_type: "unknown".to_string(),
                sha256: String::new(),
                xdr_preview: String::new(),
            },
        }
    }
}

/// Decoded view of an unsigned transaction envelope, rendered by the handoff page.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DecodedTransaction {
    /// Whether the supplied XDR was valid base64.
    pub base64_valid: bool,
    /// Size of the decoded envelope in bytes.
    pub byte_length: usize,
    /// XDR `EnvelopeType` label derived from the leading union discriminant.
    pub envelope_type: String,
    /// SHA-256 of the unsigned envelope, so the user can compare it to the wallet.
    pub sha256: String,
    /// Hex preview of the first bytes of the envelope.
    pub xdr_preview: String,
}

/// The signature a wallet (or a test double) returned.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignResult {
    /// Signed transaction envelope XDR, base64 encoded.
    pub signed_xdr: String,
}

/// A mockable wallet backend. Real runs use [`LocalhostHandoffSigner`]; tests
/// and dry runs can use [`MockWalletSigner`] and never touch a browser.
pub trait WalletSigner {
    /// Ask the backend to sign the transaction in `request`.
    fn sign(&self, request: &BrowserSignRequest) -> std::result::Result<String, HandoffError>;
}

/// Sign through the real one-time localhost handoff.
pub struct LocalhostHandoffSigner {
    timeout: Duration,
    open_browser: bool,
}

impl LocalhostHandoffSigner {
    /// Create a signer that waits `timeout` for the wallet and opens a browser.
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            open_browser: true,
        }
    }

    /// Control whether the handoff tries to open the system browser.
    pub fn with_browser(mut self, open_browser: bool) -> Self {
        self.open_browser = open_browser;
        self
    }
}

impl WalletSigner for LocalhostHandoffSigner {
    fn sign(&self, request: &BrowserSignRequest) -> std::result::Result<String, HandoffError> {
        let mut server = HandoffServer::bind(request.clone(), self.timeout)?;
        p::info(&format!(
            "Waiting for a browser wallet signature on {}",
            server.url()
        ));
        if self.open_browser && !open_in_browser(server.url()) {
            p::warn("Could not open a browser automatically; open the URL above manually.");
        }
        server.wait()
    }
}

/// Deterministic signer used by tests and offline dry runs.
pub enum MockWalletSigner {
    /// Successfully return a fixed signed XDR.
    Returning(String),
    /// Simulate a wallet that never answers.
    TimingOut,
    /// Simulate a wallet that declines the request.
    Rejected,
}

impl MockWalletSigner {
    /// A mock that returns `signed_xdr` unchanged.
    pub fn returning(signed_xdr: impl Into<String>) -> Self {
        MockWalletSigner::Returning(signed_xdr.into())
    }

    /// A mock that never produces a signature.
    pub fn timing_out() -> Self {
        MockWalletSigner::TimingOut
    }

    /// A mock that rejects the request.
    pub fn rejected() -> Self {
        MockWalletSigner::Rejected
    }
}

impl WalletSigner for MockWalletSigner {
    fn sign(&self, _request: &BrowserSignRequest) -> std::result::Result<String, HandoffError> {
        match self {
            MockWalletSigner::Returning(signed_xdr) => Ok(signed_xdr.clone()),
            MockWalletSigner::TimingOut => Err(HandoffError::Timeout(Duration::from_secs(0))),
            MockWalletSigner::Rejected => Err(HandoffError::WalletRejected(
                "rejected by mock wallet".to_string(),
            )),
        }
    }
}

/// Sign `request` with any backend. Used by the CLI and by tests.
pub fn sign_with<S: WalletSigner>(
    signer: &S,
    request: &BrowserSignRequest,
) -> Result<String, HandoffError> {
    let signed = signer.sign(request)?;
    validate_signed_xdr(&signed)
}

/// Single-use handoff nonce.
#[derive(Debug, Clone)]
pub struct NonceGuard {
    nonce: String,
    consumed: bool,
}

impl NonceGuard {
    /// Generate a fresh nonce from the OS-backed thread RNG.
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let mut bytes = [0u8; NONCE_BYTES];
        rng.fill_bytes(&mut bytes);
        Self {
            nonce: hex::encode(bytes),
            consumed: false,
        }
    }

    /// Construct a guard around a caller-supplied nonce (used in tests).
    pub fn from_nonce(nonce: impl Into<String>) -> Self {
        Self {
            nonce: nonce.into(),
            consumed: false,
        }
    }

    /// The nonce value.
    pub fn value(&self) -> &str {
        &self.nonce
    }

    /// Whether the nonce has already authorised a signature.
    pub fn is_consumed(&self) -> bool {
        self.consumed
    }

    /// Check `candidate` without consuming the nonce.
    pub fn verify(&self, candidate: &str) -> std::result::Result<(), HandoffError> {
        if candidate.is_empty() {
            return Err(HandoffError::MissingNonce);
        }
        if self.consumed {
            return Err(HandoffError::NonceAlreadyUsed);
        }
        if !constant_time_eq(candidate.as_bytes(), self.nonce.as_bytes()) {
            return Err(HandoffError::InvalidNonce);
        }
        Ok(())
    }

    /// Mark the nonce as used. Callers must `verify` first.
    pub fn consume(&mut self) {
        self.consumed = true;
    }

    /// Verify and consume in one step.
    pub fn verify_and_consume(&mut self, candidate: &str) -> std::result::Result<(), HandoffError> {
        self.verify(candidate)?;
        self.consume();
        Ok(())
    }
}

/// The one-time localhost HTTP server backing the browser handoff.
pub struct HandoffServer {
    listener: TcpListener,
    port: u16,
    nonce: NonceGuard,
    request: BrowserSignRequest,
    url: String,
    timeout: Duration,
}

impl HandoffServer {
    /// Bind to an ephemeral port on `127.0.0.1`. Never binds a public interface.
    pub fn bind(
        request: BrowserSignRequest,
        timeout: Duration,
    ) -> std::result::Result<Self, HandoffError> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        listener.set_nonblocking(true)?;
        let port = listener.local_addr()?.port();
        let nonce = NonceGuard::generate();
        let url = format!("http://127.0.0.1:{}/?nonce={}", port, nonce.value());
        Ok(Self {
            listener,
            port,
            nonce,
            request,
            url,
            timeout,
        })
    }

    /// The port the handoff is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The URL the user must open.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// The handoff nonce.
    pub fn nonce(&self) -> &str {
        self.nonce.value()
    }

    /// Serve requests until the wallet returns a signature or the timeout fires.
    pub fn wait(&mut self) -> std::result::Result<String, HandoffError> {
        let deadline = Instant::now() + self.timeout;
        while Instant::now() < deadline {
            match self.listener.accept() {
                Ok((stream, _peer)) => {
                    let _ = stream.set_nonblocking(false);
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                    let script_nonce = self.nonce.value().to_string();
                    let outcome = match read_http_request(&stream) {
                        Ok(request) => {
                            let (response, signed) = handle_request(
                                &request,
                                &mut self.nonce,
                                &self.request,
                                &script_nonce,
                            );
                            let _ = write_response(&stream, &response);
                            signed
                        }
                        Err(err) => {
                            let response =
                                HttpResponse::error(400, "Bad Request", &err.to_string());
                            let _ = write_response(&stream, &response);
                            None
                        }
                    };
                    if let Some(signed) = outcome {
                        return Ok(signed);
                    }
                }
                Err(ref err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(err) => return Err(HandoffError::Io(err)),
            }
        }
        Err(HandoffError::Timeout(self.timeout))
    }
}

/// Build the `Content-Security-Policy` used for every handoff response.
pub fn content_security_policy(script_nonce: &str) -> String {
    format!(
        "default-src 'none'; \
         script-src 'nonce-{nonce}' {kit}; \
         style-src 'unsafe-inline'; \
         connect-src 'self'; \
         img-src 'none'; font-src 'none'; object-src 'none'; \
         base-uri 'none'; form-action 'none'; frame-ancestors 'none'",
        nonce = script_nonce,
        kit = WALLET_KIT_ORIGIN,
    )
}

/// Validate a wallet-supplied signed XDR before it is handed to the CLI.
fn validate_signed_xdr(signed_xdr: &str) -> std::result::Result<String, HandoffError> {
    use base64::{engine::general_purpose, Engine as _};

    let trimmed = signed_xdr.trim();
    if trimmed.is_empty() {
        return Err(HandoffError::InvalidSignature(
            "signed XDR is empty".to_string(),
        ));
    }
    let bytes = general_purpose::STANDARD.decode(trimmed).map_err(|_| {
        HandoffError::InvalidSignature("signed XDR is not valid base64".to_string())
    })?;
    if bytes.is_empty() {
        return Err(HandoffError::InvalidSignature(
            "signed XDR decodes to zero bytes".to_string(),
        ));
    }
    Ok(trimmed.to_string())
}

/// Returns the XDR envelope-type label from the leading 4-byte discriminant.
fn envelope_label(bytes: &[u8]) -> &'static str {
    if bytes.len() < 4 {
        return "unknown";
    }
    match u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) {
        0 => "ENVELOPE_TYPE_TX_V0",
        2 => "ENVELOPE_TYPE_TX",
        5 => "ENVELOPE_TYPE_TX_FEE_BUMP",
        _ => "unknown",
    }
}

/// Constant-time byte comparison so nonce checks do not leak a prefix match.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// A parsed HTTP request.
#[derive(Debug, Clone)]
struct HttpRequest {
    method: String,
    path: String,
    query: String,
    body: Vec<u8>,
}

/// An HTTP response, including the headers the handoff always sends.
struct HttpResponse {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn new(status: u16, reason: &'static str, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            reason,
            content_type,
            headers: Vec::new(),
            body,
        }
    }

    fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }

    fn json(status: u16, reason: &'static str, body: &str) -> Self {
        Self::new(
            status,
            reason,
            "application/json; charset=utf-8",
            body.as_bytes().to_vec(),
        )
    }

    fn error(status: u16, reason: &'static str, message: &str) -> Self {
        let payload = serde_json::json!({ "error": message }).to_string();
        Self::json(status, reason, &payload)
    }

    fn page(html: String, script_nonce: &str) -> Self {
        Self::new(200, "OK", "text/html; charset=utf-8", html.into_bytes()).with_header(
            "Content-Security-Policy",
            content_security_policy(script_nonce),
        )
    }
}

/// Read and parse one HTTP/1.1 request from `stream`.
fn read_http_request(stream: &TcpStream) -> std::io::Result<HttpRequest> {
    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.trim_end().split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target, String::new()),
    };

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse::<usize>().unwrap_or(0);
            }
        }
    }

    if content_length > MAX_REQUEST_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "request body exceeds the handoff limit",
        ));
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

    Ok(HttpRequest {
        method,
        path,
        query,
        body,
    })
}

/// Write an HTTP response and close the connection.
fn write_response(stream: &TcpStream, response: &HttpResponse) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Connection: close\r\n",
        response.status,
        response.reason,
        response.content_type,
        response.body.len(),
    );
    for (name, value) in &response.headers {
        head.push_str(name);
        head.push_str(": ");
        head.push_str(value);
        head.push_str("\r\n");
    }
    head.push_str("\r\n");

    let mut stream = stream;
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

/// Route a request. Returns the response plus, on a valid signature POST, the
/// signed XDR so the waiting CLI can stop.
fn handle_request(
    request: &HttpRequest,
    nonce: &mut NonceGuard,
    sign_request: &BrowserSignRequest,
    script_nonce: &str,
) -> (HttpResponse, Option<String>) {
    match (request.method.as_str(), request.path.as_str()) {
        ("GET", "/") => match query_param(&request.query, "nonce") {
            Some(candidate) if candidate == nonce.value() => (
                HttpResponse::page(page_html(script_nonce), script_nonce),
                None,
            ),
            Some(_) => (
                HttpResponse::error(403, "Forbidden", "invalid handoff nonce"),
                None,
            ),
            None => (
                HttpResponse::error(400, "Bad Request", "missing handoff nonce"),
                None,
            ),
        },
        ("GET", "/api/transaction") => match query_param(&request.query, "nonce") {
            Some(candidate) if candidate == nonce.value() => {
                let payload = serde_json::json!({
                    "nonce": nonce.value(),
                    "network": &sign_request.network,
                    "networkPassphrase": &sign_request.network_passphrase,
                    "transactionXdr": &sign_request.transaction_xdr,
                    "decoded": sign_request.decoded_summary(),
                });
                (HttpResponse::json(200, "OK", &payload.to_string()), None)
            }
            Some(_) => (
                HttpResponse::error(403, "Forbidden", "invalid handoff nonce"),
                None,
            ),
            None => (
                HttpResponse::error(400, "Bad Request", "missing handoff nonce"),
                None,
            ),
        },
        ("POST", "/api/sign-result") => {
            let payload: SignResultBody = match serde_json::from_slice(&request.body) {
                Ok(payload) => payload,
                Err(err) => {
                    return (
                        HttpResponse::error(400, "Bad Request", &format!("invalid JSON: {}", err)),
                        None,
                    )
                }
            };
            let candidate = query_param(&request.query, "nonce")
                .filter(|candidate| !candidate.is_empty())
                .unwrap_or(payload.nonce.clone());

            if let Err(err) = nonce.verify(&candidate) {
                let status = match err {
                    HandoffError::NonceAlreadyUsed => 410,
                    HandoffError::MissingNonce => 400,
                    _ => 403,
                };
                return (
                    HttpResponse::error(status, reason_for(status), &err.to_string()),
                    None,
                );
            }

            let signed = match validate_signed_xdr(&payload.signed_xdr) {
                Ok(signed) => signed,
                Err(err) => {
                    return (
                        HttpResponse::error(400, "Bad Request", &err.to_string()),
                        None,
                    )
                }
            };

            // Only burn the nonce once a well-formed signature is in hand.
            nonce.consume();
            (HttpResponse::json(200, "OK", "{\"ok\":true}"), Some(signed))
        }
        _ => (HttpResponse::error(404, "Not Found", "not found"), None),
    }
}

/// JSON body posted by the handoff page.
#[derive(Debug, Deserialize)]
struct SignResultBody {
    #[serde(default)]
    nonce: String,
    #[serde(default, alias = "signedXdr")]
    signed_xdr: String,
}

fn reason_for(status: u16) -> &'static str {
    match status {
        400 => "Bad Request",
        403 => "Forbidden",
        410 => "Gone",
        _ => "Error",
    }
}

/// Extract a raw query parameter value.
fn query_param(query: &str, key: &str) -> Option<String> {
    for pair in query.split('&') {
        let (name, value) = match pair.split_once('=') {
            Some((name, value)) => (name, value),
            None => (pair, ""),
        };
        if name == key {
            return Some(value.to_string());
        }
    }
    None
}

/// Best-effort open of the handoff URL in the platform browser.
fn open_in_browser(url: &str) -> bool {
    use std::process::Command;

    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(url).spawn();
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(url).spawn();

    result.is_ok()
}

/// Build the one-time handoff page. `script_nonce` is embedded both as the CSP
/// nonce and as the inline-script nonce, so injected inline scripts never run.
fn page_html(script_nonce: &str) -> String {
    PAGE_TEMPLATE
        .replace("__NONCE__", script_nonce)
        .replace("__KIT_VERSION__", WALLET_KIT_VERSION)
}

const PAGE_TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8" />
<meta name="viewport" content="width=device-width, initial-scale=1" />
<title>StarForge &mdash; sign with a browser wallet</title>
<style>
  :root { color-scheme: dark; }
  body { font-family: ui-monospace, SFMono-Regular, Menlo, monospace; background:#0b0f14; color:#e6edf3; margin:0; padding:2rem; }
  .card { max-width:52rem; margin:0 auto; border:1px solid #26303b; border-radius:12px; padding:1.5rem; background:#111820; }
  h1 { font-size:1.1rem; margin:0 0 1rem; }
  h2 { font-size:.95rem; margin:1.2rem 0 .4rem; }
  pre { white-space:pre-wrap; word-break:break-all; background:#0b0f14; border:1px solid #26303b; border-radius:8px; padding:1rem; max-height:22rem; overflow:auto; }
  button { background:#2f81f7; color:#fff; border:0; border-radius:8px; padding:.7rem 1.1rem; font:inherit; cursor:pointer; }
  button[disabled] { opacity:.5; cursor:not-allowed; }
  .row { margin:.4rem 0; }
  .k { color:#8b949e; }
  #status { margin-top:1rem; }
  .err { color:#ff7b72; }
  .ok { color:#3fb950; }
</style>
</head>
<body>
<div class="card">
  <h1>StarForge &mdash; sign with your browser wallet</h1>
  <div class="row"><span class="k">Network:</span> <span id="network"></span></div>
  <div class="row"><span class="k">Envelope:</span> <span id="envelope"></span></div>
  <div class="row"><span class="k">Bytes:</span> <span id="bytes"></span></div>
  <div class="row"><span class="k">SHA-256 (unsigned):</span> <span id="digest"></span></div>
  <h2>Decoded transaction</h2>
  <pre id="decoded">loading&hellip;</pre>
  <button id="sign" disabled>Sign with browser wallet</button>
  <div id="status"></div>
</div>
<script type="module" nonce="__NONCE__">
const params = new URLSearchParams(location.search);
const nonce = params.get("nonce") || "";
const statusEl = document.getElementById("status");
const button = document.getElementById("sign");

function setStatus(text, cls) {
  statusEl.textContent = text;
  statusEl.className = cls || "";
}

async function loadRequest() {
  const res = await fetch("/api/transaction?nonce=" + encodeURIComponent(nonce), { cache: "no-store" });
  if (!res.ok) throw new Error("handoff rejected the page: HTTP " + res.status);
  const data = await res.json();
  document.getElementById("network").textContent = data.network + " (" + data.networkPassphrase + ")";
  document.getElementById("envelope").textContent = data.decoded.envelopeType;
  document.getElementById("bytes").textContent = data.decoded.byteLength;
  document.getElementById("digest").textContent = data.decoded.sha256;
  document.getElementById("decoded").textContent = JSON.stringify(data.decoded, null, 2);
  return data;
}

async function main() {
  let data;
  try {
    data = await loadRequest();
  } catch (err) {
    setStatus(String(err), "err");
    return;
  }

  button.disabled = false;
  button.addEventListener("click", async () => {
    button.disabled = true;
    setStatus("Waiting for your wallet\u2026");
    try {
      const kit = await import("https://cdn.jsdelivr.net/npm/@creit.tech/stellar-wallets-kit@__KIT_VERSION__/+esm");
      const { StellarWalletsKit, WalletNetwork, allowAllModules } = kit;
      StellarWalletsKit.init({
        network: data.network === "mainnet" ? WalletNetwork.PUBLIC : WalletNetwork.TESTNET,
        modules: allowAllModules(),
      });
      const result = await StellarWalletsKit.signTransaction(data.transactionXdr, {
        networkPassphrase: data.networkPassphrase,
      });
      const post = await fetch("/api/sign-result?nonce=" + encodeURIComponent(nonce), {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ nonce: nonce, signedXdr: result.signedTxXdr }),
      });
      if (!post.ok) throw new Error("StarForge rejected the signature: HTTP " + post.status);
      setStatus("Signature returned to the StarForge CLI. You can close this tab.", "ok");
    } catch (err) {
      setStatus(String(err), "err");
      button.disabled = false;
    }
  });
}

main();
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine as _};

    fn sample_request() -> BrowserSignRequest {
        BrowserSignRequest::new(
            general_purpose::STANDARD.encode(b"unsigned-envelope-bytes"),
            "testnet",
        )
    }

    fn request(method: &str, path: &str, query: &str, body: Vec<u8>) -> HttpRequest {
        HttpRequest {
            method: method.to_string(),
            path: path.to_string(),
            query: query.to_string(),
            body,
        }
    }

    #[test]
    fn nonce_is_single_use() {
        let mut guard = NonceGuard::generate();
        assert!(!guard.is_consumed());
        assert_eq!(guard.value().len(), NONCE_BYTES * 2);

        let nonce = guard.value().to_string();
        guard.verify_and_consume(&nonce).unwrap();
        assert!(guard.is_consumed());
        assert!(matches!(
            guard.verify_and_consume(&nonce),
            Err(HandoffError::NonceAlreadyUsed)
        ));
    }

    #[test]
    fn wrong_nonce_is_rejected_without_consuming() {
        let mut guard = NonceGuard::generate();
        assert!(matches!(
            guard.verify("deadbeef"),
            Err(HandoffError::InvalidNonce)
        ));
        assert!(matches!(guard.verify(""), Err(HandoffError::MissingNonce)));
        assert!(!guard.is_consumed());

        let nonce = guard.value().to_string();
        guard.verify_and_consume(&nonce).unwrap();
        assert!(guard.is_consumed());
    }

    #[test]
    fn csp_is_strict_and_scoped_to_the_nonce() {
        let csp = content_security_policy("abc123");
        assert!(csp.contains("default-src 'none'"));
        assert!(csp.contains("script-src 'nonce-abc123' https://cdn.jsdelivr.net"));
        assert!(csp.contains("connect-src 'self'"));
        assert!(csp.contains("object-src 'none'"));
        assert!(csp.contains("base-uri 'none'"));
        assert!(csp.contains("frame-ancestors 'none'"));
        assert!(!csp.contains("'unsafe-eval'"));
    }

    #[test]
    fn page_requires_the_nonce_and_sets_csp() {
        let sign_request = sample_request();
        let mut guard = NonceGuard::from_nonce("abc123");

        let (missing, _) = handle_request(
            &request("GET", "/", "", Vec::new()),
            &mut guard,
            &sign_request,
            "abc123",
        );
        assert_eq!(missing.status, 400);

        let (wrong, _) = handle_request(
            &request("GET", "/", "nonce=nope", Vec::new()),
            &mut guard,
            &sign_request,
            "abc123",
        );
        assert_eq!(wrong.status, 403);

        let (page, signed) = handle_request(
            &request("GET", "/", "nonce=abc123", Vec::new()),
            &mut guard,
            &sign_request,
            "abc123",
        );
        assert_eq!(page.status, 200);
        assert!(signed.is_none());
        assert_eq!(page.content_type, "text/html; charset=utf-8");
        let csp = page
            .headers
            .iter()
            .find(|(name, _)| *name == "Content-Security-Policy")
            .map(|(_, value)| value.as_str())
            .expect("page must carry a CSP header");
        assert!(csp.contains("nonce-abc123"));
        let html = String::from_utf8(page.body).unwrap();
        assert!(html.contains("nonce=\"abc123\""));
        assert!(html.contains("@creit.tech/stellar-wallets-kit"));
        assert!(!html.contains("__NONCE__"));
        assert!(!html.contains("__KIT_VERSION__"));
    }

    #[test]
    fn invalid_signature_does_not_burn_the_nonce() {
        let sign_request = sample_request();
        let mut guard = NonceGuard::from_nonce("abc123");
        let body =
            serde_json::json!({ "nonce": "abc123", "signedXdr": "not base64!!" }).to_string();

        let (response, signed) = handle_request(
            &request(
                "POST",
                "/api/sign-result",
                "nonce=abc123",
                body.into_bytes(),
            ),
            &mut guard,
            &sign_request,
            "abc123",
        );
        assert_eq!(response.status, 400);
        assert!(signed.is_none());
        assert!(
            !guard.is_consumed(),
            "a malformed payload must not burn the nonce"
        );
    }

    #[test]
    fn response_is_rejected_once_the_nonce_is_used() {
        let sign_request = sample_request();
        let signed = general_purpose::STANDARD.encode(b"signed-envelope-bytes");
        let mut guard = NonceGuard::from_nonce("abc123");

        let body = serde_json::json!({ "nonce": "abc123", "signedXdr": &signed }).to_string();
        let (first, returned) = handle_request(
            &request(
                "POST",
                "/api/sign-result",
                "nonce=abc123",
                body.clone().into_bytes(),
            ),
            &mut guard,
            &sign_request,
            "abc123",
        );
        assert_eq!(first.status, 200);
        assert_eq!(returned.as_deref(), Some(signed.as_str()));

        let (second, _) = handle_request(
            &request(
                "POST",
                "/api/sign-result",
                "nonce=abc123",
                body.into_bytes(),
            ),
            &mut guard,
            &sign_request,
            "abc123",
        );
        assert_eq!(second.status, 410);
    }

    #[test]
    fn http_handshake_returns_the_signed_xdr() {
        let sign_request = sample_request();
        let mut server =
            HandoffServer::bind(sign_request, Duration::from_secs(5)).expect("bind handoff");
        let port = server.port();
        let nonce = server.nonce().to_string();
        let signed = general_purpose::STANDARD.encode(b"signed-envelope-bytes");

        let handle = std::thread::spawn(move || server.wait());

        let mut get = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(
            get,
            "GET /api/transaction?nonce={} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n",
            nonce
        )
        .unwrap();
        let mut get_response = String::new();
        get.read_to_string(&mut get_response).unwrap();
        assert!(get_response.contains("200 OK"), "{}", get_response);
        assert!(get_response.contains("transactionXdr"), "{}", get_response);

        let payload = serde_json::json!({ "nonce": &nonce, "signedXdr": &signed }).to_string();
        let mut post = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        write!(
            post,
            "POST /api/sign-result?nonce={} HTTP/1.1\r\nHost: 127.0.0.1\r\n\
             Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            nonce,
            payload.len(),
            payload
        )
        .unwrap();
        let mut post_response = String::new();
        post.read_to_string(&mut post_response).unwrap();
        assert!(post_response.contains("200 OK"), "{}", post_response);

        let returned = handle.join().unwrap().unwrap();
        assert_eq!(returned, signed);
    }

    #[test]
    fn handoff_times_out_without_a_wallet() {
        let mut server = HandoffServer::bind(sample_request(), Duration::from_millis(150)).unwrap();
        let start = Instant::now();
        let err = server.wait().unwrap_err();
        assert!(matches!(err, HandoffError::Timeout(_)));
        assert!(start.elapsed() >= Duration::from_millis(100));
    }

    #[test]
    fn mock_signer_round_trips_and_propagates_failures() {
        let sign_request = sample_request();
        let valid = general_purpose::STANDARD.encode(b"mock-signed");
        assert_eq!(
            sign_with(&MockWalletSigner::returning(valid.clone()), &sign_request).unwrap(),
            valid
        );

        assert!(matches!(
            sign_with(&MockWalletSigner::timing_out(), &sign_request),
            Err(HandoffError::Timeout(_))
        ));
        assert!(matches!(
            sign_with(&MockWalletSigner::rejected(), &sign_request),
            Err(HandoffError::WalletRejected(_))
        ));
    }

    #[test]
    fn decoded_summary_flags_invalid_base64() {
        let bad = BrowserSignRequest {
            transaction_xdr: "!!not-base64!!".to_string(),
            network: "testnet".to_string(),
            network_passphrase: "Test SDF Network ; September 2015".to_string(),
        };
        let decoded = bad.decoded_summary();
        assert!(!decoded.base64_valid);
        assert_eq!(decoded.byte_length, 0);
        assert_eq!(decoded.envelope_type, "unknown");

        let good = BrowserSignRequest::new(
            general_purpose::STANDARD.encode([0u8, 0, 0, 2, 1, 2, 3]),
            "testnet",
        );
        let decoded = good.decoded_summary();
        assert!(decoded.base64_valid);
        assert_eq!(decoded.byte_length, 7);
        assert_eq!(decoded.envelope_type, "ENVELOPE_TYPE_TX");
        assert_eq!(decoded.sha256.len(), 64);
    }
}
