use anyhow::{Result, Context};
use serde::Deserialize;

pub fn horizon_url(network: &str) -> &'static str {
    match network {
        "mainnet" => "https://horizon.stellar.org",
        _ => "https://horizon-testnet.stellar.org",
    }
}

#[derive(Debug, Deserialize)]
pub struct AccountResponse {
    pub id: String,
    pub sequence: String,
    pub balances: Vec<Balance>,
    pub subentry_count: u32,
}

#[derive(Debug, Deserialize)]
pub struct Balance {
    pub balance: String,
    pub asset_type: String,
    pub asset_code: Option<String>,
}

pub fn fund_account(public_key: &str) -> Result<()> {
    let url = format!("https://friendbot.stellar.org?addr={}", public_key);
    let res = ureq::get(&url).call()
        .with_context(|| "Friendbot request failed")?;
    if res.status() == 200 {
        Ok(())
    } else {
        anyhow::bail!("Friendbot returned status {}", res.status())
    }
}

pub fn fetch_account(public_key: &str, network: &str) -> Result<AccountResponse> {
    let url = format!("{}/accounts/{}", horizon_url(network), public_key);
    let res = ureq::get(&url).call()
        .with_context(|| format!("Failed to reach Horizon on {}", network))?;
    if res.status() == 200 {
        let account: AccountResponse = res.into_json()
            .with_context(|| "Failed to parse account response")?;
        Ok(account)
    } else {
        anyhow::bail!("Account not found on {}", network)
    }
}

pub fn check_network(network: &str) -> bool {
    let url = format!("{}/", horizon_url(network));
    ureq::get(&url).call().map(|r| r.status() == 200).unwrap_or(false)
}

#[derive(Debug, Deserialize)]
pub struct TransactionRecord {
    pub hash: String,
    pub successful: bool,
    pub operation_count: u32,
    pub fee_charged: String,
    pub created_at: String,
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

pub fn fetch_transactions(
    public_key: &str,
    network: &str,
    limit: u8,
) -> Result<Vec<TransactionRecord>> {
    let url = format!(
        "{}/accounts/{}/transactions?order=desc&limit={}",
        horizon_url(network),
        public_key,
        limit
    );

    let res = ureq::get(&url).call().with_context(|| {
        format!(
            "Account '{}' not found on {}. Has it been funded?",
            public_key, network
        )
    })?;

    let parsed: TransactionsResponse = res
        .into_json()
        .with_context(|| "Failed to parse transactions response")?;

    Ok(parsed.embedded.records)
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

pub fn build_and_simulate_payment(
    source: &str,
    destination: &str,
    amount: &str,
    asset_code: Option<&str>,
    asset_issuer: Option<&str>,
    sequence: &str,
    network: &str,
) -> Result<TransactionSimulationResult> {
    // For now, we'll use a simplified approach by calling the stellar CLI
    // In a production implementation, you'd use stellar-xdr to build the transaction properly
    
    // Build transaction XDR using stellar-sdk patterns
    let tx_xdr = build_payment_transaction_xdr(
        source, destination, amount, asset_code, asset_issuer, sequence, network
    )?;
    
    // Simulate the transaction
    let _url = format!("{}/transactions", horizon_url(network));
    let _form_data = format!("tx={}", urlencoding::encode(&tx_xdr));
    
    // For simulation, we'll estimate the fee
    let estimated_fee = 100000u64; // 0.00001 XLM in stroops
    
    Ok(TransactionSimulationResult {
        transaction_xdr: tx_xdr,
        fee: estimated_fee,
    })
}

pub fn submit_payment_transaction(
    transaction_xdr: &str,
    secret_key: &str,
    network: &str,
) -> Result<TransactionSubmitResult> {
    // Sign the transaction
    let signed_xdr = sign_transaction_xdr(transaction_xdr, secret_key, network)?;
    
    // Submit to Horizon
    let url = format!("{}/transactions", horizon_url(network));
    let form_data = format!("tx={}", urlencoding::encode(&signed_xdr));
    
    let res = ureq::post(&url)
        .set("Content-Type", "application/x-www-form-urlencoded")
        .send_string(&form_data)
        .with_context(|| "Failed to submit transaction to Horizon")?;

    let status = res.status();
    
    if status == 200 {
        let result: serde_json::Value = res.into_json()
            .with_context(|| "Failed to parse transaction response")?;
        
        let hash = result.get("hash")
            .and_then(|h| h.as_str())
            .unwrap_or("unknown")
            .to_string();
            
        Ok(TransactionSubmitResult {
            hash,
            successful: true,
        })
    } else {
        let error_text = res.into_string().unwrap_or_else(|_| "Unknown error".to_string());
        
        // Try to parse Horizon error format
        if let Ok(horizon_error) = serde_json::from_str::<HorizonError>(&error_text) {
            let detail = horizon_error.detail.unwrap_or_else(|| "No additional details".to_string());
            anyhow::bail!("Transaction failed: {} - {}", horizon_error.title, detail);
        } else {
            anyhow::bail!("Transaction failed with status {}: {}", status, error_text);
        }
    }
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
    // This is a simplified mock implementation
    // In production, you'd use stellar-xdr crate to build proper transaction XDR
    
    let _network_passphrase = match network {
        "mainnet" => "Public Global Stellar Network ; September 2015",
        _ => "Test SDF Network ; September 2015",
    };
    
    // Mock XDR generation - in reality this would be much more complex
    let asset_info = match (asset_code, asset_issuer) {
        (None, None) => "native".to_string(),
        (Some(code), Some(issuer)) => format!("{}:{}", code, issuer),
        _ => return Err(anyhow::anyhow!("Invalid asset specification")),
    };
    
    // Generate a mock transaction XDR
    // In production, use stellar-xdr to build proper TransactionEnvelope
    let mock_xdr = format!(
        "mock_payment_tx_{}_{}_{}_{}_{}",
        source, destination, amount, asset_info, sequence
    );
    
    use base64::{Engine as _, engine::general_purpose};
    Ok(general_purpose::STANDARD.encode(mock_xdr))
}

fn sign_transaction_xdr(transaction_xdr: &str, secret_key: &str, network: &str) -> Result<String> {
    // This is a simplified mock implementation
    // In production, you'd use stellar-xdr and ed25519 signing
    
    let _network_passphrase = match network {
        "mainnet" => "Public Global Stellar Network ; September 2015",
        _ => "Test SDF Network ; September 2015",
    };
    
    // Mock signing - in reality this would involve:
    // 1. Decode the transaction XDR
    // 2. Hash the transaction with network passphrase
    // 3. Sign with ed25519 private key
    // 4. Create TransactionEnvelope with signature
    // 5. Re-encode to XDR
    
    let signed_mock = format!("signed_{}_with_{}", transaction_xdr, &secret_key[..8]);
    use base64::{Engine as _, engine::general_purpose};
    Ok(general_purpose::STANDARD.encode(signed_mock))
}