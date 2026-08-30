use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

fn read_json(path: &str) -> Value {
    let full_path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    let raw = fs::read_to_string(&full_path)
        .unwrap_or_else(|err| panic!("failed to read {}: {}", full_path.display(), err));
    serde_json::from_str(&raw)
        .unwrap_or_else(|err| panic!("failed to parse {}: {}", full_path.display(), err))
}

fn contract_fields(contract: &Value) -> BTreeMap<String, BTreeMap<String, String>> {
    let commands = contract["commands"]
        .as_object()
        .expect("contract must contain a commands object");

    commands
        .iter()
        .map(|(command, spec)| {
            let fields = spec["fields"]
                .as_array()
                .unwrap_or_else(|| panic!("{} must declare fields", command));

            let fields = fields
                .iter()
                .map(|field| {
                    let path = field["path"]
                        .as_str()
                        .unwrap_or_else(|| panic!("{} contains a field without path", command));
                    let stability = field["stability"].as_str().unwrap_or_else(|| {
                        panic!("{} field {} must declare stability", command, path)
                    });
                    (path.to_string(), stability.to_string())
                })
                .collect();

            (command.to_string(), fields)
        })
        .collect()
}

#[test]
fn every_json_contract_field_declares_a_valid_stability_tier() {
    let contract = read_json("docs/contracts/cli-json-fields.json");
    let fields_by_command = contract_fields(&contract);
    let valid_tiers = BTreeSet::from([
        "experimental".to_string(),
        "stable".to_string(),
        "deprecated".to_string(),
    ]);

    assert!(
        !fields_by_command.is_empty(),
        "at least one CLI JSON command contract must be documented"
    );

    for (command, fields) in fields_by_command {
        assert!(
            !fields.is_empty(),
            "{} must document at least one JSON field",
            command
        );

        for (path, stability) in fields {
            assert!(
                valid_tiers.contains(&stability),
                "{} field {} has invalid stability tier {}",
                command,
                path,
                stability
            );
        }
    }
}
#[test]
fn stable_baseline_fields_cannot_be_removed_without_deprecation() {
    let contract = read_json("docs/contracts/cli-json-fields.json");
    let baseline = read_json("tests/fixtures/json_contracts/stable-fields-baseline.json");
    let fields_by_command = contract_fields(&contract);
    let baseline_commands = baseline["commands"]
        .as_object()
        .expect("baseline must contain a commands object");

    for (command, fields) in baseline_commands {
        let current_fields = fields_by_command
            .get(command)
            .unwrap_or_else(|| panic!("{} was removed from the JSON contract", command));
        let fields = fields
            .as_array()
            .unwrap_or_else(|| panic!("{} baseline must be an array", command));

        for field in fields {
            let path = field
                .as_str()
                .unwrap_or_else(|| panic!("{} baseline entries must be strings", command));
            let stability = current_fields.get(path).unwrap_or_else(|| {
                panic!(
                    "{} stable field {} was removed; mark it deprecated first",
                    command, path
                )
            });

            assert!(
                stability == "stable" || stability == "deprecated",
                "{} field {} regressed from stable to {}",
                command,
                path,
                stability
            );
        }
    }
}
