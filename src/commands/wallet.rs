use crate::utils::{
    config, confirmation, crypto, hardware_wallet, horizon, mnemonic, multisig, output, print as p,
};
use anyhow::{Context, Result};
use bip39::{Language, Mnemonic};
use chrono::Utc;
use clap::Subcommand;
use colored::*;
use ed25519_dalek::{Signer, SigningKey};
use rand::RngCore;
use serde::Serialize;
use std::fs;
use std::path::PathBuf;
use stellar_strkey::ed25519::{PrivateKey as StellarPrivateKey, PublicKey as StellarPublicKey};

// The backup document types and their parsers live in `utils::wallet_import`,
// where they can be unit-tested, property-tested, and fuzzed without going
// through prompting or the filesystem.
use crate::utils::wallet_import::{self, WalletBackup, WalletBackupEntry, WALLET_BACKUP_VERSION};

fn kdf_options(
    mem: Option<u32>,
    iterations: Option<u32>,
    parallelism: Option<u32>,
    config_default: Option<&crypto::KdfOptions>,
) -> Option<crypto::KdfOptions> {
    if mem.is_none() && iterations.is_none() && parallelism.is_none() && config_default.is_none() {
        return None;
    }

    let mut options = config_default.cloned().unwrap_or_default();
    if let Some(m) = mem {
        options.mem = Some(m);
    }
    if let Some(i) = iterations {
        options.iterations = Some(i);
    }
    if let Some(p) = parallelism {
        options.parallelism = Some(p);
    }
    Some(options)
}

/// Build a backup entry from a stored wallet.
fn backup_entry_from(entry: &config::WalletEntry) -> WalletBackupEntry {
    WalletBackupEntry {
        name: entry.name.clone(),
        public_key: entry.public_key.clone(),
        secret_key: entry.secret_key.clone(),
        network: entry.network.clone(),
        created_at: entry.created_at.clone(),
        funded: entry.funded,
    }
}

#[derive(Subcommand)]
pub enum WalletCommands {
    /// Create a new keypair and save it locally
    Create {
        /// A friendly name for the wallet (e.g. "alice", "deployer")
        name: String,
        /// Fund the wallet via network-specific faucet immediately when available
        #[arg(long, default_value = "false")]
        fund: bool,
        /// Network to associate with this wallet (overrides global config)
        #[arg(long, value_parser = ["testnet", "mainnet"])]
        network: Option<String>,
        /// Encrypt the secret key with a passphrase at rest
        #[arg(long, default_value = "false")]
        encrypt: bool,
        /// Reject passphrases that score below "Strong" on the zxcvbn scale
        /// (requires --encrypt)
        #[arg(long, default_value = "false", requires = "encrypt")]
        strict: bool,
        /// Generate a BIP39 recovery phrase instead of a random key
        #[arg(long, default_value = "false")]
        mnemonic: bool,
        /// Mnemonic length: 12 or 24 words (requires --mnemonic)
        #[arg(long, default_value = "24", requires = "mnemonic", value_parser = ["12", "24"])]
        words: String,
        /// Account index for SEP-0005 path m/44'/148'/index' (requires --mnemonic)
        #[arg(long, default_value = "0", requires = "mnemonic")]
        account_index: u32,
        /// Argon2 memory cost in KiB (requires --encrypt)
        #[arg(long, requires = "encrypt")]
        mem: Option<u32>,
        /// Argon2 iteration count (requires --encrypt)
        #[arg(long, requires = "encrypt")]
        iterations: Option<u32>,
        /// Argon2 parallelism factor (requires --encrypt)
        #[arg(long, requires = "encrypt")]
        parallelism: Option<u32>,
    },
    /// List all saved wallets
    List {
        /// Emit a machine-readable JSON object instead of the human-readable table
        #[arg(long)]
        json: bool,
    },
    /// Show details of a saved wallet including live balance
    Show {
        /// Wallet name
        name: String,
        /// Reveal the secret key in plaintext
        #[arg(long, default_value = "false")]
        reveal: bool,
    },
    /// Fund a wallet via a configured network faucet
    Fund {
        /// Wallet name to fund
        name: String,
    },
    /// Remove a wallet from local storage
    Remove {
        /// Wallet name to remove
        name: String,
    },
    /// Rename a wallet
    Rename { old_name: String, new_name: String },
    /// Close a source account and send remaining XLM to a destination (account merge)
    Merge {
        /// Source wallet to close (must be saved in StarForge)
        #[arg(long)]
        from: String,
        /// Destination public key or wallet name that receives the balance
        #[arg(long)]
        to: String,
        /// Network to use (defaults to the source wallet's network)
        #[arg(long, value_parser = ["testnet", "mainnet"])]
        network: Option<String>,
        /// Skip the confirmation prompt
        #[arg(long, default_value = "false")]
        yes: bool,
        /// Remove the source wallet from local storage after a successful merge
        #[arg(long, default_value = "false")]
        remove_local: bool,
    },
    /// Rotate a wallet in place while keeping the same logical name
    Rotate {
        /// Wallet name to rotate
        name: String,
        /// Fund the new wallet via Friendbot immediately (testnet only)
        #[arg(long, default_value = "false")]
        fund: bool,
        /// Network to associate with the rotated wallet (overrides stored wallet network)
        #[arg(long, value_parser = ["testnet", "mainnet"])]
        network: Option<String>,
        /// Encrypt the replacement secret key with a passphrase at rest
        #[arg(long, default_value = "false")]
        encrypt: bool,
        /// Reject passphrases that score below "Strong" or reuse wallet details
        /// (requires --encrypt)
        #[arg(long, default_value = "false", requires = "encrypt")]
        strict: bool,
        /// Argon2 memory cost in KiB (requires --encrypt)
        #[arg(long, requires = "encrypt")]
        mem: Option<u32>,
        /// Argon2 iteration count (requires --encrypt)
        #[arg(long, requires = "encrypt")]
        iterations: Option<u32>,
        /// Argon2 parallelism factor (requires --encrypt)
        #[arg(long, requires = "encrypt")]
        parallelism: Option<u32>,
        /// Path to write a pre-rotation backup snapshot
        #[arg(long)]
        backup: Option<PathBuf>,
    },
    /// Export a wallet to a JSON backup file
    Export {
        /// Optional wallet name to export (omit with --all)
        #[arg(long, conflicts_with = "all")]
        name: Option<String>,
        /// Export all wallets
        #[arg(long, short, conflicts_with = "name")]
        all: bool,
        /// Output file path for the backup JSON
        #[arg(long)]
        output: PathBuf,
        /// Reject passphrases that score below "Strong" or reuse wallet details
        #[arg(long, default_value = "false")]
        strict: bool,
        /// Split the backup into N recovery shares (Shamir's Secret Sharing).
        /// Requires --threshold. Each share is written to a separate file.
        #[arg(long, requires = "threshold")]
        shares: Option<usize>,
        /// Minimum number of shares required to reconstruct (M in M-of-N).
        /// Requires --shares.
        #[arg(long, requires = "shares")]
        threshold: Option<usize>,
        /// Output directory for share files (default: same directory as --output)
        #[arg(long, requires = "shares")]
        shares_dir: Option<PathBuf>,
    },
    /// Import a wallet from a JSON backup, BIP39 recovery phrase, or raw Stellar secret key
    Import {
        /// Wallet name (required with --mnemonic or --key)
        name: Option<String>,
        /// Path to backup JSON file
        #[arg(long, group = "source")]
        file: Option<PathBuf>,
        /// Import from a BIP39 recovery phrase (prompted interactively)
        #[arg(long, group = "source")]
        mnemonic: bool,
        /// Import from a raw Stellar secret key (starts with 'S', 56 characters)
        #[arg(long, group = "source")]
        key: Option<String>,
        /// Account index for SEP-0005 path m/44'/148'/index'
        #[arg(long, default_value = "0")]
        account_index: u32,
        /// Network to associate with this wallet
        #[arg(long, value_parser = ["testnet", "mainnet"])]
        network: Option<String>,
        /// Encrypt the imported secret key with a passphrase at rest
        #[arg(long, default_value = "false")]
        encrypt: bool,
        /// Reject passphrases that score below "Strong" or reuse wallet details
        /// (requires --encrypt)
        #[arg(long, default_value = "false", requires = "encrypt")]
        strict: bool,
        /// Import a watch-only wallet from a connected hardware device
        #[arg(long, value_enum, group = "source")]
        hardware: Option<hardware_wallet::HardwareWalletKind>,
        /// HD derivation path when importing from hardware
        #[arg(long, default_value = hardware_wallet::STELLAR_HD_PATH)]
        hd_path: String,
    },
    /// Reconstruct a wallet backup from recovery shares
    ImportShares {
        /// Path to a share JSON file (provide at least --threshold of them)
        #[arg(long, num_args = 1..)]
        shares: Vec<PathBuf>,
        /// Output file path for the reconstructed backup JSON
        #[arg(long)]
        output: PathBuf,
    },

    /// Connect to a hardware wallet (Ledger/Trezor) and show device info
    Connect {
        #[arg(value_enum, default_value_t = hardware_wallet::HardwareWalletKind::Ledger)]
        device: hardware_wallet::HardwareWalletKind,
        /// Connection timeout (e.g. 1s, 30s)
        #[arg(long, default_value = "30s")]
        timeout: String,
    },

    /// Show the Stellar address derived from a connected hardware wallet
    HwAddress {
        /// Device type
        #[arg(value_enum)]
        device: hardware_wallet::HardwareWalletKind,
        /// HD derivation path (default: m/44'/148'/0')
        #[arg(long, default_value = hardware_wallet::STELLAR_HD_PATH)]
        path: String,
    },

    /// Show connection status of a hardware wallet without full connect
    HwStatus {
        #[arg(value_enum)]
        device: hardware_wallet::HardwareWalletKind,
    },

