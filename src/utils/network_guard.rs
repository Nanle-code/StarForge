//! Protect signing operations from accidentally targeting a different Stellar network.

use anyhow::Result;
use std::sync::atomic::{AtomicBool, Ordering};

static ALLOW_MISMATCH: AtomicBool = AtomicBool::new(false);

pub fn set_allow_mismatch(allowed: bool) {
    ALLOW_MISMATCH.store(allowed, Ordering::Relaxed);
}

pub fn allow_mismatch() -> bool {
    ALLOW_MISMATCH.load(Ordering::Relaxed)
}

pub fn compare_passphrases(configured: &str, observed: &str) -> Result<()> {
    if configured == observed || allow_mismatch() {
        return Ok(());
    }
    anyhow::bail!(
        "network passphrase mismatch: configured='{}', observed='{}'",
        configured,
        observed
    )
}

pub async fn verify(network: &str) -> Result<()> {
    let configured = crate::utils::config::get_network_passphrase(network);
    let observed = crate::utils::horizon::fetch_network_passphrase(network).await?;

    if configured == observed {
        return Ok(());
    }

    let endpoint = crate::utils::horizon::horizon_url(network)?;
    let detail = format!(
        "Network passphrase mismatch for '{}': configured='{}', endpoint='{}', observed='{}'",
        network, configured, endpoint, observed
    );
    if allow_mismatch() {
        if crate::utils::output::is_json_mode_enabled() {
            println!(
                "{}",
                serde_json::json!({
                    "warning": "network_passphrase_mismatch",
                    "network": network,
                    "configured_passphrase": configured,
                    "observed_passphrase": observed,
                    "endpoint": endpoint,
                    "override": true
                })
            );
        } else {
            crate::utils::print::warn(&format!(
                "{} Signing continues because --allow-network-passphrase-mismatch was supplied.",
                detail
            ));
        }
        return Ok(());
    }

    anyhow::bail!(
        "{}. Signing aborted. Verify the endpoint/configuration, or explicitly acknowledge the risk with --allow-network-passphrase-mismatch.",
        detail
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn matching_passphrases_are_accepted() {
        super::set_allow_mismatch(false);
        assert!(super::compare_passphrases("test", "test").is_ok());
    }

    #[test]
    fn mismatching_passphrases_are_rejected_by_default() {
        super::set_allow_mismatch(false);
        assert!(super::compare_passphrases("test", "main").is_err());
    }

    #[test]
    fn mismatch_override_defaults_to_disabled() {
        super::set_allow_mismatch(false);
        assert!(!super::allow_mismatch());
    }
}
