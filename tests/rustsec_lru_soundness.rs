//! Regression guard for RUSTSEC-2026-0002 (`lru` `IterMut` soundness).
//!
//! Advisory: <https://rustsec.org/advisories/RUSTSEC-2026-0002.html>
//!
//! `lru::IterMut::next` / `next_back` create an exclusive reference to the key,
//! which invalidates the shared pointer held by the map's internal `HashMap`
//! (a Stacked Borrows violation). The affected range is `>= 0.9.0, < 0.16.3`;
//! the advisory is patched in `lru >= 0.16.3`, and `< 0.9.0` is unaffected.
//!
//! StarForge does not depend on `lru` directly — it arrives transitively via
//! `ratatui-core` (the `ui` feature), which is why the crate does not show up
//! in `Cargo.toml`. Nothing else in the tree currently checks the *resolved*
//! version, so this test pins it: a `cargo update` (or a dependency bump) that
//! pulls an affected `lru` back into a lockfile must fail here before it can
//! land.
//!
//! It also asserts that `RUSTSEC-2026-0002` is *not* ignored in any of the
//! three places StarForge records advisory policy: `audit.toml`, `deny.toml`,
//! and the inline `ignore:` input of `.github/workflows/audit.yml` (the
//! `rustsec/audit-check` action does not read `audit.toml`, so those two must
//! be kept in sync by hand). Because the resolved `lru` is already in the
//! patched range there is no legitimate reason to ignore this advisory, and
//! adding it to any list would silently mask a real regression.
//!
//! Run with: `cargo test --test rustsec_lru_soundness`

use std::fs;
use std::path::{Path, PathBuf};

use semver::{Version, VersionReq};

// ---------------------------------------------------------------------------
// Constants and version-range helpers
// ---------------------------------------------------------------------------

/// The advisory identifier and its canonical URL, quoted in every failure
/// message so a red test is directly actionable.
const ADVISORY_ID: &str = "RUSTSEC-2026-0002";
const ADVISORY_URL: &str = "https://rustsec.org/advisories/RUSTSEC-2026-0002.html";

/// The range reported as vulnerable by RUSTSEC-2026-0002.
const AFFECTED_RANGE: &str = ">=0.9.0, <0.16.3";

/// The range that contains the fix for RUSTSEC-2026-0002.
const PATCHED_RANGE: &str = ">=0.16.3";

/// Parse the advisory's affected range with the `semver` crate (already a
/// dependency of this crate).
fn affected_range() -> VersionReq {
    VersionReq::parse(AFFECTED_RANGE).expect("AFFECTED_RANGE must be a valid semver requirement")
}

/// Parse the advisory's patched range with the `semver` crate.
fn patched_range() -> VersionReq {
    VersionReq::parse(PATCHED_RANGE).expect("PATCHED_RANGE must be a valid semver requirement")
}

/// `true` when `version` falls inside the range RUSTSEC-2026-0002 reports as
/// vulnerable (`>= 0.9.0, < 0.16.3`).
fn lru_is_affected(version: &Version) -> bool {
    affected_range().matches(version)
}

/// `true` when `version` is at or above the version that fixes
/// RUSTSEC-2026-0002 (`>= 0.16.3`).
fn lru_is_patched(version: &Version) -> bool {
    patched_range().matches(version)
}

// ---------------------------------------------------------------------------
// Filesystem helpers
// ---------------------------------------------------------------------------

/// Read a file relative to the crate root, panicking with a clear message when
/// it is missing.
fn read_project_file(relative: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {relative} at {}: {e}", path.display()))
}

/// Collect every `Cargo.lock` in the repository, skipping build/vendor output
/// so the walk stays fast and deterministic.
fn collect_lockfiles(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        // Unreadable directories are not lockfile locations we care about.
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(name.as_ref(), "target" | ".git" | "node_modules" | "vendor") {
                continue;
            }
            collect_lockfiles(&path, out);
        } else if name == "Cargo.lock" {
            out.push(path);
        }
    }
}

