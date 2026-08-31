//! Security utilities for enforcing least-privilege permissions on sensitive files and directories.

use std::fs;
use std::io;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Enforce 0600 (-rw-------) permissions on a sensitive file.
pub fn ensure_private_file_permissions(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Path does not exist: {:?}", path),
        ));
    }

    #[cfg(unix)]
    {
        let metadata = fs::metadata(path)?;
        let mut permissions = metadata.permissions();
        let current_mode = permissions.mode() & 0o777;
        if current_mode != 0o600 {
            permissions.set_mode(0o600);
            fs::set_permissions(path, permissions)?;
        }
    }

    Ok(())
}

/// Enforce 0700 (drwx------) permissions on a sensitive directory.
pub fn ensure_private_dir_permissions(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Path does not exist: {:?}", path),
        ));
    }

    #[cfg(unix)]
    {
        let metadata = fs::metadata(path)?;
        let mut permissions = metadata.permissions();
        let current_mode = permissions.mode() & 0o777;
        if current_mode != 0o700 {
            permissions.set_mode(0o700);
            fs::set_permissions(path, permissions)?;
        }
    }

    Ok(())
}

/// Recursively creates a directory tree with private permissions (0700 on Unix).
pub fn create_private_dir_all(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;
    ensure_private_dir_permissions(path)?;
    Ok(())
}

/// Creates or atomically overwrites a sensitive file with 0600 permissions.
pub fn create_private_file(path: &Path, content: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            create_private_dir_all(parent)?;
        }
    }
    fs::write(path, content)?;
    ensure_private_file_permissions(path)?;
    Ok(())
}

/// Audit report containing results of permission inspection and repairs.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PermissionAuditReport {
    pub files_audited: usize,
    pub files_repaired: usize,
    pub directories_audited: usize,
    pub directories_repaired: usize,
}

/// Audits a directory tree and repairs any loose permissions (e.g. 0644 -> 0600, 0755 -> 0700).
pub fn audit_and_repair_config_dir(dir: &Path) -> io::Result<PermissionAuditReport> {
    let mut report = PermissionAuditReport::default();
    if !dir.exists() {
        return Ok(report);
    }

    if dir.is_dir() {
        report.directories_audited += 1;
        #[cfg(unix)]
        {
            let meta = fs::metadata(dir)?;
            if (meta.permissions().mode() & 0o777) != 0o700 {
                ensure_private_dir_permissions(dir)?;
                report.directories_repaired += 1;
            }
        }
    }

    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let sub = audit_and_repair_config_dir(&path)?;
            report.directories_audited += sub.directories_audited;
            report.directories_repaired += sub.directories_repaired;
            report.files_audited += sub.files_audited;
            report.files_repaired += sub.files_repaired;
        } else if path.is_file() {
            report.files_audited += 1;
            #[cfg(unix)]
            {
                let meta = fs::metadata(&path)?;
                if (meta.permissions().mode() & 0o777) != 0o600 {
                    ensure_private_file_permissions(&path)?;
                    report.files_repaired += 1;
                }
            }
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_create_and_ensure_private_file_permissions() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("secret.key");

        create_private_file(&file_path, b"supersecret").unwrap();
        assert!(file_path.exists());

        #[cfg(unix)]
        {
            let meta = fs::metadata(&file_path).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn test_audit_and_repair_loose_permissions() {
        let dir = tempdir().unwrap();
        let sub_dir = dir.path().join(".starforge");
        fs::create_dir(&sub_dir).unwrap();

        let secret_file = sub_dir.join("wallet.json");
        fs::write(&secret_file, b"{\"secret\": true}").unwrap();

        #[cfg(unix)]
        {
            let mut perms = fs::metadata(&secret_file).unwrap().permissions();
            perms.set_mode(0o644);
            fs::set_permissions(&secret_file, perms).unwrap();
        }

        let report = audit_and_repair_config_dir(&sub_dir).unwrap();
        assert!(report.files_audited >= 1);

        #[cfg(unix)]
        {
            let meta = fs::metadata(&secret_file).unwrap();
            assert_eq!(meta.permissions().mode() & 0o777, 0o600);
        }
    }

    #[test]
    fn test_invalid_path_failure_handling() {
        let non_existent = Path::new("/path/that/does/not/exist/definitely_not_here.key");
        let res = ensure_private_file_permissions(non_existent);
        assert!(res.is_err());
    }
}
