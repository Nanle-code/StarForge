//! Snapshot tests for binding generators.
//!
//! These tests verify that the output of each language binding generator
//! matches known-good golden files. If a generator refactor changes the
//! output, the test fails until the snapshot is intentionally updated.
//!
//! ## Updating snapshots
//!
//! When a change to the binding generators is intentional:
//!
//! ```bash
//! UPDATE_SNAPSHOTS=1 cargo test --test bindings_snapshots
//! ```
//!
//! This overwrites the golden files in `tests/fixtures/snapshots/`.
//! Review the diff, then commit the updated snapshots alongside your
//! generator changes.
//!
//! ## How it works
//!
//! 1. A shared [`complex_metadata`] fixture defines a contract with functions
//!    (multiple param types, Option, Result, Vec), structs, enums, and events.
//! 2. Each language generator produces output from this metadata.
//! 3. The output is **normalized** (line endings, trailing whitespace, final newline)
//!    and compared against the golden file.
//! 4. If `UPDATE_SNAPSHOTS=1` is set, the golden file is overwritten instead of
//!    compared.

use starforge::utils::bindings::{complex_metadata, generate_from_metadata, BindingLanguage};
use std::path::{Path, PathBuf};

/// Directory containing the golden snapshot files.
fn snapshots_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("snapshots")
}

/// Normalize a binding output string for comparison.
///
/// - Converts `\r\n` to `\n`
/// - Strips trailing whitespace from each line
/// - Ensures the string ends with exactly one newline
fn normalize(output: &str) -> String {
    let normalized: String = output
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    // Ensure trailing newline
    if normalized.ends_with('\n') {
        normalized
    } else {
        format!("{}\n", normalized)
    }
}

/// Compare actual output against a golden file.
///
/// If `UPDATE_SNAPSHOTS=1` is set, writes the output to the file instead.
/// Returns Ok(()) on match or update, Err with a descriptive message on mismatch.
fn assert_snapshot(actual: &str, golden_path: &Path, label: &str) {
    let normalized = normalize(actual);

    if std::env::var("UPDATE_SNAPSHOTS").is_ok() {
        // Write mode: overwrite the golden file.
        if let Some(parent) = golden_path.parent() {
            std::fs::create_dir_all(parent).expect("failed to create snapshot directory");
        }
        std::fs::write(golden_path, &normalized).unwrap_or_else(|e| {
            panic!("failed to write snapshot {}: {}", golden_path.display(), e)
        });
        eprintln!("  📝 Updated snapshot: {}", golden_path.display());
        return;
    }

    // Compare mode.
    let golden = std::fs::read_to_string(golden_path).unwrap_or_else(|e| {
        panic!(
            "Snapshot file {} not found. Run with UPDATE_SNAPSHOTS=1 to create it.\n  Error: {}",
            golden_path.display(),
            e
        )
    });

    if golden != normalized {
        // Show a helpful diff-like message.
        let golden_lines: Vec<&str> = golden.lines().collect();
        let actual_lines: Vec<&str> = normalized.lines().collect();
        let mut diffs = Vec::new();

        let max = golden_lines.len().max(actual_lines.len());
        for i in 0..max {
            let g = golden_lines.get(i).copied();
            let a = actual_lines.get(i).copied();
            if g != a {
                diffs.push(format!("  line {}: expected {:?}, got {:?}", i + 1, g, a));
            }
        }

        panic!(
            "Snapshot mismatch for {} ({})!\n  Golden: {}\n  {} lines differ:\n{}",
            label,
            golden_path.display(),
            golden_path.display(),
            diffs.len(),
            diffs.join("\n")
        );
    }
}

// ── Snapshot tests ───────────────────────────────────────────────────────────

#[test]
fn snapshot_rust_bindings() {
    let metadata = complex_metadata();
    let output = generate_from_metadata(&metadata, BindingLanguage::Rust).unwrap();
    let golden = snapshots_dir().join("bindings_rust.rs");
    assert_snapshot(&output, &golden, "Rust bindings");
}

#[test]
fn snapshot_typescript_bindings() {
    let metadata = complex_metadata();
    let output = generate_from_metadata(&metadata, BindingLanguage::TypeScript).unwrap();
    let golden = snapshots_dir().join("bindings_typescript.ts");
    assert_snapshot(&output, &golden, "TypeScript bindings");
}

#[test]
fn snapshot_python_bindings() {
    let metadata = complex_metadata();
    let output = generate_from_metadata(&metadata, BindingLanguage::Python).unwrap();
    let golden = snapshots_dir().join("bindings_python.py");
    assert_snapshot(&output, &golden, "Python bindings");
}

#[test]
fn snapshot_go_bindings() {
    let metadata = complex_metadata();
    let output = generate_from_metadata(&metadata, BindingLanguage::Go).unwrap();
    let golden = snapshots_dir().join("bindings_go.go");
    assert_snapshot(&output, &golden, "Go bindings");
}

// ── Tests for snapshot drift detection ───────────────────────────────────────

#[test]
fn snapshot_files_exist() {
    let dir = snapshots_dir();
    for (lang, filename) in [
        ("Rust", "bindings_rust.rs"),
        ("TypeScript", "bindings_typescript.ts"),
        ("Python", "bindings_python.py"),
        ("Go", "bindings_go.go"),
    ] {
        let path = dir.join(filename);
        assert!(
            path.exists(),
            "Snapshot file for {} not found at {}. Run with UPDATE_SNAPSHOTS=1 to create.",
            lang,
            path.display()
        );
    }
}

#[test]
fn snapshot_files_are_non_empty() {
    let dir = snapshots_dir();
    for filename in &[
        "bindings_rust.rs",
        "bindings_typescript.ts",
        "bindings_python.py",
        "bindings_go.go",
    ] {
        let path = dir.join(filename);
        if path.exists() {
            let contents = std::fs::read_to_string(&path).unwrap();
            assert!(
                !contents.trim().is_empty(),
                "Snapshot file {} is empty",
                path.display()
            );
        }
    }
}

#[test]
fn snapshot_files_are_normalized() {
    let dir = snapshots_dir();
    for filename in &[
        "bindings_rust.rs",
        "bindings_typescript.ts",
        "bindings_python.py",
        "bindings_go.go",
    ] {
        let path = dir.join(filename);
        if path.exists() {
            let contents = std::fs::read_to_string(&path).unwrap();
            // Should not contain \r\n
            assert!(
                !contents.contains('\r'),
                "Snapshot {} contains \\r characters",
                path.display()
            );
            // Should end with exactly one newline
            assert!(
                contents.ends_with('\n') && !contents.ends_with("\n\n"),
                "Snapshot {} does not end with exactly one newline",
                path.display()
            );
        }
    }
}
