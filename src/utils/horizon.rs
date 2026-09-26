use crate::utils::{config, wallet_signer};
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::Deserialize;
use stellar_strkey::ed25519;
use stellar_xdr::curr::{
    AccountId, AlphaNum12, AlphaNum4, Asset, AssetCode12, AssetCode4, ChangeTrustAsset,
    ChangeTrustOp, Int64, Limits, MuxedAccount, Operation, OperationBody, PathPaymentStrictReceiveOp,
    PaymentOp, Preconditions, PublicKey, SequenceNumber, TimePoint, TimeBounds, Transaction,
    TransactionEnvelope, TransactionExt, TransactionV1Envelope, Uint256, VecM, WriteXdr,
};
use std::time::Duration;

fn build_http_client(timeout: Duration) -> Result<Client> {
    Client::builder()
        .timeout(timeout)
        .pool_max_idle_per_host(10)
        .build()
        .context("Failed to create Horizon HTTP client")
}

static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    build_http_client(Duration::from_secs(10)).expect("Failed to create shared Horizon HTTP client")
});

/// Shared HTTP client used for Horizon requests.
pub(crate) fn http_client() -> &'static Client {
    &HTTP_CLIENT
}

/// Issue an HTTP request against Horizon with bounded retries and exponential
/// backoff for transient failures (5xx / 429 / connection errors).
pub async fn send_with_retry<F, Fut>(make_request: F) -> Result<reqwest::Response>
where
    F: Fn() -> Fut,
    Fut: std::future::Future<Output = std::result::Result<reqwest::Response, reqwest::Error>>,
{
    const MAX_RETRIES: u32 = 3;
    let mut backoff = Duration::from_millis(150);

    for attempt in 1..=MAX_RETRIES {
        match make_request().await {
            Ok(res)
                if (res.status().is_server_error()
                    || res.status() == reqwest::StatusCode::TOO_MANY_REQUESTS)
                    && attempt < MAX_RETRIES =>
            {
                tokio::time::sleep(backoff).await;
                backoff *= 2;
            }
            Ok(res) => return Ok(res),
            Err(e) => {
                if attempt == MAX_RETRIES {
                    return Err(e).context("Horizon request failed after retries");
                }
                tokio::time::sleep(backoff).await;
                backoff *= 2;
            }
        }
    }
    anyhow::bail!("Exceeded maximum Horizon retries")
}

pub fn network_config(network: &str) -> Result<config::NetworkConfig> {
    let cfg = config::load()?;
    config::get_network_config(&cfg, network)
}

pub fn horizon_url(network: &str) -> Result<String> {
    Ok(network_config(network)?.horizon_url)
}

pub fn friendbot_url(network: &str) -> Result<Option<String>> {
    Ok(network_config(network)?.friendbot_url)
}

