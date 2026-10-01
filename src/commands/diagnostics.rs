use crate::utils::hardware_wallet::{self, HardwareWalletKind};
use anyhow::Result;
use clap::Args;
use colored::*;

#[derive(Args, Debug)]
pub struct DiagnosticsArgs {
    /// Specify an isolated hardware target assessment ("ledger" or "trezor")
    #[arg(short, long)]
    pub wallet: Option<String>,
}

/// Handles hardware-wallet diagnostics through StarForge's native Rust
/// hardware-wallet implementation.
pub fn handle(args: DiagnosticsArgs) -> Result<()> {
    println!("{}", "Checking hardware-wallet connectivity...".cyan());

    for kind in selected_wallets(args.wallet.as_deref())? {
        match hardware_wallet::device_status(kind) {
            Ok(status) => println!("{}", format!("✔ {status}").green()),
            Err(error) => {
                println!("{}", format!("✘ {kind}: unavailable").red());
                println!("  Error: {error}");
                println!(
                    "  Recovery: Connect and unlock the device, open the Stellar app, and close other wallet applications."
                );
            }
        }
    }

    Ok(())
}

fn selected_wallets(wallet: Option<&str>) -> Result<Vec<HardwareWalletKind>> {
    match wallet {
        None => Ok(vec![HardwareWalletKind::Ledger, HardwareWalletKind::Trezor]),
        Some(wallet) => match wallet.to_ascii_lowercase().as_str() {
            "ledger" => Ok(vec![HardwareWalletKind::Ledger]),
            "trezor" => Ok(vec![HardwareWalletKind::Trezor]),
            _ => anyhow::bail!("Unsupported wallet type '{wallet}'. Choose 'ledger' or 'trezor'."),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{selected_wallets, HardwareWalletKind};

    #[test]
    fn diagnostics_selects_both_wallets_by_default() {
        let wallets = selected_wallets(None).unwrap();
        assert!(matches!(wallets[0], HardwareWalletKind::Ledger));
        assert!(matches!(wallets[1], HardwareWalletKind::Trezor));
    }

    #[test]
    fn diagnostics_wallet_filter_is_case_insensitive() {
        let wallets = selected_wallets(Some("LEDGER")).unwrap();
        assert_eq!(wallets.len(), 1);
        assert!(matches!(wallets[0], HardwareWalletKind::Ledger));
    }

    #[test]
    fn diagnostics_rejects_unknown_wallet_filter() {
        assert!(selected_wallets(Some("other")).is_err());
    }
}
