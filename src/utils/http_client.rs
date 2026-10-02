//! Centralized HTTP client construction (#902).
//!
//! Every outbound HTTP request in StarForge is built here. Call sites ask for a
//! client instead of building a `reqwest::Client` themselves, so one place owns
//! the shared policy:
//!
//! * request and connect timeouts,
//! * connection pooling (`pool_max_idle_per_host`, `pool_idle_timeout`,
//!   `tcp_keepalive`) so repeated calls reuse sockets,
//! * the `starforge/<version>` `User-Agent`,
//! * a proxy taken from `HTTPS_PROXY` / `HTTP_PROXY` / `ALL_PROXY`, with
//!   `NO_PROXY` hosts bypassing it, and
//! * extra root certificates from `network.ca_bundle`
//!   (`starforge config set network.ca_bundle <path>`) or the
//!   `STARFORGE_CA_BUNDLE` environment variable.
//!
//! Clients are cached per settings profile: two call sites that ask for the
//! same profile share one connection pool. The cache is keyed on the *resolved*
//! settings, which is why the proxy URL and the CA bundle path are fields of
//! [`HttpClientSettings`] rather than being read once per request — changing
//! `HTTPS_PROXY` after a profile has been built keeps using the pooled client
//! for that profile.

use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use reqwest::{Certificate, Client, Proxy, StatusCode};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

/// Overall request timeout for the shared client.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Time allowed for the TCP (and TLS) connection to be established.
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// Idle connections kept per host.
pub const DEFAULT_POOL_MAX_IDLE_PER_HOST: usize = 32;
/// How long an idle pooled connection is kept before it is dropped.
pub const DEFAULT_POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// TCP keepalive interval for pooled connections.
pub const DEFAULT_TCP_KEEPALIVE: Duration = Duration::from_secs(60);

/// Number of attempts (initial try included) used by [`send_with_retry`].
pub const DEFAULT_MAX_ATTEMPTS: u32 = 3;
/// First backoff before a retry; doubles on every subsequent attempt.
pub const INITIAL_BACKOFF: Duration = Duration::from_millis(150);
/// Upper bound on the backoff so a long retry chain cannot sleep forever.
pub const MAX_BACKOFF: Duration = Duration::from_secs(5);

/// Environment variable that overrides `network.ca_bundle` from the config.
pub const CA_BUNDLE_ENV_VAR: &str = "STARFORGE_CA_BUNDLE";

/// The `User-Agent` sent by every StarForge HTTP client.
///
/// Servers use it to tell StarForge traffic apart; the version comes from the
/// crate so it can never drift from the released binary.
pub fn user_agent() -> String {
    format!("starforge/{}", env!("CARGO_PKG_VERSION"))
}

/// Everything that shapes a client. Equal settings reuse one pooled client.
///
/// The proxy URL and CA bundle path are part of the key so a client built for
/// one environment is never silently handed to another. [`Default`] is the
/// environment-independent profile: no proxy, no extra roots, 30s timeout, and
/// reqwest's own platform-proxy lookup still enabled. Use
/// [`HttpClientSettings::from_env`] to pick up the proxy environment variables
/// and the configured CA bundle.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct HttpClientSettings {
    /// `None` leaves the overall request timeout unset (reqwest default).
    pub timeout: Option<Duration>,
    pub connect_timeout: Duration,
    pub pool_max_idle_per_host: usize,
    pub pool_idle_timeout: Duration,
    pub tcp_keepalive: Duration,
    /// `false` maps to `redirect::Policy::none()`. Clients that carry bearer
    /// credentials (SEP-10 challenges, session tokens) must not chase `3xx`.
    pub follow_redirects: bool,
    pub user_agent: String,
    /// Proxy URL, e.g. `http://proxy.internal:3128`.
    pub proxy: Option<String>,
    /// Comma-separated hosts that bypass [`Self::proxy`] (`NO_PROXY` format).
    pub no_proxy: Option<String>,
    /// PEM bundle with additional root certificates to trust.
    pub ca_bundle: Option<PathBuf>,
    /// Whether reqwest may pick up the platform proxy configuration on top of
    /// [`Self::proxy`]. Tests and diagnostics turn this off so a stray
    /// `HTTP_PROXY` in the environment cannot redirect a local probe.
    pub use_system_proxy: bool,
}

