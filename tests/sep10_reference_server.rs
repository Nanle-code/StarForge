//! End-to-end coverage for `starforge sep10 auth` against a local reference
//! SEP-10 server.
//!
//! The server is a plain TCP listener on an ephemeral port: it serves a SEP-1
//! `stellar.toml`, issues a real challenge transaction, verifies the wallet's
//! signature over it, and only then returns a JWT. The CLI runs as a child
//! process with an isolated config directory, so these tests exercise the whole
//! path — clap parsing, the challenge/response handshake, XDR signing, and
//! output — without touching the network or the developer's real wallet store.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";
const CLIENT_SEED: [u8; 32] = [29u8; 32];
const HONEST_SERVER_SEED: [u8; 32] = [23u8; 32];
const IMPOSTOR_SERVER_SEED: [u8; 32] = [31u8; 32];
const WALLET_NAME: &str = "sep10-anchor";

fn secret_for(seed: [u8; 32]) -> String {
    stellar_strkey::ed25519::PrivateKey(seed).to_string()
}

fn public_for(seed: [u8; 32]) -> String {
    let signing = ed25519_dalek::SigningKey::from_bytes(&seed);
    stellar_strkey::ed25519::PublicKey(signing.verifying_key().to_bytes()).to_string()
}

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

/// The CLI under test, isolated from the developer's real configuration.
fn starforge(home: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd.env("STARFORGE_CONFIG_DIR", home.join(".starforge"));
    cmd
}

/// A config with exactly one wallet: the SEP-10 client this test signs with.
fn write_wallet(home: &std::path::Path, public_key: &str) {
    let directory = home.join(".starforge");
    std::fs::create_dir_all(&directory).expect("create config directory");
    let config = format!(
        "version = \"1\"\n\
         network = \"testnet\"\n\n\
         [[wallets]]\n\
         name = \"{WALLET_NAME}\"\n\
         public_key = \"{public_key}\"\n\
         secret_key = \"{}\"\n\
         network = \"testnet\"\n\
         created_at = \"\"\n\
         funded = false\n",
        secret_for(CLIENT_SEED)
    );
    std::fs::write(directory.join("config.toml"), config).expect("write config.toml");
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

// ── Local reference server ───────────────────────────────────────────────────

#[derive(Clone)]
enum AnchorMode {
    /// Advertises the key it signs with.
    Honest,
    /// Advertises one signing key but signs the challenge with another.
    SignedByAnotherKey,
    /// Serves a transaction that is not valid base64 XDR.
    MalformedChallenge,
}

struct AnchorContext {
    home_domain: String,
    account: String,
    authorized: Arc<AtomicBool>,
    mode: AnchorMode,
}

struct ReferenceAnchor {
    home_domain: String,
    toml_url: String,
    authorized: Arc<AtomicBool>,
}

impl ReferenceAnchor {
    fn start(account: &str, mode: AnchorMode) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind reference anchor");
        let home_domain = format!(
            "127.0.0.1:{}",
            listener.local_addr().expect("anchor address").port()
        );
        let authorized = Arc::new(AtomicBool::new(false));

        let context = Arc::new(AnchorContext {
            home_domain: home_domain.clone(),
            account: account.to_string(),
            authorized: authorized.clone(),
            mode,
        });

        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                stream
                    .set_read_timeout(Some(Duration::from_secs(10)))
                    .expect("read timeout");
                if let Err(error) = serve(stream, &context) {
                    eprintln!("reference anchor connection failed: {error}");
                }
            }
        });

        Self {
            toml_url: format!("http://{home_domain}/.well-known/stellar.toml"),
            home_domain,
            authorized,
        }
    }

    fn authenticated(&self) -> bool {
        self.authorized.load(Ordering::SeqCst)
    }
}

fn serve(stream: TcpStream, context: &AnchorContext) -> std::io::Result<()> {
    let mut stream = stream;
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    reader.read_line(&mut request_line)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }

    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        reader.read_exact(&mut body)?;
    }

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
                public_for(HONEST_SERVER_SEED),
                context.home_domain,
                context.home_domain
            );
            write_response(&mut stream, 200, "text/plain", toml.as_bytes())
        }
        ("GET", "/auth") => {
            let account = field(query, "account").unwrap_or_else(|| context.account.clone());
            let now = unix_now();

            if matches!(context.mode, AnchorMode::MalformedChallenge) {
                let body = serde_json::json!({ "transaction": "not-valid-xdr" }).to_string();
                return write_response(&mut stream, 200, "application/json", body.as_bytes());
            }

            let signer = match context.mode {
                AnchorMode::SignedByAnotherKey => secret_for(IMPOSTOR_SERVER_SEED),
                _ => secret_for(HONEST_SERVER_SEED),
            };
            let challenge = starforge::sep::sep10::build_reference_challenge(
                &signer,
                &account,
                &context.home_domain,
                &[17u8; 48],
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
            write_response(&mut stream, 200, "application/json", body.as_bytes())
        }
        ("POST", "/auth") => {
            let submitted = field(&String::from_utf8_lossy(&body), "transaction");
            let Some(submitted) = submitted else {
                return write_response(
                    &mut stream,
                    400,
                    "application/json",
                    br#"{"error":"missing transaction"}"#,
                );
            };

            let signed_by_client = starforge::sep::sep10::verify_signature(
                &submitted,
                &context.account,
                TESTNET_PASSPHRASE,
            )
            .unwrap_or(false);
            let signatures =
                starforge::sep::sep10::signature_count(&submitted).unwrap_or_default();

            if !signed_by_client || signatures != 2 {
                return write_response(
                    &mut stream,
                    400,
                    "application/json",
                    br#"{"error":"invalid challenge signature"}"#,
                );
            }

            context.authorized.store(true, Ordering::SeqCst);
            let body = serde_json::json!({ "token": reference_jwt(&context) }).to_string();
            write_response(&mut stream, 200, "application/json", body.as_bytes())
        }
        _ => write_response(&mut stream, 404, "text/plain", b"not found"),
    }
}