    /// Sign an arbitrary message using a local or hardware-backed key
    Sign {
        /// Wallet name to use (for local signing)
        name: String,
        /// Message to sign (utf-8)
        message: String,
        /// Use a hardware wallet instead of a local secret key
        #[arg(long, value_enum)]
        hardware: Option<hardware_wallet::HardwareWalletKind>,
    },
    /// Tune or upgrade KDF encryption parameters for a saved encrypted wallet
    TuneKdf {
        /// Wallet name to upgrade
        name: String,
        /// Argon2 memory cost in KiB (e.g. 65536)
        #[arg(long)]
        mem: Option<u32>,
        /// Argon2 iteration count (e.g. 4)
        #[arg(long)]
        iterations: Option<u32>,
        /// Argon2 parallelism factor (e.g. 2)
        #[arg(long)]
        parallelism: Option<u32>,
        /// Upgrade to global configuration KDF parameters
        #[arg(long, default_value = "false")]
        use_global: bool,
    },
    /// Derive all 10 Stellar addresses (m/44'/148'/0..9') from a BIP39 recovery phrase
    Derive,
    /// Multi-signature account management
    #[command(subcommand)]
    Multisig(MultisigCommands),
}

#[derive(Subcommand)]
pub enum MultisigCommands {
    /// Create a multi-sig config for an existing wallet
    ///
    /// Example:
    /// starforge wallet multisig create treasury --threshold 2 --signers alice,bob,charlie
    Create {
        /// Wallet name to treat as the multi-sig account (e.g. "treasury")
        name: String,
        /// Required weight threshold to submit
        #[arg(long)]
        threshold: u8,
        /// Comma-separated wallet names to act as signers (e.g. alice,bob,charlie)
        #[arg(long)]
        signers: String,
        /// Override network for this config
        #[arg(long)]
        network: Option<String>,
        /// Optional file path to write a setup transaction JSON/XDR payload
        #[arg(long)]
        xdr_output: Option<PathBuf>,
    },
    /// Sign a multi-sig transaction JSON with all available local signer keys
    ///
    /// Example:
    /// starforge wallet multisig sign treasury --transaction tx.json
    Sign {
        /// Multi-sig account name (created via `multisig create`)
        name: String,
        /// Path to a MultiSigTransaction JSON file
        #[arg(long)]
        transaction: PathBuf,
        /// Output file (defaults to in-place update)
        #[arg(long)]
        output: Option<PathBuf>,
        /// Sign with a hardware wallet instead of a local secret key
        #[arg(long, value_enum)]
        hardware: Option<hardware_wallet::HardwareWalletKind>,
        /// HD derivation path for hardware wallet signing
        #[arg(long, default_value = hardware_wallet::STELLAR_HD_PATH)]
        hd_path: String,
        /// Network for signing (default: testnet)
        #[arg(long, default_value = "testnet", value_parser = ["testnet", "mainnet"])]
        network: String,
    },
    /// List multi-sig accounts stored locally
    List,
    /// Show a stored multi-sig account
    Show { name: String },
    /// Submit a fully-signed multi-sig transaction to Horizon
    ///
    /// Example:
    /// starforge wallet multisig submit treasury --transaction tx.json
    Submit {
        /// Multi-sig account name
        name: String,
        /// Path to a signed MultiSigTransaction JSON file
        #[arg(long)]
        transaction: PathBuf,
        /// Network to submit on (default: testnet)
        #[arg(long, value_parser = ["testnet", "mainnet", "docker-testnet"])]
        network: Option<String>,
    },
}

pub async fn handle(cmd: WalletCommands) -> Result<()> {
    match cmd {
        WalletCommands::Create {
            name,
            fund,
            network,
            encrypt,
            strict,
            mnemonic: use_mnemonic,
            words,
            account_index,
            mem,
            iterations,
            parallelism,
        } => {
            create(
                name,
                fund,
                network,
                encrypt,
                strict,
                use_mnemonic,
                words,
                account_index,
                mem,
                iterations,
                parallelism,
            )
            .await
        }
        WalletCommands::List { json } => list(json),
        WalletCommands::Show { name, reveal } => show(name, reveal).await,
        WalletCommands::Fund { name } => fund_wallet(name).await,
        WalletCommands::Remove { name } => remove(name),
        WalletCommands::Rename { old_name, new_name } => rename(old_name, new_name),
        WalletCommands::Merge {
            from,
            to,
            network,
            yes,
            remove_local,
        } => merge_wallet(from, to, network, yes, remove_local).await,
        WalletCommands::Rotate {
            name,
            fund,
            network,
            encrypt,
            strict,
            mem,
            iterations,
            parallelism,
            backup,
        } => {
            rotate_wallet(
                name,
                fund,
                network,
                encrypt,
                strict,
                mem,
                iterations,
                parallelism,
                backup,
            )
            .await
        }
        WalletCommands::Export {
            name,
            all,
            output,
            strict,
            shares,
            threshold,
            shares_dir,
        } => export_wallet(name, all, output, strict, shares, threshold, shares_dir),
        WalletCommands::Import {
            name,
            file,
            mnemonic: from_mnemonic,
            key,
            account_index,
            network,
            encrypt,
            strict,
            hardware,
            hd_path,
        } => import_wallet(
            name,
            file,
            from_mnemonic,
            key,
            account_index,
            network,
            encrypt,
            strict,
            hardware,
            hd_path,
        ),
        WalletCommands::ImportShares { shares, output } => import_shares(shares, output),
        WalletCommands::Connect { device, timeout } => connect_hardware(device, &timeout),
        WalletCommands::HwAddress { device, path } => hw_address(device, &path),
        WalletCommands::HwStatus { device } => hw_status(device),
        WalletCommands::Sign {
            name,
            message,
            hardware,
        } => sign_message(name, message, hardware),
        WalletCommands::Derive => derive_addresses(),
        WalletCommands::TuneKdf {
            name,
            mem,
            iterations,
            parallelism,
            use_global,
        } => tune_wallet_kdf(&name, mem, iterations, parallelism, use_global),
        WalletCommands::Multisig(cmd) => handle_multisig(cmd).await,
    }
}

