use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const STARFORGE_ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn isolated_home() -> tempfile::TempDir {
    tempfile::tempdir().expect("create isolated home")
}

fn starforge(home: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_starforge"));
    cmd.arg("-q");
    cmd.env("HOME", home);
    cmd.env("USERPROFILE", home);
    cmd
}

fn plugin_library_name(name: &str) -> String {
    if cfg!(target_os = "windows") {
        format!("starforge_{name}.dll")
    } else if cfg!(target_os = "macos") {
        format!("libstarforge_{name}.dylib")
    } else {
        format!("libstarforge_{name}.so")
    }
}

fn build_loadable_plugin(home: &Path, name: &str) -> PathBuf {
    let project = home.join("loadable-plugin-src");
    let src = project.join("src");
    fs::create_dir_all(&src).expect("create plugin source dir");
    fs::write(
        project.join("Cargo.toml"),
        format!(
            r#"
[package]
name = "starforge-lifecycle-plugin"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
starforge = {{ path = "{}" }}
"#,
            STARFORGE_ROOT.replace('\\', "\\\\")
        ),
    )
    .expect("write plugin Cargo.toml");
    fs::copy(
        Path::new(STARFORGE_ROOT).join("Cargo.lock"),
        project.join("Cargo.lock"),
    )
    .expect("copy workspace Cargo.lock");
    fs::write(
        src.join("lib.rs"),
        format!(
            r#"
use starforge::plugins::{{Plugin, PluginRegistrar}};

struct LifecyclePlugin;

impl Plugin for LifecyclePlugin {{
    fn name(&self) -> &'static str {{
        "{name}"
    }}

    fn version(&self) -> &'static str {{
        "1.0.0"
    }}

    fn description(&self) -> &'static str {{
        "Lifecycle integration test plugin"
    }}

    fn execute(&self, _args: &[String]) -> Result<(), String> {{
        Ok(())
    }}
}}

unsafe fn register(registrar: &mut dyn PluginRegistrar) {{
    registrar.register_plugin(Box::new(LifecyclePlugin));
}}

starforge::export_plugin!(register);
"#
        ),
    )
    .expect("write plugin lib.rs");

    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_string());
    let output = Command::new(cargo)
        .args(["build", "--offline"])
        .current_dir(&project)
        .output()
        .expect("build loadable plugin");
    assert_success(&output, "build loadable plugin");

    let built_lib = project
        .join("target")
        .join("debug")
        .join(plugin_library_name("lifecycle_plugin"));
    assert!(
        built_lib.exists(),
        "expected built plugin library at {}",
        built_lib.display()
    );

    let plugin_dir = home.join("fixtures").join(name);
    fs::create_dir_all(&plugin_dir).expect("create loadable plugin fixture dir");
    let fixture_lib = plugin_dir.join(plugin_library_name(name));
    fs::copy(&built_lib, &fixture_lib).expect("copy loadable plugin library");
    fs::write(
        plugin_dir.join("starforge-plugin.toml"),
        format!(
            r#"
name = "{name}"
version = "1.0.0"
starforge_version = "{}"
description = "Lifecycle integration test plugin"
"#,
            env!("CARGO_PKG_VERSION")
        ),
    )
    .expect("write loadable plugin manifest");

    fixture_lib
}

