use crate::utils::config;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use stellar_strkey::ed25519::PublicKey as StellarPublicKey;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MultiSigAccount {
    pub name: String,
    pub account_id: String,
    pub signers: Vec<Signer>,
    pub thresholds: Thresholds,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Signer {
    pub public_key: String,
    pub weight: u8,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Thresholds {
    pub low: u8,
    pub medium: u8,
    pub high: u8,
}

impl Default for Thresholds {
    fn default() -> Self {
        Self {
            low: 1,
            medium: 1,
            high: 1,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiSigTransaction {
    pub id: String,
    pub account_id: String,
    pub transaction_xdr: String,
    pub signatures: Vec<Signature>,
    pub threshold_required: u8,
    pub current_weight: u8,
    pub status: TransactionStatus,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Signature {
    pub signer_key: String,
    pub signature: String,
    pub signed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransactionStatus {
    Pending,
    ReadyToSubmit,
    Submitted,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MultisigSetupStep {
    pub title: String,
    pub command: String,
}

fn multisig_dir() -> Result<PathBuf> {
    let dir = crate::utils::config::get_data_dir()?.join("multisig");
    if !dir.exists() {
        fs::create_dir_all(&dir).with_context(|| format!("Failed to create {}", dir.display()))?;
    }
    Ok(dir)
}

fn account_path(name: &str) -> Result<PathBuf> {
    Ok(multisig_dir()?.join(format!("{}.json", name)))
}

pub fn save_account(account: &MultiSigAccount) -> Result<()> {
    let path = account_path(&account.name)?;
    fs::write(&path, serde_json::to_string_pretty(account)?)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

pub fn load_account(name: &str) -> Result<MultiSigAccount> {
    let path = account_path(name)?;
    let s =
        fs::read_to_string(&path).with_context(|| format!("Failed to read {}", path.display()))?;
    let acct: MultiSigAccount =
        serde_json::from_str(&s).with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(acct)
}

pub fn list_accounts() -> Result<Vec<MultiSigAccount>> {
    let dir = multisig_dir()?;
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).with_context(|| format!("Failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(s) = fs::read_to_string(&path) else {
            continue;
        };
        if let Ok(acct) = serde_json::from_str::<MultiSigAccount>(&s) {
            out.push(acct);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

pub fn load_transaction(path: &Path) -> Result<MultiSigTransaction> {
    let s =
        fs::read_to_string(path).with_context(|| format!("Failed to read {}", path.display()))?;
    let tx: MultiSigTransaction =
        serde_json::from_str(&s).with_context(|| format!("Failed to parse {}", path.display()))?;
    Ok(tx)
}

pub fn save_transaction(path: &Path, tx: &MultiSigTransaction) -> Result<()> {
    fs::write(path, serde_json::to_string_pretty(tx)?)
        .with_context(|| format!("Failed to write {}", path.display()))?;
    Ok(())
}

#[allow(dead_code)]
pub fn validate_signer(public_key: &str) -> Result<()> {
    StellarPublicKey::from_string(public_key)
        .map_err(|_| anyhow::anyhow!("Invalid Stellar public key: {}", public_key))?;
    Ok(())
}

#[allow(dead_code)]
pub fn validate_weight(weight: u8) -> Result<()> {
    if weight == 0 {
        anyhow::bail!("Signer weight must be greater than 0");
    }
    Ok(())
}

#[allow(dead_code)]
pub fn validate_threshold(threshold: u8) -> Result<()> {
    if threshold == 0 {
        anyhow::bail!("Threshold must be greater than 0");
    }
    Ok(())
}

pub fn validate_thresholds(thresholds: &Thresholds, total_weight: u8) -> Result<()> {
    validate_threshold(thresholds.low)?;
    validate_threshold(thresholds.medium)?;
    validate_threshold(thresholds.high)?;

    if thresholds.low > total_weight {
        anyhow::bail!(
            "Low threshold ({}) exceeds total signer weight ({})",
            thresholds.low,
            total_weight
        );
    }
    if thresholds.medium > total_weight {
        anyhow::bail!(
            "Medium threshold ({}) exceeds total signer weight ({})",
            thresholds.medium,
            total_weight
        );
    }
    if thresholds.high > total_weight {
        anyhow::bail!(
            "High threshold ({}) exceeds total signer weight ({})",
            thresholds.high,
            total_weight
        );
    }

    Ok(())
}

pub fn calculate_total_weight(signers: &[Signer]) -> u8 {
    signers.iter().map(|s| s.weight).sum()
}

pub fn check_transaction_ready(tx: &MultiSigTransaction) -> bool {
    tx.current_weight >= tx.threshold_required
}

pub fn add_signature_to_transaction(
    tx: &mut MultiSigTransaction,
    signer_key: &str,
    signature: String,
) -> Result<()> {
    // Check if already signed
    if tx.signatures.iter().any(|s| s.signer_key == signer_key) {
        anyhow::bail!(
            "Signer '{}' has already signed this transaction",
            signer_key
        );
    }

    let sig = Signature {
        signer_key: signer_key.to_string(),
        signature,
        signed_at: chrono::Utc::now().to_rfc3339(),
    };

    tx.signatures.push(sig);

    // Best-effort tracking; callers should keep `current_weight` coherent.
    tx.current_weight = tx.signatures.len().min(u8::MAX as usize) as u8;

    // Update status
    if check_transaction_ready(tx) {
        tx.status = TransactionStatus::ReadyToSubmit;
    }

    Ok(())
}

#[allow(dead_code)]
pub fn build_multisig_transaction_xdr(
    source_account: &str,
    operations: &[String],
    sequence: u64,
    network: &str,
) -> Result<String> {
    // This is a simplified mock implementation
    // In production, use stellar-xdr to build proper transaction XDR

    let _network_passphrase = config::get_network_passphrase(network);

    // Mock XDR generation
    let mock_xdr = format!(
        "multisig_tx_{}_ops{}_seq{}",
        source_account,
        operations.len(),
        sequence
    );

    use base64::{engine::general_purpose, Engine as _};
    Ok(general_purpose::STANDARD.encode(mock_xdr))
}

pub fn sign_transaction_partial(
    transaction_xdr: &str,
    secret_key: &str,
    network: &str,
) -> Result<String> {
    let request = crate::utils::wallet_signer::SigningRequest::local_secret(
        zeroize::Zeroizing::new(secret_key.to_string()),
        network,
    );
    crate::utils::wallet_signer::sign_transaction_partial(transaction_xdr, &request, "local")
}

pub fn sign_transaction_partial_with_request(
    transaction_xdr: &str,
    request: &crate::utils::wallet_signer::SigningRequest,
    signer_label: &str,
) -> Result<String> {
    crate::utils::wallet_signer::sign_transaction_partial(transaction_xdr, request, signer_label)
}

pub fn combine_signatures(transaction_xdr: &str, signatures: &[Signature]) -> Result<String> {
    // This is a simplified mock implementation
    // In production, use stellar-xdr to build TransactionEnvelope with all signatures

    let combined = format!(
        "signed_multisig_tx_{}_with_{}_sigs",
        &transaction_xdr[..16],
        signatures.len()
    );

    use base64::{engine::general_purpose, Engine as _};
    Ok(general_purpose::STANDARD.encode(combined))
}

pub fn build_account_setup_transaction(
    account: &MultiSigAccount,
    network: &str,
) -> Result<MultiSigTransaction> {
    let operations = account
        .signers
        .iter()
        .map(|signer| {
            format!(
                "set_options signer={} weight={}",
                signer.public_key, signer.weight
            )
        })
        .chain(std::iter::once(format!(
            "set_options thresholds={}/{}/{}",
            account.thresholds.low, account.thresholds.medium, account.thresholds.high
        )))
        .collect::<Vec<_>>();

    let transaction_xdr =
        build_multisig_transaction_xdr(&account.account_id, &operations, 0, network)?;

    Ok(MultiSigTransaction {
        id: format!("setup-{}", account.name),
        account_id: account.account_id.clone(),
        transaction_xdr,
        signatures: Vec::new(),
        threshold_required: account.thresholds.high,
        current_weight: 0,
        status: TransactionStatus::Pending,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

pub fn build_stellar_cli_steps(account: &MultiSigAccount, network: &str) -> Vec<MultisigSetupStep> {
    let signer_args = account
        .signers
        .iter()
        .map(|signer| {
            format!(
                "--signer {} --signer-weight {}",
                signer.public_key, signer.weight
            )
        })
        .collect::<Vec<_>>()
        .join(" ");

    vec![
        MultisigSetupStep {
            title: "Inspect the current account state".to_string(),
            command: format!(
                "stellar account show --account {} --network {}",
                account.account_id, network
            ),
        },
        MultisigSetupStep {
            title: "Apply signer weights and thresholds on-chain".to_string(),
            command: format!(
                "stellar tx new set-options --source-account {} {} --low-threshold {} --med-threshold {} --high-threshold {} --network {}",
                account.account_id,
                signer_args,
                account.thresholds.low,
                account.thresholds.medium,
                account.thresholds.high,
                network
            ),
        },
        MultisigSetupStep {
            title: "Verify the account now reflects the multi-sig settings".to_string(),
            command: format!(
                "stellar account show --account {} --network {}",
                account.account_id, network
            ),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_signer() {
        // Generate a valid key for testing
        use ed25519_dalek::SigningKey;
        use rand::RngCore;
        use stellar_strkey::ed25519::PublicKey as StellarPublicKey;

        let mut rng = rand::thread_rng();
        let mut seed = [0u8; 32];
        rng.fill_bytes(&mut seed);
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();
        let valid_key = StellarPublicKey(verifying_key.to_bytes()).to_string();

        assert!(validate_signer(&valid_key).is_ok());

        let invalid_key = "INVALID_KEY";
        assert!(validate_signer(invalid_key).is_err());
    }

    #[test]
    fn test_validate_weight() {
        assert!(validate_weight(1).is_ok());
        assert!(validate_weight(255).is_ok());
        assert!(validate_weight(0).is_err());
    }

    #[test]
    fn test_calculate_total_weight() {
        let signers = vec![
            Signer {
                public_key: "GABC...".to_string(),
                weight: 10,
                name: None,
            },
            Signer {
                public_key: "GDEF...".to_string(),
                weight: 20,
                name: None,
            },
        ];
        assert_eq!(calculate_total_weight(&signers), 30);
    }

    #[test]
    fn test_validate_thresholds() {
        let thresholds = Thresholds {
            low: 10,
            medium: 20,
            high: 30,
        };
        assert!(validate_thresholds(&thresholds, 30).is_ok());
        assert!(validate_thresholds(&thresholds, 25).is_err());
    }

    #[test]
    fn test_check_transaction_ready() {
        let tx = MultiSigTransaction {
            id: "tx1".to_string(),
            account_id: "GABC...".to_string(),
            transaction_xdr: "mock_xdr".to_string(),
            signatures: vec![],
            threshold_required: 20,
            current_weight: 25,
            status: TransactionStatus::Pending,
            created_at: "2025-01-01T00:00:00Z".to_string(),
        };
        assert!(check_transaction_ready(&tx));

        let tx2 = MultiSigTransaction {
            current_weight: 15,
            ..tx
        };
        assert!(!check_transaction_ready(&tx2));
    }
}
