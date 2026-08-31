use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Well-known Soroban network configurations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SorobanNetwork {
    Testnet,
    Mainnet,
    Futurenet,
    Local,
    Custom { rpc_url: String, passphrase: String },
}

impl SorobanNetwork {
    pub fn rpc_url(&self) -> &str {
        match self {
            SorobanNetwork::Testnet => "https://soroban-testnet.stellar.org",
            SorobanNetwork::Mainnet => "https://mainnet.stellar.validationcloud.io/v1/soroban/rpc",
            SorobanNetwork::Futurenet => "https://rpc-futurenet.stellar.org",
            SorobanNetwork::Local => "http://localhost:8000/rpc",
            SorobanNetwork::Custom { rpc_url, .. } => rpc_url,
        }
    }

    pub fn passphrase(&self) -> &str {
        match self {
            SorobanNetwork::Testnet => "Test SDF Network ; September 2015",
            SorobanNetwork::Mainnet => "Public Global Stellar Network ; September 2015",
            SorobanNetwork::Futurenet => "Test SDF Future Network ; October 2022",
            SorobanNetwork::Local => "Standalone Network ; February 2017",
            SorobanNetwork::Custom { passphrase, .. } => passphrase,
        }
    }

    pub fn friendbot_url(&self) -> Option<&str> {
        match self {
            SorobanNetwork::Testnet => Some("https://friendbot.stellar.org"),
            SorobanNetwork::Futurenet => Some("https://friendbot-futurenet.stellar.org"),
            SorobanNetwork::Local => Some("http://localhost:8000/friendbot"),
            _ => None,
        }
    }

    pub fn supports_friendbot(&self) -> bool {
        self.friendbot_url().is_some()
    }
}

impl std::fmt::Display for SorobanNetwork {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SorobanNetwork::Testnet => write!(f, "testnet"),
            SorobanNetwork::Mainnet => write!(f, "mainnet"),
            SorobanNetwork::Futurenet => write!(f, "futurenet"),
            SorobanNetwork::Local => write!(f, "local"),
            SorobanNetwork::Custom { rpc_url, .. } => write!(f, "custom({})", rpc_url),
        }
    }
}

/// Configuration for a testnet session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestnetConfig {
    pub network: SorobanNetwork,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub retry_delay_ms: u64,
    pub verbose: bool,
}

impl Default for TestnetConfig {
    fn default() -> Self {
        Self {
            network: SorobanNetwork::Testnet,
            timeout_secs: 30,
            max_retries: 3,
            retry_delay_ms: 2000,
            verbose: false,
        }
    }
}

impl TestnetConfig {
    pub fn for_network(network: SorobanNetwork) -> Self {
        Self {
            network,
            ..Default::default()
        }
    }

    pub fn local() -> Self {
        Self {
            network: SorobanNetwork::Local,
            timeout_secs: 10,
            max_retries: 1,
            retry_delay_ms: 500,
            verbose: false,
        }
    }
}

/// Result of checking testnet health.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthCheckResult {
    pub healthy: bool,
    pub network: String,
    pub rpc_url: String,
    pub latest_ledger: Option<u32>,
    pub latency_ms: u64,
    pub error: Option<String>,
}

/// Result of a Friendbot funding request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FundbotResult {
    pub address: String,
    pub success: bool,
    pub transaction_hash: Option<String>,
    pub error: Option<String>,
}

/// Result of uploading a WASM binary to the testnet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmUploadResult {
    pub wasm_hash: String,
    pub transaction_hash: String,
    pub fee_charged: u64,
    pub ledger: u32,
    pub already_existed: bool,
}

/// Result of deploying a contract instance on the testnet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractDeployResult {
    pub contract_id: String,
    pub transaction_hash: String,
    pub wasm_hash: String,
    pub fee_charged: u64,
    pub ledger: u32,
}

/// Result of invoking a contract function on the testnet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InvokeResult {
    pub contract_id: String,
    pub function: String,
    pub return_value: serde_json::Value,
    pub transaction_hash: String,
    pub fee_charged: u64,
    pub ledger: u32,
    pub events: Vec<TestnetEvent>,
}

/// An event captured from a testnet transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestnetEvent {
    pub contract_id: String,
    pub topics: Vec<String>,
    pub data: String,
}

/// Ledger entry queried from the testnet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntryResult {
    pub key: String,
    pub value: String,
    pub last_modified_ledger: u32,
    pub live_until_ledger: Option<u32>,
}

// ── Raw JSON-RPC helpers ───────────────────────────────────────────────────