impl Default for HttpClientSettings {
    fn default() -> Self {
        Self {
            timeout: Some(DEFAULT_TIMEOUT),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            pool_max_idle_per_host: DEFAULT_POOL_MAX_IDLE_PER_HOST,
            pool_idle_timeout: DEFAULT_POOL_IDLE_TIMEOUT,
            tcp_keepalive: DEFAULT_TCP_KEEPALIVE,
            follow_redirects: true,
            user_agent: user_agent(),
            proxy: None,
            no_proxy: None,
            ca_bundle: None,
            use_system_proxy: true,
        }
    }
}

impl HttpClientSettings {
    /// Defaults plus the ambient proxy (`HTTPS_PROXY` / `HTTP_PROXY` /
    /// `ALL_PROXY`, `NO_PROXY`) and CA bundle (`STARFORGE_CA_BUNDLE`, then
    /// `network.ca_bundle` for the active network).
    pub fn from_env() -> Self {
        Self {
            proxy: proxy_url_from_env(),
            no_proxy: no_proxy_from_env(),
            ca_bundle: resolve_ca_bundle(),
            ..Self::default()
        }
    }

    /// Apply an overall request timeout (replacing the default).
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Do not follow redirects. Used where a redirect would re-send a bearer
    /// credential to a host the caller did not choose.
    #[must_use]
    pub fn without_redirects(mut self) -> Self {
        self.follow_redirects = false;
        self
    }

    #[must_use]
    pub fn with_proxy(mut self, proxy: Option<String>) -> Self {
        self.proxy = proxy;
        self
    }

    #[must_use]
    pub fn with_no_proxy(mut self, no_proxy: Option<String>) -> Self {
        self.no_proxy = no_proxy;
        self
    }

    #[must_use]
    pub fn with_ca_bundle(mut self, ca_bundle: Option<PathBuf>) -> Self {
        self.ca_bundle = ca_bundle;
        self
    }

    #[must_use]
    pub fn with_user_agent(mut self, user_agent: impl Into<String>) -> Self {
        self.user_agent = user_agent.into();
        self
    }

    /// Ignore the platform proxy configuration: only [`Self::proxy`] (if any)
    /// is used.
    #[must_use]
    pub fn without_system_proxy(mut self) -> Self {
        self.use_system_proxy = false;
        self
    }

