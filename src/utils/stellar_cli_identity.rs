//! Read identities created by `stellar keys generate` / `stellar keys add`
//! so they can be imported into StarForge with
//! `starforge wallet import --from-stellar-cli <identity>`.
//!
//! stellar-cli stores each identity as `identity/<name>.toml` inside its
//! config directory. A local (project) directory `.stellar/` takes precedence
//! over the global one, mirroring stellar-cli's own lookup. Legacy `soroban`
//! directories from pre-rename releases are searched last.
//!
//! Only identities that hold key material on disk can be imported:
//! `secret_key = "S..."` or `seed_phrase = "..."`. Public-key-only identities,
//! OS keychain (`secure_store`) entries and Ledger identities are rejected
//! with an explanation instead of being silently skipped.

use anyhow::{anyhow, bail, Context, Result};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;

/// Key material read from a stellar-cli identity file.
#[derive(Debug)]
pub enum StellarCliKey {
    /// A raw `S...` secret key.
    SecretKey(Zeroizing<String>),
    /// A BIP39 phrase; stellar-cli derives account index 0 by default.
    SeedPhrase(Zeroizing<String>),
}

/// An identity located on disk.
#[derive(Debug)]
pub struct StellarCliIdentity {
    pub name: String,
    pub path: PathBuf,
    pub key: StellarCliKey,
}

/// Directories searched for `identity/<name>.toml`, highest precedence first.
///
/// `STELLAR_CONFIG_HOME` replaces the global directory entirely, as it does in
/// stellar-cli.
pub fn default_search_dirs() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
    let cwd = std::env::current_dir().ok();
    let home = env("STARFORGE_HOME")
        .or_else(|| env("USERPROFILE"))
        .or_else(|| env("HOME"))
        .map(PathBuf::from)
        .or_else(dirs::home_dir);
    search_dirs(
        cwd.as_deref(),
        home.as_deref(),
        env("STELLAR_CONFIG_HOME").map(PathBuf::from),
        env("XDG_CONFIG_HOME").map(PathBuf::from),
    )
}

fn search_dirs(
    cwd: Option<&Path>,
    home: Option<&Path>,
    stellar_config_home: Option<PathBuf>,
    xdg_config_home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(cwd) = cwd {
        dirs.push(cwd.join(".stellar"));
    }
    if let Some(dir) = stellar_config_home {
        dirs.push(dir);
    } else {
        if let Some(xdg) = xdg_config_home {
            dirs.push(xdg.join("stellar"));
        }
        if let Some(home) = home {
            dirs.push(home.join(".config").join("stellar"));
        }
    }
    if let Some(cwd) = cwd {
        dirs.push(cwd.join(".soroban"));
    }
    if let Some(home) = home {
        dirs.push(home.join(".config").join("soroban"));
    }
    dirs.dedup();
    dirs
}

/// Identity names become file names, so restrict them to what
/// `stellar keys generate` accepts and rule out path traversal.
fn validate_identity_name(name: &str) -> Result<()> {
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!(
            "Invalid stellar-cli identity name '{}': use letters, digits, '-' or '_'",
            name
        );
    }
    Ok(())
}

/// Find and parse `identity/<name>.toml` in the first directory that has it.
pub fn load_identity(name: &str, dirs: &[PathBuf]) -> Result<StellarCliIdentity> {
    validate_identity_name(name)?;

    let path = dirs
        .iter()
        .map(|d| d.join("identity").join(format!("{name}.toml")))
        .find(|p| p.is_file())
        .ok_or_else(|| {
            let searched: Vec<String> = dirs
                .iter()
                .map(|d| format!("  {}", d.join("identity").display()))
                .collect();
            anyhow!(
                "stellar-cli identity '{}' not found. Searched:\n{}\n\
                 List identities with `stellar keys ls`, or set STELLAR_CONFIG_HOME.",
                name,
                searched.join("\n")
            )
        })?;

    let raw = Zeroizing::new(
        std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?,
    );
    let key = parse_identity(&raw)
        .with_context(|| format!("Cannot import stellar-cli identity {}", path.display()))?;

    Ok(StellarCliIdentity {
        name: name.to_string(),
        path,
        key,
    })
}

