use anyhow::{bail, Context, Result};
use semver::Version;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const LEGACY_WASM_TARGET: &str = "wasm32-unknown-unknown";
pub const V1_WASM_TARGET: &str = "wasm32v1-none";

pub fn target_for_rustc_version(version: &Version) -> &'static str {
    if version.major > 1 || (version.major == 1 && version.minor >= 84) {
        V1_WASM_TARGET
    } else {
        LEGACY_WASM_TARGET
    }
}

pub fn resolve_target(override_target: Option<&str>) -> Result<String> {
    if let Some(target) = override_target {
        if target != LEGACY_WASM_TARGET && target != V1_WASM_TARGET {
            bail!("Unsupported Soroban WASM target '{target}'. Use '{LEGACY_WASM_TARGET}' or '{V1_WASM_TARGET}'.");
        }
        return Ok(target.to_string());
    }

    let output = Command::new("rustc")
        .arg("--version")
        .output()
        .context("Failed to detect the Rust toolchain; make sure rustc is installed and on PATH")?;
    if !output.status.success() {
        bail!("Failed to detect Rust toolchain version: rustc --version exited with {}", output.status);
    }

    let version_text = String::from_utf8_lossy(&output.stdout);
    let version = version_text
        .split_whitespace()
        .nth(1)
        .and_then(|value| Version::parse(value).ok())
        .with_context(|| format!("Could not parse Rust version from `{}`", version_text.trim()))?;
    Ok(target_for_rustc_version(&version).to_string())
}

pub fn artifact_path(project_dir: &Path, crate_name: &str, target: &str) -> PathBuf {
    project_dir
        .join("target")
        .join(target)
        .join("release")
        .join(format!("{}.wasm", crate_name.replace('-', "_")))
}

pub fn installed_target(target: &str) -> Result<bool> {
    let output = Command::new("rustup")
        .args(["target", "list", "--installed"])
        .output()
        .context("rustup is unavailable; install rustup to check installed Rust targets")?;
    if !output.status.success() {
        bail!("rustup target list --installed exited with {}", output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|installed| installed.trim() == target))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_target_by_rust_version() {
        assert_eq!(target_for_rustc_version(&Version::new(1, 80, 0)), LEGACY_WASM_TARGET);
        assert_eq!(target_for_rustc_version(&Version::new(1, 84, 0)), V1_WASM_TARGET);
        assert_eq!(target_for_rustc_version(&Version::new(2, 0, 0)), V1_WASM_TARGET);
    }

    #[test]
    fn accepts_supported_manifest_target_override() {
        assert_eq!(resolve_target(Some(LEGACY_WASM_TARGET)).unwrap(), LEGACY_WASM_TARGET);
        assert_eq!(resolve_target(Some(V1_WASM_TARGET)).unwrap(), V1_WASM_TARGET);
        assert!(resolve_target(Some("wasm32-wasi")).is_err());
    }

    #[test]
    fn builds_artifact_path_with_resolved_target() {
        assert_eq!(
            artifact_path(Path::new("project"), "my-contract", V1_WASM_TARGET),
            PathBuf::from("project/target/wasm32v1-none/release/my_contract.wasm")
        );
    }
}