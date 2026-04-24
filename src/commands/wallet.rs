use crate::utils::{config, crypto, horizon, print as p};
use anyhow::Result;
use chrono::Utc;
use clap::Subcommand;
use colored::*;
use ed25519_dalek::SigningKey;
use rand::RngCore;
use stellar_strkey::ed25519::{PrivateKey as StellarPrivateKey, PublicKey as StellarPublicKey};

#[derive(Subcommand)]
pub enum WalletCommands {
    /// Create a new keypair and save it locally
    Create {
        /// A friendly name for the wallet (e.g. "alice", "deployer")
        name: String,
        /// Fund the wallet via Friendbot immediately (testnet only)
        #[arg(long, default_value = "false")]
        fund: bool,
        /// Network to associate with this wallet (overrides global config)
        #[arg(long, value_parser = ["testnet", "mainnet"])]
        network: Option<String>,
    },
    /// List all saved wallets
    List,
    /// Show details of a saved wallet including live balance
    Show {
        /// Wallet name
        name: String,
        /// Reveal the secret key in plaintext
        #[arg(long, default_value = "false")]
        reveal: bool,
    },
    /// Fund a wallet via Friendbot (testnet only)
    Fund {
        /// Wallet name to fund
        name: String,
    },
    /// Remove a wallet from local storage
    Remove {
        /// Wallet name to remove
        name: String,
    },

    Rename {
        old_name: String,
        new_name: String,
    },
}

pub fn handle(cmd: WalletCommands) -> Result<()> {
    match cmd {
        WalletCommands::Create {
            name,
            fund,
            network,
        } => create(name, fund, network),
        WalletCommands::List => list(),
        WalletCommands::Show { name, reveal } => show(name, reveal),
        WalletCommands::Fund { name } => fund_wallet(name),
        WalletCommands::Remove { name } => remove(name),
        WalletCommands::Rename { old_name, new_name } => rename(old_name, new_name),
    }
}

fn generate_keypair() -> (String, String) {
    let mut rng = rand::thread_rng();
    let mut seed = [0u8; 32];
    rng.fill_bytes(&mut seed);

    let signing_key = SigningKey::from_bytes(&seed);
    let verifying_key = signing_key.verifying_key();

    let public_key = StellarPublicKey(verifying_key.to_bytes()).to_string();
    let secret_key = StellarPrivateKey(seed).to_string();

    (public_key, secret_key)
}

fn create(name: String, fund: bool, network_override: Option<String>) -> Result<()> {
    let mut cfg = config::load()?;

    config::validate_wallet_name(&name)?;

    if cfg.wallets.iter().any(|w| w.name == name) {
        anyhow::bail!("A wallet named '{}' already exists.", name);
    }

    let network = network_override.unwrap_or_else(|| cfg.network.clone());

    let steps = if fund { 3 } else { 2 };
    p::header(&format!("Creating wallet '{}'", name));

    p::step(1, steps, "Generating keypair…");
    let (public_key, secret_key) = generate_keypair();
    println!();
    p::kv_accent("Public Key", &public_key);
    
    println!();
    let pwd = crypto::prompt_password("Set a secure password to encrypt this wallet", true)?;
    let encrypted_secret = crypto::encrypt_secret(&pwd, &secret_key)?;
    
    p::kv("Secret Key", &"Encrypted and safely stored.");
    println!();

    p::step(2, steps, "Saving to ~/.starforge/config.toml…");
    let wallet = config::WalletEntry {
        name: name.clone(),
        public_key: public_key.clone(),
        secret_key: Some(encrypted_secret),
        network: network.clone(),
        created_at: Utc::now().to_rfc3339(),
        funded: false,
    };
    cfg.wallets.push(wallet);

    if fund {
        if network == "mainnet" {
            p::warn("Friendbot is not available on Mainnet. Skipping fund step.");
        } else {
            p::step(3, steps, "Funding via Friendbot…");
            match horizon::fund_account(&public_key) {
                Ok(_) => {
                    if let Some(w) = cfg.wallets.iter_mut().find(|w| w.name == name) {
                        w.funded = true;
                    }
                    p::success("Funded with 10,000 XLM on testnet");
                }
                Err(e) => p::warn(&format!("Funding failed: {}", e)),
            }
        }
    }

    config::save(&cfg)?;
    println!();
    p::success(&format!("Wallet '{}' created and saved!", name));
    p::info(&format!(
        "View it with: {}",
        format!("starforge wallet show {}", name).cyan()
    ));
    Ok(())
}

fn list() -> Result<()> {
    let cfg = config::load()?;

    p::header("Saved Wallets");

    if cfg.wallets.is_empty() {
        p::info(&format!(
            "No wallets yet on {}. Run `starforge wallet create <name>` to get started.",
            cfg.network
        ));
        return Ok(());
    }

    p::separator();

    for (i, w) in cfg.wallets.iter().enumerate() {
        let status = if w.funded {
            "funded".green()
        } else {
            "unfunded".dimmed()
        };

        println!("  {:>2}. {} [{}]", i + 1, w.name.bold(), status);
        p::kv("Key", &w.public_key);
        p::kv("Net", &w.network);

        if i < cfg.wallets.len() - 1 {
            println!();
        }
    }

    p::separator();
    p::kv(
        &format!("{} wallet(s)", cfg.wallets.len()),
        &format!("on {} — {}", cfg.network, config::config_path().display()),
    );

    Ok(())
}