    /// Resolve the proxy to install, if any.
    ///
    /// An explicit URL always disables reqwest's own environment lookup, so the
    /// traffic that goes through `proxy` is exactly what the caller asked for.
    fn proxy(&self) -> Result<Option<Proxy>> {
        let Some(proxy_url) = &self.proxy else {
            return Ok(None);
        };

        let scheme = proxy_url
            .split_once("://")
            .map(|(scheme, _)| scheme.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(scheme.as_str(), "http" | "https") {
            bail!(
                "unsupported proxy URL '{}': expected an http:// or https:// proxy",
                proxy_url
            );
        }

        let mut proxy = Proxy::all(proxy_url.as_str())
            .with_context(|| format!("invalid proxy URL '{}'", proxy_url))?;
        // The explicit proxy is matched before reqwest's system proxy, so the
        // bypass list has to travel with it: without this, NO_PROXY would be
        // ignored for every host.
        if let Some(no_proxy) = &self.no_proxy {
            proxy = proxy.no_proxy(reqwest::NoProxy::from_string(no_proxy));
        }
        Ok(Some(proxy))
    }
}

/// Read the first non-empty value in `names` through `lookup`.
///
/// Empty values count as unset, so `HTTPS_PROXY=` disables the proxy rather
/// than sending every request to `""`.
fn lookup_env(names: &[&str], lookup: &impl Fn(&str) -> Option<String>) -> Option<String> {
    names
        .iter()
        .find_map(|name| lookup(name))
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// Proxy URL from the environment (`HTTPS_PROXY`, `HTTP_PROXY`, `ALL_PROXY`).
pub fn proxy_url_from_env() -> Option<String> {
    proxy_url(|name| std::env::var(name).ok())
}

/// [`proxy_url_from_env`] against a caller-supplied lookup, so the precedence
/// rules can be tested without mutating the process environment.
///
/// `HTTPS_PROXY` wins over `HTTP_PROXY`, which wins over `ALL_PROXY`; the
/// lower-case spelling of each is accepted as well.
pub fn proxy_url(lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    lookup_env(&["HTTPS_PROXY", "https_proxy"], &lookup)
        .or_else(|| lookup_env(&["HTTP_PROXY", "http_proxy"], &lookup))
        .or_else(|| lookup_env(&["ALL_PROXY", "all_proxy"], &lookup))
}

/// Comma-separated `NO_PROXY` list, if one is set.
pub fn no_proxy_from_env() -> Option<String> {
    no_proxy(|name| std::env::var(name).ok())
}

/// [`no_proxy_from_env`] against a caller-supplied lookup.
pub fn no_proxy(lookup: impl Fn(&str) -> Option<String>) -> Option<String> {
    lookup_env(&["NO_PROXY", "no_proxy"], &lookup)
}

/// `STARFORGE_CA_BUNDLE`, if set and non-empty.
pub fn ca_bundle_from_env() -> Option<PathBuf> {
    ca_bundle(|name| std::env::var(name).ok())
}

/// [`ca_bundle_from_env`] against a caller-supplied lookup.
pub fn ca_bundle(lookup: impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    lookup_env(&[CA_BUNDLE_ENV_VAR], &lookup).map(PathBuf::from)
}

/// `network.ca_bundle` of the active network, if the config can be read.
///
/// A broken config is not an error here: it is reported by the commands that
/// load the config, and a client without extra roots is still usable.
pub fn ca_bundle_from_config() -> Option<PathBuf> {
    let config = crate::utils::config::load().ok()?;
    let network = config.networks.get(&config.network)?;
    network
        .ca_bundle
        .as_ref()
        .map(|path| PathBuf::from(path.trim()))
        .filter(|path| !path.as_os_str().is_empty())
}

/// The CA bundle to trust: the environment variable wins over the config.
pub fn resolve_ca_bundle() -> Option<PathBuf> {
    ca_bundle_from_env().or_else(ca_bundle_from_config)
}

/// Parse a PEM bundle into the certificates reqwest can trust.
///
/// Every certificate in the file is returned, so a bundle can hold a whole
/// intermediate chain. The error names the file, because the usual cause is a
/// path pointing somewhere unexpected.
pub fn load_ca_certificates(path: &Path) -> Result<Vec<Certificate>> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("failed to read CA bundle '{}'", path.display()))?;
    let certificates = Certificate::from_pem_bundle(&bytes)
        .with_context(|| format!("failed to parse CA bundle '{}'", path.display()))?;
    if certificates.is_empty() {
        bail!(
            "CA bundle '{}' contains no PEM certificates (expected one or more \
             `-----BEGIN CERTIFICATE-----` blocks)",
            path.display()
        );
    }
    Ok(certificates)
}

