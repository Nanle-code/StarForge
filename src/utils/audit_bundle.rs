use anyhow::Result;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::path::Path;

type HmacSha256 = Hmac<Sha256>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuditBundle {
    pub format_version: u32,
    pub generated_at: String,
    pub versions: Vec<String>,
    pub checksums: Vec<String>,
    pub deploy_history: Vec<String>,
    pub config_hashes: Vec<String>,
    pub signature: Option<String>,
}

impl AuditBundle {
    pub fn new(
        generated_at: impl Into<String>,
        versions: Vec<String>,
        checksums: Vec<String>,
        deploy_history: Vec<String>,
        config_hashes: Vec<String>,
        signing_key: Option<&[u8]>,
    ) -> Result<Self> {
        let mut bundle = Self {
            format_version: 1,
            generated_at: generated_at.into(),
            versions,
            checksums,
            deploy_history,
            config_hashes,
            signature: None,
        };
        bundle.redact();
        if let Some(key) = signing_key {
            let unsigned = serde_json::to_vec(&bundle)?;
            let mut mac = HmacSha256::new_from_slice(key)?;
            mac.update(&unsigned);
            bundle.signature = Some(
                mac.finalize()
                    .into_bytes()
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect(),
            );
        }
        Ok(bundle)
    }

    pub fn write_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_vec_pretty(self)?)?;
        Ok(())
    }

    fn redact(&mut self) {
        for field in [
            &mut self.versions,
            &mut self.checksums,
            &mut self.deploy_history,
            &mut self.config_hashes,
        ] {
            for value in field.iter_mut() {
                *value = redact(value);
            }
        }
    }
}

fn redact(value: &str) -> String {
    if value.contains("BEGIN ")
        || value.contains("PRIVATE KEY")
        || value.to_ascii_lowercase().contains("secret")
    {
        "[REDACTED]".to_string()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_private_material_and_can_sign() {
        let bundle = AuditBundle::new(
            "2026-08-26T00:00:00Z",
            vec!["starforge 1.0".into()],
            vec!["sha256:abc".into()],
            vec![
                "deploy testnet".into(),
                "-----BEGIN PRIVATE KEY-----".into(),
            ],
            vec!["secret=config-value".into()],
            Some(b"audit-key"),
        )
        .unwrap();
        let json = serde_json::to_string(&bundle).unwrap();
        assert!(!json.contains("PRIVATE KEY"));
        assert!(!json.contains("config-value"));
        assert!(bundle.signature.is_some());
    }
}