#[derive(Debug, Deserialize)]
pub struct AccountResponse {
    #[allow(dead_code)]
    pub id: String,
    pub sequence: String,
    pub balances: Vec<Balance>,
    #[allow(dead_code)]
    pub subentry_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct Balance {
    pub balance: String,
    pub asset_type: String,
    pub asset_code: Option<String>,
}

pub async fn fund_account(public_key: &str, network: &str) -> Result<()> {
    let net_cfg = network_config(network)?;

    // Gate Friendbot by verified network identity & passphrase
    if network.eq_ignore_ascii_case("mainnet")
        || net_cfg.passphrase.as_deref() == Some("Public Global Stellar Network ; September 2015")
        || net_cfg.horizon_url.contains("horizon.stellar.org")
    {
        anyhow::bail!(
            "Friendbot cannot be used on mainnet or production networks. \
             Friendbot is only available on test networks (e.g. testnet, standalone, futurenet).\n\
             Verify your active network: starforge network show"
        );
    }

    let friendbot = match net_cfg.friendbot_url {
        Some(url) => url,
        None if network.eq_ignore_ascii_case("testnet") => {
            "https://friendbot.stellar.org".to_string()
        }
        None => {
            anyhow::bail!(
                "Network '{}' does not have a Friendbot faucet URL configured.\n\
                 Add one using: starforge network add <name> --horizon-url <url> --friendbot-url <url>",
                network
            );
        }
    };

    let separator = if friendbot.contains('?') { '&' } else { '?' };
    let url = format!("{}{}addr={}", friendbot, separator, public_key);

    let res = send_with_retry(|| HTTP_CLIENT.get(&url).send())
        .await
        .with_context(|| {
            format!(
                "Could not reach Friendbot on '{}'. Check your internet connection.",
                network
            )
        })?;

    if res.status() == 200 {
        Ok(())
    } else if res.status() == 400 {
        anyhow::bail!(
            "Friendbot rejected the funding request for '{}'.\n\
             This usually means the account has already been funded on {}.\n\
             Check the balance: starforge wallet show",
            public_key,
            network
        )
    } else {
        anyhow::bail!(
            "Friendbot returned HTTP {} for network '{}'.\n\
             Friendbot is only available on testnet — verify your active network: starforge network show",
            res.status(),
            network
        )
    }
}

pub async fn fetch_account(public_key: &str, network: &str) -> Result<AccountResponse> {
    let horizon = horizon_url(network)?;
    let url = format!("{}/accounts/{}", horizon.trim_end_matches('/'), public_key);
    let res = send_with_retry(|| HTTP_CLIENT.get(&url).send())
        .await
        .with_context(|| {
            format!(
                "Could not reach Horizon on '{}'. Check your internet connection or run: starforge network test",
                network
            )
        })?;

    if res.status() == 200 {
        let account: AccountResponse = res
            .json()
            .await
            .with_context(|| "Failed to parse account response")?;
        Ok(account)
    } else if res.status() == 404 {
        anyhow::bail!(
            "Account '{}' not found on {}.\n\
             The account may not have been activated yet.\n\
             Fund it with: starforge wallet fund",
            public_key,
            network
        )
    } else {
        anyhow::bail!(
            "Horizon returned HTTP {} for account '{}' on {}",
            res.status(),
            public_key,
            network
        )
    }
}

pub async fn check_network(network: &str) -> bool {
    match horizon_url(network) {
        Ok(url) => check_horizon_endpoint(&url).await,
        Err(_) => false,
    }
}

pub async fn check_horizon_endpoint(horizon_url: &str) -> bool {
    let base = horizon_url.trim_end_matches('/');
    let health_url = format!("{}/", base);
    HTTP_CLIENT
        .get(&health_url)
        .send()
        .await
        .map(|r| r.status() == 200)
        .unwrap_or(false)
}

#[derive(Debug, Deserialize)]
struct HorizonRoot {
    network_passphrase: String,
}

/// Read the network identity advertised by Horizon.
pub async fn fetch_network_passphrase(network: &str) -> Result<String> {
    let endpoint = horizon_url(network)?;
    let url = format!("{}/", endpoint.trim_end_matches('/'));
    let response = HTTP_CLIENT
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Could not reach Horizon endpoint '{}'", endpoint))?;
    if !response.status().is_success() {
        anyhow::bail!(
            "Horizon endpoint '{}' returned HTTP {}",
            endpoint,
            response.status()
        );
    }
    let root: HorizonRoot = response.json().await.with_context(|| {
        format!(
            "Horizon endpoint '{}' did not provide network identity",
            endpoint
        )
    })?;
    Ok(root.network_passphrase)
}

pub async fn check_soroban_rpc(soroban_url: &str) -> bool {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "getLatestLedger",
        "params": []
    });
    HTTP_CLIENT
        .post(soroban_url)
        .json(&req)
        .send()
        .await
        .map(|r| r.status() == 200)
        .unwrap_or(false)
}