// ---------------------------------------------------------------------------
// Lockfile parsing
// ---------------------------------------------------------------------------

/// Return the resolved `lru` versions declared in a lockfile, in file order.
///
/// `Cargo.lock` is plain TOML whose packages live under the array of tables
/// `[[package]]`; parsing it with the `toml` crate (already a dependency) keeps
/// this robust against formatting and ordering differences.
fn lru_versions_in_lockfile(lockfile_text: &str, label: &str) -> Vec<Version> {
    let parsed: toml::Value = toml::from_str(lockfile_text)
        .unwrap_or_else(|e| panic!("{label} is not valid TOML – cannot check {ADVISORY_ID}: {e}"));
    let packages = parsed
        .as_table()
        .and_then(|root| root.get("package"))
        .and_then(|package| package.as_array())
        .unwrap_or_else(|| panic!("{label} has no [[package]] array – is it really a Cargo.lock?"));

    let mut versions = Vec::new();
    for package in packages {
        let table = match package.as_table() {
            Some(table) => table,
            None => continue,
        };
        if table.get("name").and_then(|v| v.as_str()) != Some("lru") {
            continue;
        }
        let raw = table
            .get("version")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("{label}: the `lru` package has no version string"));
        let version = Version::parse(raw)
            .unwrap_or_else(|e| panic!("{label}: `lru` version {raw:?} is not valid semver: {e}"));
        versions.push(version);
    }
    versions
}

/// Human-readable path for messages, relative to the crate root when possible.
fn display_path(path: &Path) -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}

// ---------------------------------------------------------------------------
// Advisory policy lists
// ---------------------------------------------------------------------------

/// Extract the `[advisories].ignore` array from an `audit.toml` / `deny.toml`
/// document.
fn ignored_ids_from_toml(text: &str, label: &str) -> Vec<String> {
    let parsed: toml::Value =
        toml::from_str(text).unwrap_or_else(|e| panic!("{label} is not valid TOML: {e}"));
    parsed
        .as_table()
        .and_then(|root| root.get("advisories"))
        .and_then(|advisories| advisories.as_table())
        .and_then(|advisories| advisories.get("ignore"))
        .and_then(|ignore| ignore.as_array())
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| entry.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// Extract the comma-separated ids from the inline `ignore:` input of
/// `.github/workflows/audit.yml`.
fn ignored_ids_from_audit_workflow(text: &str) -> Vec<String> {
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("ignore:") {
            return rest
                .split(',')
                .map(|id| id.trim().to_string())
                .filter(|id| !id.is_empty())
                .collect();
        }
    }
    panic!(
        ".github/workflows/audit.yml has no `ignore:` input – the audit job's ignore list \
         must be reviewable for {ADVISORY_ID}"
    );
}

// ---------------------------------------------------------------------------
// Primary guard – the resolved `lru` in the main lockfile is patched
// ---------------------------------------------------------------------------

