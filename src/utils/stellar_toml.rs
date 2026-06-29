use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize, Default)]
pub struct StellarToml {
    #[serde(rename = "WEB_AUTH_ENDPOINT")]
    pub web_auth_endpoint: Option<String>,
    #[serde(rename = "TRANSFER_SERVER_SEP0024")]
    pub transfer_server_sep0024: Option<String>,
    #[serde(rename = "NETWORK_PASSPHRASE")]
    pub network_passphrase: Option<String>,
    #[serde(rename = "SIGNING_KEY")]
    pub signing_key: Option<String>,
}

pub fn fetch(domain: &str) -> Result<StellarToml> {
    let url = format!("https://{}/.well-known/stellar.toml", domain);
    let response = ureq::get(&url)
        .call()
        .with_context(|| format!("Failed to fetch stellar.toml from {}", url))?;
    let body = response
        .into_string()
        .context("Failed to read stellar.toml response body")?;
    let stellar_toml: StellarToml = toml::from_str(&body)
        .with_context(|| format!("Failed to parse stellar.toml from {}", domain))?;
    Ok(stellar_toml)
}
