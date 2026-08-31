use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::utils::config;

// ── Data structures ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalStatus {
    Active,
    Passed,
    TimelockReady,
    Executed,
    Rejected,
    Cancelled,
    EmergencyExecuted,
}

impl std::fmt::Display for ProposalStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let label = match self {
            ProposalStatus::Active => "active",
            ProposalStatus::Passed => "passed",
            ProposalStatus::TimelockReady => "timelock-ready",
            ProposalStatus::Executed => "executed",
            ProposalStatus::Rejected => "rejected",
            ProposalStatus::Cancelled => "cancelled",
            ProposalStatus::EmergencyExecuted => "emergency-executed",
        };
        write!(f, "{}", label)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VoteChoice {
    For,
    Against,
}

impl std::fmt::Display for VoteChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VoteChoice::For => write!(f, "for"),
            VoteChoice::Against => write!(f, "against"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vote {
    pub voter: String,
    pub choice: VoteChoice,
    pub voted_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceProposal {
    pub id: String,
    pub contract_id: String,
    pub new_wasm_hash: String,
    pub wasm_path: Option<String>,
    pub description: String,
    pub proposer: String,
    pub votes: Vec<Vote>,
    pub approval_threshold: u8,
    pub timelock_seconds: u64,
    pub timelock_expires_at: Option<String>,
    pub status: ProposalStatus,
    pub network: String,
    pub created_at: String,
    pub executed_at: Option<String>,
    pub is_emergency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceAuditEntry {
    pub id: String,
    pub proposal_id: String,
    pub action: String,
    pub actor: String,
    pub timestamp: String,
    pub details: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceConfig {
    pub default_timelock_seconds: u64,
    pub default_approval_threshold: u8,
    pub emergency_quorum: u8,
    pub emergency_guardians: Vec<String>,
}

impl Default for GovernanceConfig {
    fn default() -> Self {
        Self {
            default_timelock_seconds: 86_400,
            default_approval_threshold: 2,
            emergency_quorum: 2,
            emergency_guardians: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardSummary {
    pub total_proposals: usize,
    pub active: usize,
    pub passed: usize,
    pub timelock_ready: usize,
    pub executed: usize,
    pub rejected: usize,
    pub emergency_executed: usize,
    pub recent_audit_entries: Vec<GovernanceAuditEntry>,
}

// ── Storage ───────────────────────────────────────────────────────────────────

pub fn governance_dir() -> Result<PathBuf> {
    let dir = config::config_dir().join("governance");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn proposals_path() -> Result<PathBuf> {
    Ok(governance_dir()?.join("proposals.json"))
}

fn audit_path() -> Result<PathBuf> {
    Ok(governance_dir()?.join("audit.json"))
}

fn config_path() -> Result<PathBuf> {
    Ok(governance_dir()?.join("config.json"))
}

pub fn load_config() -> Result<GovernanceConfig> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(GovernanceConfig::default());
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

pub fn save_config(cfg: &GovernanceConfig) -> Result<()> {
    fs::write(config_path()?, serde_json::to_string_pretty(cfg)?)?;
    Ok(())
}

pub fn load_proposals() -> Result<Vec<GovernanceProposal>> {
    let path = proposals_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

pub fn save_proposals(proposals: &[GovernanceProposal]) -> Result<()> {
    fs::write(proposals_path()?, serde_json::to_string_pretty(proposals)?)?;
    Ok(())
}

pub fn load_audit_log() -> Result<Vec<GovernanceAuditEntry>> {
    let path = audit_path()?;
    if !path.exists() {
        return Ok(vec![]);
    }
    let data = fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&data).unwrap_or_default())
}

pub fn save_audit_log(entries: &[GovernanceAuditEntry]) -> Result<()> {
    fs::write(audit_path()?, serde_json::to_string_pretty(entries)?)?;
    Ok(())
}

// ── WASM helpers ──────────────────────────────────────────────────────────────

pub fn wasm_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn validate_wasm(path: &Path) -> Result<(Vec<u8>, String)> {
    if !path.exists() {
        anyhow::bail!(
            "WASM file not found: {}\nRun `stellar contract build` first.",
            path.display()
        );
    }
    let bytes = fs::read(path)?;
    if bytes.len() < 4 || &bytes[..4] != b"\0asm" {
        anyhow::bail!(
            "File does not appear to be a valid WASM binary: {}",
            path.display()
        );
    }
    let hash = wasm_hash(&bytes);
    Ok((bytes, hash))
}

// ── Audit trail ───────────────────────────────────────────────────────────────

pub fn record_audit(
    proposal_id: &str,
    action: &str,
    actor: &str,
    details: HashMap<String, String>,
) -> Result<GovernanceAuditEntry> {
    let mut entries = load_audit_log()?;
    let entry = GovernanceAuditEntry {
        id: format!("gov-audit-{}", Utc::now().timestamp_millis()),
        proposal_id: proposal_id.to_string(),
        action: action.to_string(),
        actor: actor.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        details: details.clone(),
    };
    entries.push(entry.clone());
    save_audit_log(&entries)?;

    let mut audit_details = details;
    audit_details.insert("governance_action".to_string(), action.to_string());
    let _ = crate::utils::audit::log_action(
        &format!("governance_{}", action),
        actor,
        "governance_proposal",
        proposal_id,
        audit_details,
        true,
        None,
    );

    Ok(entry)
}

// ── Voting helpers ────────────────────────────────────────────────────────────

pub fn votes_for(proposal: &GovernanceProposal) -> usize {
    proposal
        .votes
        .iter()
        .filter(|v| v.choice == VoteChoice::For)
        .count()
}

pub fn votes_against(proposal: &GovernanceProposal) -> usize {
    proposal
        .votes
        .iter()
        .filter(|v| v.choice == VoteChoice::Against)
        .count()
}

pub fn threshold_met(proposal: &GovernanceProposal) -> bool {
    votes_for(proposal) >= proposal.approval_threshold as usize
}

pub fn has_voted(proposal: &GovernanceProposal, voter: &str) -> bool {
    proposal.votes.iter().any(|v| v.voter == voter)
}

fn parse_rfc3339(ts: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .map(|dt| dt.with_timezone(&Utc))
        .with_context(|| format!("Invalid RFC3339 timestamp: {}", ts))
}

pub fn timelock_remaining(proposal: &GovernanceProposal) -> Option<Duration> {
    let expires = proposal.timelock_expires_at.as_ref()?;
    let expires_at = parse_rfc3339(expires).ok()?;
    let now = Utc::now();
    if expires_at <= now {
        None
    } else {
        Some(expires_at - now)
    }
}

pub fn refresh_timelock_status(proposal: &mut GovernanceProposal) {
    if proposal.status != ProposalStatus::Passed {
        return;
    }
    if let Some(expires) = &proposal.timelock_expires_at {
        if let Ok(expires_at) = parse_rfc3339(expires) {
            if Utc::now() >= expires_at {
                proposal.status = ProposalStatus::TimelockReady;
            }
        }
    }
}

// ── Core operations ───────────────────────────────────────────────────────────

pub fn create_proposal(
    contract_id: String,
    wasm_path: PathBuf,
    description: String,
    proposer: String,
    network: String,
    approval_threshold: Option<u8>,
    timelock_seconds: Option<u64>,
) -> Result<GovernanceProposal> {
    config::validate_contract_id(&contract_id)?;
    config::validate_network(&network)?;

    let (_, new_hash) = validate_wasm(&wasm_path)?;
    let cfg = load_config()?;
    let threshold = approval_threshold.unwrap_or(cfg.default_approval_threshold);
    let timelock = timelock_seconds.unwrap_or(cfg.default_timelock_seconds);

    if threshold == 0 {
        anyhow::bail!("Approval threshold must be at least 1");
    }

    let proposal_id = format!("gov-{}", &new_hash[..12]);
    let mut proposals = load_proposals()?;
    if proposals.iter().any(|p| p.id == proposal_id) {
        anyhow::bail!(
            "A governance proposal for this WASM hash already exists: {}",
            proposal_id
        );
    }

    let now = Utc::now().to_rfc3339();
    let proposal = GovernanceProposal {
        id: proposal_id.clone(),
        contract_id,
        new_wasm_hash: new_hash,
        wasm_path: Some(wasm_path.display().to_string()),
        description,
        proposer: proposer.clone(),
        votes: Vec::new(),
        approval_threshold: threshold,
        timelock_seconds: timelock,
        timelock_expires_at: None,
        status: ProposalStatus::Active,
        network,
        created_at: now,
        executed_at: None,
        is_emergency: false,
    };

    proposals.push(proposal.clone());
    save_proposals(&proposals)?;

    let mut details = HashMap::new();
    details.insert("wasm_hash".to_string(), proposal.new_wasm_hash.clone());
    details.insert("threshold".to_string(), threshold.to_string());
    details.insert("timelock_seconds".to_string(), timelock.to_string());
    record_audit(&proposal_id, "propose", &proposer, details)?;

    Ok(proposal)
}

pub fn cast_vote(
    proposal_id: &str,
    voter: String,
    choice: VoteChoice,
    network: &str,
) -> Result<GovernanceProposal> {
    let mut proposals = load_proposals()?;
    let proposal = proposals
        .iter_mut()
        .find(|p| p.id == proposal_id && p.network == network)
        .ok_or_else(|| anyhow::anyhow!("Proposal '{}' not found on {}", proposal_id, network))?;

    if proposal.status != ProposalStatus::Active {
        anyhow::bail!(
            "Proposal '{}' is not open for voting (status: {})",
            proposal_id,
            proposal.status
        );
    }
    if has_voted(proposal, &voter) {
        anyhow::bail!("Voter '{}' has already voted on this proposal", voter);
    }

    if proposal.is_emergency {
        let cfg = load_config()?;
        if !cfg.emergency_guardians.contains(&voter) {
            anyhow::bail!("Only emergency guardians may vote on emergency proposals");
        }
    }

    proposal.votes.push(Vote {
        voter: voter.clone(),
        choice: choice.clone(),
        voted_at: Utc::now().to_rfc3339(),
    });

    let mut details = HashMap::new();
    details.insert("vote".to_string(), choice.to_string());
    details.insert("votes_for".to_string(), votes_for(proposal).to_string());
    details.insert(
        "votes_against".to_string(),
        votes_against(proposal).to_string(),
    );
    record_audit(proposal_id, "vote", &voter, details)?;

    if threshold_met(proposal) {
        if proposal.is_emergency {
            proposal.status = ProposalStatus::EmergencyExecuted;
            proposal.executed_at = Some(Utc::now().to_rfc3339());
            let mut emerg_details = HashMap::new();
            emerg_details.insert("emergency".to_string(), "true".to_string());
            record_audit(
                proposal_id,
                "emergency_quorum_reached",
                "system",
                emerg_details,
            )?;
        } else {
            let expires = Utc::now() + Duration::seconds(proposal.timelock_seconds as i64);
            proposal.timelock_expires_at = Some(expires.to_rfc3339());
            proposal.status = ProposalStatus::Passed;

            let mut pass_details = HashMap::new();
            pass_details.insert(
                "timelock_expires_at".to_string(),
                proposal.timelock_expires_at.clone().unwrap_or_default(),
            );
            record_audit(proposal_id, "threshold_reached", "system", pass_details)?;
        }
    }

    let updated = proposal.clone();
    save_proposals(&proposals)?;
    Ok(updated)
}

pub fn reject_proposal(
    proposal_id: &str,
    actor: &str,
    network: &str,
    reason: Option<&str>,
) -> Result<GovernanceProposal> {
    let mut proposals = load_proposals()?;
    let proposal = proposals
        .iter_mut()
        .find(|p| p.id == proposal_id && p.network == network)
        .ok_or_else(|| anyhow::anyhow!("Proposal '{}' not found on {}", proposal_id, network))?;

    if !matches!(
        proposal.status,
        ProposalStatus::Active | ProposalStatus::Passed | ProposalStatus::TimelockReady
    ) {
        anyhow::bail!(
            "Proposal '{}' cannot be rejected (status: {})",
            proposal_id,
            proposal.status
        );
    }

    proposal.status = ProposalStatus::Rejected;
    let mut details = HashMap::new();
    if let Some(r) = reason {
        details.insert("reason".to_string(), r.to_string());
    }
    record_audit(proposal_id, "reject", actor, details)?;

    let updated = proposal.clone();
    save_proposals(&proposals)?;
    Ok(updated)
}

pub fn get_proposal(proposal_id: &str, network: &str) -> Result<GovernanceProposal> {
    let mut proposals = load_proposals()?;

    let index = proposals
        .iter()
        .position(|p| p.id == proposal_id && p.network == network)
        .ok_or_else(|| anyhow::anyhow!("Proposal '{}' not found on {}", proposal_id, network))?;
    refresh_timelock_status(&mut proposals[index]);
    let updated = proposals[index].clone();
    save_proposals(&proposals)?;
    Ok(updated)
}

pub fn list_proposals(
    network: Option<&str>,
    contract_id: Option<&str>,
    status: Option<&str>,
) -> Result<Vec<GovernanceProposal>> {
    let mut proposals = load_proposals()?;
    for proposal in &mut proposals {
        refresh_timelock_status(proposal);
    }
    save_proposals(&proposals)?;

    Ok(proposals
        .into_iter()
        .filter(|p| network.map_or(true, |n| p.network == n))
        .filter(|p| contract_id.map_or(true, |c| p.contract_id == c))
        .filter(|p| status.map_or(true, |s| p.status.to_string() == s))
        .collect())
}

pub fn execute_proposal(
    proposal_id: &str,
    executor: &str,
    network: &str,
) -> Result<GovernanceProposal> {
    let mut proposals = load_proposals()?;
    let proposal = proposals
        .iter_mut()
        .find(|p| p.id == proposal_id && p.network == network)
        .ok_or_else(|| anyhow::anyhow!("Proposal '{}' not found on {}", proposal_id, network))?;

    refresh_timelock_status(proposal);

    if proposal.status != ProposalStatus::TimelockReady {
        if proposal.status == ProposalStatus::Passed {
            if let Some(remaining) = timelock_remaining(proposal) {
                anyhow::bail!(
                    "Timelock not elapsed — {} hour(s) remaining",
                    remaining.num_hours()
                );
            }
        }
        anyhow::bail!(
            "Proposal '{}' is not ready for execution (status: {})",
            proposal_id,
            proposal.status
        );
    }

    if !threshold_met(proposal) {
        anyhow::bail!(
            "Approval threshold not met ({}/{} votes for)",
            votes_for(proposal),
            proposal.approval_threshold
        );
    }

    proposal.status = ProposalStatus::Executed;
    proposal.executed_at = Some(Utc::now().to_rfc3339());

    let mut details = HashMap::new();
    details.insert("wasm_hash".to_string(), proposal.new_wasm_hash.clone());
    details.insert("contract_id".to_string(), proposal.contract_id.clone());
    record_audit(proposal_id, "execute", executor, details)?;

    let updated = proposal.clone();
    save_proposals(&proposals)?;
    Ok(updated)
}

pub fn emergency_upgrade(
    contract_id: String,
    wasm_path: PathBuf,
    description: String,
    guardian: String,
    network: String,
) -> Result<GovernanceProposal> {
    config::validate_contract_id(&contract_id)?;
    config::validate_network(&network)?;

    let cfg = load_config()?;
    if cfg.emergency_guardians.is_empty() {
        anyhow::bail!(
            "No emergency guardians configured. Run `starforge governance config set --guardian <KEY>` first."
        );
    }
    if !cfg.emergency_guardians.contains(&guardian) {
        anyhow::bail!(
            "Caller '{}' is not an authorized emergency guardian",
            guardian
        );
    }

    let (_, new_hash) = validate_wasm(&wasm_path)?;
    let proposal_id = format!("gov-emerg-{}", &new_hash[..8]);
    let mut proposals = load_proposals()?;

    if proposals.iter().any(|p| p.id == proposal_id) {
        anyhow::bail!("Emergency proposal '{}' already exists", proposal_id);
    }

    let emergency_votes = vec![Vote {
        voter: guardian.clone(),
        choice: VoteChoice::For,
        voted_at: Utc::now().to_rfc3339(),
    }];

    let quorum_met = cfg.emergency_quorum <= 1;
    let status = if quorum_met {
        ProposalStatus::EmergencyExecuted
    } else {
        ProposalStatus::Active
    };

    let now = Utc::now().to_rfc3339();
    let proposal = GovernanceProposal {
        id: proposal_id.clone(),
        contract_id,
        new_wasm_hash: new_hash,
        wasm_path: Some(wasm_path.display().to_string()),
        description,
        proposer: guardian.clone(),
        votes: emergency_votes,
        approval_threshold: cfg.emergency_quorum,
        timelock_seconds: 0,
        timelock_expires_at: None,
        status,
        network,
        created_at: now.clone(),
        executed_at: if quorum_met { Some(now) } else { None },
        is_emergency: true,
    };

    proposals.push(proposal.clone());
    save_proposals(&proposals)?;

    let mut details = HashMap::new();
    details.insert("wasm_hash".to_string(), proposal.new_wasm_hash.clone());
    details.insert("emergency".to_string(), "true".to_string());
    details.insert(
        "guardian_votes".to_string(),
        votes_for(&proposal).to_string(),
    );
    record_audit(&proposal_id, "emergency_upgrade", &guardian, details)?;

    Ok(proposal)
}

pub fn add_emergency_guardian(guardian: String) -> Result<GovernanceConfig> {
    config::validate_public_key(&guardian)?;
    let mut cfg = load_config()?;
    if !cfg.emergency_guardians.contains(&guardian) {
        cfg.emergency_guardians.push(guardian);
        save_config(&cfg)?;
    }
    Ok(cfg)
}

pub fn dashboard(network: Option<&str>) -> Result<DashboardSummary> {
    let proposals = list_proposals(network, None, None)?;
    let audit = load_audit_log()?;

    let recent_audit: Vec<_> = audit.iter().rev().take(10).cloned().collect();

    Ok(DashboardSummary {
        total_proposals: proposals.len(),
        active: proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Active)
            .count(),
        passed: proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Passed)
            .count(),
        timelock_ready: proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::TimelockReady)
            .count(),
        executed: proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Executed)
            .count(),
        rejected: proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::Rejected)
            .count(),
        emergency_executed: proposals
            .iter()
            .filter(|p| p.status == ProposalStatus::EmergencyExecuted)
            .count(),
        recent_audit_entries: recent_audit,
    })
}

pub fn audit_for_proposal(proposal_id: &str) -> Result<Vec<GovernanceAuditEntry>> {
    Ok(load_audit_log()?
        .into_iter()
        .filter(|e| e.proposal_id == proposal_id)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::sync::{Mutex, OnceLock};

    static TEST_MUTEX: OnceLock<Mutex<()>> = OnceLock::new();

    fn test_home() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    fn with_isolated_governance<F: FnOnce()>(f: F) {
        let _home_guard = crate::utils::lock_home_env();
        // Recover from poisoning: one failing test must not cascade into every
        // other test that shares this lock.
        let _guard = TEST_MUTEX
            .get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let home = test_home();
        env::set_var("HOME", home.path());
        env::set_var("USERPROFILE", home.path());
        f();
    }

    /// Write a minimal WASM file whose hash is unique to `tag` and to this
    /// process.
    ///
    /// A proposal is keyed by the hash of its WASM, so two tests writing
    /// identical bytes collide in any store they end up sharing — and so do
    /// two runs, if the store outlives them. The tag separates tests and the
    /// process id separates runs, which keeps these tests independent of how
    /// well the governance directory happens to be isolated.
    fn write_test_wasm(dir: &Path, tag: &str) -> PathBuf {
        let path = dir.join("test.wasm");
        let mut bytes = b"\0asm\x01\x00\x00\x00".to_vec();
        bytes.extend_from_slice(tag.as_bytes());
        bytes.extend_from_slice(&std::process::id().to_le_bytes());
        fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn create_and_vote_reaches_threshold() {
        with_isolated_governance(|| {
            let dir = tempfile::tempdir().unwrap();
            let wasm = write_test_wasm(dir.path(), "create_and_vote");

            let proposal = create_proposal(
                "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM".to_string(),
                wasm,
                "Upgrade v2".to_string(),
                "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string(),
                "testnet".to_string(),
                Some(2),
                Some(0),
            )
            .unwrap();

            assert_eq!(proposal.status, ProposalStatus::Active);

            let p1 = cast_vote(
                &proposal.id,
                "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string(),
                VoteChoice::For,
                "testnet",
            )
            .unwrap();
            assert_eq!(p1.status, ProposalStatus::Active);

            let p2 = cast_vote(
                &proposal.id,
                "GBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB".to_string(),
                VoteChoice::For,
                "testnet",
            )
            .unwrap();
            assert_eq!(p2.status, ProposalStatus::Passed);
            assert!(p2.timelock_expires_at.is_some());
        });
    }

    #[test]
    fn double_vote_rejected() {
        with_isolated_governance(|| {
            let dir = tempfile::tempdir().unwrap();
            let wasm = write_test_wasm(dir.path(), "double_vote");
            let voter = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string();

            let proposal = create_proposal(
                "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM".to_string(),
                wasm,
                "Upgrade".to_string(),
                voter.clone(),
                "testnet".to_string(),
                Some(3),
                Some(3600),
            )
            .unwrap();

            cast_vote(&proposal.id, voter.clone(), VoteChoice::For, "testnet").unwrap();
            let err = cast_vote(&proposal.id, voter, VoteChoice::For, "testnet").unwrap_err();
            assert!(err.to_string().contains("already voted"));
        });
    }

    #[test]
    fn timelock_zero_allows_immediate_execute() {
        with_isolated_governance(|| {
            let dir = tempfile::tempdir().unwrap();
            let wasm = write_test_wasm(dir.path(), "timelock_zero");

            let proposal = create_proposal(
                "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM".to_string(),
                wasm,
                "Hotfix".to_string(),
                "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string(),
                "testnet".to_string(),
                Some(1),
                Some(0),
            )
            .unwrap();

            cast_vote(
                &proposal.id,
                "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string(),
                VoteChoice::For,
                "testnet",
            )
            .unwrap();

            let mut proposals = load_proposals().unwrap();
            let p = proposals.iter_mut().find(|p| p.id == proposal.id).unwrap();
            refresh_timelock_status(p);
            assert_eq!(p.status, ProposalStatus::TimelockReady);
            save_proposals(&proposals).unwrap();

            let executed = execute_proposal(
                &proposal.id,
                "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF",
                "testnet",
            )
            .unwrap();
            assert_eq!(executed.status, ProposalStatus::Executed);
        });
    }

    #[test]
    fn audit_trail_records_proposal_lifecycle() {
        with_isolated_governance(|| {
            let dir = tempfile::tempdir().unwrap();
            let wasm = write_test_wasm(dir.path(), "audit_trail");

            let proposal = create_proposal(
                "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM".to_string(),
                wasm,
                "Audit test".to_string(),
                "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string(),
                "testnet".to_string(),
                Some(1),
                Some(0),
            )
            .unwrap();

            let audit = audit_for_proposal(&proposal.id).unwrap();
            assert!(audit.iter().any(|e| e.action == "propose"));
        });
    }

    #[test]
    fn emergency_requires_guardian() {
        with_isolated_governance(|| {
            let dir = tempfile::tempdir().unwrap();
            let wasm = write_test_wasm(dir.path(), "emergency_guardian");
            let guardian = "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string();

            add_emergency_guardian(guardian.clone()).unwrap();

            let mut cfg = load_config().unwrap();
            cfg.emergency_quorum = 1;
            save_config(&cfg).unwrap();

            let proposal = emergency_upgrade(
                "CAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAD2KM".to_string(),
                wasm,
                "Critical patch".to_string(),
                guardian,
                "testnet".to_string(),
            )
            .unwrap();

            assert!(proposal.is_emergency);
            assert_eq!(proposal.status, ProposalStatus::EmergencyExecuted);
        });
    }
}