/// Parse the contents of a stellar-cli identity file.
///
/// Errors never include the file contents, which hold secret material.
pub fn parse_identity(raw: &str) -> Result<StellarCliKey> {
    let value: toml::Value = raw
        .parse()
        .map_err(|_| anyhow!("identity file is not valid TOML"))?;
    let table = value
        .as_table()
        .ok_or_else(|| anyhow!("identity file is not a TOML table"))?;

    let string_field = |key: &str| -> Result<Option<Zeroizing<String>>> {
        match table.get(key) {
            None => Ok(None),
            Some(toml::Value::String(s)) => Ok(Some(Zeroizing::new(s.trim().to_string()))),
            Some(_) => bail!("'{}' must be a string", key),
        }
    };

    if let Some(secret) = string_field("secret_key")? {
        return Ok(StellarCliKey::SecretKey(secret));
    }
    if let Some(phrase) = string_field("seed_phrase")? {
        return Ok(StellarCliKey::SeedPhrase(phrase));
    }
    if table.contains_key("public_key") {
        bail!(
            "this identity only stores a public key, so there is nothing to sign with. \
             Import the secret key instead, or use `starforge wallet import <name> --hardware ledger` for a Ledger"
        );
    }
    if table.keys().any(|k| k.contains("secure_store")) {
        bail!(
            "this identity is kept in the OS keychain (secure store), which StarForge cannot read. \
             Export it with `stellar keys show <name>` and use `starforge wallet import <name> --key`"
        );
    }
    if table.keys().any(|k| k.contains("ledger")) {
        bail!(
            "this is a Ledger identity. Use `starforge wallet import <name> --hardware ledger` instead"
        );
    }
    bail!("identity file has neither 'secret_key' nor 'seed_phrase'")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn parses_secret_key_identity() {
        let key = parse_identity("secret_key = \"SABC\"\n").unwrap();
        assert!(matches!(key, StellarCliKey::SecretKey(s) if s.as_str() == "SABC"));
    }

    #[test]
    fn parses_seed_phrase_identity() {
        let key = parse_identity("seed_phrase = \" abandon ability \"\n").unwrap();
        assert!(matches!(key, StellarCliKey::SeedPhrase(s) if s.as_str() == "abandon ability"));
    }

    #[test]
    fn rejects_public_key_only_identity() {
        let err = parse_identity("public_key = \"GABC\"\n").unwrap_err();
        assert!(err.to_string().contains("only stores a public key"));
    }

    #[test]
    fn rejects_unknown_identity_without_leaking_contents() {
        let err = parse_identity("secret = \"SSECRETVALUE\"\n").unwrap_err();
        assert!(!format!("{err:#}").contains("SSECRETVALUE"));
        let err = parse_identity("not toml = = SSECRETVALUE").unwrap_err();
        assert!(!format!("{err:#}").contains("SSECRETVALUE"));
    }

    #[test]
    fn rejects_path_traversal_names() {
        for bad in ["../evil", "a/b", "", "a b"] {
            assert!(load_identity(bad, &[]).is_err(), "{bad} should be rejected");
        }
    }

    #[test]
    fn local_directory_wins_over_global() {
        let local = tempdir().unwrap();
        let global = tempdir().unwrap();
        for (dir, secret) in [(&local, "SLOCAL"), (&global, "SGLOBAL")] {
            let id = dir.path().join("identity");
            std::fs::create_dir_all(&id).unwrap();
            std::fs::write(
                id.join("alice.toml"),
                format!("secret_key = \"{secret}\"\n"),
            )
            .unwrap();
        }
        let found = load_identity(
            "alice",
            &[local.path().to_path_buf(), global.path().to_path_buf()],
        )
        .unwrap();
        assert!(matches!(found.key, StellarCliKey::SecretKey(s) if s.as_str() == "SLOCAL"));
    }

    #[test]
    fn missing_identity_lists_searched_dirs() {
        let dir = tempdir().unwrap();
        let err = load_identity("nobody", &[dir.path().to_path_buf()]).unwrap_err();
        assert!(err.to_string().contains(&dir.path().display().to_string()));
    }

    #[test]
    fn stellar_config_home_replaces_global_dirs() {
        let dirs = search_dirs(
            Some(Path::new("/proj")),
            Some(Path::new("/home/u")),
            Some(PathBuf::from("/custom")),
            Some(PathBuf::from("/xdg")),
        );
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/proj/.stellar"),
                PathBuf::from("/custom"),
                PathBuf::from("/proj/.soroban"),
                PathBuf::from("/home/u/.config/soroban"),
            ]
        );
    }

    #[test]
    fn default_global_dirs_include_xdg_and_home() {
        let dirs = search_dirs(None, Some(Path::new("/home/u")), None, Some("/xdg".into()));
        assert_eq!(
            dirs,
            vec![
                PathBuf::from("/xdg/stellar"),
                PathBuf::from("/home/u/.config/stellar"),
                PathBuf::from("/home/u/.config/soroban"),
            ]
        );
    }
}
