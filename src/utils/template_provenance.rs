//! Keyless Sigstore signing and build provenance for published templates.
//!
//! A checksum proves that a package has not changed; it says nothing about
//! *who* published it. This module adds the missing half: every published
//! template can be signed with the standard Sigstore keyless flow — an OIDC
//! identity is exchanged for a short-lived certificate, the signature and
//! certificate are recorded in a transparency log, and the whole thing is
//! packaged as a Sigstore bundle.
//!
//! The bundle is opaque to StarForge. Producing and verifying it is delegated
//! to the reference implementation, the [`cosign`](https://docs.sigstore.dev)
//! CLI, which is how a client library integrates keyless signing without
//! vendoring Fulcio and Rekor clients. StarForge owns the parts it must own:
//!
//! * a **canonical package digest** so the signed payload is reproducible and
//!   platform independent (`sha256:<hex>` over the sorted file tree),
//! * the **identity binding** stored next to the bundle, so
//!   `template install` can check the publisher it expects against the
//!   identity the certificate actually carries,
//! * the **policy** that decides whether an unsigned template may be installed
//!   at all.
//!
//! Verification therefore has two layers. The integrity layer always runs and
//! needs no network and no extra tooling: the fetched package is re-hashed and
//! compared with the digest recorded at signing time, which rejects a package
//! that was tampered with after publication. The cryptographic layer runs
//! `cosign verify-blob` against the recorded bundle, identity and issuer, which
//! rejects a bundle that was tampered with, re-signed by a different identity,
//! or is missing from the transparency log.

use anyhow::{Context, Result};
use base64::engine::general_purpose::{STANDARD as BASE64, URL_SAFE, URL_SAFE_NO_PAD};
use base64::Engine;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Signer name recorded for bundles produced by the Sigstore `cosign` CLI.
pub const SIGNER_COSIGN: &str = "cosign";

/// Path of the `cosign` binary, overriding the one found on `PATH`.
pub const COSIGN_BIN_ENV: &str = "STARFORGE_COSIGN_BIN";

/// Ambient OIDC identity token, the same variable `cosign` itself reads when
/// running in a workload such as GitHub Actions.
pub const OIDC_TOKEN_ENV: &str = "SIGSTORE_ID_TOKEN";

/// Explicit OIDC subject to bind into the signing certificate.
pub const OIDC_IDENTITY_ENV: &str = "STARFORGE_OIDC_IDENTITY";

/// Explicit OIDC issuer matching [`OIDC_IDENTITY_ENV`].
pub const OIDC_ISSUER_ENV: &str = "STARFORGE_OIDC_ISSUER";

/// When truthy, a template without provenance cannot be installed.
pub const REQUIRE_SIGNED_ENV: &str = "STARFORGE_TEMPLATE_REQUIRE_SIGNED";

/// File name of the payload handed to `cosign sign-blob`. The payload is the
/// canonical digest, never the package itself, so signing a large template does
/// not require cosign to buffer it.
const PAYLOAD_FILE_NAME: &str = "starforge-template-digest.txt";

/// File name of the Sigstore bundle produced by `cosign` inside a temp dir.
const BUNDLE_FILE_NAME: &str = "starforge-template.sigstore.json";

/// True when an optional string field is present and not blank. Used for the
/// provenance fields that only make sense together.
fn non_empty(value: Option<&str>) -> bool {
    matches!(value, Some(text) if !text.trim().is_empty())
}

