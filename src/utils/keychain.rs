//! OS-native secret storage with a file-based fallback (issue #928).
//!
//! Wallet secrets normally live in the StarForge configuration. This module
//! adds an OS-native backend: the macOS Keychain, the Windows Credential
//! Manager (via `PasswordVault`), and the freedesktop Secret Service on Linux.
//! It reaches those stores by shelling out to the platform tool, so no new
//! crate is added and `Cargo.lock` stays untouched.
//!
//! The OS backend is opt-in at build time behind the `keychain` feature and at
//! runtime by running `starforge wallet migrate --to keychain`. When the
//! feature is disabled, or the platform tool is missing (a headless CI runner,
//! for example), [`default_backend`] falls back to a permission-restricted
//! JSON file so that migration still succeeds without key loss. This is the
//! documented **headless-CI fallback**.
//!
//! Secrets are addressed by a stable key (see [`wallet_secret_key`]). The
//! configuration keeps only a reference of the form `keychain:<key>` (see
//! [`secret_reference`]) instead of the plaintext secret; callers resolve it
//! back through [`resolve_secret`].

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Service/collection name used for every OS keychain entry.
pub const SERVICE_NAME: &str = "starforge";

/// File name of the fallback secret store, created next to the config file.
pub const SECRETS_FILE_NAME: &str = "secrets.json";

/// Prefix marking a configuration value as a reference to a stored secret.
pub const KEYCHAIN_REF_PREFIX: &str = "keychain:";

// ── Backend selection ─────────────────────────────────────────────────────────

/// Which backend is actually used for a store/get/delete operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendKind {
    /// The OS-native keychain (macOS Keychain, Windows Credential Manager, or
    /// the Linux Secret Service).
    OsKeychain,
    /// The permission-restricted file fallback used off-feature and on hosts
    /// with no usable OS keychain (headless CI).
    File,
}

impl BackendKind {
    /// Stable, machine-readable name used in reports and diagnostics.
    pub fn as_str(&self) -> &'static str {
        match self {
            BackendKind::OsKeychain => "keychain",
            BackendKind::File => "file",
        }
    }

    /// Human-readable description for CLI output.
    pub fn description(&self) -> &'static str {
        match self {
            BackendKind::OsKeychain => {
                "OS keychain (macOS Keychain / Windows Credential Manager / Secret Service)"
            }
            BackendKind::File => "file fallback (headless CI; permission-restricted)",
        }
    }
}

impl std::fmt::Display for BackendKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Abstraction over a secret store so the OS keychain and the file fallback can
/// be swapped without changing callers.
pub trait SecretBackend {
    /// Which kind of backend this is.
    fn kind(&self) -> BackendKind;

    /// Whether this backend can be used on the current host.
    fn is_available(&self) -> bool;

    /// Store (or overwrite) the secret for `key`.
    fn store(&self, key: &str, value: &str) -> Result<()>;

    /// Read the secret for `key`, or `None` when it is not present.
    fn get(&self, key: &str) -> Result<Option<String>>;

    /// Delete the secret for `key`, returning `true` when one was removed.
    fn delete(&self, key: &str) -> Result<bool>;
}

/// The backend chosen for the current host and build.
///
/// Prefers the OS keychain when the `keychain` feature is enabled and the
/// platform tool is present, and otherwise returns the file fallback.
pub fn default_backend() -> Box<dyn SecretBackend> {
    #[cfg(feature = "keychain")]
    {
        let os = OsKeychainBackend;
        if os.is_available() {
            return Box::new(os);
        }
    }
    Box::new(FileBackend::default_dir())
}

/// The kind of backend that [`default_backend`] would select right now.
pub fn backend_kind() -> BackendKind {
    default_backend().kind()
}

/// Whether an OS-native keychain is available. `false` means operations will
/// use the documented file fallback.
pub fn is_available() -> bool {
    backend_kind() == BackendKind::OsKeychain
}

/// Store a secret using the default backend.
pub fn store_secret(key: &str, value: &str) -> Result<()> {
    default_backend().store(key, value)
}

/// Read a secret using the default backend.
pub fn get_secret(key: &str) -> Result<Option<String>> {
    default_backend().get(key)
}