pub fn build_transaction_query_url(
    public_key: &str,
    network: &str,
    filter: &TxFilter,
) -> Result<String> {
    let horizon = horizon_url(network)?;
    let mut url = format!(
        "{}/accounts/{}/transactions?order={}&limit={}",
        horizon,
        public_key,
        filter.order.as_deref().unwrap_or("desc"),
        filter.limit.min(200)
    );

    if let Some(ref cursor) = filter.cursor {
        url.push_str(&format!("&cursor={}", cursor));
    }
    if let Some(ref type_filter) = filter.type_filter {
        url.push_str(&format!("&type={}", type_filter));
    }

    Ok(url)
}

#[derive(Debug, Deserialize, Clone)]
pub struct TransactionRecord {
    pub hash: String,
    pub successful: bool,
    pub operation_count: u32,
    pub fee_charged: String,
    pub created_at: String,
    pub memo_type: Option<String>,
    pub memo: Option<String>,
    pub source_account: Option<String>,
    #[serde(rename = "type")]
    pub transaction_type: Option<String>,
    pub paging_token: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct FeeStats {
    #[serde(rename = "low_fee")]
    pub low_fee: String,
    #[serde(rename = "mode_fee")]
    pub mode_fee: String,
    #[serde(rename = "high_fee")]
    pub high_fee: String,
}

pub async fn fetch_fee_stats(network: &str) -> Result<FeeStats> {
    let horizon = horizon_url(network)?;
    let url = format!("{}/fee_stats", horizon);
    let res = HTTP_CLIENT
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch fee stats from {}", network))?;
    if res.status() == 200 {
        let stats: FeeStats = res
            .json()
            .await
            .with_context(|| "Failed to parse fee stats response")?;
        Ok(stats)
    } else {
        anyhow::bail!("Failed to get fee stats: HTTP {}", res.status())
    }
}

#[derive(Debug, Deserialize)]
struct TransactionsResponse {
    #[serde(rename = "_embedded")]
    embedded: TransactionsEmbedded,
}

#[derive(Debug, Deserialize)]
struct TransactionsEmbedded {
    records: Vec<TransactionRecord>,
}

pub struct TxFilter {
    pub limit: u8,
    pub cursor: Option<String>,
    pub order: Option<String>,
    pub type_filter: Option<String>,
    pub after: Option<String>,
    pub before: Option<String>,
    pub successful_only: Option<bool>,
}

#[allow(dead_code)]
pub async fn fetch_transactions(
    public_key: &str,
    network: &str,
    limit: u8,
) -> Result<Vec<TransactionRecord>> {
    fetch_transactions_filtered(
        public_key,
        network,
        TxFilter {
            limit,
            cursor: None,
            order: None,
            type_filter: None,
            after: None,
            before: None,
            successful_only: None,
        },
    )
    .await
}

pub async fn fetch_transactions_filtered(
    public_key: &str,
    network: &str,
    filter: TxFilter,
) -> Result<Vec<TransactionRecord>> {
    let url = build_transaction_query_url(public_key, network, &filter)?;
    let res = HTTP_CLIENT.get(&url).send().await.with_context(|| {
        format!(
            "Account '{}' not found on {}. Has it been funded?",
            public_key, network
        )
    })?;

    let parsed: TransactionsResponse = res
        .json()
        .await
        .with_context(|| "Failed to parse transactions response")?;

    let mut records = parsed.embedded.records;

    if let Some(ref type_filter) = filter.type_filter {
        records.retain(|tx| tx.transaction_type.as_deref() == Some(type_filter.as_str()));
    }
    if let Some(ref after) = filter.after {
        records.retain(|tx| tx.created_at.as_str() >= after.as_str());
    }
    if let Some(ref before) = filter.before {
        records.retain(|tx| tx.created_at.as_str() <= before.as_str());
    }
    if let Some(successful_only) = filter.successful_only {
        records.retain(|tx| tx.successful == successful_only);
    }

    Ok(records)
}

#[derive(Debug, Deserialize)]
pub struct TransactionSimulationResult {
    pub transaction_xdr: String,
    pub fee: u64,
}

#[derive(Debug, Deserialize)]
pub struct TransactionSubmitResult {
    pub hash: String,
    pub successful: bool,
}

#[derive(Debug, Deserialize)]
struct HorizonError {
    pub title: String,
    pub detail: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BatchPaymentOp {
    pub destination: String,
    pub amount: String,
    pub asset_code: Option<String>,
    pub asset_issuer: Option<String>,
}

pub fn build_and_simulate_batch(
    source: &str,
    operations: &[BatchPaymentOp],
    sequence: &str,
    network: &str,
) -> Result<TransactionSimulationResult> {
    let tx_xdr = build_batch_transaction_xdr(source, operations, sequence, network)?;

    let base_fee_per_op = 100_000u64;
    let estimated_fee = base_fee_per_op.saturating_mul(operations.len() as u64);

    Ok(TransactionSimulationResult {
        transaction_xdr: tx_xdr,
        fee: estimated_fee,
    })
}

pub fn build_and_simulate_account_merge(
    source: &str,
    destination: &str,
    sequence: &str,
    network: &str,
) -> Result<TransactionSimulationResult> {
    let tx_xdr = build_account_merge_transaction_xdr(source, destination, sequence, network)?;

    Ok(TransactionSimulationResult {
        transaction_xdr: tx_xdr,
        fee: 100_000,
    })
}

pub fn build_and_simulate_payment(
    source: &str,
    destination: &str,
    amount: &str,
    asset_code: Option<&str>,
    asset_issuer: Option<&str>,
    sequence: &str,
    network: &str,
) -> Result<TransactionSimulationResult> {
    let tx_xdr = build_payment_transaction_xdr(
        source,
        destination,
        amount,
        asset_code,
        asset_issuer,
        sequence,
        network,
    )?;

    let estimated_fee = 100000u64;

    Ok(TransactionSimulationResult {
        transaction_xdr: tx_xdr,
        fee: estimated_fee,
    })
}

pub async fn submit_payment_transaction(
    transaction_xdr: &str,
    secret_key: &str,
    network: &str,
) -> Result<TransactionSubmitResult> {
    let request = wallet_signer::SigningRequest::local_secret(
        zeroize::Zeroizing::new(secret_key.to_string()),
        network,
    );
    submit_payment_with_signing(transaction_xdr, &request, network).await
}

pub async fn submit_payment_with_signing(
    transaction_xdr: &str,
    request: &wallet_signer::SigningRequest,
    network: &str,
) -> Result<TransactionSubmitResult> {
    crate::utils::network_guard::verify(network).await?;
    let signed_xdr = wallet_signer::sign_transaction_xdr(transaction_xdr, request)?;

    let horizon = horizon_url(network)?;
    let url = format!("{}/transactions", horizon);
    let form_data = [("tx", urlencoding::encode(&signed_xdr))];

    let res = HTTP_CLIENT
        .post(&url)
        .form(&form_data)
        .send()
        .await
        .with_context(|| "Failed to submit transaction to Horizon")?;

    let status = res.status();

    if status == 200 {
        let result: serde_json::Value = res
            .json()
            .await
            .with_context(|| "Failed to parse transaction response")?;

        let hash = result
            .get("hash")
            .and_then(|h| h.as_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(TransactionSubmitResult {
            hash,
            successful: true,
        })
    } else {
        let error_text = res
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());

        if let Ok(horizon_error) = serde_json::from_str::<HorizonError>(&error_text) {
            let detail = horizon_error
                .detail
                .unwrap_or_else(|| "No additional details".to_string());
            anyhow::bail!("Transaction failed: {} - {}", horizon_error.title, detail);
        } else {
            anyhow::bail!("Transaction failed with status {}: {}", status, error_text);
        }
    }
}

pub async fn submit_multisig_transaction(
    signed_transaction_xdr: &str,
    network: &str,
) -> Result<TransactionSubmitResult> {
    let horizon = horizon_url(network)?;
    let url = format!("{}/transactions", horizon);
    let form_data = [("tx", urlencoding::encode(signed_transaction_xdr))];

    let res = HTTP_CLIENT
        .post(&url)
        .form(&form_data)
        .send()
        .await
        .with_context(|| "Failed to submit transaction to Horizon")?;

    let status = res.status();
    if status == 200 {
        let result: serde_json::Value = res
            .json()
            .await
            .with_context(|| "Failed to parse transaction response")?;

        let hash = result
            .get("hash")
            .and_then(|h| h.as_str())
            .unwrap_or("unknown")
            .to_string();

        Ok(TransactionSubmitResult {
            hash,
            successful: true,
        })
    } else {
        let error_text = res
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".to_string());

        if let Ok(horizon_error) = serde_json::from_str::<HorizonError>(&error_text) {
            let detail = horizon_error
                .detail
                .unwrap_or_else(|| "No additional details".to_string());
            anyhow::bail!("Transaction failed: {} - {}", horizon_error.title, detail);
        } else {
            anyhow::bail!("Transaction failed with status {}: {}", status, error_text);
        }
    }
}

fn build_batch_transaction_xdr(
    source: &str,
    operations: &[BatchPaymentOp],
    sequence: &str,
    network: &str,
) -> Result<String> {
    if operations.is_empty() {
        anyhow::bail!("Batch transaction requires at least one operation");
    }

    let _network_passphrase = config::get_network_passphrase(network);

    let op_parts: Vec<String> = operations
        .iter()
        .enumerate()
        .map(|(i, op)| {
            let asset_info = match (&op.asset_code, &op.asset_issuer) {
                (None, None) => "native".to_string(),
                (Some(code), Some(issuer)) => format!("{}:{}", code, issuer),
                _ => return Err(anyhow::anyhow!("Invalid asset in operation {}", i + 1)),
            };
            Ok(format!(
                "pay:{}:{}:{}",
                op.destination, op.amount, asset_info
            ))
        })
        .collect::<Result<Vec<_>>>()?;

    let mock_xdr = format!(
        "mock_batch_tx_{}_{}_{}_{}",
        source,
        op_parts.join("|"),
        sequence,
        network
    );

    use base64::{engine::general_purpose, Engine as _};
    Ok(general_purpose::STANDARD.encode(mock_xdr))
}

fn build_account_merge_transaction_xdr(
    source: &str,
    destination: &str,
    sequence: &str,
    network: &str,
) -> Result<String> {
    let _network_passphrase = network_passphrase(network);

    let mock_xdr = format!(
        "mock_merge_tx_{}_{}_{}_{}",
        source, destination, sequence, network
    );

    use base64::{engine::general_purpose, Engine as _};
    Ok(general_purpose::STANDARD.encode(mock_xdr))
}

fn network_passphrase(network: &str) -> String {
    config::get_network_passphrase(network)
}

fn build_payment_transaction_xdr(
    source: &str,
    destination: &str,
    amount: &str,
    asset_code: Option<&str>,
    asset_issuer: Option<&str>,
    sequence: &str,
    network: &str,
) -> Result<String> {
    let _network_passphrase = network_passphrase(network);

    let asset_info = match (asset_code, asset_issuer) {
        (None, None) => "native".to_string(),
        (Some(code), Some(issuer)) => format!("{}:{}", code, issuer),
        _ => return Err(anyhow::anyhow!("Invalid asset specification")),
    };

    let mock_xdr = format!(
        "mock_payment_tx_{}_{}_{}_{}_{}",
        source, destination, amount, asset_info, sequence
    );

    use base64::{engine::general_purpose, Engine as _};
    Ok(general_purpose::STANDARD.encode(mock_xdr))
}

/// Build a ChangeTrust operation transaction
pub fn build_change_trust_transaction(
    source_account: &str,
    asset_code: &str,
    asset_issuer: &str,
    limit: Option<&str>,
    sequence: u64,
    network: &str,
) -> Result<String> {
    let source_pk = ed25519::PublicKey::from_string(source_account)
        .with_context(|| format!("Invalid source account: {}", source_account))?;
    let issuer_pk = ed25519::PublicKey::from_string(asset_issuer)
        .with_context(|| format!("Invalid asset issuer: {}", asset_issuer))?;

    let asset = parse_asset(asset_code, asset_issuer)?;
    let trust_asset = match asset {
        Asset::Native => anyhow::bail!("Cannot create trustline for native XLM"),
        Asset::CreditAlphanum4(a) => ChangeTrustAsset::CreditAlphanum4(a),
        Asset::CreditAlphanum12(a) => ChangeTrustAsset::CreditAlphanum12(a),
        Asset::PoolShare(_) => anyhow::bail!("Pool share assets not supported yet"),
    };

    let limit_amount = if let Some(lim) = limit {
        parse_amount(lim)?
    } else {
        i64::MAX // Max trustline
    };

    let change_trust_op = ChangeTrustOp {
        line: trust_asset,
        limit: Int64(limit_amount),
    };

    let operation = Operation {
        source_account: None,
        body: OperationBody::ChangeTrust(change_trust_op),
    };

    let tx = build_transaction(
        &source_pk,
        vec![operation],
        sequence,
        100, // base fee
        network,
    )?;

    envelope_to_base64(&tx)
}

/// Build a Payment operation transaction
pub fn build_payment_transaction(
    source_account: &str,
    destination: &str,
    amount: &str,
    asset_code: Option<&str>,
    asset_issuer: Option<&str>,
    sequence: u64,
    network: &str,
) -> Result<String> {
    let source_pk = ed25519::PublicKey::from_string(source_account)
        .with_context(|| format!("Invalid source account: {}", source_account))?;
    let dest_pk = ed25519::PublicKey::from_string(destination)
        .with_context(|| format!("Invalid destination account: {}", destination))?;

    let asset = match (asset_code, asset_issuer) {
        (None, None) => Asset::Native,
        (Some(code), Some(issuer)) => parse_asset(code, issuer)?,
        _ => anyhow::bail!("Asset code and issuer must be provided together"),
    };

    let amount_stroops = parse_amount(amount)?;

    let payment_op = PaymentOp {
        destination: MuxedAccount::Ed25519(Uint256(dest_pk.0)),
        asset,
        amount: Int64(amount_stroops),
    };

    let operation = Operation {
        source_account: None,
        body: OperationBody::Payment(payment_op),
    };

    let tx = build_transaction(&source_pk, vec![operation], sequence, 100, network)?;

    envelope_to_base64(&tx)
}

/// Build a PathPaymentStrictReceive operation transaction
pub fn build_path_payment_transaction(
    source_account: &str,
    send_asset_code: Option<&str>,
    send_asset_issuer: Option<&str>,
    send_max: &str,
    destination: &str,
    dest_asset_code: Option<&str>,
    dest_asset_issuer: Option<&str>,
    dest_amount: &str,
    path: Vec<Asset>,
    sequence: u64,
    network: &str,
) -> Result<String> {
    let source_pk = ed25519::PublicKey::from_string(source_account)
        .with_context(|| format!("Invalid source account: {}", source_account))?;
    let dest_pk = ed25519::PublicKey::from_string(destination)
        .with_context(|| format!("Invalid destination account: {}", destination))?;

    let send_asset = match (send_asset_code, send_asset_issuer) {
        (None, None) => Asset::Native,
        (Some(code), Some(issuer)) => parse_asset(code, issuer)?,
        _ => anyhow::bail!("Send asset code and issuer must be provided together"),
    };

    let dest_asset = match (dest_asset_code, dest_asset_issuer) {
        (None, None) => Asset::Native,
        (Some(code), Some(issuer)) => parse_asset(code, issuer)?,
        _ => anyhow::bail!("Destination asset code and issuer must be provided together"),
    };

    let send_max_stroops = parse_amount(send_max)?;
    let dest_amount_stroops = parse_amount(dest_amount)?;

    let path_vec = VecM::try_from(path)
        .map_err(|_| anyhow::anyhow!("Path too long (max 5 intermediate assets)"))?;

    let path_payment_op = PathPaymentStrictReceiveOp {
        send_asset,
        send_max: Int64(send_max_stroops),
        destination: MuxedAccount::Ed25519(Uint256(dest_pk.0)),
        dest_asset,
        dest_amount: Int64(dest_amount_stroops),
        path: path_vec,
    };

    let operation = Operation {
        source_account: None,
        body: OperationBody::PathPaymentStrictReceive(path_payment_op),
    };

    let tx = build_transaction(&source_pk, vec![operation], sequence, 100, network)?;

    envelope_to_base64(&tx)
}

/// Helper: Parse asset code and issuer into Asset
fn parse_asset(asset_code: &str, asset_issuer: &str) -> Result<Asset> {
    let issuer_pk = ed25519::PublicKey::from_string(asset_issuer)
        .with_context(|| format!("Invalid asset issuer: {}", asset_issuer))?;

    let issuer = AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(issuer_pk.0)));

