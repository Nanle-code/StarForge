use crate::utils::http_client;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Configuration for the remote template registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// URL of the remote registry API (e.g., https://registry.starforge.dev)
    pub url: String,
    /// API token for authentication (set when user publishes templates)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    /// User's registry username
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    /// Email associated with the registry account
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            url: "https://registry.starforge.dev".to_string(),
            token: None,
            username: None,
            email: None,
        }
    }
}

/// Request payload for publishing a template to the remote registry.
#[derive(Debug, Serialize)]
pub struct PublishTemplateRequest {
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub documentation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_version_min: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cli_version_max: Option<String>,
    /// Base64-encoded template archive (.zip)
    pub content: String,
}

/// Response from publishing a template.
#[derive(Debug, Deserialize)]
pub struct PublishTemplateResponse {
    pub success: bool,
    pub message: String,
    pub template_id: Option<String>,
    pub url: Option<String>,
}

/// Request for user authentication/login.
#[derive(Debug, Serialize)]
pub struct AuthRequest {
    pub email: String,
    pub password: String,
}

/// Response from authentication endpoint.
#[derive(Debug, Deserialize)]
pub struct AuthResponse {
    pub success: bool,
    pub message: String,
    pub token: Option<String>,
    pub username: Option<String>,
}

/// Request to search remote registry.
#[derive(Debug, Serialize)]
pub struct SearchRequest {
    pub query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_quality: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
}

/// Response from remote search.
#[derive(Debug, Deserialize, Clone)]
pub struct SearchResponse {
    pub success: bool,
    pub results: Vec<RemoteTemplateEntry>,
    pub total: u32,
    pub limit: u32,
    pub offset: u32,
}

/// Template entry from remote registry (may have additional remote-specific fields).
#[derive(Debug, Deserialize, Clone, Serialize)]
pub struct RemoteTemplateEntry {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub author: String,
    pub tags: Vec<String>,
    pub license: Option<String>,
    pub repository: Option<String>,
    pub homepage: Option<String>,
    pub documentation: Option<String>,
    pub downloads: u32,
    pub verified: bool,
    pub created_at: String,
    pub updated_at: String,
    pub ratings: TemplateRatings,
    pub download_url: String,
    #[serde(default)]
    pub expected_sha256: Option<String>,
}

/// Rating and review statistics for a template.
#[derive(Debug, Deserialize, Clone, Serialize, Default)]
pub struct TemplateRatings {
    pub average_rating: f32,
    pub review_count: u32,
    pub five_star: u32,
    pub four_star: u32,
    pub three_star: u32,
    pub two_star: u32,
    pub one_star: u32,
}

/// Request to post a review for a template.
#[derive(Debug, Serialize)]
pub struct ReviewRequest {
    pub template_id: String,
    pub rating: u8, // 1-5
    pub comment: Option<String>,
}

/// Response from posting a review.
#[derive(Debug, Deserialize)]
pub struct ReviewResponse {
    pub success: bool,
    pub message: String,
}

/// Remote registry client for API interactions.
pub struct RegistryClient {
    registry_url: String,
    token: Option<String>,
}

impl RegistryClient {
    pub fn new(url: String, token: Option<String>) -> Self {
        Self {
            registry_url: url,
            token,
        }
    }

    /// Helper to make authenticated HTTP requests.
    fn build_headers(&self) -> Vec<(String, String)> {
        let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
        if let Some(ref token) = self.token {
            headers.push(("Authorization".to_string(), format!("Bearer {}", token)));
        }
        headers
    }

    /// Search the remote registry.
    pub async fn search(&self, req: &SearchRequest) -> Result<SearchResponse> {
        let url = format!("{}/api/templates/search", self.registry_url);
        let client = http_client::get_client();

        let mut http_req = client.post(&url).json(req);
        for (key, value) in self.build_headers() {
            http_req = http_req.header(&key, &value);
        }

        let resp = http_req
            .send()
            .await
            .with_context(|| format!("Failed to search remote registry at {}", url))?;

        if resp.status() != 200 {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Search failed with status {}: {}", status, body);
        }

        let result: SearchResponse = resp.json().await?;
        Ok(result)
    }

    /// Get details of a specific template.
    pub async fn get_template(
        &self,
        name: &str,
        version: Option<&str>,
    ) -> Result<RemoteTemplateEntry> {
        let version_param = version.unwrap_or("latest");
        let url = format!(
            "{}/api/templates/{}/{}",
            self.registry_url, name, version_param
        );

        let resp = http_client::get_client()
            .get(&url)
            .send()
            .await
            .with_context(|| format!("Failed to fetch template from remote registry: {}", url))?;

        if resp.status() != 200 {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Template not found with status {}: {}", status, body);
        }

        let result: RemoteTemplateEntry = resp.json().await?;
        Ok(result)
    }