/// Delete a secret using the default backend.
pub fn delete_secret(key: &str) -> Result<bool> {
    default_backend().delete(key)
}

// ── References ────────────────────────────────────────────────────────────────

/// The stable key under which a wallet's secret is stored.
pub fn wallet_secret_key(wallet_name: &str) -> String {
    format!("wallet/{wallet_name}")
}

/// Render the configuration reference for a stored secret key.
pub fn secret_reference(key: &str) -> String {
    format!("{KEYCHAIN_REF_PREFIX}{key}")
}

/// Whether a configuration value is a reference to a stored secret rather
/// than the secret itself.
pub fn is_secret_reference(value: &str) -> bool {
    value.trim_start().starts_with(KEYCHAIN_REF_PREFIX)
}

/// Extract the backend key from a `keychain:<key>` reference.
pub fn reference_key(value: &str) -> Option<&str> {
    value.trim_start().strip_prefix(KEYCHAIN_REF_PREFIX)
}

/// Resolve a configuration value that may be a plaintext secret or a
/// `keychain:<key>` reference.
pub fn resolve_secret(value: &str) -> Result<Option<String>> {
    match reference_key(value) {
        Some(key) => get_secret(key),
        None => Ok(Some(value.to_string())),
    }
}

// ── File fallback backend ─────────────────────────────────────────────────────

/// Permission-restricted JSON store used when the OS keychain is unavailable.
#[derive(Debug, Clone)]
pub struct FileBackend {
    root: PathBuf,
}

impl FileBackend {
    /// Create a file backend rooted at `root`; the store lives in
    /// `<root>/secrets.json`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// File backend rooted in the StarForge config directory.
    pub fn default_dir() -> Self {
        Self::new(crate::utils::config::config_dir())
    }

    /// Path of the JSON store.
    pub fn secrets_path(&self) -> PathBuf {
        self.root.join(SECRETS_FILE_NAME)
    }

    fn load_map(&self) -> Result<BTreeMap<String, String>> {
        let path = self.secrets_path();
        if !path.exists() {
            return Ok(BTreeMap::new());
        }
        let data = fs::read_to_string(&path)
            .with_context(|| format!("failed to read secret store at {}", path.display()))?;
        if data.trim().is_empty() {
            return Ok(BTreeMap::new());
        }
        serde_json::from_str(&data)
            .with_context(|| format!("failed to parse secret store at {}", path.display()))
    }

    fn save_map(&self, map: &BTreeMap<String, String>) -> Result<()> {
        let path = self.secrets_path();
        let data = serde_json::to_string_pretty(map)?;
        crate::utils::fs_permissions::create_private_file(&path, data.as_bytes())
            .with_context(|| format!("failed to write secret store at {}", path.display()))?;
        Ok(())
    }
}

impl SecretBackend for FileBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::File
    }

    fn is_available(&self) -> bool {
        true
    }

    fn store(&self, key: &str, value: &str) -> Result<()> {
        let mut map = self.load_map()?;
        map.insert(key.to_string(), value.to_string());
        self.save_map(&map)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.load_map()?.get(key).cloned())
    }

    fn delete(&self, key: &str) -> Result<bool> {
        let mut map = self.load_map()?;
        let removed = map.remove(key).is_some();
        if removed {
            self.save_map(&map)?;
        }
        Ok(removed)
    }
}

// ── OS keychain backend ───────────────────────────────────────────────────────

/// OS-native keychain backend. Only compiled when the `keychain` feature is on;
/// each method shells out to the platform tool.
#[cfg(feature = "keychain")]
#[derive(Debug, Clone, Copy, Default)]
pub struct OsKeychainBackend;

impl SecretBackend for OsKeychainBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::OsKeychain
    }

    fn is_available(&self) -> bool {
        let tool = os_tool_name();
        !tool.is_empty() && command_available(tool)
    }

    fn store(&self, key: &str, value: &str) -> Result<()> {
        os_store(key, value)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        os_get(key)
    }

    fn delete(&self, key: &str) -> Result<bool> {
        os_delete(key)
    }
}