    if asset_code.len() <= 4 {
        let mut code_bytes = [0u8; 4];
        let bytes = asset_code.as_bytes();
        code_bytes[..bytes.len()].copy_from_slice(bytes);

        Ok(Asset::CreditAlphanum4(AlphaNum4 {
            asset_code: AssetCode4(code_bytes),
            issuer,
        }))
    } else if asset_code.len() <= 12 {
        let mut code_bytes = [0u8; 12];
        let bytes = asset_code.as_bytes();
        code_bytes[..bytes.len()].copy_from_slice(bytes);

        Ok(Asset::CreditAlphanum12(AlphaNum12 {
            asset_code: AssetCode12(code_bytes),
            issuer,
        }))
    } else {
        anyhow::bail!("Asset code must be 1-12 characters")
    }
}

/// Helper: Parse amount string into stroops (7 decimal places)
fn parse_amount(amount: &str) -> Result<i64> {
    let parts: Vec<&str> = amount.split('.').collect();
    let whole = parts[0].parse::<i64>().with_context(|| "Invalid amount")?;

    let fractional = if parts.len() > 1 {
        let frac_str = parts[1];
        if frac_str.len() > 7 {
            anyhow::bail!("Amount precision cannot exceed 7 decimal places");
        }
        let padded = format!("{:0<7}", frac_str);
        padded.parse::<i64>().with_context(|| "Invalid amount")?
    } else {
        0
    };

    Ok(whole
        .checked_mul(10_000_000)
        .and_then(|w| w.checked_add(fractional))
        .ok_or_else(|| anyhow::anyhow!("Amount overflow"))?)
}

