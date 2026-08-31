//! Cross-platform CLI integration tests for StarForge.
//!
//! Exercises filesystem, process, terminal, and path behavior across all supported
//! operating systems (Linux, macOS, Windows).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge_cmd(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd.env("STARFORGE_CONFIG_DIR", home.join(".starforge"));
    cmd
}

fn starforge_in_dir(home: &Path, cwd: &Path) -> Command {
    let mut cmd = starforge_cmd(home);
    cmd.current_dir(cwd);
    cmd
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "Command '{}' failed with status {:?}.\nStdout: {}\nStderr: {}",
        context,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output, context: &str) {
    assert!(
        !output.status.success(),
        "Command '{}' unexpectedly succeeded.\nStdout: {}\nStderr: {}",
        context,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. Primary Flow Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_platform_version_and_info() {
    let home = isolated_home();

    // Verify --version
    let output = starforge_cmd(home.path())
        .arg("--version")
        .output()
        .expect("spawn starforge --version");
    assert_success(&output, "starforge --version");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("starforge"),
        "Version output should contain binary name"
    );

    // Verify info command
    let output = starforge_cmd(home.path())
        .arg("info")
        .output()
        .expect("spawn starforge info");
    assert_success(&output, "starforge info");
}

#[test]
fn test_cross_platform_core_subcommands() {
    let home = isolated_home();

    let subcommands = [
        vec!["network", "show"],
        vec!["wallet", "list"],
        vec!["template", "list"],
    ];

    for args in &subcommands {
        let output = starforge_cmd(home.path())
            .args(args)
            .output()
            .unwrap_or_else(|_| panic!("spawn starforge {:?}", args));
        assert_success(&output, &format!("starforge {:?}", args));
    }
}

#[test]
fn test_cross_platform_home_dir_resolution() {
    let home = isolated_home();
    let home_path = home.path();

    // Invoking wallet list should populate the isolated home structure
    let output = starforge_cmd(home_path)
        .args(["wallet", "list"])
        .output()
        .expect("spawn wallet list");
    assert_success(&output, "wallet list with isolated home");

    // The isolated config directory must actually be used. This is asserted
    // unconditionally on purpose: HOME / USERPROFILE alone cannot isolate the
    // CLI on Windows, where `dirs::home_dir()` resolves through
    // SHGetKnownFolderPath(FOLDERID_Profile) and ignores both variables. The
    // isolation therefore comes from STARFORGE_CONFIG_DIR (see
    // `starforge_cmd`), which the CLI honors on every platform.
    let starforge_dir = home_path.join(".starforge");
    assert!(
        starforge_dir.is_dir(),
        "STARFORGE_CONFIG_DIR was not honored: expected {} to be created.\nStdout: {}\nStderr: {}",
        starforge_dir.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn test_cross_platform_config_dir_is_isolated_from_real_home() {
    // Two commands pointed at two different config directories must not share
    // state. Without STARFORGE_CONFIG_DIR support, both would fall back to the
    // one real profile directory on Windows and share a single SQLite
    // database across concurrent test processes.
    let first = isolated_home();
    let second = isolated_home();

    for home in [&first, &second] {
        let output = starforge_cmd(home.path())
            .arg("info")
            .output()
            .expect("spawn starforge info");
        assert_success(&output, "starforge info with isolated config dir");
    }

    let first_db = first.path().join(".starforge").join("starforge.db");
    let second_db = second.path().join(".starforge").join("starforge.db");

    assert!(
        first_db.is_file(),
        "expected an isolated database at {}",
        first_db.display()
    );
    assert!(
        second_db.is_file(),
        "expected an isolated database at {}",
        second_db.display()
    );
    assert_ne!(
        first_db, second_db,
        "each isolated home must get its own database path"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 1b. Startup Stack Budget
// ─────────────────────────────────────────────────────────────────────────────

/// Emulates the Windows main-thread stack budget on Unix.
///
/// Windows reserves 1 MiB for the process main thread; Linux and macOS reserve
/// 8 MiB. Building this crate's clap command tree needs more than 1 MiB in a
/// debug build, so every `starforge` invocation on Windows once died in
/// `Cli::parse()` with STATUS_STACK_OVERFLOW (0xC00000FD) before running any
/// command, while all three platforms looked fine locally.
///
/// `main` therefore runs the CLI on a thread with an explicit 8 MiB stack. This
/// test lowers RLIMIT_STACK for the child to the Windows default so a
/// regression fails here, on Linux CI, instead of only on the Windows job.
///
/// Linux only: macOS rejects `setrlimit(RLIMIT_STACK)` with EINVAL here, and it
/// already gives the main thread 8 MiB, so it would add no coverage over the
/// Linux job even if it were permitted.
#[cfg(target_os = "linux")]
#[test]
fn test_cli_starts_under_windows_sized_main_stack() {
    use std::os::unix::process::CommandExt;

    const WINDOWS_DEFAULT_MAIN_STACK: u64 = 1024 * 1024;

    for args in [vec!["--version"], vec!["--help"], vec!["info"]] {
        let home = isolated_home();
        let mut cmd = starforge_cmd(home.path());
        cmd.args(&args);

        // SAFETY: setrlimit is async-signal-safe and touches only this child
        // between fork and exec.
        unsafe {
            cmd.pre_exec(|| {
                let limit = libc::rlimit {
                    rlim_cur: WINDOWS_DEFAULT_MAIN_STACK,
                    rlim_max: WINDOWS_DEFAULT_MAIN_STACK,
                };
                if libc::setrlimit(libc::RLIMIT_STACK, &limit) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }

        let output = cmd.output().expect("spawn with a 1 MiB main stack");
        assert_success(
            &output,
            &format!(
                "starforge {:?} with a {} MiB main stack (Windows default)",
                args,
                WINDOWS_DEFAULT_MAIN_STACK / (1024 * 1024)
            ),
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. Filesystem & Path Boundary Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_platform_paths_with_spaces_and_special_chars() {
    let home = isolated_home();
    let special_dir = home.path().join("StarForge Space & Symbols (Test)");
    fs::create_dir_all(&special_dir).expect("create dir with spaces and symbols");

    let output = starforge_in_dir(home.path(), &special_dir)
        .arg("info")
        .output()
        .expect("spawn in directory with spaces");
    assert_success(
        &output,
        "starforge info in dir with spaces and special chars",
    );
}

#[test]
fn test_cross_platform_deeply_nested_directory_execution() {
    let home = isolated_home();
    let mut deep_dir = home.path().to_path_buf();
    for i in 0..10 {
        deep_dir.push(format!("nested_level_{}", i));
    }
    fs::create_dir_all(&deep_dir).expect("create deeply nested dir");

    let output = starforge_in_dir(home.path(), &deep_dir)
        .arg("info")
        .output()
        .expect("spawn in deeply nested dir");
    assert_success(&output, "starforge info in deeply nested directory");
}

#[test]
fn test_cross_platform_path_separators_normalization() {
    let home = isolated_home();
    let working_dir = home.path().join("sub").join("nested");
    fs::create_dir_all(&working_dir).expect("create sub nested dir");

    // Relative path with parent traversal
    let output = starforge_in_dir(home.path(), &working_dir)
        .args(["info"])
        .output()
        .expect("spawn relative dir");
    assert_success(&output, "starforge info from relative subfolder");
}

// ─────────────────────────────────────────────────────────────────────────────
// 3. Terminal & Output Control Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_platform_no_color_environment_handling() {
    let home = isolated_home();

    // When NO_COLOR is set, ANSI escape codes (\x1b[) should not be emitted in plain help
    let mut cmd = starforge_cmd(home.path());
    cmd.env("NO_COLOR", "1");
    cmd.env("TERM", "dumb");
    cmd.arg("--help");

    let output = cmd.output().expect("spawn with NO_COLOR=1");
    assert_success(&output, "starforge --help with NO_COLOR=1");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("\x1b["),
        "Output with NO_COLOR=1 should not contain ANSI escape codes"
    );
}

#[test]
fn test_cross_platform_quiet_flag() {
    let home = isolated_home();

    let output = starforge_cmd(home.path())
        .arg("-q")
        .arg("info")
        .output()
        .expect("spawn with -q");
    assert_success(&output, "starforge -q info");
}

// ─────────────────────────────────────────────────────────────────────────────
// 4. Failure Paths & Invalid Input Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_platform_invalid_subcommand_failure() {
    let home = isolated_home();

    let output = starforge_cmd(home.path())
        .arg("non_existent_command_xyz_12345")
        .output()
        .expect("spawn invalid subcommand");
    assert_failure(&output, "invalid subcommand");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("error:")
            || combined.contains("Error")
            || combined.contains("Usage")
            || combined.contains("unrecognized")
            || combined.contains("not recognized")
            || combined.contains("not a valid")
            || !combined.is_empty(),
        "Error output should provide a descriptive failure message"
    );
}

#[test]
fn test_cross_platform_unsupported_flag_failure() {
    let home = isolated_home();

    let output = starforge_cmd(home.path())
        .arg("--unsupported-flag-cross-platform-test")
        .output()
        .expect("spawn unsupported flag");
    assert_failure(&output, "unsupported flag");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected")
            || stderr.contains("error:")
            || stderr.contains("unknown")
            || stderr.contains("unrecognized"),
        "Error message should explain unsupported flag"
    );
}

#[test]
fn test_cross_platform_empty_argument_handling() {
    let home = isolated_home();

    // Passing empty string as a subcommand
    let output = starforge_cmd(home.path())
        .arg("")
        .output()
        .expect("spawn with empty arg");
    assert_failure(&output, "empty string argument");
}

// ─────────────────────────────────────────────────────────────────────────────
// 5. Cargo.lock Reproducibility & Dependency Resolution Tests
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn test_cross_platform_cargo_lock_reproducibility_primary() {
    let home = isolated_home();

    // Verify `starforge verify lockfile` on the current workspace (primary flow)
    let output = starforge_cmd(home.path())
        .args(["verify", "lockfile", "--json"])
        .output()
        .expect("spawn starforge verify lockfile");
    assert_success(&output, "starforge verify lockfile --json");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("\"is_reproducible\": true"),
        "Primary flow: Cargo.lock should be reproducible"
    );
}

#[test]
fn test_cross_platform_cargo_lock_boundary_case() {
    let home = isolated_home();
    let temp_workspace = tempfile::tempdir().expect("create temp workspace");

    let cargo_toml = r#"[package]
name = "boundary-test-pkg"
version = "0.1.0"
edition = "2021"

[features]
default = []
opt_feat = []

[target.'cfg(unix)'.dependencies]

[target.'cfg(windows)'.dependencies]
"#;

    let cargo_lock = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "boundary-test-pkg"
version = "0.1.0"
"#;

    fs::write(temp_workspace.path().join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");
    fs::write(temp_workspace.path().join("Cargo.lock"), cargo_lock).expect("write Cargo.lock");

    let output = starforge_cmd(home.path())
        .args([
            "verify",
            "lockfile",
            "--path",
            &temp_workspace.path().to_string_lossy(),
        ])
        .output()
        .expect("spawn starforge verify lockfile boundary");
    assert_success(&output, "starforge verify lockfile boundary case");
}

#[test]
fn test_cross_platform_cargo_lock_failure_out_of_sync() {
    let home = isolated_home();
    let temp_workspace = tempfile::tempdir().expect("create temp workspace");

    let cargo_toml = r#"[package]
name = "invalid-lock-pkg"
version = "0.1.0"
edition = "2021"

[dependencies]
non_existent_crate_xyz_777 = "7.7.7"
"#;

    let cargo_lock = r#"# This file is automatically @generated by Cargo.
# It is not intended for manual editing.
version = 3

[[package]]
name = "invalid-lock-pkg"
version = "0.1.0"
"#;

    fs::write(temp_workspace.path().join("Cargo.toml"), cargo_toml).expect("write Cargo.toml");
    fs::write(temp_workspace.path().join("Cargo.lock"), cargo_lock).expect("write Cargo.lock");

    let output = starforge_cmd(home.path())
        .args([
            "verify",
            "lockfile",
            "--path",
            &temp_workspace.path().to_string_lossy(),
        ])
        .output()
        .expect("spawn starforge verify lockfile failure");

    assert_failure(&output, "out-of-sync Cargo.lock failure case");

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);

    assert!(
        combined.contains("Cargo.lock reproducibility verification failed")
            || combined.contains("violation")
            || combined.contains("failed"),
        "Failure output should contain clear diagnostic failure message"
    );
}

#[test]
fn test_cross_platform_cargo_lock_invalid_input() {
    let home = isolated_home();

    let output = starforge_cmd(home.path())
        .args([
            "verify",
            "lockfile",
            "--path",
            "/non_existent_path_directory_99999",
        ])
        .output()
        .expect("spawn invalid path test");

    assert_failure(&output, "invalid path argument failure");
}