fn reference_jwt(context: &AnchorContext) -> String {
    use base64::Engine as _;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let header = engine.encode(r#"{"alg":"EdDSA","typ":"JWT"}"#);
    let now = unix_now();
    let payload = engine.encode(format!(
        r#"{{"iss":"{}","sub":"{}","iat":{now},"exp":{}}}"#,
        context.home_domain,
        context.account,
        now + 3600
    ));
    format!("{header}.{payload}.anchor-signature")
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn field(body: &str, name: &str) -> Option<String> {
    body.split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .map(|(_, value)| {
            urlencoding::decode(value)
                .map(|decoded| decoded.into_owned())
                .unwrap_or_else(|_| value.to_string())
        })
}

fn write_response(
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
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    stream.shutdown(std::net::Shutdown::Both)
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn authenticates_against_a_local_reference_server() {
    let account = public_for(CLIENT_SEED);
    let anchor = ReferenceAnchor::start(&account, AnchorMode::Honest);
    let home = isolated_home();
    write_wallet(home.path(), &account);

    let output = starforge(home.path())
        .args([
            "--json",
            "sep10",
            "auth",
            "--domain",
            &anchor.home_domain,
            "--wallet",
            WALLET_NAME,
            "--toml-url",
            &anchor.toml_url,
        ])
        .output()
        .expect("spawn sep10 auth");

    assert!(
        output.status.success(),
        "sep10 auth failed: {}",
        stderr(&output)
    );

    let parsed: serde_json::Value = serde_json::from_str(&stdout(&output)).expect("valid JSON");
    assert_eq!(parsed["version"], 1);
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["data"]["home_domain"], anchor.home_domain.as_str());
    assert_eq!(parsed["data"]["account"], account.as_str());
    assert_eq!(
        parsed["data"]["challenge_data_name"],
        format!("{} auth", anchor.home_domain)
    );
    assert_eq!(parsed["data"]["signatures_after_signing"], 2);

    let jwt = parsed["data"]["jwt"].as_str().expect("jwt string");
    assert_eq!(
        jwt.split('.').count(),
        3,
        "the JWT must have three segments"
    );
    assert_eq!(parsed["data"]["jwt_claims"]["sub"], account.as_str());
    assert_eq!(
        parsed["data"]["jwt_claims"]["iss"],
        anchor.home_domain.as_str()
    );

    assert!(
        anchor.authenticated(),
        "the anchor must only issue a JWT for a validly signed challenge"
    );
}

#[test]
fn writes_the_jwt_to_a_file_when_asked() {
    let account = public_for(CLIENT_SEED);
    let anchor = ReferenceAnchor::start(&account, AnchorMode::Honest);
    let home = isolated_home();
    write_wallet(home.path(), &account);
    let directory = std::env::temp_dir().join(format!("starforge-sep10-{}", std::process::id()));
    std::fs::create_dir_all(&directory).expect("create directory");
    let jwt_path = directory.join("token.jwt");

    let output = starforge(home.path())
        .args([
            "sep10",
            "auth",
            "--domain",
            &anchor.home_domain,
            "--wallet",
            WALLET_NAME,
            "--toml-url",
            &anchor.toml_url,
            "--output",
            jwt_path.to_str().expect("path"),
        ])
        .output()
        .expect("spawn sep10 auth");

    assert!(
        output.status.success(),
        "sep10 auth failed: {}",
        stderr(&output)
    );
    let written = std::fs::read_to_string(&jwt_path).expect("read jwt file");
    assert_eq!(written.trim().split('.').count(), 3);
    assert!(stdout(&output).contains(written.trim()));

    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn rejects_a_challenge_signed_by_the_wrong_key() {
    let account = public_for(CLIENT_SEED);
    let anchor = ReferenceAnchor::start(&account, AnchorMode::SignedByAnotherKey);
    let home = isolated_home();
    write_wallet(home.path(), &account);

    let output = starforge(home.path())
        .args([
            "sep10",
            "auth",
            "--domain",
            &anchor.home_domain,
            "--wallet",
            WALLET_NAME,
            "--toml-url",
            &anchor.toml_url,
        ])
        .output()
        .expect("spawn sep10 auth");

    assert!(!output.status.success(), "a forged challenge must fail");
    let message = stderr(&output);
    assert!(
        message.contains("is not valid for SIGNING_KEY"),
        "the error must name the specific validation failure, got: {message}"
    );
    assert!(!anchor.authenticated());
}

#[test]
fn rejects_a_malformed_challenge() {
    let account = public_for(CLIENT_SEED);
    let anchor = ReferenceAnchor::start(&account, AnchorMode::MalformedChallenge);
    let home = isolated_home();
    write_wallet(home.path(), &account);

    let output = starforge(home.path())
        .args([
            "sep10",
            "auth",
            "--domain",
            &anchor.home_domain,
            "--wallet",
            WALLET_NAME,
            "--toml-url",
            &anchor.toml_url,
        ])
        .output()
        .expect("spawn sep10 auth");

    assert!(!output.status.success(), "a malformed challenge must fail");
    let message = stderr(&output);
    assert!(
        message.contains("not valid base64 XDR"),
        "the error must name the specific validation failure, got: {message}"
    );
    assert!(!anchor.authenticated());
}