/// Helper: Build a generic transaction with operations
fn build_transaction(
    source: &ed25519::PublicKey,
    operations: Vec<Operation>,
    sequence: u64,
    base_fee: u32,
    _network: &str,
) -> Result<TransactionV1Envelope> {
    let ops_vec = VecM::try_from(operations)
        .map_err(|_| anyhow::anyhow!("Too many operations in transaction"))?;

    let tx = Transaction {
        source_account: MuxedAccount::Ed25519(Uint256(source.0)),
        fee: base_fee,
        seq_num: SequenceNumber(sequence as i64),
        cond: Preconditions::None,
        memo: stellar_xdr::curr::Memo::None,
        operations: ops_vec,
        ext: TransactionExt::V0,
    };

    Ok(TransactionV1Envelope {
        tx,
        signatures: VecM::try_from(Vec::new())
            .map_err(|_| anyhow::anyhow!("Failed to create signatures vector"))?,
    })
}

/// Helper: Encode transaction envelope to base64 XDR
fn envelope_to_base64(envelope: &TransactionV1Envelope) -> Result<String> {
    let tx_envelope = TransactionEnvelope::Tx(envelope.clone());
    let xdr_bytes = tx_envelope
        .to_xdr(Limits::none())
        .with_context(|| "Failed to encode transaction to XDR")?;
    use base64::{engine::general_purpose, Engine as _};
    Ok(general_purpose::STANDARD.encode(xdr_bytes))
}