/// Build a client for `settings`. Never cached — the caller owns it.
///
/// Fails when the environment cannot be honored: an unreadable `ca_bundle`, a
/// proxy URL reqwest cannot parse, or a TLS backend that refuses to start.
pub fn build_client(settings: &HttpClientSettings) -> Result<Client> {
    let mut builder = Client::builder()
        .connect_timeout(settings.connect_timeout)
        .pool_max_idle_per_host(settings.pool_max_idle_per_host)
        .pool_idle_timeout(settings.pool_idle_timeout)
        .tcp_keepalive(settings.tcp_keepalive)
        .user_agent(settings.user_agent.clone());

    if let Some(timeout) = settings.timeout {
        builder = builder.timeout(timeout);
    }

    if !settings.follow_redirects {
        builder = builder.redirect(reqwest::redirect::Policy::none());
    }

    if let Some(proxy) = settings.proxy()? {
        builder = builder.proxy(proxy);
    } else if !settings.use_system_proxy {
        builder = builder.no_proxy();
    }

    if let Some(bundle) = &settings.ca_bundle {
        for certificate in load_ca_certificates(bundle)? {
            // Built-in roots stay enabled: the bundle *adds* trust.
            builder = builder.add_root_certificate(certificate);
        }
    }

    builder.build().context("failed to build HTTP client")
}

/// Pooled clients, keyed by the settings they were built for.
static CLIENTS: Lazy<Mutex<HashMap<HttpClientSettings, Client>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Cached client for `settings`, built on first use.
pub fn client_for(settings: HttpClientSettings) -> Result<Client> {
    if let Some(client) = CLIENTS
        .lock()
        .expect("http client cache poisoned")
        .get(&settings)
        .cloned()
    {
        return Ok(client);
    }

    let client = build_client(&settings)?;
    let mut cache = CLIENTS.lock().expect("http client cache poisoned");
    // Another thread may have built the same profile while we were building.
    Ok(cache
        .entry(settings)
        .or_insert_with(|| client.clone())
        .clone())
}

/// The shared client: 30s timeout, ambient proxy and CA bundle.
///
/// Infallible because it is used from constructors that cannot report an error.
/// A CA bundle that cannot be loaded is reported on stderr and the client is
/// built without it rather than leaving the CLI unusable; `starforge config
/// set network.ca_bundle` rejects a broken bundle before it can get that far.
pub fn client() -> &'static Client {
    static CLIENT: Lazy<Client> = Lazy::new(|| shared_client(HttpClientSettings::from_env()));
    &CLIENT
}

/// Shared client with a custom overall timeout.
///
/// Pooled per timeout, so asking twice for `10s` reuses one connection pool.
pub fn client_with_timeout(timeout: Duration) -> Client {
    shared_client(HttpClientSettings::from_env().with_timeout(timeout))
}

/// Shared client that never follows redirects.
pub fn client_no_redirect() -> Client {
    shared_client(HttpClientSettings::from_env().without_redirects())
}

/// Build a client for `settings`, degrading to "no extra roots" when the CA
/// bundle cannot be used, and reporting why on stderr.
fn shared_client(settings: HttpClientSettings) -> Client {
    match client_for(settings.clone()) {
        Ok(client) => client,
        Err(error) => {
            eprintln!(
                "warning: {}; continuing without the custom CA bundle and proxy",
                error
            );
            let fallback = HttpClientSettings {
                proxy: None,
                no_proxy: None,
                ca_bundle: None,
                ..settings
            };
            client_for(fallback.clone()).unwrap_or_else(|fallback_error| {
                build_client(&fallback).unwrap_or_else(|_| {
                    // Last resort: settings without the environment additions
                    // cannot fail on anything sourced from the environment.
                    build_client(&HttpClientSettings {
                        proxy: None,
                        no_proxy: None,
                        ca_bundle: None,
                        ..fallback
                    })
                    .expect("a client without proxy and CA bundle must build")
                })
            })
        }
    }
}

/// Backwards-compatible alias for [`client`].
pub fn get_client() -> &'static Client {
    client()
}

/// `true` when a response status is worth retrying: transient server failures
/// and rate limiting.
pub fn is_retryable_status(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
}