#[test]
fn cargo_lock_lru_is_in_the_patched_range() {
    let text = read_project_file("Cargo.lock");
    let versions = lru_versions_in_lockfile(&text, "Cargo.lock");

    // Absence handling: `lru` is not a direct dependency; it is resolved
    // transitively through `ratatui-core` (reached via the optional `ui`
    // feature), and a lockfile resolves the full graph regardless of which
    // features are enabled. So `lru` being absent is not itself a
    // vulnerability, but it *does* mean this guard has silently stopped
    // proving anything: the expected provider changed and a maintainer has to
    // decide whether the guard still belongs. We therefore fail loudly instead
    // of passing without evidence (a silent pass), with a message that says how
    // to resolve it.
    assert!(
        !versions.is_empty(),
        "Cargo.lock does not resolve `lru` at all, so this guard for {ADVISORY_ID} \
         ({ADVISORY_URL}) is no longer checking anything. `lru` is expected to arrive \
         transitively via `ratatui-core` (the `ui` feature's `ratatui` dependency). \
         If that dependency was intentionally removed, delete this test; otherwise \
         investigate why the lockfile changed."
    );

    for version in &versions {
        assert!(
            lru_is_patched(version),
            "{ADVISORY_ID} ({ADVISORY_URL}): Cargo.lock resolves `lru {version}`, which is \
             outside the patched range `{PATCHED_RANGE}`. `lru::IterMut::next` / `next_back` \
             create an exclusive reference to the key and invalidate the internal HashMap's \
             shared pointer (Stacked Borrows violation); the affected range is \
             `{AFFECTED_RANGE}` and anything below 0.9.0 predates the advisory. \
             Remediation: upgrade to `lru >= 0.16.3` (e.g. `cargo update -p lru`) or move the \
             crate that pulls it in (`ratatui-core`) forward."
        );
    }
}

// ---------------------------------------------------------------------------
// Every other lockfile in the repo is free of the affected range
// ---------------------------------------------------------------------------

#[test]
fn other_lockfiles_do_not_resolve_an_affected_lru() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut lockfiles = Vec::new();
    collect_lockfiles(root, &mut lockfiles);
    assert!(
        lockfiles.iter().any(|p| p == &root.join("Cargo.lock")),
        "expected to find the root Cargo.lock while walking {}",
        root.display()
    );

    for path in lockfiles {
        if path == root.join("Cargo.lock") {
            // Covered by `cargo_lock_lru_is_in_the_patched_range`, which also
            // verifies the crate is present at all.
            continue;
        }
        let label = display_path(&path);
        let text =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("Failed to read {label}: {e}"));
        let versions = lru_versions_in_lockfile(&text, &label);
        // Unlike the root lockfile, absence here is a pass: a lockfile that
        // never resolves `lru` cannot link an affected version. Only a
        // resolved version inside the advisory's range is a failure.
        for version in &versions {
            assert!(
                !lru_is_affected(version),
                "{ADVISORY_ID} ({ADVISORY_URL}): {label} resolves `lru {version}`, which is \
                 inside the affected range `{AFFECTED_RANGE}`. Update that lockfile so `lru` \
                 is `{PATCHED_RANGE}` (its own crate owns that lockfile – e.g. \
                 `cargo update --manifest-path fuzz/Cargo.toml -p lru`)."
            );
        }
    }
}

// ---------------------------------------------------------------------------
// `RUSTSEC-2026-0002` must not be ignored in any of the three policy lists
// ---------------------------------------------------------------------------

#[test]
fn advisory_is_not_ignored_in_audit_or_deny_or_ci() {
    let audit_toml = ignored_ids_from_toml(&read_project_file("audit.toml"), "audit.toml");
    let deny_toml = ignored_ids_from_toml(&read_project_file("deny.toml"), "deny.toml");
    let audit_ci =
        ignored_ids_from_audit_workflow(&read_project_file(".github/workflows/audit.yml"));

    // Sanity check: an empty list would make the membership assertions below
    // vacuously true, so make sure the parsing actually found the ignore lists.
    assert!(
        !audit_toml.is_empty(),
        "audit.toml parsed with an empty [advisories].ignore list – parsing likely broke"
    );
    assert!(
        !deny_toml.is_empty(),
        "deny.toml parsed with an empty [advisories].ignore list – parsing likely broke"
    );
    assert!(
        !audit_ci.is_empty(),
        ".github/workflows/audit.yml parsed with an empty `ignore:` input – parsing likely broke"
    );

    let in_audit_toml = audit_toml.iter().any(|id| id == ADVISORY_ID);
    let in_deny_toml = deny_toml.iter().any(|id| id == ADVISORY_ID);
    let in_audit_ci = audit_ci.iter().any(|id| id == ADVISORY_ID);

    // The three lists must agree on whether this advisory is ignored. The
    // `rustsec/audit-check` action only reads the inline `ignore:` input, while
    // cargo-audit (local) reads audit.toml and cargo-deny reads deny.toml, so
    // drift between them silently weakens whichever gate was missed.
    assert_eq!(
        in_audit_toml, in_deny_toml,
        "audit.toml and deny.toml disagree about ignoring {ADVISORY_ID}"
    );
    assert_eq!(
        in_deny_toml, in_audit_ci,
        "deny.toml and .github/workflows/audit.yml disagree about ignoring {ADVISORY_ID}"
    );

    // No excuse exists for ignoring this advisory: its only provider resolves
    // to a patched version (asserted above), so an ignore entry would mask a
    // real regression rather than document a knowingly accepted risk.
    assert!(
        !in_audit_toml && !in_deny_toml && !in_audit_ci,
        "{ADVISORY_ID} must not be ignored: the resolved `lru` is already in the patched \
         range `{PATCHED_RANGE}`, so no exception is warranted. If a bump ever reintroduces \
         the affected range, fix the dependency instead of ignoring the advisory."
    );
}

