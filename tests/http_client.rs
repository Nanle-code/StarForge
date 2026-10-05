//! Integration coverage for the central HTTP client factory (#902).
//!
//! The tests drive real sockets, because the point of the factory is the wire
//! behaviour it applies to every caller:
//!
//! * a local "origin" server shows the client reaches a plain endpoint, sends
//!   the `starforge/<version>` user agent, follows redirects by default and
//!   does not follow them when asked not to,
//! * a local proxy shows a configured proxy is used (absolute-form request
//!   line) and that `NO_PROXY` hosts bypass it,
//! * a local `openssl s_server` instance shows a CA bundle is actually trusted:
//!   the same request fails without the bundle and succeeds with it. The test
//!   skips itself when `openssl` is not installed.
//!
//! Settings are always injected through [`HttpClientSettings`], never through
//! the process environment, so a stray `HTTP_PROXY` in the environment cannot
//! change the result and the tests can run in parallel.

use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use starforge::utils::http_client::{build_client, user_agent, HttpClientSettings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// What the test server answers with.
#[derive(Clone)]
enum Reply {
    /// `200 OK` with this body.
    Body(&'static str),
    /// `302 Found` pointing at this absolute URL.
    Redirect(String),
    /// `200 OK` with this body, but only after `delay`.
    DelayedBody(Duration, &'static str),
}

/// A minimal HTTP/1.1 server on an ephemeral port.
///
/// It records the head of every request it receives *before* answering, so a
/// test can assert on the request as soon as the client's response is back.
struct TestServer {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
}

impl TestServer {
    async fn start(reply: Reply) -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("bind the test server");
        let port = listener.local_addr().expect("test server address").port();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let reply = reply.clone();
                let recorded = Arc::clone(&recorded);
                tokio::spawn(async move {
                    let mut head = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
                        match socket.read(&mut chunk).await {
                            Ok(0) | Err(_) => return,
                            Ok(read) => head.extend_from_slice(&chunk[..read]),
                        }
                    }

                    let head = String::from_utf8_lossy(&head).to_string();
                    recorded.lock().expect("request log").push(head);

                    let response = match reply {
                        Reply::Body(body) => ok_response(body),
                        Reply::Redirect(location) => format!(
                            "HTTP/1.1 302 Found\r\nLocation: {location}\r\n\
                             Content-Length: 0\r\nConnection: close\r\n\r\n"
                        ),
                        Reply::DelayedBody(delay, body) => {
                            tokio::time::sleep(delay).await;
                            ok_response(body)
                        }
                    };
                    let _ = socket.write_all(response.as_bytes()).await;
                    let _ = socket.flush().await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        Self { port, requests }
    }

    /// URL of a path on this server, for a client connecting directly.
    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Proxy URL for this server, for a client connecting through it.
    fn proxy_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn request_heads(&self) -> Vec<String> {
        self.requests.lock().expect("request log").clone()
    }

    /// The request target (second token of the request line) of every request.
    fn request_targets(&self) -> Vec<String> {
        self.request_heads()
            .iter()
            .filter_map(|head| head.lines().next())
            .filter_map(|line| line.split_whitespace().nth(1))
            .map(str::to_string)
            .collect()
    }
}

fn ok_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// A client that ignores the ambient proxy configuration but keeps the
/// platform roots, so these tests do not depend on the machine's environment.
fn hermetic_settings() -> HttpClientSettings {
    HttpClientSettings::default().without_system_proxy()
}

#[tokio::test]
async fn the_shared_client_reaches_a_local_server_and_identifies_itself() {
    let origin = TestServer::start(Reply::Body("origin")).await;
    let client = build_client(&hermetic_settings()).expect("build client");

    let response = client
        .get(origin.url("/direct"))
        .send()
        .await
        .expect("local request succeeds");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.text().await.expect("body"), "origin");

    let head = origin.request_heads();
    assert_eq!(head.len(), 1, "expected exactly one request: {head:?}");
    assert!(
        head[0].starts_with("GET /direct HTTP/1.1"),
        "unexpected request line: {}",
        head[0].lines().next().unwrap_or_default()
    );
    assert!(
        head[0].to_ascii_lowercase().contains(&format!(
            "user-agent: {}",
            user_agent().to_ascii_lowercase()
        )),
        "the factory must set the starforge user agent: {head:?}"
    );
}

#[tokio::test]
async fn a_configured_proxy_receives_the_absolute_form_request() {
    let proxy = TestServer::start(Reply::Body("proxied")).await;
    let settings = hermetic_settings().with_proxy(Some(proxy.proxy_url()));
    let client = build_client(&settings).expect("build client");

    // The host does not resolve: reaching it at all proves the proxy was used.
    let response = client
        .get("http://proxy-target.invalid/through-proxy")
        .send()
        .await
        .expect("the proxy answers");
    assert_eq!(response.text().await.expect("body"), "proxied");

    assert_eq!(
        proxy.request_targets(),
        vec!["http://proxy-target.invalid/through-proxy".to_string()],
        "an HTTP proxy must receive the absolute request target"
    );
    assert!(
        proxy.request_heads()[0].contains("Host: proxy-target.invalid"),
        "the original host must survive"
    );
}

#[tokio::test]
async fn no_proxy_hosts_bypass_the_configured_proxy() {
    let origin = TestServer::start(Reply::Body("direct")).await;
    let proxy = TestServer::start(Reply::Body("proxied")).await;

    let settings = hermetic_settings()
        .with_proxy(Some(proxy.proxy_url()))
        .with_no_proxy(Some("127.0.0.1".to_string()));
    let client = build_client(&settings).expect("build client");

    let response = client
        .get(origin.url("/direct"))
        .send()
        .await
        .expect("the origin answers");
    assert_eq!(response.text().await.expect("body"), "direct");

    assert_eq!(
        origin.request_targets(),
        vec!["/direct".to_string()],
        "the request must reach the origin itself"
    );
    assert!(
        proxy.request_heads().is_empty(),
        "NO_PROXY must bypass the proxy, but it saw: {:?}",
        proxy.request_heads()
    );
}

#[tokio::test]
async fn redirects_are_followed_by_default_but_not_when_disabled() {
    let target = TestServer::start(Reply::Body("final")).await;
    let redirector = TestServer::start(Reply::Redirect(target.url("/final"))).await;

    let following = build_client(&hermetic_settings()).expect("build client");
    let response = following
        .get(redirector.url("/start"))
        .send()
        .await
        .expect("request succeeds");
    assert_eq!(response.text().await.expect("body"), "final");
    assert_eq!(
        target.request_targets(),
        vec!["/final".to_string()],
        "the redirect must have been followed"
    );

    let strict = HttpClientSettings::default()
        .without_system_proxy()
        .without_redirects();
    let strict = build_client(&strict).expect("build client");
    let response = strict
        .get(redirector.url("/start"))
        .send()
        .await
        .expect("request succeeds");
    assert_eq!(
        response.status(),
        reqwest::StatusCode::FOUND,
        "a client carrying credentials must surface the 3xx"
    );
    assert_eq!(
        target.request_targets(),
        vec!["/final".to_string()],
        "the disabled-redirect client must not add another request"
    );
}

#[tokio::test]
async fn a_short_timeout_trips_before_a_slow_server_answers() {
    let slow = TestServer::start(Reply::DelayedBody(Duration::from_secs(30), "late")).await;
    let settings = hermetic_settings().with_timeout(Duration::from_millis(250));
    let client = build_client(&settings).expect("build client");

    let started = Instant::now();
    let error = client
        .get(slow.url("/slow"))
        .send()
        .await
        .expect_err("the request must time out");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the timeout must be honoured, took {:?}",
        started.elapsed()
    );
    assert!(error.is_timeout(), "expected a timeout error, got: {error}");
}

// ── Custom CA bundle ────────────────────────────────────────────────────────

/// An `openssl s_server` child process that is killed when the test ends.
struct TlsServer {
    child: Child,
    port: u16,
}

impl Drop for TlsServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Generate a private CA plus a leaf certificate for `127.0.0.1`/`localhost`.
///
/// Returns `(ca_pem, server_pem, server_key)`.
fn generate_test_certificates(dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    let ca_key = dir.join("ca.key");
    let ca_pem = dir.join("ca.pem");
    let server_key = dir.join("server.key");
    let server_csr = dir.join("server.csr");
    let server_pem = dir.join("server.pem");
    let extensions = dir.join("server.ext");

    // A CA that is allowed to sign certificates, not an end-entity certificate.
    openssl(&[
        "req",
        "-x509",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        ca_key.to_str().expect("utf-8 path"),
        "-out",
        ca_pem.to_str().expect("utf-8 path"),
        "-days",
        "2",
        "-subj",
        "/CN=StarForge Test CA",
        "-addext",
        "basicConstraints=critical,CA:TRUE,pathlen:0",
        "-addext",
        "keyUsage=critical,keyCertSign,cRLSign",
    ]);

    openssl(&[
        "req",
        "-newkey",
        "rsa:2048",
        "-nodes",
        "-keyout",
        server_key.to_str().expect("utf-8 path"),
        "-out",
        server_csr.to_str().expect("utf-8 path"),
        "-subj",
        "/CN=127.0.0.1",
    ]);

    // rustls matches the leaf against the IP literal, so the SAN is required.
    std::fs::write(
        &extensions,
        "subjectAltName=IP:127.0.0.1,DNS:localhost\nextendedKeyUsage=serverAuth\n",
    )
    .expect("write the extension file");

    openssl(&[
        "x509",
        "-req",
        "-in",
        server_csr.to_str().expect("utf-8 path"),
        "-CA",
        ca_pem.to_str().expect("utf-8 path"),
        "-CAkey",
        ca_key.to_str().expect("utf-8 path"),
        "-CAcreateserial",
        "-out",
        server_pem.to_str().expect("utf-8 path"),
        "-days",
        "2",
        "-extfile",
        extensions.to_str().expect("utf-8 path"),
    ]);

    (ca_pem, server_pem, server_key)
}

fn openssl(args: &[&str]) {
    let output = Command::new("openssl")
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("run `openssl {}`: {error}", args.join(" ")));
    assert!(
        output.status.success(),
        "`openssl {}` failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn openssl_is_available() -> bool {
    Command::new("openssl")
        .arg("version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn start_tls_server(dir: &Path, cert: &Path, key: &Path) -> TlsServer {
    // Reserve a port, then hand it to openssl. The gap is short and the port is
    // only probed by this test.
    let port = {
        let probe = TcpListener::bind(("127.0.0.1", 0)).expect("reserve a port");
        probe.local_addr().expect("reserved address").port()
    };

    let child = Command::new("openssl")
        .args([
            "s_server",
            "-accept",
            &port.to_string(),
            "-cert",
            cert.to_str().expect("utf-8 path"),
            "-key",
            key.to_str().expect("utf-8 path"),
            "-www",
            "-quiet",
        ])
        .current_dir(dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("start openssl s_server");

    // Wait until the listener accepts connections so the first request is not a
    // connect error.
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        if std::net::TcpStream::connect(("127.0.0.1", port)).is_ok() {
            return TlsServer { child, port };
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    let mut server = TlsServer { child, port };
    let _ = server.child.kill();
    panic!("openssl s_server did not start listening on port {port}");
}

#[tokio::test]
async fn a_ca_bundle_makes_a_private_ca_trusted() {
    if !openssl_is_available() {
        eprintln!("skipping: `openssl` is not available to serve a private CA");
        return;
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let (ca_pem, server_pem, server_key) = generate_test_certificates(dir.path());
    let server = start_tls_server(dir.path(), &server_pem, &server_key);
    let url = format!("https://127.0.0.1:{}/", server.port);

    // Without the bundle the private CA is unknown, so the handshake fails.
    let untrusting = build_client(&hermetic_settings()).expect("build client");
    let error = untrusting
        .get(&url)
        .send()
        .await
        .expect_err("an unknown CA must not verify");
    assert!(
        error.is_connect() || error.is_request(),
        "expected a TLS failure, got: {error}"
    );

    // With the bundle the same request is trusted. Only the response head is
    // read: `openssl s_server -www` closes after the status page, so waiting
    // for a body would test openssl's framing rather than our TLS trust.
    let trusting = build_client(
        &hermetic_settings()
            .with_ca_bundle(Some(ca_pem))
            .with_timeout(Duration::from_secs(10)),
    )
    .expect("build a client that trusts the test CA");
    let response = trusting
        .get(&url)
        .send()
        .await
        .expect("the bundle must make the test CA trusted");
    assert!(
        response.status().is_success(),
        "unexpected status: {}",
        response.status()
    );
}

#[test]
fn a_bundle_that_does_not_exist_is_reported_by_name() {
    let missing = PathBuf::from("/definitely/not/here/starforge-ca.pem");
    let error = build_client(&hermetic_settings().with_ca_bundle(Some(missing.clone())))
        .expect_err("a missing bundle must fail the build");
    assert!(
        error.to_string().contains(&missing.display().to_string()),
        "the error must name the bundle: {error}"
    );
}

// ── Centralization lint ─────────────────────────────────────────────────────

/// Every client must come from the factory (#902): a raw construction anywhere
/// else silently drops the proxy, the custom CA bundle and the user agent.
#[test]
fn raw_client_construction_is_confined_to_the_factory() {
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let factory = source_root.join("utils/http_client.rs");

    let mut offenders = Vec::new();
    collect_offenders(&source_root, &factory, &mut offenders);

    assert!(
        offenders.is_empty(),
        "build HTTP clients through `utils::http_client` instead of \
         `Client::new()` / `Client::builder()`: {offenders:#?}"
    );
}

fn collect_offenders(directory: &Path, factory: &Path, offenders: &mut Vec<String>) {
    let entries = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()));

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_offenders(&path, factory, offenders);
            continue;
        }
        if path.extension().and_then(|extension| extension.to_str()) != Some("rs") {
            continue;
        }
        if path == factory {
            // The factory itself is the one place allowed to build a client.
            continue;
        }

        let contents = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        for (number, line) in contents.lines().enumerate() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains("Client::new()") || line.contains("Client::builder()") {
                offenders.push(format!(
                    "{}:{}: {}",
                    path.display(),
                    number + 1,
                    line.trim()
                ));
            }
        }
    }
}
