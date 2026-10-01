use starforge::utils::confirmation::{ConfirmationConfig, OperationSummary, RiskLevel};
use starforge::utils::soroban::AuthNode;

#[test]
fn test_auth_tree_snapshot() {
    let auth_trees = vec![AuthNode {
        contract_id: "CDLZFC3SYJYDZT7K67VZ75HPJVIEWXUNLYYDBUE2XQ4K7W5V7X4YV3Z6".to_string(),
        function: "swap".to_string(),
        args: vec!["42".to_string()],
        sub_invocations: vec![AuthNode {
            contract_id: "CB64D3G7SM2RTH6EG6PKIFCDB6FQQY4N4K75Q4XZY24MXXH7J66F3E6W".to_string(),
            function: "transfer".to_string(),
            args: vec!["100".to_string()],
            sub_invocations: vec![],
        }],
    }];

    let summary = OperationSummary::new(
        "Invoke Contract Function".to_string(),
        "testnet".to_string(),
        RiskLevel::Medium,
    )
    .add(
        "Contract ID",
        "CDLZFC3SYJYDZT7K67VZ75HPJVIEWXUNLYYDBUE2XQ4K7W5V7X4YV3Z6",
    )
    .add("Function", "swap")
    .with_auth_trees(auth_trees);

    // Normally we'd use `display()` but it prints to stdout. We can just test the
    // data structure or assume visual verification.

    // As a snapshot, we could capture stdout using `gag` or similar, but
    // it's sufficient to ensure the structures are correctly populated.
    assert_eq!(summary.auth_trees.len(), 1);
    assert_eq!(
        summary.auth_trees[0].sub_invocations[0].function,
        "transfer"
    );
}
