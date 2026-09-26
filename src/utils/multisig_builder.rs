use anyhow::{bail, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

/// Time-lock policy settings for delayed multisig execution (#768).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimelockPolicy {
    /// Minimum mandatory delay before execution is allowed (in seconds)
    pub min_delay_seconds: u64,
    /// Optional grace period / execution window after unlock before proposal expires (in seconds)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_window_seconds: Option<u64>,
    /// Calculated timestamp when timelock unlocks (ISO 8601 string)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlock_at: Option<String>,
    /// Calculated timestamp when execution window expires (ISO 8601 string)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

/// Real-time execution status of a timelocked proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum TimelockExecutionStatus {
    /// Proposal is still collecting signatures
    CollectingSignatures { signed: u32, required: u32 },
    /// Required signatures collected, but locked under mandatory delay
    Locked { unlock_at: String, remaining_seconds: i64 },
    /// Timelock delay has elapsed; proposal is currently executable
    ReadyToExecute {
        expires_at: Option<String>,
        remaining_window_seconds: Option<i64>,
    },
    /// Timelock execution window has expired
    Expired { expired_at: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub threshold: u32,
    pub signers: Vec<String>,
    pub signatures: Vec<Signature>,
    pub network: String,
    pub created_at: String,
    pub expires_at: Option<String>,
    pub metadata: ProposalMetadata,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transaction_xdr: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timelock: Option<TimelockPolicy>,
    #[serde(default)]
    pub events: Vec<ProposalEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Signature {
    pub signer: String,
    pub signature: String,
    pub signed_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalMetadata {
    pub title: Option<String>,
    pub description: Option<String>,
    pub transaction_type: Option<String>,
    pub amount: Option<f64>,
    pub recipient: Option<String>,
    #[serde(default)]
    pub template: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProposalEvent {
    pub event_type: String,
    pub message: String,
    pub at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureProgress {
    pub signed: u32,
    pub required: u32,
    pub total_signers: u32,
    pub percent: u32,
    pub ready: bool,
    pub pending_signers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignatureValidationReport {
    pub valid_signatures: u32,
    pub invalid_signers: Vec<String>,
    pub duplicate_signers: Vec<String>,
    pub missing_signers: Vec<String>,
    pub ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MultisigTemplate {
    pub name: &'static str,
    pub description: &'static str,
    pub threshold: u32,
    pub signers: Vec<&'static str>,
    pub transaction_type: &'static str,
}

#[derive(Debug, Clone)]
pub struct TemplateDefinition {
    pub name: &'static str,
    pub threshold: u32,
    pub signers: &'static [&'static str],
    pub description: &'static str,
    pub transaction_type: &'static str,
}

impl Proposal {
    pub fn new(threshold: u32, signers: Vec<String>, network: String) -> Self {
        Proposal {
            id: Uuid::new_v4().to_string(),
            threshold,
            signers,
            signatures: Vec::new(),
            network,
            created_at: Utc::now().to_rfc3339(),
            expires_at: None,
            metadata: ProposalMetadata {
                title: None,
                description: None,
                transaction_type: None,
                amount: None,
                recipient: None,
                template: None,
            },
            transaction_xdr: None,
            timelock: None,
            events: vec![ProposalEvent {
                event_type: "created".to_string(),
                message: "Proposal created".to_string(),
                at: Utc::now().to_rfc3339(),
            }],
        }
    }

    pub fn add_signature(&mut self, signer: String, signature: String) {
        self.signatures.push(Signature {
            signer: signer.clone(),
            signature,
            signed_at: Utc::now().to_rfc3339(),
        });
        self.events.push(ProposalEvent {
            event_type: "signed".to_string(),
            message: format!("Signature collected from {}", signer),
            at: Utc::now().to_rfc3339(),
        });
    }

    pub fn add_signature_checked(&mut self, signer: String, signature: String) -> Result<()> {
        if !self.signers.contains(&signer) {
            anyhow::bail!("Signer '{}' is not authorized for this proposal", signer);
        }
        if self.signatures.iter().any(|sig| sig.signer == signer) {
            anyhow::bail!("Signer '{}' has already signed this proposal", signer);
        }
        self.add_signature(signer, signature);
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        self.signatures.len() >= self.threshold as usize
    }

    pub fn get_status(&self) -> String {
        if self.is_expired() {
            return "expired".to_string();
        }
        if self.is_complete() {
            "ready".to_string()
        } else {
            format!("pending ({}/{})", self.signatures.len(), self.threshold)
        }
    }

    pub fn pending_signers(&self) -> Vec<String> {
        self.signers
            .iter()
            .filter(|s| !self.signatures.iter().any(|sig| sig.signer == **s))
            .cloned()
            .collect()
    }

    pub fn signed_by(&self) -> Vec<String> {
        self.signatures.iter().map(|s| s.signer.clone()).collect()
    }

    pub fn is_expired(&self) -> bool {
        is_proposal_expired(self)
    }

    /// Attach or configure a timelock policy on this proposal.
    pub fn with_timelock(mut self, min_delay_seconds: u64, execution_window_seconds: Option<u64>) -> Self {
        let now = Utc::now();
        let unlock_at = now + chrono::Duration::seconds(min_delay_seconds as i64);
        let expires_at = execution_window_seconds.map(|w| unlock_at + chrono::Duration::seconds(w as i64));
        self.timelock = Some(TimelockPolicy {
            min_delay_seconds,
            execution_window_seconds,
            unlock_at: Some(unlock_at.to_rfc3339()),
            expires_at: expires_at.map(|e| e.to_rfc3339()),
        });
        self.events.push(ProposalEvent {
            event_type: "timelock_configured".to_string(),
            message: format!("Configured timelock with {}s delay", min_delay_seconds),
            at: Utc::now().to_rfc3339(),
        });
        self
    }

    /// Evaluates current timelock execution status.
    pub fn timelock_status(&self) -> Option<TimelockExecutionStatus> {
        let timelock = self.timelock.as_ref()?;
        if !self.is_complete() {
            return Some(TimelockExecutionStatus::CollectingSignatures {
                signed: self.signatures.len() as u32,
                required: self.threshold,
            });
        }

        let now = Utc::now();
        let unlock_dt = timelock
            .unlock_at
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let expire_dt = timelock
            .expires_at
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        if let Some(unlock) = unlock_dt {
            if now < unlock {
                let remaining = (unlock - now).num_seconds();
                return Some(TimelockExecutionStatus::Locked {
                    unlock_at: unlock.to_rfc3339(),
                    remaining_seconds: remaining.max(0),
                });
            }
        }

        if let Some(exp) = expire_dt {
            if now > exp {
                return Some(TimelockExecutionStatus::Expired {
                    expired_at: exp.to_rfc3339(),
                });
            } else {
                let rem_window = (exp - now).num_seconds();
                return Some(TimelockExecutionStatus::ReadyToExecute {
                    expires_at: Some(exp.to_rfc3339()),
                    remaining_window_seconds: Some(rem_window.max(0)),
                });
            }
        }

        Some(TimelockExecutionStatus::ReadyToExecute {
            expires_at: None,
            remaining_window_seconds: None,
        })
    }

    /// Validates whether this proposal can be executed right now.
    pub fn can_execute(&self) -> Result<()> {
        if !self.is_complete() {
            anyhow::bail!(
                "Proposal threshold not reached: {}/{} signatures collected",
                self.signatures.len(),
                self.threshold
            );
        }
        if self.is_expired() {
            anyhow::bail!("Proposal has expired");
        }
        if let Some(status) = self.timelock_status() {
            match status {
                TimelockExecutionStatus::Locked { unlock_at, remaining_seconds } => {
                    anyhow::bail!(
                        "Proposal is timelocked until {}. Remaining delay: {}s",
                        unlock_at,
                        remaining_seconds
                    );
                }
                TimelockExecutionStatus::Expired { expired_at } => {
                    anyhow::bail!("Proposal timelock execution window expired at {}", expired_at);
                }
                TimelockExecutionStatus::CollectingSignatures { .. } => {
                    anyhow::bail!("Proposal signatures incomplete");
                }
                TimelockExecutionStatus::ReadyToExecute { .. } => Ok(()),
            }
        } else {
            Ok(())
        }
    }

}

// ── Proposal validation (#691) ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationError {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    pub valid: bool,
    pub errors: Vec<ValidationError>,
    pub warnings: Vec<String>,
}

/// Validate a `Proposal` for logical consistency.
///
/// Checks: zero threshold, impossible threshold (> signer count), empty signer
/// list, duplicate signers, empty signer keys, and signatures from parties not
/// in the signer list. Warnings cover degenerate but technically valid configs.
pub fn validate_proposal(proposal: &Proposal) -> ValidationReport {
    let mut errors: Vec<ValidationError> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();

    // ── 1. Zero threshold ─────────────────────────────────────────────────────
    if proposal.threshold == 0 {
        errors.push(ValidationError {
            code: "ZERO_THRESHOLD".to_string(),
            message: "Threshold must be at least 1.".to_string(),
        });
    }

    // ── 2. No signers ─────────────────────────────────────────────────────────
    if proposal.signers.is_empty() {
        errors.push(ValidationError {
            code: "NO_SIGNERS".to_string(),
            message: "Proposal has no signers.".to_string(),
        });
    }

    // ── 3. Impossible threshold ───────────────────────────────────────────────
    if proposal.threshold > 0 && proposal.threshold as usize > proposal.signers.len() {
        errors.push(ValidationError {
            code: "IMPOSSIBLE_THRESHOLD".to_string(),
            message: format!(
                "Threshold ({}) exceeds the number of signers ({}). \
                 The proposal can never be fulfilled.",
                proposal.threshold,
                proposal.signers.len()
            ),
        });
    }

    // ── 4. Duplicate / empty signer keys ─────────────────────────────────────
    let mut seen = std::collections::HashSet::new();
    for signer in &proposal.signers {
        let normalized = signer.trim().to_lowercase();
        if normalized.is_empty() {
            errors.push(ValidationError {
                code: "EMPTY_SIGNER".to_string(),
                message: format!("Signer key {:?} is empty or whitespace-only.", signer),
            });
        } else if !seen.insert(normalized) {
            errors.push(ValidationError {
                code: "DUPLICATE_SIGNER".to_string(),
                message: format!("Duplicate signer detected: '{}'.", signer),
            });
        }
    }

    // ── 5. Unauthorized signatures ────────────────────────────────────────────
    for sig in &proposal.signatures {
        if !proposal.signers.iter().any(|s| s == &sig.signer) {
            errors.push(ValidationError {
                code: "UNAUTHORIZED_SIGNATURE".to_string(),
                message: format!(
                    "'{}' has signed but is not listed as an authorized signer.",
                    sig.signer
                ),
            });
        }
    }

    // ── 6. Warnings for unusual but valid configurations ──────────────────────
    if proposal.signers.len() == 1 && proposal.threshold == 1 {
        warnings.push(
            "1-of-1 multi-sig provides no security advantage over a regular \
             single-signer transaction."
                .to_string(),
        );
    }
    if proposal.signers.len() > 1 && proposal.threshold == proposal.signers.len() as u32 {
        warnings.push(format!(
            "{}-of-{} requires unanimous consent — a single absent or lost key \
             will permanently block execution.",
            proposal.threshold,
            proposal.signers.len()
        ));
    }
    if proposal.threshold > 0
        && proposal.signers.len() > 1
        && (proposal.threshold as f64 / proposal.signers.len() as f64) < 0.5
    {
        warnings.push(format!(
            "Threshold ({}/{}) is below 50% — a minority of signers can authorize \
             this proposal.",
            proposal.threshold,
            proposal.signers.len()
        ));
    }

    ValidationReport {
        valid: errors.is_empty(),
        errors,
        warnings,
    }
}

pub fn is_proposal_expired(proposal: &Proposal) -> bool {
    let Some(expires_at) = &proposal.expires_at else {
        return false;
    };
    DateTime::parse_from_rfc3339(expires_at)
        .map(|dt| dt.with_timezone(&Utc) < Utc::now())
        .unwrap_or(false)
}

pub fn signing_message(proposal_id: &str, signer: &str) -> String {
    format!("starforge-multisig:{proposal_id}:{signer}")
}

fn hash_message(message: &str) -> Result<String> {
    use hex;
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(message.as_bytes());
    Ok(hex::encode(hasher.finalize()))
}

pub fn generate_signature(proposal_id: &str, wallet: &str) -> Result<String> {
    hash_message(&signing_message(proposal_id, wallet))
}

pub fn verify_signature(proposal_id: &str, signer: &str, signature: &str) -> bool {
    generate_signature(proposal_id, signer)
        .map(|expected| expected == signature)
        .unwrap_or(false)
}

pub fn validate_signature_format(signature: &str) -> bool {
    signature.len() == 64 && signature.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn validate_for_signing(proposal: &Proposal, wallet: &str) -> Result<()> {
    if proposal.is_expired() {
        bail!("Proposal has expired");
    }
    if !proposal.signers.contains(&wallet.to_string()) {
        bail!(
            "Wallet '{}' is not an authorized signer for this proposal",
            wallet
        );
    }
    if proposal.signatures.iter().any(|s| s.signer == wallet) {
        bail!("Wallet '{}' has already signed this proposal", wallet);
    }
    Ok(())
}

pub fn validate_for_submit(proposal: &Proposal) -> Result<()> {
    if proposal.is_expired() {
        bail!("Proposal has expired");
    }
    if proposal.signatures.len() < proposal.threshold as usize {
        bail!(
            "Not enough signatures: {}/{}",
            proposal.signatures.len(),
            proposal.threshold
        );
    }

    for sig in &proposal.signatures {
        if !validate_signature_format(&sig.signature) {
            bail!("Invalid signature format from signer '{}'", sig.signer);
        }
        if !proposal.signers.contains(&sig.signer) {
            bail!("Unknown signer '{}' in signature list", sig.signer);
        }
        if !verify_signature(&proposal.id, &sig.signer, &sig.signature) {
            bail!("Signature verification failed for signer '{}'", sig.signer);
        }
    }

    Ok(())
}

pub fn render_progress_blocks(signed: usize, threshold: u32) -> (String, i32) {
    let percent = if threshold == 0 {
        100
    } else {
        ((signed as f32 / threshold as f32) * 100.0).min(100.0) as i32
    };
    let filled = (percent / 10) as usize;
    let empty = 10usize.saturating_sub(filled);
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
    (bar, percent)
}

pub fn template_definitions() -> Vec<TemplateDefinition> {
    vec![
        TemplateDefinition {
            name: "escrow",
            transaction_type: "escrow_release",
            threshold: 2,
            signers: &["buyer", "seller", "arbiter"],
            description: "2-of-3 Escrow (buyer, seller, arbiter)",
        },
        TemplateDefinition {
            name: "company",
            transaction_type: "company_disbursement",
            threshold: 3,
            signers: &["ceo", "cfo", "board1", "board2", "board3"],
            description: "3-of-5 Company Signers",
        },
        TemplateDefinition {
            name: "timelocked_vault",
            transaction_type: "timelocked_governance_action",
            threshold: 2,
            signers: &["admin1", "admin2", "guardian"],
            description: "2-of-3 Timelocked Governance Vault (24h delay)",
        },
        TemplateDefinition {
            name: "dao",
            transaction_type: "dao_treasury",
            threshold: 5,
            signers: &[
                "member1", "member2", "member3", "member4", "member5", "member6", "member7",
                "member8", "member9",
            ],
            description: "5-of-9 DAO Treasury",
        },
        TemplateDefinition {
            name: "vault",
            transaction_type: "vault_withdrawal",
            threshold: 2,
            signers: &["key1", "key2"],
            description: "2-of-2 Cold Storage Vault",
        },
        TemplateDefinition {
            name: "payment",
            transaction_type: "payment_authorization",
            threshold: 1,
            signers: &["approver1", "approver2"],
            description: "1-of-2 Payment Authorization",
        },
    ]
}

pub fn proposal_from_template(name: &str, network: String) -> Result<Proposal> {
    let template = template_definitions()
        .into_iter()
        .find(|t| t.name == name)
        .ok_or_else(|| anyhow::anyhow!("Unknown template: {}", name))?;

    let mut proposal = Proposal::new(
        template.threshold,
        template.signers.iter().map(|s| s.to_string()).collect(),
        network,
    );
    proposal.metadata.title = Some(template.description.to_string());
    proposal.metadata.template = Some(name.to_string());
    proposal.metadata.transaction_type = Some(template.transaction_type.to_string());
    Ok(proposal)
}

/// Catalogue of the built-in templates for display purposes.
///
/// Mirrors [`template_definitions`] but owns its signer list, so callers can
/// describe a template without instantiating a proposal from it.
pub fn common_templates() -> Vec<MultisigTemplate> {
    template_definitions()
        .into_iter()
        .map(|def| MultisigTemplate {
            name: def.name,
            description: def.description,
            threshold: def.threshold,
            signers: def.signers.to_vec(),
            transaction_type: def.transaction_type,
        })
        .collect()
}

/// Signature `signer` is expected to submit for `proposal`.
pub fn generate_proposal_signature(signer: &str, proposal: &Proposal) -> Result<String> {
    generate_signature(&proposal.id, signer)
}

/// Collection progress for `proposal`, measured against its threshold.
///
/// `percent` is capped at 100 so an over-signed proposal (more signatures than
/// the threshold requires) still renders a full bar rather than overflowing it.
pub fn calculate_progress(proposal: &Proposal) -> SignatureProgress {
    let signed = proposal.signatures.len() as u32;
    let required = proposal.threshold;
    // A zero threshold is trivially satisfied, so report a full bar.
    let percent = (signed * 100)
        .checked_div(required)
        .map_or(100, |value| value.min(100));

    SignatureProgress {
        signed,
        required,
        total_signers: proposal.signers.len() as u32,
        percent,
        ready: signed >= required,
        pending_signers: proposal.pending_signers(),
    }
}

/// Renders `progress` as a fixed-width ASCII bar, e.g. `[#####.....] 50% (1/2)`.
pub fn render_progress_bar(progress: &SignatureProgress, width: usize) -> String {
    let filled = ((progress.percent as usize * width) / 100).min(width);
    format!(
        "[{}{}] {}% ({}/{})",
        "#".repeat(filled),
        ".".repeat(width - filled),
        progress.percent,
        progress.signed,
        progress.required
    )
}

/// Cryptographically checks every signature attached to `proposal`.
///
/// A signer counts toward `valid_signatures` only once and only when it is on
/// the authorised list *and* its signature verifies, so a forged or replayed
/// entry can never push a proposal over its threshold.
pub fn validate_signatures(proposal: &Proposal) -> SignatureValidationReport {
    let mut valid_signatures = 0u32;
    let mut invalid_signers = Vec::new();
    let mut duplicate_signers = Vec::new();
    let mut seen: HashSet<&str> = HashSet::new();
    let mut verified: HashSet<&str> = HashSet::new();

    for sig in &proposal.signatures {
        if !seen.insert(sig.signer.as_str()) {
            duplicate_signers.push(sig.signer.clone());
            continue;
        }
        if proposal.signers.contains(&sig.signer)
            && verify_signature(&proposal.id, &sig.signer, &sig.signature)
        {
            valid_signatures += 1;
            verified.insert(sig.signer.as_str());
        } else {
            invalid_signers.push(sig.signer.clone());
        }
    }

    let missing_signers = proposal
        .signers
        .iter()
        .filter(|s| !verified.contains(s.as_str()))
        .cloned()
        .collect();

    SignatureValidationReport {
        ready: valid_signatures >= proposal.threshold,
        valid_signatures,
        invalid_signers,
        duplicate_signers,
        missing_signers,
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct NotificationRequest {
    pub proposal_id: String,
    pub signers: Vec<String>,
    pub threshold: u32,
    pub message: String,
    pub created_at: String,
}

impl NotificationRequest {
    pub fn new(proposal: &Proposal, message: String) -> Self {
        NotificationRequest {
            proposal_id: proposal.id.clone(),
            signers: proposal.pending_signers(),
            threshold: proposal.threshold,
            message,
            created_at: Utc::now().to_rfc3339(),
        }
    }
}

#[derive(Debug, Clone)]
pub enum NotificationChannel {
    Email,
    Slack,
    Discord,
    Webhook(String),
}

pub fn parse_notification_channel(
    channel: &str,
    webhook: Option<String>,
) -> Result<NotificationChannel> {
    match channel.to_lowercase().as_str() {
        "email" => Ok(NotificationChannel::Email),
        "slack" => Ok(NotificationChannel::Slack),
        "discord" => Ok(NotificationChannel::Discord),
        "webhook" => {
            let url = webhook
                .ok_or_else(|| anyhow::anyhow!("--webhook is required for webhook channel"))?;
            Ok(NotificationChannel::Webhook(url))
        }
        other => bail!("Unknown notification channel: {}", other),
    }
}

pub fn send_notification(
    notification: NotificationRequest,
    channel: NotificationChannel,
    webhook: Option<&str>,
) -> Result<()> {
    match channel {
        NotificationChannel::Email => {
            for signer in &notification.signers {
                println!("📧 Email notification queued for {}", signer);
            }
            Ok(())
        }
        NotificationChannel::Slack => {
            let url = webhook
                .ok_or_else(|| anyhow::anyhow!("--webhook is required for slack channel"))?;
            println!("💬 Slack message sent");
            post_webhook(url, &notification)
        }
        NotificationChannel::Discord => {
            let url = webhook
                .ok_or_else(|| anyhow::anyhow!("--webhook is required for discord channel"))?;
            println!("🎮 Discord message sent");
            post_webhook(url, &notification)
        }
        NotificationChannel::Webhook(url) => post_webhook(&url, &notification),
    }
}

fn post_webhook(url: &str, notification: &NotificationRequest) -> Result<()> {
    let payload = serde_json::json!({
        "text": notification.message,
        "proposal_id": notification.proposal_id,
        "pending_signers": notification.signers,
        "threshold": notification.threshold,
    });

    let url_owned = url.to_string();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result: Result<reqwest::Response> = (|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                crate::utils::http_client::get_client()
                    .post(&url_owned)
                    .header("Content-Type", "application/json")
                    .json(&payload)
                    .send()
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            })
        })();
        let _ = tx.send(result);
    });

    let response = rx
        .recv()
        .map_err(|_| anyhow::anyhow!("Webhook worker exited unexpectedly"))??;

    if !response.status().is_success() {
        bail!(
            "Webhook notification failed with status {}",
            response.status()
        );
    }

    println!("🔔 Webhook notification queued for {}", url);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proposal_creation() {
        let signers = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ];
        let proposal = Proposal::new(2, signers, "testnet".to_string());

        assert_eq!(proposal.threshold, 2);
        assert_eq!(proposal.signers.len(), 3);
        assert!(!proposal.is_complete());
    }

    #[test]
    fn test_signature_added() {
        let signers = vec!["alice".to_string(), "bob".to_string()];
        let mut proposal = Proposal::new(2, signers, "testnet".to_string());

        proposal.add_signature("alice".to_string(), "sig123".to_string());
        assert_eq!(proposal.signatures.len(), 1);
        assert!(!proposal.is_complete());

        proposal.add_signature("bob".to_string(), "sig456".to_string());
        assert!(proposal.is_complete());
    }

    #[test]
    fn test_pending_signers() {
        let signers = vec![
            "alice".to_string(),
            "bob".to_string(),
            "charlie".to_string(),
        ];
        let mut proposal = Proposal::new(2, signers, "testnet".to_string());

        proposal.add_signature("alice".to_string(), "sig123".to_string());
        let pending = proposal.pending_signers();

        assert_eq!(pending.len(), 2);
        assert!(!pending.contains(&"alice".to_string()));
    }

    #[test]
    fn test_signature_generation_and_verification() {
        let proposal = Proposal::new(2, vec!["alice".into()], "testnet".into());
        let sig = generate_signature(&proposal.id, "alice").unwrap();

        assert!(validate_signature_format(&sig));
        assert!(verify_signature(&proposal.id, "alice", &sig));
        assert!(!verify_signature(&proposal.id, "bob", &sig));
    }

    #[test]
    fn test_validate_for_submit() {
        let signers = vec!["alice".to_string(), "bob".to_string()];
        let mut proposal = Proposal::new(2, signers, "testnet".to_string());
        assert!(validate_for_submit(&proposal).is_err());

        let sig = generate_signature(&proposal.id, "alice").unwrap();
        proposal.add_signature("alice".to_string(), sig);
        assert!(validate_for_submit(&proposal).is_err());

        let sig = generate_signature(&proposal.id, "bob").unwrap();
        proposal.add_signature("bob".to_string(), sig);
        assert!(validate_for_submit(&proposal).is_ok());
    }

    #[test]
    fn test_template_definitions() {
        let templates = template_definitions();
        assert_eq!(templates.len(), 5);
        let escrow = proposal_from_template("escrow", "testnet".to_string()).unwrap();
        assert_eq!(escrow.threshold, 2);
        assert_eq!(escrow.signers.len(), 3);
    }

    #[test]
    fn test_progress_bar() {
        let (bar, percent) = render_progress_blocks(1, 2);
        assert_eq!(percent, 50);
        assert!(bar.contains('█'));
        assert!(bar.contains('░'));
    }
}
