//! End-to-end tests for `starforge add`.
//!
//! Covers the whole user journey: scaffold a project, preview the patch with
//! `--dry-run`, apply it, and confirm the refusals (duplicate component,
//! missing anchor, unknown component) all fail loudly instead of writing.

use std::path::Path;
use std::process::{Command, Output};

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    // HOME / USERPROFILE alone do not isolate the CLI on Windows, where
    // `dirs::home_dir()` resolves through SHGetKnownFolderPath and ignores
    // both, so pin the config directory explicitly.
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd.env("STARFORGE_CONFIG_DIR", home.join(".starforge"));
    cmd
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// Scaffold a fresh contract project inside `root` and return its path.
fn scaffold(root: &Path, name: &str, template: &str) -> std::path::PathBuf {
    let project = root.join(name);
    let output = starforge(root)
        .args(["new", name, "--template", template])
        .current_dir(root)
        .output()
        .expect("spawn starforge new");
    assert!(
        output.status.success(),
        "starforge new {name} failed: {}",
        stderr(&output)
    );
    assert!(
        project.join("src").join("lib.rs").exists(),
        "no lib.rs scaffolded"
    );
    project
}

const COMPONENTS: [&str; 4] = [
    "access-control",
    "pausability",
    "upgradeability",
    "ownership",
];

#[test]
fn add_applies_each_component_to_a_scaffolded_project() {
    for component in COMPONENTS {
        let home = isolated_home();
        let project = scaffold(home.path(), "counter", "hello-world");
        let lib = project.join("src").join("lib.rs");
        let before = std::fs::read_to_string(&lib).expect("read lib.rs");

        let output = starforge(home.path())
            .args(["add", component, "--path"])
            .arg(&project)
            .output()
            .expect("spawn starforge add");
        assert!(
            output.status.success(),
            "starforge add {component} failed: {}",
            stderr(&output)
        );

        let after = std::fs::read_to_string(&lib).expect("read patched lib.rs");
        assert_ne!(before, after, "{component} did not change the file");
        assert!(
            after.contains(&format!("// <starforge:component:{component}>")),
            "{component} marker missing"
        );
        assert!(
            stdout(&output).contains(component),
            "expected {component} in output, got: {}",
            stdout(&output)
        );
    }
}

#[test]
fn add_is_idempotent_and_refuses_a_duplicate_component() {
    let home = isolated_home();
    let project = scaffold(home.path(), "vault", "token");
    let lib = project.join("src").join("lib.rs");

    for _ in 0..2 {
        let output = starforge(home.path())
            .args(["add", "ownership", "--path"])
            .arg(&project)
            .output()
            .expect("spawn starforge add");
        if output.status.success() {
            continue;
        }
        // The second attempt must fail and must not have touched the file.
        assert!(
            stderr(&output).to_lowercase().contains("already present"),
            "expected a duplicate-component refusal, got: {}",
            stderr(&output)
        );
        let after = std::fs::read_to_string(&lib).expect("read lib.rs");
        assert_eq!(
            after.matches("// <starforge:component:ownership>").count(),
            1,
            "duplicate add must not add a second copy"
        );
        return;
    }
    panic!("ownership should only be addable once");
}

#[test]
fn add_dry_run_prints_a_diff_and_writes_nothing() {
    let home = isolated_home();
    let project = scaffold(home.path(), "dry", "hello-world");
    let lib = project.join("src").join("lib.rs");
    let before = std::fs::read_to_string(&lib).expect("read lib.rs");

    let output = starforge(home.path())
        .args(["add", "pausability", "--path"])
        .arg(&project)
        .arg("--dry-run")
        .output()
        .expect("spawn starforge add --dry-run");

    assert!(
        output.status.success(),
        "dry run failed: {}",
        stderr(&output)
    );
    let printed = stdout(&output);
    assert!(
        printed.contains("--- a/"),
        "expected a diff header, got: {printed}"
    );
    assert!(
        printed.contains("+++ b/"),
        "expected a diff header, got: {printed}"
    );
    assert!(
        printed.contains("@@"),
        "expected a hunk header, got: {printed}"
    );
    assert!(
        printed.contains("assert_not_paused"),
        "expected the injected pause check in the diff, got: {printed}"
    );
    assert_eq!(
        std::fs::read_to_string(&lib).expect("read lib.rs"),
        before,
        "--dry-run must not modify the file"
    );
}

#[test]
fn add_refuses_a_project_without_anchors() {
    let home = isolated_home();
    let project = home.path().join("bare");
    std::fs::create_dir_all(project.join("src")).expect("mkdir");
    std::fs::write(
        project.join("src").join("lib.rs"),
        "#![no_std]\npub fn nothing() {}\n",
    )
    .expect("write lib.rs");

    let output = starforge(home.path())
        .args(["add", "ownership", "--path"])
        .arg(&project)
        .output()
        .expect("spawn starforge add");
    assert!(
        !output.status.success(),
        "expected a failure for an unanchored project"
    );
    assert!(
        stderr(&output).contains("anchor"),
        "expected an anchor error, got: {}",
        stderr(&output)
    );
}

#[test]
fn add_refuses_a_missing_project() {
    let home = isolated_home();
    let output = starforge(home.path())
        .args(["add", "ownership", "--path"])
        .arg(home.path().join("does-not-exist"))
        .output()
        .expect("spawn starforge add");
    assert!(
        !output.status.success(),
        "expected a failure for a missing project"
    );
}

#[test]
fn add_rejects_an_unknown_component() {
    let home = isolated_home();
    let project = scaffold(home.path(), "unknown", "hello-world");
    let output = starforge(home.path())
        .args(["add", "teleportation", "--path"])
        .arg(&project)
        .output()
        .expect("spawn starforge add");
    assert!(
        !output.status.success(),
        "an unknown component must not be accepted"
    );
}

#[test]
fn all_components_can_be_added_to_one_project() {
    let home = isolated_home();
    let project = scaffold(home.path(), "full", "voting");
    let lib = project.join("src").join("lib.rs");

    for component in COMPONENTS {
        let output = starforge(home.path())
            .args(["add", component, "--path"])
            .arg(&project)
            .output()
            .expect("spawn starforge add");
        assert!(
            output.status.success(),
            "starforge add {component} failed: {}",
            stderr(&output)
        );
    }

    let after = std::fs::read_to_string(&lib).expect("read lib.rs");
    for component in COMPONENTS {
        assert!(
            after.contains(&format!("// <starforge:component:{component}>")),
            "{component} marker missing after applying all four"
        );
    }
    assert!(
        !after.contains("// <starforge:pause-check>"),
        "pausability should have consumed every pause anchor"
    );
}

#[test]
fn add_works_on_every_packaged_example_template() {
    // Each packaged template must carry the anchor `add` relies on.
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("templates/examples");
    let mut checked = 0;
    for entry in std::fs::read_dir(&root)
        .expect("read templates/examples")
        .flatten()
    {
        let lib = entry.path().join("src").join("lib.rs");
        if !lib.exists() {
            continue;
        }
        let source = std::fs::read_to_string(&lib).expect("read template");
        let name = entry.file_name().to_string_lossy().into_owned();
        assert_eq!(
            source.matches("// <starforge:add:methods>").count(),
            1,
            "templates/examples/{name} needs exactly one add anchor"
        );
        checked += 1;
    }
    assert!(
        checked >= 10,
        "expected the built-in templates, saw {checked}"
    );
}