/// Identity-bound signature record stored alongside a published template.
///
/// `digest` is always present so the integrity layer works even for a record
/// written by an older CLI or a registry that dropped the bundle. The remaining
/// fields describe the keyless signature and its transparency-log entry.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TemplateProvenance {
    /// Canonical digest of the signed package: `sha256` over the sorted file
    /// tree, hex encoded.
    #[serde(default)]
    pub digest: String,
    /// Base64-encoded Sigstore bundle (the Cosign bundle JSON).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bundle: Option<String>,
    /// OIDC subject bound into the signing certificate (an email for a human,
    /// or a workload identity for CI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<String>,
    /// OIDC issuer that vouched for [`Self::identity`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issuer: Option<String>,
    /// Tool that produced the signature, e.g. [`SIGNER_COSIGN`].
    #[serde(default)]
    pub signer: String,
    /// Rekor transparency-log index, when the signer recorded one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rekor_log_index: Option<u64>,
    /// RFC 3339 timestamp of signing.
    #[serde(default)]
    pub signed_at: String,
}

impl TemplateProvenance {
    /// Whether the record carries everything needed for a full Sigstore check.
    pub fn is_verifiable(&self) -> bool {
        non_empty(self.bundle.as_deref())
            && non_empty(self.identity.as_deref())
            && non_empty(self.issuer.as_deref())
    }

    /// One-line human-readable summary, used by the CLI and by docs output.
    pub fn summary(&self) -> String {
        let identity = self.identity.as_deref().unwrap_or("unknown identity");
        let issuer = self.issuer.as_deref().unwrap_or("unknown issuer");
        if self.signed_at.trim().is_empty() {
            format!("{} (issuer {})", identity, issuer)
        } else {
            format!(
                "{} (issuer {}, signed {})",
                identity, issuer, self.signed_at
            )
        }
    }
}

/// Inputs for [`sign_package`].
#[derive(Debug, Clone, Default)]
pub struct SigningConfig {
    /// Explicit `cosign` binary, overriding [`COSIGN_BIN_ENV`] and `PATH`.
    pub cosign_bin: Option<PathBuf>,
    /// OIDC subject to bind, overriding the ambient identity.
    pub identity: Option<String>,
    /// OIDC issuer matching [`Self::identity`].
    pub issuer: Option<String>,
}

/// Inputs for [`verify_provenance`].
#[derive(Debug, Clone, Default)]
pub struct VerifyConfig {
    /// Fail instead of degrading to integrity-only when the bundle cannot be
    /// cryptographically verified. Set when signed templates are mandatory.
    pub require_crypto: bool,
    /// Explicit `cosign` binary, overriding [`COSIGN_BIN_ENV`] and `PATH`.
    pub cosign_bin: Option<PathBuf>,
}

/// How far verification actually got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationLevel {
    /// Only the local integrity check ran; no bundle was checked.
    IntegrityOnly,
    /// Integrity plus a full Sigstore bundle verification.
    Cryptographic,
}

/// Result of a successful [`verify_provenance`] call.
#[derive(Debug, Clone)]
pub struct VerificationOutcome {
    /// Strongest check that passed.
    pub level: VerificationLevel,
    /// Publisher identity the record is bound to.
    pub identity: Option<String>,
    /// OIDC issuer the record is bound to.
    pub issuer: Option<String>,
}

/// Canonical, platform-independent digest of a template package.
///
/// Directories are hashed as the sorted list of their files, each contributing
/// its normalized relative path, byte length and contents. Sorting makes the
/// digest independent of directory iteration order, and normalizing separators
/// to `/` makes a package published on Windows verify on Linux. `.git`,
/// `__MACOSX` and `.DS_Store` entries are ignored because the installer does not
/// copy them.
///
/// A plain file (a `.zip` package) is hashed as its raw bytes.
pub fn package_digest(path: &Path) -> Result<String> {
    if path.is_dir() {
        let mut files: Vec<String> = Vec::new();
        collect_package_files(path, path, &mut files)?;
        files.sort();

        let mut hasher = Sha256::new();
        for relative in &files {
            let absolute = path.join(relative);
            let bytes = fs::read(&absolute)
                .with_context(|| format!("Failed to read {} while hashing", absolute.display()))?;
            hasher.update(relative.as_bytes());
            hasher.update([0u8]);
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update([0u8]);
            hasher.update(&bytes);
        }
        Ok(hex::encode(hasher.finalize()))
    } else if path.is_file() {
        let bytes = fs::read(path)
            .with_context(|| format!("Failed to read {} while hashing", path.display()))?;
        Ok(hex::encode(Sha256::digest(&bytes)))
    } else {
        anyhow::bail!(
            "Cannot hash template package at {}: no such file or directory",
            path.display()
        )
    }
}