#[derive(Debug, Serialize)]
// Not currently called from any code path in this crate. Kept rather than
// removed since deleting it is a product decision, not a lint-scoping one.
#[allow(dead_code)]
struct RpcRequest<'a> {
    jsonrpc: &'static str,
    id: u64,
    method: &'a str,
    params: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct RpcResponse {
    result: Option<serde_json::Value>,
    error: Option<RpcError>,
}

#[derive(Debug, Deserialize)]
struct RpcError {
    code: i64,
    message: String,
}

fn rpc_post(url: &str, method: &str, params: serde_json::Value) -> Result<serde_json::Value> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });

    let url = url.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .context("Failed to create tokio runtime for RPC")?;
            rt.block_on(async {
                let response = crate::utils::http_client::get_client()
                    .post(&url)
                    .header("Content-Type", "application/json")
                    .json(&body)
                    .timeout(Duration::from_secs(30))
                    .send()
                    .await
                    .context("RPC request failed")?;
                let text = response
                    .text()
                    .await
                    .context("Failed to read RPC response")?;
                Ok::<_, anyhow::Error>(text)
            })
        })();
        let _ = tx.send(result);
    });

    let text = rx
        .recv()
        .map_err(|_| anyhow::anyhow!("RPC worker exited unexpectedly"))??;
    let parsed: RpcResponse = serde_json::from_str(&text).context("Invalid RPC response")?;

    if let Some(error) = parsed.error {
        anyhow::bail!("RPC error {}: {}", error.code, error.message);
    }

    parsed.result.context("RPC response missing result field")
}

/// Client for interacting with a Soroban RPC endpoint.
pub struct TestnetClient {
    pub config: TestnetConfig,
}

impl TestnetClient {
    pub fn new(config: TestnetConfig) -> Self {
        Self { config }
    }

    pub fn for_testnet() -> Self {
        Self::new(TestnetConfig::default())
    }

    pub fn for_local() -> Self {
        Self::new(TestnetConfig::local())
    }

    /// Check that the RPC endpoint is reachable and responsive.
    pub fn health_check(&self) -> HealthCheckResult {
        let start = Instant::now();
        let url = self.config.network.rpc_url();

        match rpc_post(url, "getLatestLedger", serde_json::json!(null)) {
            Ok(result) => {
                let latest_ledger = result
                    .get("sequence")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
                HealthCheckResult {
                    healthy: true,
                    network: self.config.network.to_string(),
                    rpc_url: url.to_string(),
                    latest_ledger,
                    latency_ms: start.elapsed().as_millis() as u64,
                    error: None,
                }
            }
            Err(e) => HealthCheckResult {
                healthy: false,
                network: self.config.network.to_string(),
                rpc_url: url.to_string(),
                latest_ledger: None,
                latency_ms: start.elapsed().as_millis() as u64,
                error: Some(e.to_string()),
            },
        }
    }

    /// Fund an address using Friendbot (testnet/futurenet/local only).
    pub fn fund_account(&self, address: &str) -> Result<FundbotResult> {
        let bot_url = self
            .config
            .network
            .friendbot_url()
            .context("Friendbot is not available for this network")?;

        let url = format!("{}?addr={}", bot_url, urlencoding::encode(address));
        let timeout_secs = self.config.timeout_secs;
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = (|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("Failed to create tokio runtime for Friendbot")?;
                rt.block_on(async {
                    let response = crate::utils::http_client::get_client()
                        .get(&url)
                        .timeout(Duration::from_secs(timeout_secs))
                        .send()
                        .await
                        .context("Friendbot request failed")?;
                    let text = response
                        .text()
                        .await
                        .context("Failed to read Friendbot response")?;
                    Ok::<_, anyhow::Error>(text)
                })
            })();
            let _ = tx.send(result);
        });

        match rx
            .recv()
            .map_err(|_| anyhow::anyhow!("Friendbot worker exited unexpectedly"))?
        {
            Ok(text) => {
                let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
                Ok(FundbotResult {
                    address: address.to_string(),
                    success: true,
                    transaction_hash: parsed
                        .get("hash")
                        .and_then(|h| h.as_str())
                        .map(String::from),
                    error: None,
                })
            }
            Err(e) => Ok(FundbotResult {
                address: address.to_string(),
                success: false,
                transaction_hash: None,
                error: Some(e.to_string()),
            }),
        }
    }

    /// Query the latest ledger sequence number.
    pub fn latest_ledger(&self) -> Result<u32> {
        let result = rpc_post(
            self.config.network.rpc_url(),
            "getLatestLedger",
            serde_json::json!(null),
        )?;
        result
            .get("sequence")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32)
            .context("Missing 'sequence' in getLatestLedger response")
    }

    /// Simulate a contract invocation without submitting a transaction.
    pub fn simulate_invoke(
        &self,
        contract_id: &str,
        function: &str,
        args: &[serde_json::Value],
    ) -> Result<serde_json::Value> {
        let params = serde_json::json!({
            "transaction": {
                "contract_id": contract_id,
                "function": function,
                "args": args,
            }
        });
        rpc_post(self.config.network.rpc_url(), "simulateTransaction", params)
    }

    /// Query ledger entries by key XDR strings.
    pub fn get_ledger_entries(&self, key_xdrs: &[&str]) -> Result<Vec<LedgerEntryResult>> {
        let params = serde_json::json!({
            "keys": key_xdrs
        });
        let result = rpc_post(self.config.network.rpc_url(), "getLedgerEntries", params)?;

        let entries = result
            .get("entries")
            .and_then(|e| e.as_array())
            .cloned()
            .unwrap_or_default();

        Ok(entries
            .into_iter()
            .filter_map(|entry| {
                Some(LedgerEntryResult {
                    key: entry.get("key")?.as_str()?.to_string(),
                    value: entry.get("xdr")?.as_str()?.to_string(),
                    last_modified_ledger: entry
                        .get("lastModifiedLedgerSeq")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32,
                    live_until_ledger: entry
                        .get("liveUntilLedgerSeq")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u32),
                })
            })
            .collect())
    }
}

