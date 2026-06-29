#![allow(clippy::result_large_err)]

use crate::utils::config;
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::Deserialize;
use std::time::Duration;

static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .expect("Failed to create HTTP client")
});

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
    let friendbot =
        friendbot_url(network)?.unwrap_or_else(|| "https://friendbot.stellar.org".to_string());
    let separator = if friendbot.contains('?') { '&' } else { '?' };
    let url = format!("{}{}addr={}", friendbot, separator, public_key);
    let res = HTTP_CLIENT
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Friendbot request failed for {}", network))?;
    if res.status() == 200 {
        Ok(())
    } else {
        anyhow::bail!("Friendbot returned status {}", res.status())
    }
}

pub async fn fetch_account(public_key: &str, network: &str) -> Result<AccountResponse> {
    let horizon = horizon_url(network)?;
    let url = format!("{}/accounts/{}", horizon, public_key);
    let res = HTTP_CLIENT
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to reach Horizon on {}", network))?;
    if res.status() == 200 {
        let account: AccountResponse = res
            .json()
            .await
            .with_context(|| "Failed to parse account response")?;
        Ok(account)
    } else {
        anyhow::bail!("Account not found on {}", network)
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
    pub extras: Option<HorizonErrorExtras>,
}

#[derive(Debug, Deserialize)]
struct HorizonErrorExtras {
    pub result_codes: Option<HorizonResultCodes>,
}

#[derive(Debug, Deserialize)]
struct HorizonResultCodes {
    pub transaction: Option<String>,
    pub operations: Option<Vec<String>>,
}

fn format_horizon_error_body(body: &str) -> Option<String> {
    let horizon_error = serde_json::from_str::<HorizonError>(body).ok()?;
    let mut parts = vec![horizon_error.title];

    if let Some(detail) = horizon_error.detail.filter(|detail| !detail.is_empty()) {
        parts.push(detail);
    }

    if let Some(result_codes) = horizon_error.extras.and_then(|extras| extras.result_codes) {
        let mut codes = Vec::new();
        if let Some(transaction) = result_codes.transaction {
            codes.push(format!("transaction={transaction}"));
        }
        if let Some(operations) = result_codes.operations {
            if !operations.is_empty() {
                codes.push(format!("operations={}", operations.join(",")));
            }
        }
        if !codes.is_empty() {
            parts.push(format!("result_codes: {}", codes.join("; ")));
        }
    }

    Some(parts.join(" - "))
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
    let signed_xdr = sign_transaction_xdr(transaction_xdr, secret_key, network)?;

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

        if let Some(detail) = format_horizon_error_body(&error_text) {
            anyhow::bail!("Transaction failed: {}", detail);
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

fn sign_transaction_xdr(transaction_xdr: &str, secret_key: &str, network: &str) -> Result<String> {
    let _network_passphrase = config::get_network_passphrase(network);

    let signed_mock = format!("signed_{}_with_{}", transaction_xdr, &secret_key[..8]);
    use base64::{engine::general_purpose, Engine as _};
    Ok(general_purpose::STANDARD.encode(signed_mock))
}
