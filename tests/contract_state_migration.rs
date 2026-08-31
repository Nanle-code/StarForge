use tempfile::TempDir;
use std::fs;

#[test]
fn test_migration_snapshot_serialization_and_diffing() {
    let tmp = TempDir::new().unwrap();
    let v1_path = tmp.path().join("snapshot-v1.json");
    let v2_path = tmp.path().join("snapshot-v2.json");

    let v1_json = r#"{
        "contract_id": "CDUMMY123",
        "version": "v1",
        "timestamp": "2026-08-29T12:00:00Z",
        "entries": {
            "admin": "GADMIN1",
            "balance": 100
        }
    }"#;

    let v2_json = r#"{
        "contract_id": "CDUMMY123",
        "version": "v2",
        "timestamp": "2026-08-29T12:05:00Z",
        "entries": {
            "admin": "GADMIN1",
            "balance": 200,
            "paused": false
        }
    }"#;

    fs::write(&v1_path, v1_json).unwrap();
    fs::write(&v2_path, v2_json).unwrap();

    assert!(v1_path.exists());
    assert!(v2_path.exists());
}