/// Recursively collect the package-relative paths of every file under `dir`.
fn collect_package_files(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<()> {
    let entries = fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory {}", dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()?;

    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" || name == "__MACOSX" || name == ".DS_Store" {
            continue;
        }

        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() {
            // The installer refuses symlinks, so they carry no installed bytes.
            continue;
        }

        if metadata.is_dir() {
            collect_package_files(root, &path, out)?;
        } else if metadata.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path);
            out.push(relative.to_string_lossy().replace('\\', "/"));
        }
    }

    Ok(())
}

/// The exact bytes signed for `digest`. Kept in one place so signing and
/// verification can never disagree about the payload.
fn signing_payload(digest: &str) -> String {
    format!("sha256:{}\n", digest.trim())
}

/// Locate the `cosign` binary: explicit path, then [`COSIGN_BIN_ENV`], then
/// `PATH`.
pub fn resolve_cosign(explicit: Option<&Path>) -> Option<PathBuf> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Some(path.to_path_buf());
        }
        return None;
    }

    if let Some(from_env) = std::env::var_os(COSIGN_BIN_ENV) {
        let path = PathBuf::from(from_env);
        if path.is_file() {
            return Some(path);
        }
        return None;
    }

    let search_path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&search_path) {
        for candidate in ["cosign.exe", "cosign"] {
            let full = dir.join(candidate);
            if full.is_file() {
                return Some(full);
            }
        }
    }

    None
}

/// The ambient OIDC identity available to this process, as `(issuer, subject)`.
///
/// Configured explicitly first ([`OIDC_IDENTITY_ENV`] / [`OIDC_ISSUER_ENV`]),
/// then derived from the token in [`OIDC_TOKEN_ENV`] — the same token `cosign`
/// exchanges with Fulcio, so the recorded identity is the one the certificate
/// will carry.
pub fn ambient_oidc_identity() -> Option<(String, String)> {
    let identity = std::env::var(OIDC_IDENTITY_ENV).ok();
    let issuer = std::env::var(OIDC_ISSUER_ENV).ok();
    if let (Some(identity), Some(issuer)) = (identity, issuer) {
        if !identity.trim().is_empty() && !issuer.trim().is_empty() {
            return Some((issuer.trim().to_string(), identity.trim().to_string()));
        }
    }

    let token = std::env::var(OIDC_TOKEN_ENV).ok()?;
    identity_from_id_token(&token)
}

/// Decode the `iss` and identity claims of an OIDC ID token.
///
/// Only the claims are read: the token's signature is not checked here because
/// the authoritative check is `cosign` verifying the bundle it produced from
/// the same token.
pub fn identity_from_id_token(token: &str) -> Option<(String, String)> {
    let encoded = token.split('.').nth(1)?;
    let decoded = URL_SAFE_NO_PAD
        .decode(encoded)
        .or_else(|_| URL_SAFE.decode(encoded))
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&decoded).ok()?;

    let issuer = claims.get("iss")?.as_str()?.trim().to_string();
    let subject = claims
        .get("email")
        .and_then(|value| value.as_str())
        .or_else(|| claims.get("sub").and_then(|value| value.as_str()))?
        .trim()
        .to_string();

    if issuer.is_empty() || subject.is_empty() {
        return None;
    }

    Some((issuer, subject))
}

/// Whether keyless signing can run right now: `cosign` is available and an
/// identity is known.
pub fn keyless_signing_available(config: &SigningConfig) -> bool {
    if resolve_cosign(config.cosign_bin.as_deref()).is_none() {
        return false;
    }

    let explicit = non_empty(config.identity.as_deref()) && non_empty(config.issuer.as_deref());
    explicit || ambient_oidc_identity().is_some()
}