fn write_plugin_fixture(dir: &Path, name: &str, starforge_version: &str) -> PathBuf {
    fs::create_dir_all(dir).expect("create plugin fixture dir");
    let lib = dir.join(plugin_library_name(name));
    fs::write(&lib, b"not-a-real-dynamic-library").expect("write plugin library");
    fs::write(
        dir.join("starforge-plugin.toml"),
        format!(
            r#"
name = "{name}"
version = "1.0.0"
starforge_version = "{starforge_version}"
description = "test plugin"
"#
        ),
    )
    .expect("write plugin manifest");
    lib
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{} failed\nstdout:\n{}\nstderr:\n{}",
        context,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn assert_failure(output: &Output, context: &str) {
    assert!(
        !output.status.success(),
        "{} unexpectedly succeeded\nstdout:\n{}\nstderr:\n{}",
        context,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn plugin_lifecycle_install_list_verify_load_uninstall() {
    let home = isolated_home();
    let lib = build_loadable_plugin(home.path(), "trusted");

    let install = starforge(home.path())
        .args([
            "plugin",
            "install",
            "trusted",
            "--path",
            lib.to_str().unwrap(),
            "--source",
            "https://github.com/StarForge-Labs/trusted",
        ])
        .output()
        .expect("run plugin install");
    assert_success(&install, "plugin install");

    let list = starforge(home.path())
        .args(["plugin", "list"])
        .output()
        .expect("run plugin list");
    assert_success(&list, "plugin list");
    let list_stdout = String::from_utf8_lossy(&list.stdout);
    assert!(list_stdout.contains("trusted"));
    assert!(list_stdout.contains("Lifecycle integration test plugin"));

    let verify = starforge(home.path())
        .args(["plugin", "verify", "trusted", "--deep"])
        .output()
        .expect("run plugin verify --deep");
    assert_success(&verify, "plugin verify --deep");

    let audit = starforge(home.path())
        .args(["plugin", "audit", "trusted"])
        .output()
        .expect("run plugin audit");
    assert_success(&audit, "plugin audit");

    let runtime_audit = starforge(home.path())
        .args(["plugin", "audit", "trusted", "--runtime-check"])
        .output()
        .expect("run plugin audit --runtime-check");
    assert_success(&runtime_audit, "plugin audit --runtime-check");

    let load = starforge(home.path())
        .args(["plugin", "load"])
        .output()
        .expect("run plugin load");
    assert_success(&load, "plugin load");
    assert!(String::from_utf8_lossy(&load.stdout).contains("trusted"));

    let uninstall = starforge(home.path())
        .args(["plugin", "uninstall", "trusted"])
        .output()
        .expect("run plugin uninstall");
    assert_success(&uninstall, "plugin uninstall");

    let final_list = starforge(home.path())
        .args(["plugin", "list"])
        .output()
        .expect("run final plugin list");
    assert_success(&final_list, "final plugin list");
    assert!(!String::from_utf8_lossy(&final_list.stdout).contains("trusted"));
}

#[test]
fn plugin_audit_reports_untrusted_sources_without_runtime_check() {
    let home = isolated_home();
    let fixture_dir = home.path().join("fixtures").join("untrusted");
    let lib = write_plugin_fixture(&fixture_dir, "untrusted", env!("CARGO_PKG_VERSION"));

    let refused = starforge(home.path())
        .args([
            "plugin",
            "install",
            "untrusted",
            "--path",
            lib.to_str().unwrap(),
            "--source",
            "https://example.com/untrusted",
        ])
        .output()
        .expect("run refused plugin install");
    assert_failure(&refused, "untrusted plugin install without force");

    let registry_dir = home.path().join(".starforge").join("plugins");
    fs::create_dir_all(&registry_dir).expect("create registry dir");
    fs::write(
        registry_dir.join("registry.json"),
        format!(
            r#"{{
  "plugins": [
    {{
      "name": "untrusted",
      "path": "{}",
      "source": "https://example.com/untrusted",
      "trust": "unknown",
      "starforge_version": "{}",
      "plugin_version": "1.0.0"
    }}
  ]
}}"#,
            lib.display(),
            env!("CARGO_PKG_VERSION")
        ),
    )
    .expect("write registry");

    let audit = starforge(home.path())
        .args(["plugin", "audit", "untrusted"])
        .output()
        .expect("run plugin audit");
    assert_success(&audit, "plugin audit for untrusted plugin");
    let stdout = String::from_utf8_lossy(&audit.stdout);
    assert!(stdout.contains("trust"));
    assert!(stdout.contains("warn"));
    assert!(stdout.contains("unknown"));
}

#[test]
fn plugin_audit_fails_for_missing_library_file() {
    let home = isolated_home();
    let fixture_dir = home.path().join("fixtures").join("missing");
    let lib = write_plugin_fixture(&fixture_dir, "missing", env!("CARGO_PKG_VERSION"));

    let registry_dir = home.path().join(".starforge").join("plugins");
    fs::create_dir_all(&registry_dir).expect("create registry dir");
    fs::write(
        registry_dir.join("registry.json"),
        format!(
            r#"{{
  "plugins": [
    {{
      "name": "missing",
      "path": "{}",
      "source": "",
      "trust": "local",
      "starforge_version": "{}",
      "plugin_version": "1.0.0"
    }}
  ]
}}"#,
            lib.display(),
            env!("CARGO_PKG_VERSION")
        ),
    )
    .expect("write registry");

    fs::remove_file(&lib).expect("remove plugin library");

    let audit = starforge(home.path())
        .args(["plugin", "audit", "missing"])
        .output()
        .expect("run plugin audit");
    assert_failure(&audit, "plugin audit with missing library");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&audit.stdout),
        String::from_utf8_lossy(&audit.stderr)
    );
    assert!(output.contains("Library missing"));
}

#[test]
fn plugin_audit_fails_for_registry_version_mismatch() {
    let home = isolated_home();
    let fixture_dir = home.path().join("fixtures").join("mismatch");
    let lib = write_plugin_fixture(&fixture_dir, "mismatch", env!("CARGO_PKG_VERSION"));
    let registry_dir = home.path().join(".starforge").join("plugins");
    fs::create_dir_all(&registry_dir).expect("create registry dir");
    fs::write(
        registry_dir.join("registry.json"),
        format!(
            r#"{{
  "plugins": [
    {{
      "name": "mismatch",
      "path": "{}",
      "source": "",
      "trust": "local",
      "starforge_version": "999.0.0",
      "plugin_version": "1.0.0"
    }}
  ]
}}"#,
            lib.display()
        ),
    )
    .expect("write registry");

    let audit = starforge(home.path())
        .args(["plugin", "audit", "mismatch"])
        .output()
        .expect("run plugin audit");
    assert_failure(&audit, "plugin audit with version mismatch");
    let output = format!(
        "{}{}",
        String::from_utf8_lossy(&audit.stdout),
        String::from_utf8_lossy(&audit.stderr)
    );
    assert!(output.contains("targets StarForge 999.0.0"));
}