/// Platform tool used for keychain access, or `""` when unsupported.
#[cfg(feature = "keychain")]
fn os_tool_name() -> &'static str {
    match std::env::consts::OS {
        "macos" => "security",
        "windows" => "powershell",
        "linux" => "secret-tool",
        _ => "",
    }
}

#[cfg(feature = "keychain")]
fn command_available(bin: &str) -> bool {
    if bin.is_empty() {
        return false;
    }
    Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok()
}

#[cfg(feature = "keychain")]
fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_string()
}

#[cfg(feature = "keychain")]
fn trim_newline(value: &str) -> String {
    value.trim_end_matches(['\r', '\n']).to_string()
}

#[cfg(feature = "keychain")]
fn os_store(key: &str, value: &str) -> Result<()> {
    match std::env::consts::OS {
        "macos" => {
            let output = Command::new("security")
                .args([
                    "add-generic-password",
                    "-U",
                    "-s",
                    SERVICE_NAME,
                    "-a",
                    key,
                    "-w",
                    value,
                ])
                .output()
                .context("failed to invoke the macOS `security` tool")?;
            if !output.status.success() {
                anyhow::bail!(
                    "macOS Keychain rejected the secret for '{}': {}",
                    key,
                    stderr_of(&output)
                );
            }
            Ok(())
        }
        "linux" => {
            let label = format!("{SERVICE_NAME} {key}");
            let mut child = Command::new("secret-tool")
                .arg("store")
                .arg("--label")
                .arg(&label)
                .arg("service")
                .arg(SERVICE_NAME)
                .arg("account")
                .arg(key)
                .stdin(Stdio::piped())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .context("failed to invoke `secret-tool`")?;
            {
                use std::io::Write as _;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin
                        .write_all(value.as_bytes())
                        .context("failed to send secret to `secret-tool`")?;
                }
            }
            let output = child
                .wait_with_output()
                .context("failed to wait for `secret-tool`")?;
            if !output.status.success() {
                anyhow::bail!(
                    "Secret Service rejected the secret for '{}': {}",
                    key,
                    stderr_of(&output)
                );
            }
            Ok(())
        }
        "windows" => {
            let output = run_powershell(POWERSHELL_STORE, key, Some(value))?;
            if !output.status.success() {
                anyhow::bail!(
                    "Windows Credential Manager rejected the secret for '{}': {}",
                    key,
                    stderr_of(&output)
                );
            }
            Ok(())
        }
        other => anyhow::bail!("OS keychain backend is not supported on '{other}'"),
    }
}

#[cfg(feature = "keychain")]
fn os_get(key: &str) -> Result<Option<String>> {
    match std::env::consts::OS {
        "macos" => {
            let output = Command::new("security")
                .args(["find-generic-password", "-s", SERVICE_NAME, "-a", key, "-w"])
                .output()
                .context("failed to invoke the macOS `security` tool")?;
            if !output.status.success() {
                return Ok(None);
            }
            Ok(Some(trim_newline(&String::from_utf8_lossy(&output.stdout))))
        }
        "linux" => {
            let output = Command::new("secret-tool")
                .arg("lookup")
                .arg("service")
                .arg(SERVICE_NAME)
                .arg("account")
                .arg(key)
                .output()
                .context("failed to invoke `secret-tool`")?;
            if !output.status.success() {
                return Ok(None);
            }
            let value = trim_newline(&String::from_utf8_lossy(&output.stdout));
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value))
            }
        }
        "windows" => {
            let output = run_powershell(POWERSHELL_GET, key, None)?;
            if !output.status.success() {
                return Ok(None);
            }
            let value = trim_newline(&String::from_utf8_lossy(&output.stdout));
            if value.is_empty() {
                Ok(None)
            } else {
                Ok(Some(value))
            }
        }
        other => anyhow::bail!("OS keychain backend is not supported on '{other}'"),
    }
}