/// Fetch payment path from Horizon for path payments
#[derive(Debug, Deserialize)]
pub struct PathRecord {
    pub source_asset_type: String,
    pub source_asset_code: Option<String>,
    pub source_asset_issuer: Option<String>,
    pub source_amount: String,
    pub destination_asset_type: String,
    pub destination_asset_code: Option<String>,
    pub destination_asset_issuer: Option<String>,
    pub destination_amount: String,
    pub path: Vec<PathAsset>,
}

#[derive(Debug, Deserialize)]
pub struct PathAsset {
    pub asset_type: String,
    pub asset_code: Option<String>,
    pub asset_issuer: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PathsResponse {
    #[serde(rename = "_embedded")]
    embedded: PathsEmbedded,
}

#[derive(Debug, Deserialize)]
struct PathsEmbedded {
    records: Vec<PathRecord>,
}

/// Find payment paths using Horizon's /paths/strict-receive endpoint
///
/// # Security Note
///
/// This function transmits only public Stellar account addresses (source_account,
/// destination_account) to Horizon, never secret keys. All built-in networks
/// (testnet, mainnet) are enforced to use HTTPS in `config::get_network_config()`.
/// Custom networks using non-HTTPS URLs generate a warning to the user.
///
/// CodeQL may flag this as "cleartext transmission of sensitive information" due to
/// taint tracking from `validate_secret_key()` on the Wallet struct, but the secret_key
/// field is never accessed or transmitted by this function.
pub async fn find_payment_paths(
    source_account: &str,
    destination_account: &str,
    destination_asset_code: Option<&str>,
    destination_asset_issuer: Option<&str>,
    destination_amount: &str,
    network: &str,
) -> Result<Vec<PathRecord>> {
    let horizon = horizon_url(network)?;
    
    let dest_asset = match (destination_asset_code, destination_asset_issuer) {
        (None, None) => "native".to_string(),
        (Some(code), Some(issuer)) => format!("{}:{}", code, issuer),
        _ => anyhow::bail!("Destination asset code and issuer must be provided together"),
    };

    let url = format!(
        "{}/paths/strict-receive?source_account={}&destination_account={}&destination_asset_type={}&destination_amount={}",
        horizon,
        source_account,
        destination_account,
        if destination_asset_code.is_none() { "native" } else { "credit_alphanum4" },
        destination_amount
    );

    let url = if let (Some(code), Some(issuer)) = (destination_asset_code, destination_asset_issuer) {
        format!("{}&destination_asset_code={}&destination_asset_issuer={}", url, code, issuer)
    } else {
        url
    };

    let res = send_with_retry(|| HTTP_CLIENT.get(&url).send())
        .await
        .with_context(|| "Failed to fetch payment paths from Horizon")?;

    if res.status() == 200 {
        let paths_response: PathsResponse = res
            .json()
            .await
            .with_context(|| "Failed to parse paths response")?;
        Ok(paths_response.embedded.records)
    } else {
        anyhow::bail!("Failed to find payment paths: HTTP {}", res.status())
    }
}