// ── Testnet deployer ───────────────────────────────────────────────────────

/// Handles deploying WASM and contract instances to the testnet.
pub struct TestnetDeployer {
    client: TestnetClient,
}

impl TestnetDeployer {
    pub fn new(client: TestnetClient) -> Self {
        Self { client }
    }

    pub fn for_testnet() -> Self {
        Self::new(TestnetClient::for_testnet())
    }

    /// Compute the expected WASM hash for a binary without uploading.
    pub fn compute_wasm_hash(wasm_bytes: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(wasm_bytes))
    }

    /// Check whether the given WASM hash is already present on chain.
    pub fn wasm_exists(&self, wasm_hash: &str) -> Result<bool> {
        let key_b64 = format!("wasm:{}", wasm_hash);
        let entries = self.client.get_ledger_entries(&[&key_b64])?;
        Ok(!entries.is_empty())
    }

    /// Upload WASM bytes and return the on-chain hash.
    ///
    /// This is a stub that simulates the upload for non-interactive
    /// environments (e.g. CI without a funded key). Real submission
    /// requires signing and is delegated to the `soroban` command.
    pub fn upload_wasm_dry_run(&self, wasm_bytes: &[u8]) -> Result<WasmUploadResult> {
        let hash = Self::compute_wasm_hash(wasm_bytes);
        let already_existed = self.wasm_exists(&hash).unwrap_or(false);

        Ok(WasmUploadResult {
            wasm_hash: hash,
            transaction_hash: "dry-run-no-tx".into(),
            fee_charged: 0,
            ledger: self.client.latest_ledger().unwrap_or(0),
            already_existed,
        })
    }
}

// ── Testnet session ────────────────────────────────────────────────────────

/// A high-level testnet session that coordinates funding, deployment, and
/// invocation across a test run.
pub struct TestnetSession {
    pub config: TestnetConfig,
    pub client: TestnetClient,
    pub deployer: TestnetDeployer,
    pub funded_accounts: Vec<FundbotResult>,
    pub deployed_contracts: Vec<ContractDeployResult>,
}

impl TestnetSession {
    pub fn new(config: TestnetConfig) -> Self {
        let client = TestnetClient::new(config.clone());
        let deployer = TestnetDeployer::new(TestnetClient::new(config.clone()));
        Self {
            config,
            client,
            deployer,
            funded_accounts: Vec::new(),
            deployed_contracts: Vec::new(),
        }
    }

    pub fn for_testnet() -> Self {
        Self::new(TestnetConfig::default())
    }

    pub fn for_local() -> Self {
        Self::new(TestnetConfig::local())
    }

    /// Verify the testnet is reachable before running tests.
    pub fn verify_connectivity(&self) -> Result<HealthCheckResult> {
        let check = self.client.health_check();
        if !check.healthy {
            anyhow::bail!(
                "Cannot connect to {} ({}): {}",
                self.config.network,
                check.rpc_url,
                check.error.as_deref().unwrap_or("unknown error")
            );
        }
        Ok(check)
    }

    /// Fund a test account via Friendbot and record the result.
    pub fn fund_test_account(&mut self, address: &str) -> Result<&FundbotResult> {
        let result = self.client.fund_account(address)?;
        self.funded_accounts.push(result);
        Ok(self.funded_accounts.last().unwrap())
    }