#[cfg(feature = "keychain")]
fn os_delete(key: &str) -> Result<bool> {
    match std::env::consts::OS {
        "macos" => {
            let output = Command::new("security")
                .args(["delete-generic-password", "-s", SERVICE_NAME, "-a", key])
                .output()
                .context("failed to invoke the macOS `security` tool")?;
            Ok(output.status.success())
        }
        "linux" => {
            let output = Command::new("secret-tool")
                .arg("clear")
                .arg("service")
                .arg(SERVICE_NAME)
                .arg("account")
                .arg(key)
                .output()
                .context("failed to invoke `secret-tool`")?;
            Ok(output.status.success())
        }
        "windows" => {
            let output = run_powershell(POWERSHELL_DELETE, key, None)?;
            Ok(output.status.success())
        }
        other => anyhow::bail!("OS keychain backend is not supported on '{other}'"),
    }
}

#[cfg(feature = "keychain")]
fn run_powershell(script: &str, key: &str, value: Option<&str>) -> Result<std::process::Output> {
    let mut command = Command::new("powershell");
    command
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(script)
        .env("SF_SERVICE", SERVICE_NAME)
        .env("SF_ACCOUNT", key);
    if let Some(value) = value {
        command.env("SF_SECRET", value);
    }
    command
        .output()
        .context("failed to invoke PowerShell credential vault")
}

#[cfg(feature = "keychain")]
const POWERSHELL_STORE: &str = r#"
$ErrorActionPreference = 'Stop'
$vault = New-Object Windows.Security.Credentials.PasswordVault
try { $vault.Remove($vault.Retrieve($env:SF_SERVICE, $env:SF_ACCOUNT)) } catch { }
$credential = New-Object Windows.Security.Credentials.PasswordCredential($env:SF_SERVICE, $env:SF_ACCOUNT, $env:SF_SECRET)
$vault.Add($credential)
"#;

#[cfg(feature = "keychain")]
const POWERSHELL_GET: &str = r#"
$ErrorActionPreference = 'Stop'
$vault = New-Object Windows.Security.Credentials.PasswordVault
$credential = $vault.Retrieve($env:SF_SERVICE, $env:SF_ACCOUNT)
$credential.RetrievePassword()
[Console]::Out.Write($credential.Password)
"#;

#[cfg(feature = "keychain")]
const POWERSHELL_DELETE: &str = r#"
$ErrorActionPreference = 'Stop'
$vault = New-Object Windows.Security.Credentials.PasswordVault
$vault.Remove($vault.Retrieve($env:SF_SERVICE, $env:SF_ACCOUNT))
"#;

// ── Migration ─────────────────────────────────────────────────────────────────

/// Summary of a `wallet migrate --to keychain` run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MigrationReport {
    /// Backend that stored the secrets (`keychain` or `file`).
    pub backend: String,
    /// Wallet names whose secrets were moved out of the config.
    pub migrated: Vec<String>,
    /// Wallet names that already held a `keychain:` reference.
    pub already_migrated: Vec<String>,
    /// Wallet names that carry no secret (watch-only or missing entries).
    pub skipped: Vec<String>,
    /// Number of secrets written to the backend.
    pub secrets_stored: usize,
}

impl MigrationReport {
    fn new(backend: &dyn SecretBackend) -> Self {
        Self {
            backend: backend.kind().as_str().to_string(),
            migrated: Vec::new(),
            already_migrated: Vec::new(),
            skipped: Vec::new(),
            secrets_stored: 0,
        }
    }
}

/// Move every plaintext wallet secret in `config` into `backend`, replacing the
/// secret with a `keychain:<key>` reference. Split out so it can be unit-tested
/// with a temporary backend.
pub fn migrate_config_with(
    backend: &dyn SecretBackend,
    config: &mut crate::utils::config::Config,
) -> Result<MigrationReport> {
    let mut report = MigrationReport::new(backend);
    for wallet in config.wallets.iter_mut() {
        let secret = match wallet.secret_key.clone() {
            Some(secret) if !secret.trim().is_empty() => secret,
            _ => {
                report.skipped.push(wallet.name.clone());
                continue;
            }
        };
        if is_secret_reference(&secret) {
            report.already_migrated.push(wallet.name.clone());
            continue;
        }
        let key = wallet_secret_key(&wallet.name);
        backend
            .store(&key, &secret)
            .with_context(|| format!("failed to store secret for wallet '{}'", wallet.name))?;
        wallet.secret_key = Some(secret_reference(&key));
        report.secrets_stored += 1;
        report.migrated.push(wallet.name.clone());
    }
    Ok(report)
}