fn tune_wallet_kdf(
    name: &str,
    mem: Option<u32>,
    iterations: Option<u32>,
    parallelism: Option<u32>,
    use_global: bool,
) -> Result<()> {
    p::header(&format!("Tune KDF Encryption Parameters: '{}'", name));

    let cfg = config::load()?;
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?;

    let secret_bundle = wallet
        .secret_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' has no secret key saved", name))?;

    if !secret_bundle.contains(':') {
        anyhow::bail!(
            "Wallet '{}' is unencrypted. Run `wallet rotate --encrypt` to enable encryption first.",
            name
        );
    }

    let current_meta = wallet
        .kdf_metadata()
        .ok_or_else(|| anyhow::anyhow!("Failed to parse current KDF metadata for '{}'", name))?;

    p::kv("Current KDF Version", &current_meta.version.to_string());
    p::kv("Current Memory", &format!("{} KiB", current_meta.mem));
    p::kv("Current Iterations", &current_meta.iterations.to_string());
    p::kv("Current Parallelism", &current_meta.parallelism.to_string());

    if !use_global && mem.is_none() && iterations.is_none() && parallelism.is_none() {
        anyhow::bail!(
            "Specify at least one parameter to update (--mem, --iterations, --parallelism) or use --use-global."
        );
    }

    let global_default = cfg.wallet_encryption.as_ref();
    let target_options = if use_global {
        global_default.cloned().unwrap_or_default()
    } else {
        crypto::KdfOptions {
            mem: mem.or(Some(current_meta.mem)),
            iterations: iterations.or(Some(current_meta.iterations)),
            parallelism: parallelism.or(Some(current_meta.parallelism)),
        }
    };

    target_options.validate()?;

    let password = crypto::prompt_password("Enter wallet passphrase", false)?;

    config::upgrade_wallet_kdf(name, &password, Some(target_options))?;

    let updated_cfg = config::load()?;
    let updated_wallet = updated_cfg
        .wallets
        .iter()
        .find(|w| w.name == name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found after upgrade", name))?;

    if let Some(new_meta) = updated_wallet.kdf_metadata() {
        p::separator();
        p::success(&format!(
            "Successfully upgraded KDF parameters for wallet '{}'",
            name
        ));
        p::kv("Upgraded KDF Version", &new_meta.version.to_string());
        p::kv("Upgraded Memory", &format!("{} KiB", new_meta.mem));
        p::kv("Upgraded Iterations", &new_meta.iterations.to_string());
        p::kv("Upgraded Parallelism", &new_meta.parallelism.to_string());
    }

    Ok(())
}

fn parse_duration(input: &str) -> Result<std::time::Duration> {
    let trimmed = input.trim().to_lowercase();
    if trimmed.ends_with("ms") {
        let value: u64 = trimmed
            .trim_end_matches("ms")
            .parse()
            .context("Invalid timeout")?;
        return Ok(std::time::Duration::from_millis(value));
    }
    if trimmed.ends_with('s') {
        let value: u64 = trimmed
            .trim_end_matches('s')
            .parse()
            .context("Invalid timeout")?;
        return Ok(std::time::Duration::from_secs(value));
    }
    anyhow::bail!("Invalid timeout '{}'. Use values like 1s or 500ms.", input)
}

fn connect_hardware(device: hardware_wallet::HardwareWalletKind, timeout: &str) -> Result<()> {
    let timeout_duration = parse_duration(timeout)?;
    p::header("Hardware Wallet — Connect");
    p::step(1, 3, &format!("Initializing HID subsystem for {}…", device));
    let info = hardware_wallet::connect_with_timeout(device, timeout_duration)
        .map_err(|err| hardware_wallet::map_signing_error(err, device))?;
    p::step(
        2,
        3,
        &format!("{} HID device(s) visible", info.device_count),
    );
    p::step(3, 3, "Connection verified");
    println!();
    p::success(&format!("{} connected", device));
    p::kv("Devices visible", &info.device_count.to_string());
    p::kv("HD path", &info.hd_path);
    p::info("Run `starforge wallet hw-address <device>` to derive your Stellar address.");
    Ok(())
}

fn hw_address(device: hardware_wallet::HardwareWalletKind, path: &str) -> Result<()> {
    p::header("Hardware Wallet â€” Stellar Address");
    p::step(
        1,
        2,
        &format!("Requesting address from {} at {}", device, path),
    );
    let address = hardware_wallet::get_stellar_address(device, path)?;
    p::step(2, 2, "Address received");
    println!();
    p::kv("Device", &device.to_string());
    p::kv("HD Path", path);
    p::kv_accent("Stellar Address", &address);
    Ok(())
}

fn hw_status(device: hardware_wallet::HardwareWalletKind) -> Result<()> {
    p::header("Hardware Wallet â€” Status");
    let status = hardware_wallet::device_status(device)?;
    p::kv("Status", &status);
    Ok(())
}

fn sign_message(
    name: String,
    message: String,
    hardware: Option<hardware_wallet::HardwareWalletKind>,
) -> Result<()> {
    p::header("Sign Message");
    p::kv("Wallet", &name);

    let msg_bytes = message.as_bytes();

    if let Some(kind) = hardware {
        p::kv("Signer", &format!("{:?}", kind));
        let passphrase = config::get_network_passphrase("testnet");
        let sig = hardware_wallet::sign_transaction(
            kind,
            hardware_wallet::STELLAR_HD_PATH,
            msg_bytes,
            &passphrase,
        )
        .map_err(|err| hardware_wallet::map_signing_error(err, kind))?;
        p::separator();
        p::kv_accent("Message", &message);
        p::kv("Signature (hex)", &hex::encode(sig));
        p::separator();
        return Ok(());
    }

    let cfg = config::load()?;
    let w = cfg
        .wallets
        .iter()
        .find(|w| w.name == name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?;

    let sk = w
        .secret_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' has no local secret key", name))?;

    let plain_sk = if !sk.contains(':') && sk.starts_with('S') && sk.len() == 56 {
        sk.clone()
    } else {
        let pwd = crypto::prompt_password(&format!("Enter password for wallet '{}'", name), false)?;
        crypto::decrypt_secret(&pwd, sk)
            .map_err(|_| anyhow::anyhow!("Incorrect password or unable to decrypt."))?
    };

    let decoded_secret = StellarPrivateKey::from_string(&plain_sk)?;
    let signing_key = SigningKey::from_bytes(&decoded_secret.0);
    let sig = signing_key.sign(msg_bytes);

    p::separator();
    p::kv_accent("Message", &message);
    p::kv("Signature (hex)", &hex::encode(sig.to_bytes()));
    p::separator();
    Ok(())
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

fn parse_word_count(words: &str) -> Result<mnemonic::WordCount> {
    match words {
        "12" => Ok(mnemonic::WordCount::Words12),
        "24" => Ok(mnemonic::WordCount::Words24),
        _ => anyhow::bail!("--words must be 12 or 24"),
    }
}

fn prompt_recovery_phrase() -> Result<String> {
    use dialoguer::Input;
    let phrase: String = Input::new()
        .with_prompt("Enter recovery phrase (12 or 24 words)")
        .interact_text()
        .map_err(|e| anyhow::anyhow!("Failed to read recovery phrase: {}", e))?;
    if phrase.trim().is_empty() {
        anyhow::bail!("Recovery phrase cannot be empty");
    }
    Ok(phrase)
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
async fn create(
    name: String,
    fund: bool,
    network_override: Option<String>,
    encrypt: bool,
    strict: bool,
    use_mnemonic: bool,
    words: String,
    account_index: u32,
    mem: Option<u32>,
    iterations: Option<u32>,
    parallelism: Option<u32>,
) -> Result<()> {
    let mut cfg = config::load()?;

    config::validate_wallet_name(&name)?;

    if cfg.wallets.iter().any(|w| w.name == name) {
        anyhow::bail!("A wallet named '{}' already exists.", name);
    }

    let network = network_override.unwrap_or_else(|| cfg.network.clone());

    let steps = if fund { 3 } else { 2 };
    p::header(&format!("Creating wallet '{}'", name));

    let (public_key, secret_key) = if use_mnemonic {
        let word_count = parse_word_count(&words)?;
        p::step(
            1,
            steps,
            &format!("Generating {}-word recovery phrase…", word_count.as_usize()),
        );
        let phrase = mnemonic::generate_phrase(word_count)?;
        println!();
        p::warn("Write down this recovery phrase in order. Anyone with it can access your funds.");
        p::kv_accent("Recovery Phrase", &phrase);
        mnemonic::keypair_from_phrase(&phrase, "", account_index)?
    } else {
        p::step(1, steps, "Generating keypair…");
        let (pk, sk) = generate_keypair();
        (pk, zeroize::Zeroizing::new(sk))
    };
    println!();
    p::kv_accent("Public Key", &public_key);

    println!();
    let secret_to_store = if encrypt {
        if strict {
            p::info(&format!(
                "--strict mode active: passphrase must be {} characters or longer \
                 and score \"{}\" or better.",
                crypto::MIN_PASSPHRASE_LEN,
                "Strong"
            ));
        } else {
            p::info(&format!(
                "Passphrase must be at least {} characters. \
                 Add --strict to enforce a stronger passphrase.",
                crypto::MIN_PASSPHRASE_LEN
            ));
        }
        println!();
        let context = [name.as_str(), public_key.as_str(), network.as_str()];
        let pwd = crypto::prompt_passphrase_with_inputs(
            "Set a passphrase to encrypt this wallet",
            strict,
            &context,
        )?;
        crypto::encrypt_secret(
            &pwd,
            &secret_key,
            kdf_options(mem, iterations, parallelism, cfg.wallet_encryption.as_ref()).as_ref(),
        )?
    } else {
        secret_key.to_string()
    };

    let status = if encrypt {
        "Encrypted and safely stored."
    } else {
        "Stored in plaintext (not recommended for mainnet)."
    };
    p::kv("Secret Key", status);
    println!();

    p::step(2, steps, "Saving to ~/.starforge/config.tomlâ€¦");
    let kdf = if encrypt {
        kdf_options(mem, iterations, parallelism, cfg.wallet_encryption.as_ref())
    } else {
        None
    };
    let wallet = config::WalletEntry {
        name: name.clone(),
        public_key: public_key.clone(),
        secret_key: Some(secret_to_store),
        network: network.clone(),
        created_at: Utc::now().to_rfc3339(),
        funded: false,
        kdf_options: kdf,
        rotation_history: Vec::new(),
    };
    cfg.wallets.push(wallet);

    if fund {
        let net_cfg = config::get_network_config(&cfg, &network)?;
        if net_cfg.friendbot_url.is_none() && network == "mainnet" {
            p::warn("Friendbot is not available on Mainnet. Skipping fund step.");
        } else {
            p::step(3, steps, "Funding via network faucet…");
            match horizon::fund_account(&public_key, &network).await {
                Ok(_) => {
                    if let Some(w) = cfg.wallets.iter_mut().find(|w| w.name == name) {
                        w.funded = true;
                    }
                    p::success("Account funded via configured faucet");
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

fn list(json: bool) -> Result<()> {
    let cfg = config::load()?;
    let emit_json = json || output::is_json_mode_enabled();

    if emit_json {
        #[derive(Serialize)]
        struct WalletListResponse {
            network: String,
            wallet_count: usize,
            wallets: Vec<WalletSummary>,
        }

        #[derive(Serialize)]
        struct WalletSummary {
            name: String,
            public_key: String,
            network: String,
            funded: bool,
            created_at: String,
        }

        let wallets: Vec<WalletSummary> = cfg
            .wallets
            .iter()
            .map(|w| WalletSummary {
                name: w.name.clone(),
                public_key: w.public_key.clone(),
                network: w.network.clone(),
                funded: w.funded,
                created_at: w.created_at.clone(),
            })
            .collect();

        return output::print_json(&WalletListResponse {
            network: cfg.network.clone(),
            wallet_count: wallets.len(),
            wallets,
        });
    }

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
        &format!("on {} â€” {}", cfg.network, config::config_path().display()),
    );

    Ok(())
}

async fn show(name: String, reveal: bool) -> Result<()> {
    let cfg = config::load()?;
    let w = cfg
        .wallets
        .iter()
        .find(|w| w.name == name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?;

    if reveal {
        let summary = confirmation::OperationSummary::new(
            "Reveal Wallet Secret".to_string(),
            w.network.clone(),
            confirmation::RiskLevel::High,
        )
        .add("Wallet", &w.name)
        .add("Public Key", &w.public_key)
        .add("Network", &w.network);

        let confirm_config = confirmation::ConfirmationConfig {
            risk_level: confirmation::RiskLevel::High,
            network: w.network.clone(),
            skip_confirm: false,
            dry_run: false,
            prompt: Some("Reveal the secret key for this wallet?".to_string()),
            require_type_confirmation: true,
            destructive_action: Some(confirmation::DestructiveAction::SecretReveal),
            challenge_phrase: None,
        };

        if !confirmation::confirm_operation(&summary, &confirm_config)? {
            return Ok(());
        }
    }

    p::header(&format!("Wallet: {}", w.name));
    p::separator();
    p::kv_accent("Public Key", &w.public_key);

    if reveal {
        if let Some(sk) = &w.secret_key {
            // Check if it's plaintext
            if !sk.contains(':') && sk.starts_with('S') && sk.len() == 56 {
                p::warn("Warning: This wallet's secret key is stored unencrypted (plaintext)!");
                p::kv("Secret Key", sk);
            } else {
                let pwd = crypto::prompt_password(
                    &format!("Enter password for wallet '{}'", name),
                    false,
                )?;
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
    if !w.rotation_history.is_empty() {
        p::kv("Rotations", &w.rotation_history.len().to_string());
        if let Some(last_rotation) = w.rotation_history.last() {
            p::kv("Previous Key", &last_rotation.previous_public_key);
            p::kv("Rotated At", &last_rotation.rotated_at);
        }
    }
    p::separator();

    p::info(&format!("Fetching live balance on {}â€¦", w.network));
    match horizon::fetch_account(&w.public_key, &w.network).await {
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

async fn fund_wallet(name: String) -> Result<()> {
    config::validate_wallet_name(&name)?;
    let mut cfg = config::load()?;

    if cfg.network == "mainnet" {
        let net_cfg = config::get_network_config(&cfg, &cfg.network)?;
        if net_cfg.friendbot_url.is_none() {
            anyhow::bail!("Friendbot is not available on Mainnet.");
        }
    }

    let public_key = cfg
        .wallets
        .iter()
        .find(|w| w.name == name)
        .map(|w| w.public_key.clone())
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?;

    p::info(&format!(
        "Funding '{}' via configured network faucet…",
        name
    ));
    horizon::fund_account(&public_key, &cfg.network).await?;

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
fn resolve_merge_destination(to: &str, cfg: &config::Config) -> Result<String> {
    if to.starts_with('G') {
        config::validate_public_key(to)?;
        return Ok(to.to_string());
    }

    config::validate_wallet_name(to)?;
    cfg.wallets
        .iter()
        .find(|w| w.name == to)
        .map(|w| w.public_key.clone())
        .ok_or_else(|| anyhow::anyhow!("Destination wallet '{}' not found", to))
}

fn wallet_secret_key(wallet: &config::WalletEntry) -> Result<String> {
    let sk = wallet
        .secret_key
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' has no secret key stored", wallet.name))?;

    if sk.contains(':') {
        let pwd = crypto::prompt_password(
            &format!("Enter password to decrypt wallet '{}'", wallet.name),
            false,
        )?;
        crypto::decrypt_secret(&pwd, sk)
            .map_err(|_| anyhow::anyhow!("Incorrect password or unable to decrypt."))
    } else {
        Ok(sk.clone())
    }
}

fn validate_account_mergeable(account: &horizon::AccountResponse) -> Result<()> {
    for balance in &account.balances {
        if balance.asset_type == "native" {
            continue;
        }
        let amount: f64 = balance.balance.parse().unwrap_or(0.0);
        if amount > 0.0 {
            let label = balance.asset_code.as_deref().unwrap_or(&balance.asset_type);
            anyhow::bail!(
                "Cannot merge: account still holds {} {}. Remove trustlines and balances first.",
                balance.balance,
                label
            );
        }
    }

    if account.subentry_count > 0 {
        anyhow::bail!(
            "Cannot merge: account has {} subentries (trustlines, signers, data, etc.). \
             Remove them before merging.",
            account.subentry_count
        );
    }

    Ok(())
}

fn native_xlm_balance(account: &horizon::AccountResponse) -> f64 {
    account
        .balances
        .iter()
        .find(|b| b.asset_type == "native")
        .and_then(|b| b.balance.parse::<f64>().ok())
        .unwrap_or(0.0)
}

async fn merge_wallet(
    from: String,
    to: String,
    network_override: Option<String>,
    skip_confirm: bool,
    remove_local: bool,
) -> Result<()> {
    config::validate_wallet_name(&from)?;

    let cfg = config::load()?;
    let wallet = cfg.wallets.iter().find(|w| w.name == from).ok_or_else(|| {
        anyhow::anyhow!(
            "Wallet '{}' not found in StarForge. Run `starforge wallet list`",
            from
        )
    })?;

    let network = network_override
        .clone()
        .unwrap_or_else(|| wallet.network.clone());
    config::validate_network(&network)?;

    let destination = resolve_merge_destination(&to, &cfg)?;

    if wallet.public_key == destination {
        anyhow::bail!("Source and destination accounts must be different");
    }

    p::header("Account Merge");
    p::warn("This permanently closes the source account on-chain. This cannot be undone.");
    p::separator();
    p::kv("Source Wallet", &wallet.name);
    p::kv("Source Address", &wallet.public_key);
    p::kv("Destination", &destination);
    p::kv("Network", &network);

    if network == "mainnet" {
        p::warn("You are merging on MAINNET. All remaining XLM will move to the destination.");
    }

    p::separator();
    println!();
    p::step(1, 3, "Fetching source account…");
    let source_account = horizon::fetch_account(&wallet.public_key, &network)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Source account not found on {}: {}\nIt may already be merged or never funded.",
                network,
                e
            )
        })?;

    validate_account_mergeable(&source_account)?;
    let xlm_balance = native_xlm_balance(&source_account);
    p::kv(
        "XLM to Transfer",
        &format!("{:.7} XLM (minus fee)", xlm_balance),
    );

    if xlm_balance <= 0.00001 {
        anyhow::bail!("Source account has insufficient XLM to cover transaction fees");
    }

    p::step(2, 3, "Validating destination account…");
    horizon::fetch_account(&destination, &network)
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "Destination account does not exist on {}. \
             The destination must be funded before it can receive a merge.",
                network
            )
        })?;
    p::kv("Destination", "✓ Account exists");

    p::step(3, 3, "Building account merge transaction…");
    let tx_result = horizon::build_and_simulate_account_merge(
        &wallet.public_key,
        &destination,
        &source_account.sequence,
        &network,
    )?;

    p::kv(
        "Estimated Fee",
        &format!("{:.7} XLM", tx_result.fee as f64 / 10_000_000.0),
    );

    // Build operation summary for confirmation
    let risk_level = if network == "mainnet" {
        confirmation::RiskLevel::High
    } else {
        confirmation::RiskLevel::Medium
    };

    let summary = confirmation::OperationSummary::new(
        "Account Merge".to_string(),
        network.clone(),
        risk_level,
    )
    .add("Source Wallet", &wallet.name)
    .add("Source Address", &wallet.public_key)
    .add("Destination", &destination)
    .add(
        "XLM to Transfer",
        format!("{:.7} XLM (minus fee)", xlm_balance),
    )
    .add(
        "Estimated Fee",
        format!("{:.7} XLM", tx_result.fee as f64 / 10_000_000.0),
    )
    .add("Remove Local", if remove_local { "Yes" } else { "No" });

    let confirm_config = confirmation::ConfirmationConfig {
        risk_level,
        network: network.clone(),
        skip_confirm,
        dry_run: false,
        prompt: Some(format!(
            "Type '{}' to confirm merge of account {}:",
            wallet.name, wallet.name
        )),
        require_type_confirmation: true,
        destructive_action: Some(confirmation::DestructiveAction::AccountMerge),
        challenge_phrase: Some(wallet.name.clone()),
    };

    if !confirmation::confirm_operation(&summary, &confirm_config)? {
        return Ok(());
    }

    println!();
    let secret_key = wallet_secret_key(wallet)?;
    p::info("Submitting account merge…");
    let submit_result =
        horizon::submit_payment_transaction(&tx_result.transaction_xdr, &secret_key, &network)
            .await?;

    println!();
    p::separator();
    println!(
        "  {} {}",
        "✓".green().bold(),
        "Account merge submitted successfully!".bright_white()
    );
    println!();
    p::kv_accent("Transaction Hash", &submit_result.hash);

    let explorer_base = if network == "mainnet" {
        "https://stellar.expert/explorer/public/tx"
    } else {
        "https://stellar.expert/explorer/testnet/tx"
    };
    p::kv(
        "Stellar Expert",
        &format!("{}/{}", explorer_base, submit_result.hash),
    );
    p::separator();

    if remove_local {
        let mut cfg = config::load()?;
        let before = cfg.wallets.len();
        cfg.wallets.retain(|w| w.name != from);
        if cfg.wallets.len() < before {
            config::save(&cfg)?;
            p::success(&format!("Removed wallet '{}' from local storage", from));
        }
    } else {
        p::info(&format!(
            "Local wallet '{}' is still saved. Remove it with: {}",
            from,
            format!("starforge wallet remove {}", from).cyan()
        ));
    }

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
    p::success(&format!("Wallet renamed: '{}' ? '{}'", old_name, new_name));
    p::info(&format!(
        "View it with: {}",
        format!("starforge wallet show {}", new_name).cyan()
    ));
    Ok(())
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
async fn rotate_wallet(
    name: String,
    fund: bool,
    network_override: Option<String>,
    encrypt: bool,
    strict: bool,
    mem: Option<u32>,
    iterations: Option<u32>,
    parallelism: Option<u32>,
    backup: Option<PathBuf>,
) -> Result<()> {
    config::validate_wallet_name(&name)?;
    let mut cfg = config::load()?;
    let wallet_index = cfg
        .wallets
        .iter()
        .position(|wallet| wallet.name == name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?;

    let stored_network = cfg.wallets[wallet_index].network.clone();
    let original_public_key = cfg.wallets[wallet_index].public_key.clone();
    let original_secret_key = cfg.wallets[wallet_index].secret_key.clone();
    let original_funded = cfg.wallets[wallet_index].funded;
    let network = network_override.unwrap_or(stored_network);

    let preserve_secret = backup.is_some();
    let steps = if fund { 4 } else { 3 };
    p::header(&format!("Rotating wallet '{}'", name));
    p::kv("Old Public Key", &original_public_key);
    p::kv("Network", &network);

    // ── Step 1: optional pre-rotation backup snapshot ────────────────────────
    if let Some(ref backup_path) = backup {
        p::step(1, steps, "Writing pre-rotation backup snapshot...");
        let mut snapshot = WalletBackup {
            version: WALLET_BACKUP_VERSION.to_string(),
            exported_at: Utc::now().to_rfc3339(),
            wallets: vec![backup_entry_from(&cfg.wallets[wallet_index])],
            recovery_shares: None,
            integrity_tag: None,
        };
        let snap_tag =
            wallet_import::compute_integrity_tag(&snapshot, wallet_import::BACKUP_HMAC_KEY)
                .context("Failed to compute integrity tag for backup snapshot")?;
        snapshot.integrity_tag = Some(snap_tag);
        let json = serde_json::to_string_pretty(&snapshot)
            .context("Failed to serialize backup snapshot")?;
        if let Some(parent) = backup_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("Failed to create {}", parent.display()))?;
            }
        }
        let passphrase =
            crypto::prompt_passphrase("Set a passphrase to encrypt the backup snapshot", false)?;
        let encrypted = crypto::encrypt_secret(&passphrase, &json, None)?;
        fs::write(backup_path, encrypted)
            .with_context(|| format!("Failed to write backup to {}", backup_path.display()))?;
        p::success(&format!("Backup written to {}", backup_path.display()));
        p::info("Keep this file safe — it contains the previous secret key.");
    } else {
        p::step(
            1,
            steps,
            "Skipping backup (pass --backup <file> to save a snapshot)...",
        );
    }

    p::step(2, steps, "Generating replacement keypair...");
    let (public_key, secret_key) = generate_keypair();

    let secret_to_store = if encrypt {
        let context = [
            name.as_str(),
            original_public_key.as_str(),
            public_key.as_str(),
            network.as_str(),
        ];
        let pwd = crypto::prompt_passphrase_with_inputs(
            "Set a secure passphrase to encrypt the rotated wallet",
            strict,
            &context,
        )?;
        crypto::encrypt_secret(
            &pwd,
            &secret_key,
            kdf_options(mem, iterations, parallelism, cfg.wallet_encryption.as_ref()).as_ref(),
        )?
    } else {
        secret_key.clone()
    };

    p::step(
        3,
        steps,
        "Archiving previous keypair in rotation history...",
    );
    {
        let wallet = &mut cfg.wallets[wallet_index];
        wallet.rotation_history.push(config::WalletRotationRecord {
            rotated_at: Utc::now().to_rfc3339(),
            previous_public_key: original_public_key.clone(),
            previous_network: wallet.network.clone(),
            previous_funded: wallet.funded,
            previous_secret_key: if preserve_secret {
                original_secret_key
            } else {
                None
            },
        });
        wallet.public_key = public_key.clone();
        wallet.secret_key = Some(secret_to_store);
        wallet.network = network.clone();
        wallet.funded = false;
    }

    if fund {
        if network == "mainnet" {
            p::warn("Friendbot is not available on Mainnet. Skipping fund step.");
        } else {
            p::step(4, steps, "Funding the replacement wallet via Friendbot...");
            match horizon::fund_account(&public_key, &network).await {
                Ok(_) => {
                    if let Some(wallet) = cfg.wallets.iter_mut().find(|wallet| wallet.name == name)
                    {
                        wallet.funded = true;
                    }
                    p::success("Replacement wallet funded on testnet");
                }
                Err(e) => p::warn(&format!("Funding failed: {}", e)),
            }
        }
    }

    config::save(&cfg)?;

    println!();
    p::success(&format!("Wallet '{}' rotated", name));
    p::kv_accent("New Public Key", &public_key);
    p::warn(
        "The wallet name stayed the same, but the on-chain account changed. Update any funding, signer, or deploy flows that referenced the old public key.",
    );
    if original_funded {
        p::info("The previous key remains an on-chain account; rotation only updates the local wallet mapping.");
    }
    if preserve_secret {
        p::info("Previous secret key preserved in rotation history. View with: starforge wallet history <name> --reveal");
    }
    Ok(())
}

// Not currently called from any code path in this crate. Kept rather than
// removed since deleting it is a product decision, not a lint-scoping one.
#[allow(dead_code)]
fn wallet_history(name: String, reveal: bool) -> Result<()> {
    config::validate_wallet_name(&name)?;
    let cfg = config::load()?;
    let wallet = cfg
        .wallets
        .iter()
        .find(|w| w.name == name)
        .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?;

    p::header(&format!("Rotation history for '{}'", name));
    p::kv_accent("Current Public Key", &wallet.public_key);
    p::kv("Network", &wallet.network);
    p::kv("Funded", if wallet.funded { "yes" } else { "no" });

    if wallet.rotation_history.is_empty() {
        println!();
        p::info("No rotations recorded. This wallet has never been rotated.");
        return Ok(());
    }

    p::kv(
        "Total rotations",
        &wallet.rotation_history.len().to_string(),
    );
    p::separator();

    for (i, record) in wallet.rotation_history.iter().enumerate().rev() {
        println!("  Rotation #{}", i + 1);
        p::kv("  Rotated At", &record.rotated_at);
        p::kv("  Previous Public Key", &record.previous_public_key);
        p::kv("  Previous Network", &record.previous_network);
        p::kv(
            "  Was Funded",
            if record.previous_funded { "yes" } else { "no" },
        );

        match &record.previous_secret_key {
            Some(sk) if reveal => {
                if sk.contains(':') {
                    // Encrypted bundle — prompt for passphrase
                    let pwd = crypto::prompt_password(
                        &format!(
                            "Enter passphrase to decrypt previous key for rotation #{}",
                            i + 1
                        ),
                        false,
                    )?;
                    match crypto::decrypt_secret(&pwd, sk) {
                        Ok(plain) => p::kv("  Previous Secret Key", &plain),
                        Err(_) => {
                            p::warn("  Could not decrypt previous secret key (wrong passphrase?)")
                        }
                    }
                } else {
                    p::kv("  Previous Secret Key", sk);
                }
            }
            Some(_) => {
                p::kv("  Previous Secret Key", "(stored — use --reveal to show)");
            }
            None => {
                p::kv(
                    "  Previous Secret Key",
                    "(not preserved — use --backup on next rotation)",
                );
            }
        }

        if i > 0 {
            println!();
        }
    }

    p::separator();
    p::info("To export a full backup: starforge wallet export --name <name> --output backup.json");
    Ok(())
}

fn export_wallet(
    name_opt: Option<String>,
    all: bool,
    output: PathBuf,
    strict: bool,
    shares: Option<usize>,
    threshold: Option<usize>,
    shares_dir: Option<PathBuf>,
) -> Result<()> {
    let cfg = config::load()?;
    let wallets_to_export: Vec<WalletBackupEntry> = if all {
        cfg.wallets.iter().map(backup_entry_from).collect()
    } else {
        let name = name_opt
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wallet name must be provided unless --all is used"))?;
        config::validate_wallet_name(name)?;
        let wallet = cfg
            .wallets
            .iter()
            .find(|w| &w.name == name)
            .ok_or_else(|| anyhow::anyhow!("Wallet '{}' not found", name))?;
        vec![backup_entry_from(wallet)]
    };

    if output.exists() && output.is_dir() {
        anyhow::bail!("Output path is a directory: {}", output.display());
    }
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
    }

    let mut backup = WalletBackup {
        version: WALLET_BACKUP_VERSION.to_string(),
        exported_at: Utc::now().to_rfc3339(),
        wallets: wallets_to_export.clone(),
        recovery_shares: None,
        integrity_tag: None,
    };
    let export_tag = wallet_import::compute_integrity_tag(&backup, wallet_import::BACKUP_HMAC_KEY)
        .context("Failed to compute integrity tag for wallet backup")?;
    backup.integrity_tag = Some(export_tag);

    let context: Vec<&str> = backup
        .wallets
        .iter()
        .flat_map(|wallet| {
            [
                wallet.name.as_str(),
                wallet.public_key.as_str(),
                wallet.network.as_str(),
            ]
        })
        .collect();

    let json = serde_json::to_string_pretty(&backup)
        .with_context(|| "Failed to serialize wallet backup")?;

    if let (Some(num_shares), Some(thresh)) = (shares, threshold) {
        // ── Recovery shares mode ──────────────────────────────────────────
        if num_shares < 2 {
            anyhow::bail!("--shares must be at least 2");
        }
        if thresh < 2 {
            anyhow::bail!("--threshold must be at least 2");
        }
        if thresh > num_shares {
            anyhow::bail!(
                "--threshold ({}) cannot exceed --shares ({})",
                thresh,
                num_shares
            );
        }

        p::header("Exporting with recovery shares");
        p::kv(
            "Scheme",
            &format!("{}-of-{} Shamir's Secret Sharing", thresh, num_shares),
        );
        println!();

        let encrypted = crypto::encrypt_secret("", &json, None)?;
        let recovery_shares = crate::utils::shamir::split(encrypted.as_bytes(), thresh, num_shares)
            .map_err(|e| anyhow::anyhow!("Failed to split backup into shares: {}", e))?;

        // Determine output directory for share files.
        let dir = shares_dir.unwrap_or_else(|| {
            output
                .parent()
                .unwrap_or_else(|| std::path::Path::new("."))
                .to_path_buf()
        });
        fs::create_dir_all(&dir)
            .with_context(|| format!("Failed to create shares directory: {}", dir.display()))?;

        // Build a descriptive stem from the output filename.
        let stem = output
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("backup");

        // Write individual share files.
        let mut share_paths = Vec::new();
        for share in &recovery_shares {
            let share_filename = format!("{}.share-{}.json", stem, share.index);
            let share_path = dir.join(&share_filename);
            let share_json = serde_json::to_string_pretty(share)
                .with_context(|| "Failed to serialize recovery share")?;
            fs::write(&share_path, &share_json).with_context(|| {
                format!(
                    "Failed to write share {}: {}",
                    share.index,
                    share_path.display()
                )
            })?;
            share_paths.push(share_path.clone());
            p::kv(
                &format!("Share {}", share.index),
                &share_path.display().to_string(),
            );
        }

        // Also write the backup file itself (encrypted, without shares embedded).
        let encrypted_backup = crypto::encrypt_secret("", &json, None)?;
        fs::write(&output, &encrypted_backup)
            .with_context(|| format!("Failed to write {}", output.display()))?;

        // Write a manifest listing all share files.
        let manifest_path = dir.join(format!("{}.shares-manifest.json", stem));
        let manifest = serde_json::json!({
            "scheme": format!("{}-of-{}", thresh, num_shares),
            "threshold": thresh,
            "total_shares": num_shares,
            "share_files": share_paths.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "encrypted_backup": output.display().to_string(),
            "secret_hash": recovery_shares[0].secret_hash,
        });
        fs::write(&manifest_path, serde_json::to_string_pretty(&manifest)?)
            .with_context(|| format!("Failed to write manifest: {}", manifest_path.display()))?;

        println!();
        p::success(&format!(
            "Wallet(s) exported with {}-of-{} recovery shares",
            thresh, num_shares
        ));
        p::kv("Manifest", &manifest_path.display().to_string());
        p::kv("Encrypted backup", &output.display().to_string());
        println!();
        p::warn("Distribute each share to a separate custodian.");
        p::warn("Any threshold of shares can reconstruct the backup.");
        p::warn("Losing more than (total - threshold) shares means the backup is unrecoverable.");
    } else {
        // ── Standard passphrase mode ─────────────────────────────────────────
        let passphrase = crypto::prompt_passphrase_with_inputs(
            "Enter passphrase to encrypt backup",
            strict,
            &context,
        )?;
        let encrypted = crypto::encrypt_secret(&passphrase, &json, None)?;
        fs::write(&output, encrypted)
            .with_context(|| format!("Failed to write {}", output.display()))?;

        let name_display = if all {
            "all wallets".to_string()
        } else {
            name_opt.clone().unwrap()
        };
        p::success(&format!("Wallet(s) {} exported", name_display));
        p::kv("Backup file", &output.display().to_string());
        p::info("Secrets are only stored in the backup file; they are not printed to stdout.");
    }

    Ok(())
}

// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
fn import_wallet(
    name: Option<String>,
    file: Option<PathBuf>,
    from_mnemonic: bool,
    key: Option<String>,
    account_index: u32,
    network_override: Option<String>,
    encrypt: bool,
    strict: bool,
    hardware: Option<hardware_wallet::HardwareWalletKind>,
    hd_path: String,
) -> Result<()> {
    if let Some(device) = hardware {
        let name = name.ok_or_else(|| {
            anyhow::anyhow!(
                "Wallet name is required for hardware import (e.g. starforge wallet import ledger-alice --hardware ledger)"
            )
        })?;
        return import_from_hardware(name, device, &hd_path, network_override);
    }

    if from_mnemonic {
        let name = name.ok_or_else(|| {
            anyhow::anyhow!("Wallet name is required for mnemonic import (e.g. starforge wallet import alice --mnemonic)")
        })?;
        return import_from_mnemonic(name, account_index, network_override, encrypt, strict);
    }

    if let Some(secret_key) = key {
        let name = name.ok_or_else(|| {
            anyhow::anyhow!(
                "Wallet name is required when using --key (e.g. starforge wallet import alice --key SXXX...)"
            )
        })?;
        return import_from_secret_key(name, secret_key, network_override, encrypt);
    }

    let file = file.ok_or_else(|| {
        anyhow::anyhow!(
            "Provide --file <backup.json>, --mnemonic, or --key <SXXX...> to import a wallet"
        )
    })?;
    import_wallets(file)
}

fn import_from_hardware(
    name: String,
    device: hardware_wallet::HardwareWalletKind,
    hd_path: &str,
    network_override: Option<String>,
) -> Result<()> {
    config::validate_wallet_name(&name)?;
    let cfg = config::load()?;
    if cfg.wallets.iter().any(|w| w.name == name) {
        anyhow::bail!("Wallet '{}' already exists", name);
    }

    let public_key = hardware_wallet::get_stellar_address(device, hd_path)
        .map_err(|err| hardware_wallet::map_signing_error(err, device))?;
    let network = network_override.unwrap_or_else(|| cfg.network.clone());

    let mut updated_cfg = cfg;
    updated_cfg.wallets.push(config::WalletEntry {
        name: name.clone(),
        public_key,
        secret_key: None,
        network,
        created_at: Utc::now().to_rfc3339(),
        funded: false,
        kdf_options: None,
        rotation_history: vec![],
    });
    config::save(&updated_cfg)?;

    p::success(&format!(
        "Wallet '{}' imported from {} hardware device",
        name, device
    ));
    p::kv("HD Path", hd_path);
    p::info("This wallet is watch-only. Sign transactions with --hardware.");
    Ok(())
}

fn import_from_mnemonic(
    name: String,
    account_index: u32,
    network_override: Option<String>,
    encrypt: bool,
    strict: bool,
) -> Result<()> {
    let mut cfg = config::load()?;
    config::validate_wallet_name(&name)?;

    if cfg.wallets.iter().any(|w| w.name == name) {
        anyhow::bail!("A wallet named '{}' already exists.", name);
    }

    let network = network_override.unwrap_or_else(|| cfg.network.clone());
    p::header(&format!("Importing wallet '{}' from recovery phrase", name));

    let phrase = prompt_recovery_phrase()?;
    let (public_key, secret_key) = mnemonic::keypair_from_phrase(&phrase, "", account_index)?;

    println!();
    p::kv_accent("Public Key", &public_key);

    let secret_to_store = if encrypt {
        println!();
        let context = [name.as_str(), public_key.as_str(), network.as_str()];
        let pwd = crypto::prompt_passphrase_with_inputs(
            "Set a passphrase to encrypt this wallet",
            strict,
            &context,
        )?;
        crypto::encrypt_secret(&pwd, &secret_key, None)?
    } else {
        secret_key.to_string()
    };

    let kdf = if encrypt {
        kdf_options(None, None, None, cfg.wallet_encryption.as_ref())
    } else {
        None
    };
    cfg.wallets.push(config::WalletEntry {
        name: name.clone(),
        public_key,
        secret_key: Some(secret_to_store),
        network: network.clone(),
        created_at: Utc::now().to_rfc3339(),
        funded: false,
        kdf_options: kdf,
        rotation_history: Vec::new(),
    });

    config::save(&cfg)?;
    p::success(&format!("Wallet '{}' imported from recovery phrase", name));
    p::info(&format!(
        "View it with: {}",
        format!("starforge wallet show {}", name).cyan()
    ));
    Ok(())
}

fn import_from_secret_key(
    name: String,
    secret_key: String,
    network_override: Option<String>,
    encrypt: bool,
) -> Result<()> {
    if secret_key.contains(':') {
        anyhow::bail!(
            "--key expects a raw Stellar secret key (starts with 'S', 56 characters), \
             not an encrypted bundle. Use --file to import an encrypted backup."
        );
    }
    config::validate_secret_key(&secret_key)?;

    let mut cfg = config::load()?;
    config::validate_wallet_name(&name)?;

    if cfg.wallets.iter().any(|w| w.name == name) {
        anyhow::bail!("A wallet named '{}' already exists.", name);
    }

    let decoded_secret = StellarPrivateKey::from_string(&secret_key)
        .map_err(|_| anyhow::anyhow!("Invalid Stellar secret key format"))?;
    let signing_key = SigningKey::from_bytes(&decoded_secret.0);
    let public_key = StellarPublicKey(signing_key.verifying_key().to_bytes()).to_string();

    let network = network_override.unwrap_or_else(|| cfg.network.clone());

    p::header(&format!("Importing wallet '{}' from secret key", name));
    p::kv_accent("Public Key", &public_key);

    let secret_to_store = if encrypt {
        println!();
        let pwd = crypto::prompt_passphrase("Set a passphrase to encrypt this wallet", false)?;
        crypto::encrypt_secret(&pwd, &secret_key, None)?
    } else {
        secret_key
    };

    let kdf = if encrypt {
        kdf_options(None, None, None, cfg.wallet_encryption.as_ref())
    } else {
        None
    };
    cfg.wallets.push(config::WalletEntry {
        name: name.clone(),
        public_key,
        secret_key: Some(secret_to_store),
        network,
        created_at: Utc::now().to_rfc3339(),
        funded: false,
        kdf_options: kdf,
        rotation_history: Vec::new(),
    });

    config::save(&cfg)?;
    p::success(&format!("Wallet '{}' imported from secret key", name));
    p::info(&format!(
        "View it with: {}",
        format!("starforge wallet show {}", name).cyan()
    ));
    Ok(())
}

fn import_wallets(file: PathBuf) -> Result<()> {
    config::validate_file_path(&file, Some("json"))?;
    let raw_contents =
        fs::read_to_string(&file).with_context(|| format!("Failed to read {}", file.display()))?;

    // Encrypted bundles have 3, 5, or 6 base64 parts depending on whether
    // custom Argon2 parameters were used. The envelope is structurally checked
    // before a passphrase is requested, so a corrupt file fails fast instead of
    // after an Argon2 derivation.
    let contents = match wallet_import::classify_payload(&raw_contents) {
        wallet_import::PayloadKind::Encrypted => {
            wallet_import::parse_encrypted_envelope(&raw_contents).map_err(|e| {
                anyhow::anyhow!("Backup file is not a readable encrypted bundle: {}", e)
            })?;
            let passphrase = crypto::prompt_password("Enter passphrase to decrypt backup", false)?;
            crypto::decrypt_secret(&passphrase, raw_contents.trim())?
        }
        wallet_import::PayloadKind::Plaintext => raw_contents,
    };

    let parsed = wallet_import::parse_wallet_backup(&contents)
        .map_err(|e| anyhow::anyhow!("Backup file rejected: {}", e))?;
    for warning in &parsed.warnings {
        p::warn(warning);
    }
    let backup = parsed.backup;

    let mut cfg = config::load()?;

    for wallet in &backup.wallets {
        config::validate_network_exists(&cfg, &wallet.network)?;

        if cfg.wallets.iter().any(|w| w.name == wallet.name) {
            anyhow::bail!("Wallet '{}' already exists", wallet.name);
        }
    }

    let imported = backup.wallets.len();
    for wallet in backup.wallets {
        let kdf_options = wallet
            .secret_key
            .as_ref()
            .and_then(|s| crypto::extract_kdf_metadata(s).ok())
            .map(|m| crypto::KdfOptions {
                mem: Some(m.mem),
                iterations: Some(m.iterations),
                parallelism: Some(m.parallelism),
            });
        cfg.wallets.push(config::WalletEntry {
            name: wallet.name,
            public_key: wallet.public_key,
            secret_key: wallet.secret_key,
            network: wallet.network,
            created_at: wallet.created_at,
            funded: wallet.funded,
            kdf_options,
            rotation_history: Vec::new(),
        });
    }

    config::save(&cfg)?;
    p::success(&format!(
        "Imported {} wallet(s) from {}",
        imported,
        file.display()
    ));
    Ok(())
}

fn import_shares(share_paths: Vec<PathBuf>, output: PathBuf) -> Result<()> {
    p::header("Reconstructing backup from recovery shares");

    if share_paths.is_empty() {
        anyhow::bail!("Provide at least one share file via --shares");
    }

    // Read and parse all share files.
    let mut shares = Vec::new();
    for path in &share_paths {
        config::validate_file_path(path, Some("json"))?;
        let contents = fs::read_to_string(path)
            .with_context(|| format!("Failed to read share file: {}", path.display()))?;
        let share: crate::utils::shamir::RecoveryShare = serde_json::from_str(&contents)
            .map_err(|e| anyhow::anyhow!("Failed to parse share file {}: {}", path.display(), e))?;
        p::step(
            shares.len() + 1,
            share_paths.len(),
            &format!("Loaded share {} from {}", share.index, path.display()),
        );
        shares.push(share);
    }

    // Validate shares.
    wallet_import::validate_recovery_shares(&shares)
        .map_err(|e| anyhow::anyhow!("Share validation failed: {}", e))?;

    let threshold = shares[0].threshold;
    let total = shares[0].total_shares;
    println!();
    p::kv("Scheme", &format!("{}-of-{}", threshold, total));
    p::kv("Shares provided", &shares.len().to_string());

    if (shares.len() as u8) < threshold {
        anyhow::bail!(
            "Need at least {} shares for reconstruction, but only {} were provided",
            threshold,
            shares.len()
        );
    }

    // Reconstruct the encrypted bundle.
    let encrypted = wallet_import::reconstruct_from_shares(&shares)
        .map_err(|e| anyhow::anyhow!("Reconstruction failed: {}", e))?;

    p::success("Shares reconstructed successfully");
    println!();

    // The reconstructed data is an encrypted backup bundle.
    // In share mode, the backup is encrypted with an empty passphrase.
    let contents = match wallet_import::classify_payload(&encrypted) {
        wallet_import::PayloadKind::Encrypted => {
            wallet_import::parse_encrypted_envelope(&encrypted).map_err(|e| {
                anyhow::anyhow!("Reconstructed data is not a valid encrypted bundle: {}", e)
            })?;
            let passphrase = crypto::prompt_password("Enter passphrase to decrypt backup", false)?;
            crypto::decrypt_secret(&passphrase, &encrypted)?
        }
        wallet_import::PayloadKind::Plaintext => encrypted,
    };

    // Parse the reconstructed backup.
    let parsed = wallet_import::parse_wallet_backup(&contents)
        .map_err(|e| anyhow::anyhow!("Reconstructed backup is invalid: {}", e))?;
    for warning in &parsed.warnings {
        p::warn(warning);
    }

    // Write the reconstructed backup to the output file.
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
    }

    let pretty = serde_json::to_string_pretty(&parsed.backup)
        .with_context(|| "Failed to serialize reconstructed backup")?;
    fs::write(&output, &pretty).with_context(|| format!("Failed to write {}", output.display()))?;

    println!();
    p::success(&format!(
        "Backup reconstructed with {} wallet(s)",
        parsed.backup.wallets.len()
    ));
    p::kv("Output file", &output.display().to_string());
    p::info("You can now import with: starforge wallet import --file <output>");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::generate_keypair;
    use crate::utils::config::{WalletEntry, WalletRotationRecord};
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

    // ── Rotation history / backup tests ─────────────────────────────────────

    fn make_wallet(name: &str, public_key: &str) -> WalletEntry {
        WalletEntry {
            name: name.to_string(),
            public_key: public_key.to_string(),
            secret_key: Some(
                "SAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string(),
            ),
            network: "testnet".to_string(),
            created_at: "2025-01-01T00:00:00Z".to_string(),
            funded: true,
            rotation_history: vec![],
        }
    }

    #[test]
    fn rotation_record_without_backup_has_no_secret() {
        let record = WalletRotationRecord {
            rotated_at: "2025-06-01T00:00:00Z".to_string(),
            previous_public_key: "GABC".to_string(),
            previous_network: "testnet".to_string(),
            previous_funded: true,
            previous_secret_key: None,
        };
        assert!(record.previous_secret_key.is_none());
    }

    #[test]
    fn rotation_record_with_backup_preserves_secret() {
        let secret = "SAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
        let record = WalletRotationRecord {
            rotated_at: "2025-06-01T00:00:00Z".to_string(),
            previous_public_key: "GABC".to_string(),
            previous_network: "testnet".to_string(),
            previous_funded: true,
            previous_secret_key: Some(secret.to_string()),
        };
        assert_eq!(record.previous_secret_key.as_deref(), Some(secret));
    }

    #[test]
    fn rotation_history_accumulates_across_multiple_rotations() {
        let mut wallet = make_wallet("alice", "GABC");

        // Simulate two rotations
        for i in 0..2 {
            wallet.rotation_history.push(WalletRotationRecord {
                rotated_at: format!("2025-0{}-01T00:00:00Z", i + 1),
                previous_public_key: format!("GPREV{}", i),
                previous_network: "testnet".to_string(),
                previous_funded: false,
                previous_secret_key: Some(format!("SPREV{}", i)),
            });
        }

        assert_eq!(wallet.rotation_history.len(), 2);
        assert_eq!(wallet.rotation_history[0].previous_public_key, "GPREV0");
        assert_eq!(wallet.rotation_history[1].previous_public_key, "GPREV1");
    }

    #[test]
    fn rotation_record_serialises_without_secret_when_none() {
        let record = WalletRotationRecord {
            rotated_at: "2025-06-01T00:00:00Z".to_string(),
            previous_public_key: "GABC".to_string(),
            previous_network: "testnet".to_string(),
            previous_funded: false,
            previous_secret_key: None,
        };
        let json = serde_json::to_string(&record).unwrap();
        // previous_secret_key should be omitted entirely (skip_serializing_if)
        assert!(!json.contains("previous_secret_key"));
    }

    #[test]
    fn rotation_record_serialises_with_secret_when_present() {
        let record = WalletRotationRecord {
            rotated_at: "2025-06-01T00:00:00Z".to_string(),
            previous_public_key: "GABC".to_string(),
            previous_network: "testnet".to_string(),
            previous_funded: false,
            previous_secret_key: Some("SKEY".to_string()),
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("previous_secret_key"));
        assert!(json.contains("SKEY"));
    }
}

fn derive_addresses() -> Result<()> {
    p::header("Derive Stellar Addresses from Mnemonic");
    p::info("Enter your BIP39 recovery phrase to derive all 10 Stellar addresses.");
    println!();

    let phrase = prompt_recovery_phrase()?;
    let passphrase = "";

    println!();
    p::step(1, 2, "Validating recovery phrase…");
    let normalized = phrase.split_whitespace().collect::<Vec<_>>().join(" ");
    let _ = Mnemonic::parse_in(Language::English, normalized)
        .map_err(|e| anyhow::anyhow!("Invalid recovery phrase: {}", e))?;
    p::success("Recovery phrase is valid");

    println!();
    p::step(2, 2, "Deriving addresses for account indices 0-9…");
    println!();
    p::separator();

    for account_index in 0..10 {
        let result = mnemonic::keypair_from_phrase(&phrase, passphrase, account_index);

        match result {
            Ok((public_key, _)) => {
                let derivation_path = format!("m/44'/148'/{}'", account_index);
                p::kv(&format!("[{}]", account_index), &derivation_path);
                p::kv_accent(&format!("    Address {}", account_index), &public_key);
                println!();
            }
            Err(e) => {
                p::warn(&format!(
                    "Failed to derive account {}: {}",
                    account_index, e
                ));
                println!();
            }
        }
    }

    p::separator();
    p::info(
        "These addresses are derived deterministically from your recovery phrase. \
         Entering the same phrase will always produce the same addresses.",
    );
    p::warn("Do not share your recovery phrase with anyone. Anyone with it can access all these addresses.");
    Ok(())
}

async fn handle_multisig(cmd: MultisigCommands) -> Result<()> {
    match cmd {
        MultisigCommands::Create {
            name,
            threshold,
            signers,
            network,
            xdr_output,
        } => multisig_create(name, threshold, signers, network, xdr_output),
        MultisigCommands::Sign {
            name,
            transaction,
            output,
            hardware,
            hd_path,
            network,
        } => multisig_sign(name, transaction, output, hardware, hd_path, network).await,
        MultisigCommands::List => multisig_list(),
        MultisigCommands::Show { name } => multisig_show(name),
        MultisigCommands::Submit {
            name,
            transaction,
            network,
        } => multisig_submit(name, transaction, network).await,
    }
}

fn multisig_create(
    name: String,
    threshold: u8,
    signers: String,
    network: Option<String>,
    xdr_output: Option<PathBuf>,
) -> Result<()> {
    config::validate_wallet_name(&name)?;
    multisig::validate_threshold(threshold)?;

    let cfg = config::load()?;
    let wallet = cfg.wallets.iter().find(|w| w.name == name).ok_or_else(|| {
        anyhow::anyhow!(
            "Wallet '{}' not found. Create it first with `starforge wallet create {}`",
            name,
            name
        )
    })?;

    let network = network.unwrap_or_else(|| wallet.network.clone());
    config::validate_network(&network)?;

    let signer_names: Vec<String> = signers
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    if signer_names.is_empty() {
        anyhow::bail!("Provide at least one signer wallet via --signers alice,bob,...");
    }

    let mut signer_entries = Vec::new();
    for signer_name in signer_names {
        config::validate_wallet_name(&signer_name)?;
        let signer_wallet = cfg
            .wallets
            .iter()
            .find(|w| w.name == signer_name)
            .ok_or_else(|| {
                anyhow::anyhow!("Signer wallet '{}' not found in local config", signer_name)
            })?;
        signer_entries.push(multisig::Signer {
            public_key: signer_wallet.public_key.clone(),
            weight: 1,
            name: Some(signer_wallet.name.clone()),
        });
    }

    let total_weight = multisig::calculate_total_weight(&signer_entries);
    let thresholds = multisig::Thresholds {
        low: threshold,
        medium: threshold,
        high: threshold,
    };
    multisig::validate_thresholds(&thresholds, total_weight)?;

    let account = multisig::MultiSigAccount {
        name: name.clone(),
        account_id: wallet.public_key.clone(),
        signers: signer_entries,
        thresholds,
        created_at: chrono::Utc::now().to_rfc3339(),
    };

    multisig::save_account(&account)?;
    let setup_steps = multisig::build_stellar_cli_steps(&account, &network);

    println!();
    p::header(&format!("Multi-sig: {}", name));
    p::success("Multi-sig config saved");
    p::kv_accent("Account ID", &account.account_id);
    p::kv("Network", &network);
    p::kv("Threshold", &threshold.to_string());
    p::kv("Signers", &account.signers.len().to_string());
    if let Some(path) = xdr_output {
        let setup_tx = multisig::build_account_setup_transaction(&account, &network)?;
        multisig::save_transaction(&path, &setup_tx)?;
        p::kv("Setup XDR JSON", &path.display().to_string());
    }
    println!();
    p::info("Next steps to configure the account on-chain:");
    for (index, step) in setup_steps.iter().enumerate() {
        println!("  {}. {}", index + 1, step.title);
        println!("     {}", step.command.cyan());
    }
    println!();
    p::info("After your account is updated on-chain, collect signatures with:");
    println!(
        "  {}",
        format!(
            "starforge wallet multisig sign {} --transaction tx.json",
            account.name
        )
        .cyan()
    );
    Ok(())
}

async fn multisig_sign(
    name: String,
    transaction: PathBuf,
    output: Option<PathBuf>,
    hardware: Option<hardware_wallet::HardwareWalletKind>,
    hd_path: String,
    network: String,
) -> Result<()> {
    config::validate_wallet_name(&name)?;
    config::validate_file_path(&transaction, Some("json"))?;
    config::validate_network(&network)?;
    crate::utils::network_guard::verify(&network).await?;

    let account = multisig::load_account(&name)?;
    let cfg = config::load()?;

    let mut tx = multisig::load_transaction(&transaction)?;

    p::header(&format!("Multi-sig Sign: {}", name));
    p::kv("Account", &account.account_id);
    p::kv("Transaction", &transaction.display().to_string());
    p::kv("Network", &network);

    let mut signed = 0u32;

    if let Some(device) = hardware {
        let matching_signer = account.signers.iter().find(|signer| {
            cfg.wallets
                .iter()
                .any(|wallet| wallet.public_key == signer.public_key && wallet.secret_key.is_none())
        });

        let signer_key = if let Some(signer) = matching_signer {
            signer.public_key.clone()
        } else if let Some(first) = account.signers.first() {
            first.public_key.clone()
        } else {
            anyhow::bail!("Multi-sig account has no configured signers");
        };

        let wallet_ref = cfg
            .wallets
            .iter()
            .find(|w| w.public_key == signer_key)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No local wallet entry found for signer {}. Import it with --hardware first.",
                    signer_key
                )
            })?;

        let signing_request = crate::utils::wallet_signer::SigningRequest::from_options(
            Some(wallet_ref),
            Some(device),
            Some(&hd_path),
            &network,
            false,
            "multi-sig transaction",
        )?;

        let sig = multisig::sign_transaction_partial_with_request(
            &tx.transaction_xdr,
            &signing_request,
            &wallet_ref.name,
        )?;
        multisig::add_signature_to_transaction(&mut tx, &wallet_ref.public_key, sig)?;
        signed = 1;
    } else {
        // Attempt to sign with every configured signer that we have a local secret key for.
        for s in &account.signers {
            let wallet_name = s.name.clone().unwrap_or_else(|| s.public_key.clone());
            let Some(w) = cfg.wallets.iter().find(|w| w.public_key == s.public_key) else {
                continue;
            };
            let Some(sk) = &w.secret_key else {
                continue;
            };

            let plain_sk = if !sk.contains(':') && sk.starts_with('S') && sk.len() == 56 {
                sk.clone()
            } else {
                let pwd = crypto::prompt_password(
                    &format!("Enter password for signer wallet '{}'", w.name),
                    false,
                )?;
                crypto::decrypt_secret(&pwd, sk)
                    .map_err(|_| anyhow::anyhow!("Incorrect password or unable to decrypt."))?
            };

            let sig = multisig::sign_transaction_partial(&tx.transaction_xdr, &plain_sk, &network)?;
            if multisig::add_signature_to_transaction(&mut tx, &wallet_name, sig).is_ok() {
                signed += 1;
            }
        }
    }

    tx.threshold_required = account.thresholds.high;
    tx.current_weight = tx.signatures.len().min(u8::MAX as usize) as u8;
    if multisig::check_transaction_ready(&tx) {
        tx.status = multisig::TransactionStatus::ReadyToSubmit;
    }

    let out_path = output.unwrap_or_else(|| transaction.clone());
    multisig::save_transaction(&out_path, &tx)?;

    println!();
    p::success("Signatures updated");
    p::kv("Signatures added", &signed.to_string());
    p::kv("Total signatures", &tx.signatures.len().to_string());
    p::kv("Output", &out_path.display().to_string());

    if tx.status == multisig::TransactionStatus::ReadyToSubmit {
        p::info("Transaction meets threshold and is ready to submit.");
    } else {
        p::warn("Transaction does not yet meet threshold.");
    }

    Ok(())
}

fn multisig_list() -> Result<()> {
    p::header("Multi-Signature Accounts");
    let accounts = multisig::list_accounts().unwrap_or_default();
    if accounts.is_empty() {
        p::info("No multi-sig accounts found. Create one with: starforge wallet multisig create");
        return Ok(());
    }

    p::separator();
    for (i, acct) in accounts.iter().enumerate() {
        println!("  {:>2}. {}", i + 1, acct.name.bold());
        p::kv("Account ID", &acct.account_id);
        p::kv("Signers", &acct.signers.len().to_string());
        p::kv("Threshold", &acct.thresholds.high.to_string());
        if i < accounts.len() - 1 {
            println!();
        }
    }
    p::separator();
    Ok(())
}

fn multisig_show(name: String) -> Result<()> {
    let multisig_account = multisig::load_account(&name)?;

    p::header(&format!("Multi-Sig Account: {}", name));
    p::separator();
    p::kv_accent("Account ID", &multisig_account.account_id);
    p::kv("Created", &multisig_account.created_at);

    println!();
    p::info("Thresholds:");
    p::kv("  Low", &multisig_account.thresholds.low.to_string());
    p::kv("  Medium", &multisig_account.thresholds.medium.to_string());
    p::kv("  High", &multisig_account.thresholds.high.to_string());

    println!();
    p::info(&format!("Signers ({}):", multisig_account.signers.len()));

    if multisig_account.signers.is_empty() {
        p::warn("  No signers configured yet");
    } else {
        for (i, signer) in multisig_account.signers.iter().enumerate() {
            println!();
            p::kv(&format!("  [{}] Key", i + 1), &signer.public_key);
            p::kv(&format!("  [{}] Weight", i + 1), &signer.weight.to_string());
            if let Some(ref signer_name) = signer.name {
                p::kv(&format!("  [{}] Name", i + 1), signer_name);
            }
        }
    }

    let total_weight = multisig::calculate_total_weight(&multisig_account.signers);
    println!();
    p::kv_accent("Total Weight", &total_weight.to_string());

    p::separator();
    Ok(())
}

async fn multisig_submit(
    name: String,
    transaction: PathBuf,
    network: Option<String>,
) -> Result<()> {
    config::validate_wallet_name(&name)?;
    config::validate_file_path(&transaction, Some("json"))?;

    let account = multisig::load_account(&name)?;
    let tx = multisig::load_transaction(&transaction)?;

    let network = network.unwrap_or_else(|| "testnet".to_string());
    config::validate_network(&network)?;

    p::header(&format!("Multi-Sig Submit: {}", name));
    p::kv("Account", &account.account_id);
    p::kv("Network", &network);
    p::kv("Signatures", &tx.signatures.len().to_string());
    p::kv("Threshold", &tx.threshold_required.to_string());

    if tx.status != multisig::TransactionStatus::ReadyToSubmit {
        anyhow::bail!(
            "Transaction is not ready to submit (status: {:?}). \
             Collect enough signatures first with `starforge wallet multisig sign`.",
            tx.status
        );
    }

    crate::utils::network_guard::verify(&network).await?;

    p::step(1, 2, "Combining signatures into final envelopeâ€¦");
    let signed_xdr = multisig::combine_signatures(&tx.transaction_xdr, &tx.signatures)?;

    p::step(2, 2, &format!("Submitting to Horizon ({})â€¦", network));
    let result = horizon::submit_multisig_transaction(&signed_xdr, &network).await?;

    println!();
    p::success("Transaction submitted");
    p::kv_accent("Hash", &result.hash);
    p::kv("Successful", &result.successful.to_string());
    p::info(&format!(
        "View on explorer: https://stellar.expert/explorer/{}/tx/{}",
        network, result.hash
    ));
    Ok(())
}
