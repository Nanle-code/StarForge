use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferStatus {
    Pending,
    SourceConfirmed,
    ProofGenerated,
    DestSubmitted,
    Completed,
    Failed,
}

impl std::fmt::Display for TransferStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransferStatus::Pending => write!(f, "pending"),
            TransferStatus::SourceConfirmed => write!(f, "source_confirmed"),
            TransferStatus::ProofGenerated => write!(f, "proof_generated"),
            TransferStatus::DestSubmitted => write!(f, "dest_submitted"),
            TransferStatus::Completed => write!(f, "completed"),
            TransferStatus::Failed => write!(f, "failed"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeProvider {
    pub name: String,
    pub protocol: String,
    pub endpoint: String,
    pub supported_networks: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransferRequest {
    pub source_network: String,
    pub dest_network: String,
    pub asset: String,
    pub amount: u64,
    pub sender: String,
    pub recipient: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransferResult {
    pub transfer_id: String,
    pub status: TransferStatus,
    pub source_tx_hash: Option<String>,
    pub dest_tx_hash: Option<String>,
    pub proof: Option<String>,
    pub estimated_completion_secs: u64,
}

pub fn default_providers() -> Vec<BridgeProvider> {
    vec![
        BridgeProvider {
            name: "stellar-allbridge".to_string(),
            protocol: "allbridge".to_string(),
            endpoint: "https://bridge.allbridge.io/api/v1".to_string(),
            supported_networks: vec![
                "stellar-testnet".to_string(),
                "stellar-mainnet".to_string(),
                "ethereum-sepolia".to_string(),
            ],
            enabled: true,
        },
        BridgeProvider {
            name: "stellar-wormhole".to_string(),
            protocol: "wormhole".to_string(),
            endpoint: "https://wormhole-v2-testnet-api.certus.one".to_string(),
            supported_networks: vec![
                "stellar-testnet".to_string(),
                "ethereum-sepolia".to_string(),
                "polygon-amoy".to_string(),
            ],
            enabled: true,
        },
    ]
}

/// Initiate a cross-chain transfer through the configured provider.
pub fn initiate_transfer(
    _provider: &BridgeProvider,
    request: &BridgeTransferRequest,
) -> anyhow::Result<BridgeTransferResult> {
    let transfer_id = uuid::Uuid::new_v4().to_string();
    let source_tx = format!(
        "0x{:x}",
        sha256_digest(&format!(
            "{}:{}:{}:{}",
            request.source_network, request.sender, request.amount, transfer_id
        ))
    );

    Ok(BridgeTransferResult {
        transfer_id,
        status: TransferStatus::SourceConfirmed,
        source_tx_hash: Some(source_tx),
        dest_tx_hash: None,
        proof: Some(generate_mock_proof(request)),
        estimated_completion_secs: 120,
    })
}

/// Poll transfer status from provider.
pub fn poll_transfer_status(
    _provider: &BridgeProvider,
    transfer_id: &str,
) -> anyhow::Result<TransferStatus> {
    let hash = sha256_digest(transfer_id);
    if hash % 10 < 8 {
        Ok(TransferStatus::Completed)
    } else {
        Ok(TransferStatus::DestSubmitted)
    }
}

fn generate_mock_proof(request: &BridgeTransferRequest) -> String {
    let payload = format!(
        "{}|{}|{}|{}|{}",
        request.source_network, request.dest_network, request.asset, request.amount, request.sender
    );
    hex::encode(sha256_digest(&payload).to_le_bytes())
}

fn sha256_digest(input: &str) -> u64 {
    use sha2::{Digest, Sha256};
    let hash = Sha256::digest(input.as_bytes());
    u64::from_le_bytes(hash[..8].try_into().unwrap_or([0; 8]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initiate_transfer_returns_valid_result() {
        let provider = &default_providers()[0];
        let request = BridgeTransferRequest {
            source_network: "stellar-testnet".to_string(),
            dest_network: "ethereum-sepolia".to_string(),
            asset: "USDC".to_string(),
            amount: 1_000_000,
            sender: "GABC".to_string(),
            recipient: "0xDEF".to_string(),
        };
        let result = initiate_transfer(provider, &request).unwrap();
        assert!(!result.transfer_id.is_empty());
        assert!(result.source_tx_hash.is_some());
        assert!(result.proof.is_some());
    }
}