/// Move every plaintext wallet secret in the live config into the default
/// backend.
pub fn migrate_config(config: &mut crate::utils::config::Config) -> Result<MigrationReport> {
    let backend = default_backend();
    migrate_config_with(&*backend, config)
}

/// Migrate the wallet secrets held in the TOML configuration at `config_path`,
/// rewriting the file so it stores only `keychain:` references.
///
/// The backend is chosen for the host: the OS keychain when the `keychain`
/// feature is enabled and available, otherwise a `secrets.json` file created
/// next to `config_path` (the headless-CI fallback).
pub fn migrate_to_keychain(config_path: &Path) -> Result<MigrationReport> {
    let contents = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read config at {}", config_path.display()))?;
    let mut document: toml::Value =
        toml::from_str(&contents).context("failed to parse configuration TOML")?;

    let backend = backend_for_path(config_path);
    let report = migrate_toml_document(&*backend, &mut document)?;

    let rendered =
        toml::to_string_pretty(&document).context("failed to serialize migrated configuration")?;
    crate::utils::config::atomic_write(config_path, rendered.as_bytes())?;
    Ok(report)
}

fn backend_for_path(config_path: &Path) -> Box<dyn SecretBackend> {
    #[cfg(feature = "keychain")]
    {
        let os = OsKeychainBackend;
        if os.is_available() {
            return Box::new(os);
        }
    }
    let root = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    Box::new(FileBackend::new(root))
}