// ---------------------------------------------------------------------------
// The two lists that are required to mirror each other stay in sync
// ---------------------------------------------------------------------------

#[test]
fn audit_toml_and_ci_ignore_lists_mirror_each_other() {
    // `.github/workflows/audit.yml` carries an explicit "Keep in sync with the
    // documented, justified exceptions in audit.toml" note, because
    // `rustsec/audit-check` never reads audit.toml. `deny.toml` is a different
    // tool over a different graph and is allowed to carry additional ids (it
    // currently adds five wasmtime advisories), so only these two lists – the
    // ones the workflow itself declares must match – are required to be equal.
    let mut audit_toml = ignored_ids_from_toml(&read_project_file("audit.toml"), "audit.toml");
    let mut audit_ci =
        ignored_ids_from_audit_workflow(&read_project_file(".github/workflows/audit.yml"));
    audit_toml.sort();
    audit_ci.sort();
    assert_eq!(
        audit_toml, audit_ci,
        "audit.toml and .github/workflows/audit.yml must list the same ignored advisories: \
         the audit-check action reads only the workflow's inline `ignore:` input, so an \
         entry present in one and missing from the other is silently unenforced"
    );
}

// ---------------------------------------------------------------------------
// Boundary case – the affected-range helper rejects/accepts the right versions
// ---------------------------------------------------------------------------

#[test]
fn affected_range_boundaries_are_rejected_correctly() {
    // The advisory lists 0.12.5 as an affected version.
    let affected = Version::parse("0.12.5").expect("0.12.5 is valid semver");
    assert!(
        lru_is_affected(&affected),
        "0.12.5 is the version named in {ADVISORY_ID} and must report as affected"
    );

    // Lower and upper boundaries of `>= 0.9.0, < 0.16.3`.
    assert!(
        lru_is_affected(&Version::new(0, 9, 0)),
        "0.9.0 is the inclusive lower bound of the affected range"
    );
    assert!(
        lru_is_affected(&Version::new(0, 16, 2)),
        "0.16.2 is the last affected release before the fix"
    );

    // Versions just outside the range must not be reported as affected.
    assert!(
        !lru_is_affected(&Version::new(0, 8, 9)),
        "0.8.9 predates the advisory's affected range"
    );
    assert!(
        !lru_is_affected(&Version::new(0, 16, 3)),
        "0.16.3 is the first patched release"
    );
    assert!(
        !lru_is_affected(&Version::new(0, 18, 5)),
        "0.18.5 is the version currently resolved by Cargo.lock"
    );

    // And the patched-range helper agrees with the advisory.
    assert!(lru_is_patched(&Version::new(0, 16, 3)));
    assert!(lru_is_patched(&Version::new(0, 18, 5)));
    assert!(!lru_is_patched(&Version::new(0, 16, 2)));
}