    /// Returns the number of contracts deployed in this session.
    pub fn contract_count(&self) -> usize {
        self.deployed_contracts.len()
    }
}

// ── Report ─────────────────────────────────────────────────────────────────

/// Summary of a testnet test run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestnetTestReport {
    pub network: String,
    pub rpc_url: String,
    pub health: HealthCheckResult,
    pub total_tests: u32,
    pub passed: u32,
    pub failed: u32,
    pub total_duration_ms: u64,
    pub contracts_deployed: u32,
    pub accounts_funded: u32,
    pub results: Vec<TestnetTestResult>,
    pub generated_at: String,
}

/// Result of a single testnet test case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestnetTestResult {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
    pub ledger: Option<u32>,
}

/// Runs a smoke test against the given session to confirm basic connectivity.
pub fn run_connectivity_smoke_test(session: &TestnetSession) -> TestnetTestResult {
    let start = Instant::now();
    match session.verify_connectivity() {
        Ok(_) => TestnetTestResult {
            name: "connectivity_smoke_test".into(),
            passed: true,
            duration_ms: start.elapsed().as_millis() as u64,
            error: None,
            ledger: session.client.latest_ledger().ok(),
        },
        Err(e) => TestnetTestResult {
            name: "connectivity_smoke_test".into(),
            passed: false,
            duration_ms: start.elapsed().as_millis() as u64,
            error: Some(e.to_string()),
            ledger: None,
        },
    }
}

/// Writes a testnet report to the given path as JSON.
pub fn write_testnet_report(report: &TestnetTestReport, path: &std::path::Path) -> Result<()> {
    let json = serde_json::to_string_pretty(report)?;
    std::fs::write(path, json)
        .with_context(|| format!("Failed to write testnet report to {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_rpc_urls() {
        assert!(SorobanNetwork::Testnet.rpc_url().starts_with("https://"));
        assert!(SorobanNetwork::Local
            .rpc_url()
            .starts_with("http://localhost"));
        assert!(SorobanNetwork::Testnet.supports_friendbot());
        assert!(!SorobanNetwork::Mainnet.supports_friendbot());
    }

    #[test]
    fn default_config_is_testnet() {
        let cfg = TestnetConfig::default();
        assert_eq!(cfg.network, SorobanNetwork::Testnet);
        assert_eq!(cfg.max_retries, 3);
    }

    #[test]
    fn local_config() {
        let cfg = TestnetConfig::local();
        assert_eq!(cfg.network, SorobanNetwork::Local);
        assert_eq!(cfg.max_retries, 1);
    }

    #[test]
    fn wasm_hash_computation() {
        let bytes = b"\0asm\x01\0\0\0";
        let hash = TestnetDeployer::compute_wasm_hash(bytes);
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn connectivity_smoke_test_returns_result_without_panic() {
        let session = TestnetSession::for_local();
        let result = run_connectivity_smoke_test(&session);
        // The local node may or may not be running; we just check it
        // doesn't panic and produces a structured result.
        assert!(!result.name.is_empty());
    }

    #[test]
    fn testnet_report_serialises() {
        let report = TestnetTestReport {
            network: "testnet".into(),
            rpc_url: SorobanNetwork::Testnet.rpc_url().into(),
            health: HealthCheckResult {
                healthy: true,
                network: "testnet".into(),
                rpc_url: SorobanNetwork::Testnet.rpc_url().into(),
                latest_ledger: Some(1000),
                latency_ms: 42,
                error: None,
            },
            total_tests: 3,
            passed: 2,
            failed: 1,
            total_duration_ms: 300,
            contracts_deployed: 1,
            accounts_funded: 2,
            results: vec![],
            generated_at: "2024-01-01T00:00:00Z".into(),
        };

        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("testnet"));
        assert!(json.contains("total_tests"));
    }

    #[test]
    fn session_for_testnet_factory() {
        let session = TestnetSession::for_testnet();
        assert_eq!(session.config.network, SorobanNetwork::Testnet);
        assert_eq!(session.funded_accounts.len(), 0);
        assert_eq!(session.contract_count(), 0);
    }

    #[test]
    fn custom_network() {
        let net = SorobanNetwork::Custom {
            rpc_url: "http://custom:8080/rpc".into(),
            passphrase: "Custom Network".into(),
        };
        assert_eq!(net.rpc_url(), "http://custom:8080/rpc");
        assert_eq!(net.passphrase(), "Custom Network");
        assert!(!net.supports_friendbot());
    }
}
