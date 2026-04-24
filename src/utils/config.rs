use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Validates that a string is a well-formed Stellar Ed25519 public key.
///
/// A valid Stellar public key:
/// - Starts with 'G'
/// - Is exactly 56 characters long
/// - Contains only valid base32 characters (A-Z, 2-7)
///
/// Returns `Ok(())` if the key is valid, or an error with a descriptive message.
pub fn validate_public_key(key: &str) -> Result<()> {
    if !key.starts_with('G') {
        anyhow::bail!(
            "Invalid public key: must start with 'G'.\n  \
             A valid Stellar public key looks like: GABC...XYZ (56 characters, starting with G)."
        );
    }

    if key.len() != 56 {
        anyhow::bail!(
            "Invalid public key: expected 56 characters, got {}.\n  \
             A valid Stellar public key is exactly 56 characters long.",
            key.len()
        );
    }

    // Validate base32 character set (A-Z, 2-7)
    if let Some(bad_char) = key.chars().find(|c| !matches!(c, 'A'..='Z' | '2'..='7')) {
        anyhow::bail!(
            "Invalid public key: contains invalid character '{}'.\n  \
             A valid Stellar public key uses only uppercase letters A-Z and digits 2-7.",
            bad_char
        );
    }
    Ok(())
}

/// Validates a Soroban contract ID.
/// Must start with 'C', be exactly 56 chars long, and use valid base32 chars.
pub fn validate_contract_id(id: &str) -> Result<()> {
    if !id.starts_with('C') {
        anyhow::bail!("Invalid contract ID: must start with 'C'.");
    }
    if id.len() != 56 {
        anyhow::bail!("Invalid contract ID: expected 56 characters, got {}.", id.len());
    }
    if let Some(bad_char) = id.chars().find(|c| !matches!(c, 'A'..='Z' | '2'..='7')) {
        anyhow::bail!("Invalid contract ID: contains invalid character '{}'.", bad_char);
    }
    Ok(())
}

/// Validates a file path exists and optionally matches an extension.
pub fn validate_file_path(path: &std::path::Path, expected_ext: Option<&str>) -> Result<()> {
    if !path.exists() {
        anyhow::bail!("Path does not exist: {}", path.display());
    }
    if !path.is_file() {
        anyhow::bail!("Path is not a file: {}", path.display());
    }
    if let Some(ext) = expected_ext {
        if path.extension().and_then(|e| e.to_str()) != Some(ext) {
            anyhow::bail!("Invalid file type: expected '{}' extension.", ext);
        }
    }
    Ok(())
}

/// Validates network setting.
pub fn validate_network(network: &str) -> Result<()> {
    match network {
        "testnet" | "mainnet" => Ok(()),
        _ => anyhow::bail!("Unsupported network '{}'. Use 'testnet' or 'mainnet'.", network),
    }
}

/// Validates an amount string parses to a positive f64.
pub fn validate_amount(amount: &str) -> Result<f64> {
    let amt: f64 = amount.parse().map_err(|_| anyhow::anyhow!("Invalid amount format: '{}'", amount))?;
    if amt <= 0.0 {
        anyhow::bail!("Amount must be strictly positive, got {}", amt);
    }
    Ok(amt)
}

/// Validates a wallet name.
/// Must not be empty and must contain only alphanumeric chars, dashes, or underscores.
pub fn validate_wallet_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Wallet name cannot be empty.");
    }
    if let Some(bad_char) = name.chars().find(|c| !c.is_alphanumeric() && *c != '-' && *c != '_') {
        anyhow::bail!("Invalid wallet name '{}': contains invalid character '{}'. Use alphanumeric, dash, or underscore.", name, bad_char);
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub network: String,
    pub wallets: Vec<WalletEntry>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WalletEntry {
    pub name: String,
    pub public_key: String,
    pub secret_key: Option<String>,
    pub network: String,
    pub created_at: String,
    pub funded: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            network: "testnet".to_string(),
            wallets: vec![],
        }
    }
}

pub fn config_dir() -> PathBuf {
    let home = dirs::home_dir().expect("Could not find home directory");
    home.join(".starforge")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.toml")
}

pub fn load() -> Result<Config> {
    let path = config_path();
    if !path.exists() {
        return Ok(Config::default());
    }
    let contents = fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config at {:?}", path))?;
    let config: Config =
        toml::from_str(&contents).with_context(|| "Failed to parse config file")?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_public_key() {
        // Well-formed Stellar public key (56 chars, starts with G, valid base32)
        let key = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";
        assert!(validate_public_key(key).is_ok());
    }

    #[test]
    fn test_rejects_key_not_starting_with_g() {
        let key = "SAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";
        let err = validate_public_key(key).unwrap_err();
        assert!(err.to_string().contains("must start with 'G'"));
    }

    #[test]
    fn test_rejects_key_wrong_length() {
        let key = "GAAZI4TCR3TY5";
        let err = validate_public_key(key).unwrap_err();
        assert!(err.to_string().contains("expected 56 characters"));
    }

    #[test]
    fn test_rejects_key_invalid_characters() {
        // Lowercase letters are not valid base32
        let key = "Gaazi4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWN";
        let err = validate_public_key(key).unwrap_err();
        assert!(err.to_string().contains("invalid character"));
    }

    #[test]
    fn test_rejects_empty_key() {
        let err = validate_public_key("").unwrap_err();
        assert!(err.to_string().contains("must start with 'G'"));
    }

    #[test]
    fn test_valid_contract_id() {
        let id = "CAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWNW";
        assert!(validate_contract_id(id).is_ok());
    }

    #[test]
    fn test_rejects_contract_id_not_starting_with_c() {
        // Starts with 'G'
        let id = "GAAZI4TCR3TY5OJHCTJC2A4QSY6CJWJH5IAJTGKIN2ER7LBNVKOCCWNW";
        let err = validate_contract_id(id).unwrap_err();
        assert!(err.to_string().contains("must start with 'C'"));
    }

    #[test]
    fn test_valid_amount() {
        assert_eq!(validate_amount("10.5").unwrap(), 10.5);
        assert_eq!(validate_amount("1").unwrap(), 1.0);
    }

    #[test]
    fn test_invalid_amount() {
        assert!(validate_amount("-5").is_err());
        assert!(validate_amount("0").is_err());
        assert!(validate_amount("abc").is_err());
    }

    #[test]
    fn test_valid_wallet_name() {
        assert!(validate_wallet_name("alice-123_DEPLOY").is_ok());
    }

    #[test]
    fn test_invalid_wallet_name() {
        assert!(validate_wallet_name("").is_err());
        assert!(validate_wallet_name("alice!").is_err());
        assert!(validate_wallet_name("my wallet").is_err());
    }
}

pub fn save(config: &Config) -> Result<()> {
    let dir = config_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create config dir {:?}", dir))?;
    }
    let contents = toml::to_string_pretty(config).with_context(|| "Failed to serialize config")?;
    fs::write(config_path(), contents).with_context(|| "Failed to write config file")?;
    Ok(())
}