    /// Publish a template to the remote registry.
    pub async fn publish(&self, req: &PublishTemplateRequest) -> Result<PublishTemplateResponse> {
        let url = format!("{}/api/templates/publish", self.registry_url);
        let client = http_client::get_client();

        let mut http_req = client.post(&url).json(req);
        for (key, value) in self.build_headers() {
            http_req = http_req.header(&key, &value);
        }

        let resp = http_req
            .send()
            .await
            .with_context(|| format!("Failed to publish template to {}", url))?;

        let result: PublishTemplateResponse = resp.json().await?;
        Ok(result)
    }

    /// Authenticate with the remote registry.
    pub async fn authenticate(&self, email: &str, password: &str) -> Result<AuthResponse> {
        let url = format!("{}/api/auth/login", self.registry_url);
        let req = AuthRequest {
            email: email.to_string(),
            password: password.to_string(),
        };

        let resp = http_client::get_client()
            .post(&url)
            .json(&req)
            .send()
            .await
            .with_context(|| format!("Failed to authenticate with remote registry at {}", url))?;

        if resp.status() != 200 {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Authentication failed with status {}: {}", status, body);
        }

        let result: AuthResponse = resp.json().await?;
        Ok(result)
    }

    /// Sign up for a new registry account.
    pub async fn signup(
        &self,
        email: &str,
        username: &str,
        password: &str,
    ) -> Result<AuthResponse> {
        let url = format!("{}/api/auth/signup", self.registry_url);
        let req = serde_json::json!({
            "email": email,
            "username": username,
            "password": password
        });

        let resp = http_client::get_client()
            .post(&url)
            .json(&req)
            .send()
            .await
            .with_context(|| format!("Failed to sign up for remote registry at {}", url))?;

        if resp.status() != 201 {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Signup failed with status {}: {}", status, body);
        }

        let result: AuthResponse = resp.json().await?;
        Ok(result)
    }

    /// Download a template archive from the remote registry.
    pub async fn download_template(
        &self,
        download_url: &str,
        expected_sha256: Option<&str>,
    ) -> Result<Vec<u8>> {
        let resp = http_client::get_client()
            .get(download_url)
            .send()
            .await
            .with_context(|| format!("Failed to download template from {}", download_url))?;

        if resp.status() != 200 {
            anyhow::bail!(
                "Download failed with status {} for {}",
                resp.status(),
                download_url
            );
        }

        let bytes = resp.bytes().await?;
        if let Some(expected_hex) = expected_sha256 {
            crate::utils::templates::verify_archive_checksum(&bytes, expected_hex)?;
        }
        Ok(bytes.to_vec())
    }

    /// Post a review/rating for a template.
    pub async fn post_review(
        &self,
        template_id: &str,
        rating: u8,
        comment: Option<&str>,
    ) -> Result<ReviewResponse> {
        let url = format!(
            "{}/api/templates/{}/reviews",
            self.registry_url, template_id
        );
        let req = ReviewRequest {
            template_id: template_id.to_string(),
            rating,
            comment: comment.map(str::to_string),
        };
        let client = http_client::get_client();

        let mut http_req = client.post(&url).json(&req);
        for (key, value) in self.build_headers() {
            http_req = http_req.header(&key, &value);
        }

        let resp = http_req
            .send()
            .await
            .with_context(|| format!("Failed to post review to {}", url))?;

        let result: ReviewResponse = resp.json().await?;
        Ok(result)
    }
}

/// Load registry configuration from ~/.starforge/registry.toml
pub fn load_registry_config() -> Result<RegistryConfig> {
    let config_path = crate::utils::config::config_dir().join("registry.toml");

    if config_path.exists() {
        let contents = fs::read_to_string(&config_path)?;
        toml::from_str(&contents).with_context(|| {
            format!(
                "Failed to parse registry config at {}",
                config_path.display()
            )
        })
    } else {
        Ok(RegistryConfig::default())
    }
}

/// Save registry configuration to ~/.starforge/registry.toml
pub fn save_registry_config(config: &RegistryConfig) -> Result<()> {
    let dir = crate::utils::config::config_dir();
    let config_path = dir.join("registry.toml");

    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }

    let contents = toml::to_string_pretty(config)?;
    fs::write(&config_path, contents).with_context(|| {
        format!(
            "Failed to write registry config to {}",
            config_path.display()
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_config_default() {
        let config = RegistryConfig::default();
        assert_eq!(config.url, "https://registry.starforge.dev");
        assert_eq!(config.token, None);
    }

    #[test]
    fn test_search_request_serialization() {
        let req = SearchRequest {
            query: "counter".to_string(),
            tags: Some(vec!["example".to_string()]),
            verified: Some(true),
            min_quality: Some(50),
            limit: Some(20),
            offset: Some(0),
        };

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("counter"));
        assert!(json.contains("example"));
    }
}