/// Send a request with the default retry budget.
///
/// `make_request` is called again for connection errors, `5xx` and `429`, with
/// exponential backoff between attempts. Use this instead of building retry
/// loops at call sites so every caller backs off the same way.
pub async fn send_with_retry<F, Fut>(make_request: F) -> Result<reqwest::Response>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<reqwest::Response, reqwest::Error>>,
{
    send_with_retry_attempts(make_request, DEFAULT_MAX_ATTEMPTS).await
}

/// [`send_with_retry`] with an explicit attempt budget.
pub async fn send_with_retry_attempts<F, Fut>(
    make_request: F,
    max_attempts: u32,
) -> Result<reqwest::Response>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<reqwest::Response, reqwest::Error>>,
{
    if max_attempts == 0 {
        bail!("HTTP retry budget must be at least 1 attempt");
    }

    let mut backoff = INITIAL_BACKOFF;
    let mut attempt = 1;

    loop {
        let is_last = attempt >= max_attempts;
        match make_request().await {
            Ok(response) if !is_last && is_retryable_status(response.status()) => {
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
            Ok(response) => return Ok(response),
            Err(error) => {
                if is_last {
                    return Err(error).context("HTTP request failed after retries");
                }
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(MAX_BACKOFF);
            }
        }
        attempt += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[test]
    fn default_settings_are_environment_independent() {
        let settings = HttpClientSettings::default();
        assert_eq!(settings.timeout, Some(DEFAULT_TIMEOUT));
        assert!(settings.proxy.is_none());
        assert!(settings.ca_bundle.is_none());
        assert!(settings.follow_redirects);
        assert!(settings.user_agent.starts_with("starforge/"));
    }

    #[test]
    fn user_agent_carries_the_crate_version() {
        assert_eq!(
            user_agent(),
            format!("starforge/{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn settings_builders_replace_only_what_they_name() {
        let settings = HttpClientSettings::default()
            .with_timeout(Duration::from_secs(3))
            .without_redirects()
            .with_proxy(Some("http://proxy.internal:3128".to_string()))
            .with_ca_bundle(Some(PathBuf::from("/tmp/ca.pem")));

        assert_eq!(settings.timeout, Some(Duration::from_secs(3)));
        assert!(!settings.follow_redirects);
        assert_eq!(
            settings.proxy.as_deref(),
            Some("http://proxy.internal:3128")
        );
        assert_eq!(settings.ca_bundle, Some(PathBuf::from("/tmp/ca.pem")));
        // Untouched fields keep the defaults.
        assert_eq!(
            settings.pool_max_idle_per_host,
            DEFAULT_POOL_MAX_IDLE_PER_HOST
        );
    }

    /// A lookup over a fixed set of variables, standing in for the process
    /// environment so the precedence rules are tested hermetically.
    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + '_ {
        move |name: &str| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_string())
        }
    }

    #[test]
    fn https_proxy_wins_over_http_and_all_proxy() {
        assert_eq!(
            proxy_url(env(&[
                ("HTTP_PROXY", "http://http-proxy:1"),
                ("HTTPS_PROXY", "http://https-proxy:2"),
                ("ALL_PROXY", "http://all-proxy:3"),
            ]))
            .as_deref(),
            Some("http://https-proxy:2")
        );
        assert_eq!(
            proxy_url(env(&[
                ("HTTP_PROXY", "http://http-proxy:1"),
                ("ALL_PROXY", "http://all-proxy:3"),
            ]))
            .as_deref(),
            Some("http://http-proxy:1")
        );
        assert_eq!(
            proxy_url(env(&[("ALL_PROXY", "http://all-proxy:3")])).as_deref(),
            Some("http://all-proxy:3")
        );
        assert_eq!(proxy_url(env(&[])), None);
    }

    #[test]
    fn lowercase_proxy_variables_are_accepted() {
        assert_eq!(
            proxy_url(env(&[("https_proxy", "http://lowercase-proxy:8080")])).as_deref(),
            Some("http://lowercase-proxy:8080")
        );
    }

    #[test]
    fn empty_proxy_variables_count_as_unset() {
        assert_eq!(
            proxy_url(env(&[("HTTPS_PROXY", ""), ("HTTP_PROXY", "   ")])),
            None
        );
        // An empty higher-priority variable must not mask a lower-priority one.
        assert_eq!(
            proxy_url(env(&[
                ("HTTPS_PROXY", ""),
                ("HTTP_PROXY", "http://http-proxy:1"),
            ]))
            .as_deref(),
            Some("http://http-proxy:1")
        );
    }

    #[test]
    fn no_proxy_and_ca_bundle_are_read_from_their_variables() {
        assert_eq!(
            no_proxy(env(&[("NO_PROXY", "localhost,127.0.0.1")])).as_deref(),
            Some("localhost,127.0.0.1")
        );
        assert_eq!(
            ca_bundle(env(&[(CA_BUNDLE_ENV_VAR, "/etc/ssl/private-ca.pem")])),
            Some(PathBuf::from("/etc/ssl/private-ca.pem"))
        );
        assert_eq!(ca_bundle(env(&[])), None);
    }

    #[test]
    fn retryable_statuses_are_server_errors_and_rate_limits() {
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable_status(StatusCode::BAD_GATEWAY));
        assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(!is_retryable_status(StatusCode::OK));
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST));
    }

    #[test]
    fn missing_ca_bundle_is_reported_with_its_path() {
        let error = load_ca_certificates(Path::new("/nonexistent/starforge-ca.pem"))
            .expect_err("a missing bundle must not load");
        let message = error.to_string();
        assert!(
            message.contains("/nonexistent/starforge-ca.pem"),
            "error should name the bundle: {message}"
        );
    }

    #[test]
    fn ca_bundle_without_certificates_is_rejected() {
        let dir = tempfile::tempdir().expect("temp dir");
        let bundle = dir.path().join("empty.pem");
        std::fs::write(&bundle, b"# not a certificate\n").expect("write bundle");

        let error = load_ca_certificates(&bundle).expect_err("no certificates must be an error");
        assert!(
            error.to_string().contains("no PEM certificates"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn unsupported_proxy_scheme_is_rejected() {
        let settings =
            HttpClientSettings::default().with_proxy(Some("socks5://127.0.0.1:9050".to_string()));
        let error = build_client(&settings).expect_err("socks proxies are not supported");
        assert!(
            error.to_string().contains("unsupported proxy URL"),
            "unexpected error: {error}"
        );
    }

    /// A request that can never succeed: port 1 refuses connections.
    async fn refused_request() -> reqwest::Result<reqwest::Response> {
        reqwest::Client::new()
            .get("http://127.0.0.1:1/unreachable")
            .send()
            .await
    }

    /// A request that must never be attempted.
    async fn never_sent() -> reqwest::Result<reqwest::Response> {
        unreachable!("the first attempt must not run with a zero retry budget")
    }

    #[tokio::test]
    async fn retry_budget_is_bounded_and_attempts_are_counted() {
        let attempts = AtomicU32::new(0);

        let error = send_with_retry_attempts(
            || {
                attempts.fetch_add(1, Ordering::SeqCst);
                refused_request()
            },
            3,
        )
        .await
        .expect_err("a connection that always fails must surface an error");

        assert!(
            error.to_string().contains("failed after retries"),
            "unexpected error: {error}"
        );
        assert_eq!(attempts.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn a_zero_retry_budget_is_a_programming_error() {
        let attempts = AtomicU32::new(0);

        let error = send_with_retry_attempts(
            || {
                attempts.fetch_add(1, Ordering::SeqCst);
                never_sent()
            },
            0,
        )
        .await
        .expect_err("zero attempts cannot succeed");

        assert!(error.to_string().contains("at least 1 attempt"));
        assert_eq!(attempts.load(Ordering::SeqCst), 0);
    }
}
