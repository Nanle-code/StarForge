use starforge::utils::governance::{
    GovernanceConfig, GovernanceProposal, ProposalStatus, VoteChoice,
};

#[test]
fn test_governance_proposal_status_transitions() {
    let proposal = GovernanceProposal {
        id: "prop-1".to_string(),
        contract_id: "CDUMMYCONTRACT123".to_string(),
        new_wasm_hash: "abc123hash".to_string(),
        wasm_path: None,
        description: "Upgrade payment contract".to_string(),
        proposer: "GADMIN".to_string(),
        votes: vec![],
        approval_threshold: 2,
        timelock_seconds: 3600,
        timelock_expires_at: None,
        status: ProposalStatus::Active,
        network: "testnet".to_string(),
        created_at: "2026-08-29T12:00:00Z".to_string(),
        executed_at: None,
        is_emergency: false,
    };

    assert_eq!(proposal.status.to_string(), "active");
    assert_eq!(VoteChoice::For.to_string(), "for");
    assert_eq!(VoteChoice::Against.to_string(), "against");
}

#[test]
fn test_governance_config_defaults() {
    let config = GovernanceConfig::default();
    assert_eq!(config.default_approval_threshold, 1);
    assert_eq!(config.default_timelock_seconds, 86400);
}