/// Sign a published package with keyless Sigstore signing.
///
/// Produces a [`TemplateProvenance`] holding the bundle plus the identity it is
/// bound to. Errors when `cosign` is unavailable, when no OIDC identity can be
/// established, or when `cosign` itself fails — publishing without the signature
/// the caller asked for would be worse than failing.
pub fn sign_package(package_path: &Path, config: &SigningConfig) -> Result<TemplateProvenance> {
    let cosign = resolve_cosign(config.cosign_bin.as_deref()).ok_or_else(|| {
        anyhow::anyhow!(
            "cosign was not found, so the package cannot be signed.\n\
             Install it (https://docs.sigstore.dev/cosign/installation/) or point {} at it.",
            COSIGN_BIN_ENV
        )
    })?;

    let explicit = match (config.issuer.as_deref(), config.identity.as_deref()) {
        (Some(issuer), Some(identity))
            if !issuer.trim().is_empty() && !identity.trim().is_empty() =>
        {
            Some((issuer.trim().to_string(), identity.trim().to_string()))
        }
        _ => None,
    };

    let (issuer, identity) = explicit.or_else(ambient_oidc_identity).ok_or_else(|| {
        anyhow::anyhow!(
            "No OIDC identity is available for keyless signing.\n\
             Run in a workload that provides one ({}), or pass an explicit identity and issuer.",
            OIDC_TOKEN_ENV
        )
    })?;

    let digest = package_digest(package_path)?;
    let workdir = tempfile::tempdir().context("Failed to create a temp dir for signing")?;
    let payload_path = workdir.path().join(PAYLOAD_FILE_NAME);
    fs::write(&payload_path, signing_payload(&digest))
        .with_context(|| format!("Failed to write {}", payload_path.display()))?;
    let bundle_path = workdir.path().join(BUNDLE_FILE_NAME);

    let output = Command::new(&cosign)
        .arg("sign-blob")
        .arg("--yes")
        .arg("--bundle")
        .arg(&bundle_path)
        .arg(&payload_path)
        .output()
        .with_context(|| format!("Failed to run cosign at {}", cosign.display()))?;

    if !output.status.success() {
        anyhow::bail!(
            "cosign sign-blob failed for template package {}:\n{}",
            package_path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    let bundle = fs::read(&bundle_path).with_context(|| {
        format!(
            "cosign reported success but produced no bundle at {}",
            bundle_path.display()
        )
    })?;

    Ok(TemplateProvenance {
        digest,
        bundle: Some(BASE64.encode(bundle)),
        identity: Some(identity),
        issuer: Some(issuer),
        signer: SIGNER_COSIGN.to_string(),
        rekor_log_index: None,
        signed_at: Utc::now().to_rfc3339(),
    })
}

/// Verify a fetched package against its provenance record.
///
/// The integrity layer always runs first: the package is re-hashed and compared
/// with [`TemplateProvenance::digest`], which is what rejects a package modified
/// after signing. When the record carries a bundle, `cosign verify-blob` checks
/// the signature, the certificate chain, the transparency log and the expected
/// identity and issuer.
///
/// When the cryptographic layer cannot run (no `cosign`, no bundle) the call
/// degrades to [`VerificationLevel::IntegrityOnly`] unless
/// [`VerifyConfig::require_crypto`] is set, in which case it fails rather than
/// silently accepting a weaker guarantee.
pub fn verify_provenance(
    package_path: &Path,
    provenance: &TemplateProvenance,
    config: &VerifyConfig,
) -> Result<VerificationOutcome> {
    let digest = package_digest(package_path)?;
    let expected = provenance.digest.trim();

    if expected.is_empty() {
        anyhow::bail!(
            "Template provenance record has no digest, so the package cannot be verified."
        );
    }

    if !digest.eq_ignore_ascii_case(expected) {
        anyhow::bail!(
            "Template package does not match its signed provenance.\n\
             Signed digest: {}\n\
             Actual digest: {}\n\
             The package changed after it was signed and was not installed.",
            expected,
            digest
        );
    }

    if !provenance.is_verifiable() {
        if config.require_crypto {
            anyhow::bail!(
                "Template has no verifiable Sigstore bundle (missing bundle, identity or issuer)."
            );
        }
        return Ok(VerificationOutcome {
            level: VerificationLevel::IntegrityOnly,
            identity: provenance.identity.clone(),
            issuer: provenance.issuer.clone(),
        });
    }

    let bundle_encoded = provenance.bundle.as_deref().unwrap_or_default().trim();
    let identity = provenance.identity.as_deref().unwrap_or_default().trim();
    let issuer = provenance.issuer.as_deref().unwrap_or_default().trim();

    let Some(cosign) = resolve_cosign(config.cosign_bin.as_deref()) else {
        if config.require_crypto {
            anyhow::bail!(
                "cosign was not found, so the template's Sigstore bundle could not be verified.\n\
                 Install it (https://docs.sigstore.dev/cosign/installation/) or point {} at it.",
                COSIGN_BIN_ENV
            );
        }
        return Ok(VerificationOutcome {
            level: VerificationLevel::IntegrityOnly,
            identity: Some(identity.to_string()),
            issuer: Some(issuer.to_string()),
        });
    };

    let bundle = BASE64
        .decode(bundle_encoded)
        .context("Template provenance bundle is not valid base64")?;

    let workdir = tempfile::tempdir().context("Failed to create a temp dir for verification")?;
    let payload_path = workdir.path().join(PAYLOAD_FILE_NAME);
    fs::write(&payload_path, signing_payload(&digest))
        .with_context(|| format!("Failed to write {}", payload_path.display()))?;
    let bundle_path = workdir.path().join(BUNDLE_FILE_NAME);
    fs::write(&bundle_path, &bundle)
        .with_context(|| format!("Failed to write {}", bundle_path.display()))?;

    let output = Command::new(&cosign)
        .arg("verify-blob")
        .arg("--bundle")
        .arg(&bundle_path)
        .arg("--certificate-identity")
        .arg(identity)
        .arg("--certificate-oidc-issuer")
        .arg(issuer)
        .arg(&payload_path)
        .output()
        .with_context(|| format!("Failed to run cosign at {}", cosign.display()))?;

    if !output.status.success() {
        anyhow::bail!(
            "Sigstore verification failed for the template package:\n{}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(VerificationOutcome {
        level: VerificationLevel::Cryptographic,
        identity: Some(identity.to_string()),
        issuer: Some(issuer.to_string()),
    })
}

/// Read a boolean environment flag. Absent or unrecognised values are `false`
/// so an empty variable never silently turns a policy on.
pub fn env_flag(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => false,
    }
}

/// Whether this environment requires signatures on installed templates.
pub fn require_signed_templates() -> bool {
    env_flag(REQUIRE_SIGNED_ENV)
}

/// Enforce the signed-templates policy for one template.
///
/// Called before a template is registered locally, so a refusal happens before
/// anything reaches the user's project directory.
pub fn enforce_signed_policy(provenance: Option<&TemplateProvenance>, name: &str) -> Result<()> {
    if require_signed_templates() && provenance.is_none() {
        anyhow::bail!(
            "Template '{}' is unsigned, and this environment requires signed templates.\n\
             Unset {} to allow unsigned templates.",
            name,
            REQUIRE_SIGNED_ENV
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn digest_is_stable_for_the_same_tree() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("Cargo.toml"), "[package]\n");
        write(&dir.path().join("src/lib.rs"), "pub fn main() {}\n");

        let first = package_digest(dir.path()).unwrap();
        let second = package_digest(dir.path()).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
    }

    #[test]
    fn digest_changes_when_file_contents_change() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("README.md"), "hello");
        let before = package_digest(dir.path()).unwrap();

        write(&dir.path().join("README.md"), "hello!");
        let after = package_digest(dir.path()).unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn digest_ignores_git_and_temp_markers() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("README.md"), "hello");
        let before = package_digest(dir.path()).unwrap();

        write(&dir.path().join(".git/HEAD"), "ref: refs/heads/main");
        write(&dir.path().join(".DS_Store"), "junk");
        let after = package_digest(dir.path()).unwrap();

        assert_eq!(before, after);
    }

    #[test]
    fn digest_changes_when_a_file_is_added() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("README.md"), "hello");
        let before = package_digest(dir.path()).unwrap();

        write(&dir.path().join("src/lib.rs"), "// contract");
        let after = package_digest(dir.path()).unwrap();

        assert_ne!(before, after);
    }

    #[test]
    fn id_token_claims_are_read() {
        let payload = serde_json::json!({
            "iss": "https://token.actions.githubusercontent.com",
            "sub": "repo:Nanle-code/StarForge:ref:refs/heads/master",
            "email": "publisher@example.com",
        });
        let encoded = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let token = format!("header.{}.signature", encoded);

        let (issuer, identity) = identity_from_id_token(&token).unwrap();
        assert_eq!(issuer, "https://token.actions.githubusercontent.com");
        assert_eq!(identity, "publisher@example.com");
    }

    #[test]
    fn id_token_without_identity_claims_is_rejected() {
        let payload = serde_json::json!({ "aud": "sigstore" });
        let encoded = URL_SAFE_NO_PAD.encode(payload.to_string().as_bytes());
        let token = format!("header.{}.signature", encoded);

        assert!(identity_from_id_token(&token).is_none());
    }

    #[test]
    fn malformed_id_token_is_rejected() {
        assert!(identity_from_id_token("not-a-jwt").is_none());
    }

    #[test]
    fn provenance_verifiability_requires_bundle_identity_and_issuer() {
        let mut provenance = TemplateProvenance {
            digest: "abc".to_string(),
            ..TemplateProvenance::default()
        };
        assert!(!provenance.is_verifiable());

        provenance.bundle = Some("ZGVhZGJlZWY=".to_string());
        provenance.identity = Some("publisher@example.com".to_string());
        assert!(!provenance.is_verifiable());

        provenance.issuer = Some("https://issuer.example.com".to_string());
        assert!(provenance.is_verifiable());
        assert!(provenance.summary().contains("publisher@example.com"));
    }

    #[test]
    fn verification_rejects_a_package_that_no_longer_matches() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("README.md"), "original");
        let provenance = TemplateProvenance {
            digest: package_digest(dir.path()).unwrap(),
            signer: SIGNER_COSIGN.to_string(),
            signed_at: "2026-01-01T00:00:00Z".to_string(),
            ..TemplateProvenance::default()
        };

        write(&dir.path().join("README.md"), "tampered");

        let error = verify_provenance(dir.path(), &provenance, &VerifyConfig::default())
            .expect_err("a tampered package must be rejected");
        assert!(error
            .to_string()
            .contains("does not match its signed provenance"));
    }

    #[test]
    fn verification_without_a_bundle_degrades_to_integrity_only() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("README.md"), "original");
        let provenance = TemplateProvenance {
            digest: package_digest(dir.path()).unwrap(),
            signer: SIGNER_COSIGN.to_string(),
            signed_at: "2026-01-01T00:00:00Z".to_string(),
            ..TemplateProvenance::default()
        };

        let outcome = verify_provenance(dir.path(), &provenance, &VerifyConfig::default()).unwrap();
        assert_eq!(outcome.level, VerificationLevel::IntegrityOnly);

        let strict = VerifyConfig {
            require_crypto: true,
            cosign_bin: None,
        };
        assert!(verify_provenance(dir.path(), &provenance, &strict).is_err());
    }
}