fn show(name: String, reveal: bool) -> Result<()> {
    let cfg = config::load()?;
    let w = cfg
        .wallets
        .iter()
        .find(|w| w.name == name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?;

    p::header(&format!("Wallet: {}", w.name));
    p::separator();
    p::kv_accent("Public Key", &w.public_key);

    if reveal {
        if let Some(sk) = &w.secret_key {
            // Check if it's plaintext
            if !sk.contains(':') && sk.starts_with('S') && sk.len() == 56 {
                p::warn("Warning: This wallet is using an unencrypted legacy key!");
                p::kv("Secret Key", sk);
            } else {
                let pwd = crypto::prompt_password(&format!("Enter password for wallet '{}'", name), false)?;
                match crypto::decrypt_secret(&pwd, sk) {
                    Ok(plain_sk) => p::kv("Secret Key", &plain_sk),
                    Err(_) => anyhow::bail!("Incorrect password or unable to decrypt."),
                }
            }
        }
    } else {
        p::kv(
            "Secret Key",
            &format!("{} (--reveal to show)", "*".repeat(20)),
        );
    }

    p::kv("Network", &w.network);
    p::kv("Funded", if w.funded { "yes" } else { "no" });
    p::kv("Created", &w.created_at);
    p::separator();

    p::info(&format!("Fetching live balance on {}…", w.network));
    match horizon::fetch_account(&w.public_key, &w.network) {
        Ok(account) => {
            println!();
            for bal in &account.balances {
                let asset = bal.asset_code.as_deref().unwrap_or("XLM");
                p::kv_accent(asset, &format!("{} {}", bal.balance, asset));
            }
        }
        Err(_) => {
            p::warn("Account not yet active on-chain. Fund it with `starforge wallet fund`");
        }
    }
    Ok(())
}

fn fund_wallet(name: String) -> Result<()> {
    config::validate_wallet_name(&name)?;
    let mut cfg = config::load()?;

    if cfg.network == "mainnet" {
        anyhow::bail!("Friendbot is not available on Mainnet.");
    }

    let public_key = cfg
        .wallets
        .iter()
        .find(|w| w.name == name)
        .map(|w| w.public_key.clone())
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?;

    p::info(&format!("Funding '{}' via Friendbot…", name));
    horizon::fund_account(&public_key)?;

    if let Some(w) = cfg.wallets.iter_mut().find(|w| w.name == name) {
        w.funded = true;
    }
    config::save(&cfg)?;

    println!();
    p::success("Account funded with 10,000 XLM on testnet!");
    p::kv_accent("Public Key", &public_key);
    Ok(())
}

fn remove(name: String) -> Result<()> {
    config::validate_wallet_name(&name)?;
    let mut cfg = config::load()?;
    let before = cfg.wallets.len();
    cfg.wallets.retain(|w| w.name != name);

    if cfg.wallets.len() == before {
        anyhow::bail!("No wallet named '{}' found", name);
    }

    config::save(&cfg)?;
    p::success(&format!("Wallet '{}' removed", name));
    Ok(())
}
fn rename(old_name: String, new_name: String) -> Result<()> {
    config::validate_wallet_name(&old_name)?;
    config::validate_wallet_name(&new_name)?;
    
    let mut cfg = config::load()?;
    if !cfg.wallets.iter().any(|w| w.name == old_name) {
        anyhow::bail!("No wallet named '{}' found", old_name);
    }

    if cfg.wallets.iter().any(|w| w.name == new_name) {
        anyhow::bail!("A wallet named '{}' already exists", new_name);
    }
    if let Some(w) = cfg.wallets.iter_mut().find(|w| w.name == old_name) {
        w.name = new_name.clone();
    }

    config::save(&cfg)?;
    println!();
    p::success(&format!("Wallet renamed: '{}' → '{}'", old_name, new_name));
    p::info(&format!(
        "View it with: {}",
        format!("starforge wallet show {}", new_name).cyan()
    ));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::generate_keypair;
    use ed25519_dalek::{Signer, SigningKey, Verifier, VerifyingKey};
    use std::collections::HashSet;
    use stellar_strkey::ed25519::{PrivateKey as StellarPrivateKey, PublicKey as StellarPublicKey};

    #[test]
    fn generates_valid_unique_stellar_ed25519_keypairs() {
        let mut public_keys = HashSet::new();
        let mut secret_keys = HashSet::new();
        let message = b"starforge wallet keypair validation";

        for _ in 0..1000 {
            let (public_key, secret_key) = generate_keypair();

            assert!(public_key.starts_with('G'));
            assert!(secret_key.starts_with('S'));
            assert!(public_keys.insert(public_key.clone()));
            assert!(secret_keys.insert(secret_key.clone()));

            let decoded_public = StellarPublicKey::from_string(&public_key).unwrap();
            let decoded_secret = StellarPrivateKey::from_string(&secret_key).unwrap();

            assert_eq!(decoded_public.to_string(), public_key);
            assert_eq!(decoded_secret.to_string(), secret_key);

            let signing_key = SigningKey::from_bytes(&decoded_secret.0);
            let verifying_key = VerifyingKey::from_bytes(&decoded_public.0).unwrap();

            assert_eq!(signing_key.verifying_key().to_bytes(), decoded_public.0);

            let signature = signing_key.sign(message);
            verifying_key.verify(message, &signature).unwrap();
        }
    }
}