fn migrate_toml_document(
    backend: &dyn SecretBackend,
    document: &mut toml::Value,
) -> Result<MigrationReport> {
    let mut report = MigrationReport::new(backend);
    let wallets = match document
        .get_mut("wallets")
        .and_then(toml::Value::as_array_mut)
    {
        Some(wallets) => wallets,
        None => return Ok(report),
    };

    for entry in wallets.iter_mut() {
        let table = match entry.as_table_mut() {
            Some(table) => table,
            None => continue,
        };
        let name = match table.get("name").and_then(toml::Value::as_str) {
            Some(name) if !name.is_empty() => name.to_string(),
            _ => {
                report.skipped.push("<unnamed>".to_string());
                continue;
            }
        };
        let secret = match table.get("secret_key").and_then(toml::Value::as_str) {
            Some(secret) if !secret.trim().is_empty() => secret.to_string(),
            _ => {
                report.skipped.push(name);
                continue;
            }
        };
        if is_secret_reference(&secret) {
            report.already_migrated.push(name);
            continue;
        }
        let key = wallet_secret_key(&name);
        backend
            .store(&key, &secret)
            .with_context(|| format!("failed to store secret for wallet '{name}'"))?;
        table.insert(
            "secret_key".to_string(),
            toml::Value::String(secret_reference(&key)),
        );
        report.secrets_stored += 1;
        report.migrated.push(name);
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const PLAINTEXT: &str = "SAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";

    #[test]
    fn file_backend_round_trips_and_overwrites() {
        let dir = tempdir().unwrap();
        let backend = FileBackend::new(dir.path());

        assert!(backend.get("wallet/alice").unwrap().is_none());
        backend.store("wallet/alice", PLAINTEXT).unwrap();
        assert_eq!(
            backend.get("wallet/alice").unwrap().as_deref(),
            Some(PLAINTEXT)
        );

        backend.store("wallet/alice", "SBBBB").unwrap();
        assert_eq!(
            backend.get("wallet/alice").unwrap().as_deref(),
            Some("SBBBB")
        );

        assert!(backend.delete("wallet/alice").unwrap());
        assert!(backend.get("wallet/alice").unwrap().is_none());
        assert!(!backend.delete("wallet/alice").unwrap());
    }

    #[test]
    fn file_backend_persists_across_instances() {
        let dir = tempdir().unwrap();
        FileBackend::new(dir.path())
            .store("wallet/bob", PLAINTEXT)
            .unwrap();
        let reopened = FileBackend::new(dir.path());
        assert_eq!(
            reopened.get("wallet/bob").unwrap().as_deref(),
            Some(PLAINTEXT)
        );
    }

    #[test]
    fn migrate_replaces_secret_with_reference_without_key_loss() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "version = \"1\"\nnetwork = \"testnet\"\n\n[[wallets]]\n\
                 name = \"alice\"\npublic_key = \"GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF7\"\n\
                 secret_key = \"{PLAINTEXT}\"\nnetwork = \"testnet\"\ncreated_at = \"now\"\nfunded = false\n"
            ),
        )
        .unwrap();

        let report = migrate_to_keychain(&config_path).unwrap();
        assert_eq!(report.backend, "file");
        assert_eq!(report.migrated, vec!["alice".to_string()]);
        assert_eq!(report.secrets_stored, 1);

        let contents = fs::read_to_string(&config_path).unwrap();
        let document: toml::Value = toml::from_str(&contents).unwrap();
        let stored = document["wallets"][0]["secret_key"].as_str().unwrap();
        assert!(is_secret_reference(stored));
        assert_eq!(stored, secret_reference("wallet/alice"));

        let backend = FileBackend::new(dir.path());
        assert_eq!(
            backend.get(&wallet_secret_key("alice")).unwrap().as_deref(),
            Some(PLAINTEXT),
            "the original key must be recoverable from the backend"
        );
        assert_eq!(
            resolve_secret_with(&backend, stored).unwrap().as_deref(),
            Some(PLAINTEXT)
        );
    }

    #[test]
    fn migrate_is_idempotent() {
        let dir = tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(
            &config_path,
            format!(
                "version = \"1\"\nnetwork = \"testnet\"\n\n[[wallets]]\nname = \"alice\"\n\
                 public_key = \"GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF7\"\n\
                 secret_key = \"{PLAINTEXT}\"\nnetwork = \"testnet\"\ncreated_at = \"now\"\nfunded = false\n"
            ),
        )
        .unwrap();

        migrate_to_keychain(&config_path).unwrap();
        let second = migrate_to_keychain(&config_path).unwrap();
        assert!(second.migrated.is_empty());
        assert_eq!(second.already_migrated, vec!["alice".to_string()]);
        assert_eq!(second.secrets_stored, 0);

        let backend = FileBackend::new(dir.path());
        assert_eq!(
            backend.get(&wallet_secret_key("alice")).unwrap().as_deref(),
            Some(PLAINTEXT),
            "re-running migration must not lose the first-stored key"
        );
    }

    #[test]
    fn migrate_config_with_handles_watch_only_and_references() {
        let dir = tempdir().unwrap();
        let backend = FileBackend::new(dir.path());
        let carol_reference = secret_reference("wallet/carol");
        let mut config = crate::utils::config::Config::default();
        config.wallets.push(wallet_entry("alice", Some(PLAINTEXT)));
        config.wallets.push(wallet_entry("watch", None));
        config
            .wallets
            .push(wallet_entry("carol", Some(carol_reference.as_str())));

        let report = migrate_config_with(&backend, &mut config).unwrap();
        assert_eq!(report.migrated, vec!["alice".to_string()]);
        assert_eq!(report.skipped, vec!["watch".to_string()]);
        assert_eq!(report.already_migrated, vec!["carol".to_string()]);
        assert_eq!(
            config.wallets[0].secret_key.as_deref(),
            Some("keychain:wallet/alice")
        );
        assert!(config.wallets[1].secret_key.is_none());
        assert_eq!(
            config.wallets[2].secret_key.as_deref(),
            Some("keychain:wallet/carol")
        );
    }

    fn resolve_secret_with(backend: &dyn SecretBackend, value: &str) -> Result<Option<String>> {
        match reference_key(value) {
            Some(key) => backend.get(key),
            None => Ok(Some(value.to_string())),
        }
    }

    fn wallet_entry(name: &str, secret: Option<&str>) -> crate::utils::config::WalletEntry {
        crate::utils::config::WalletEntry {
            name: name.to_string(),
            public_key: "GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF7".to_string(),
            secret_key: secret.map(str::to_string),
            network: "testnet".to_string(),
            created_at: "now".to_string(),
            funded: false,
            kdf_options: None,
            rotation_history: Vec::new(),
        }
    }
}
