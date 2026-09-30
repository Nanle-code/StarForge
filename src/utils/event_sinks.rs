use crate::utils::event_monitoring::PersistedEvent;
use crate::utils::stream::SorobanEvent;
use anyhow::{Context, Result};
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
type HmacSha256 = Hmac<Sha256>;
use tokio_postgres::NoTls;

#[async_trait]
pub trait EventSink: Send + Sync {
    async fn process_batch(&mut self, events: &[SorobanEvent]) -> Result<()>;
    async fn get_cursor(&self) -> Result<Option<String>>;
    async fn save_cursor(&mut self, cursor: String) -> Result<()>;
}

// Config structs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventSinksConfig {
    #[serde(default)]
    pub webhook: Option<WebhookSinkConfig>,
    #[serde(default)]
    pub postgres: Option<PostgresSinkConfig>,
    #[serde(default)]
    pub ndjson: Option<NdjsonSinkConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebhookSinkConfig {
    pub url: String,
    pub hmac_secret: String,
    pub cursor_file: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PostgresSinkConfig {
    pub connection_string: String,
    pub sink_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NdjsonSinkConfig {
    pub file_path: String,
    pub cursor_file: Option<String>,
}

pub struct FileCursorStore {
    file_path: PathBuf,
}

impl FileCursorStore {
    pub fn new(path: PathBuf) -> Self {
        Self { file_path: path }
    }

    pub fn get_cursor(&self) -> Result<Option<String>> {
        if !self.file_path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&self.file_path)
            .with_context(|| format!("failed to read cursor file {}", self.file_path.display()))?;
        let record: CursorRecord = serde_json::from_str(&content)
            .with_context(|| format!("invalid cursor file {}", self.file_path.display()))?;
        if record.version != 1 || record.checksum != cursor_checksum(&record.cursor)? {
            anyhow::bail!(
                "invalid cursor file {}: integrity check failed",
                self.file_path.display()
            );
        }
        Ok(Some(record.cursor))
    }

    pub fn save_cursor(&self, cursor: &str) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let record = CursorRecord {
            version: 1,
            cursor: cursor.to_owned(),
            checksum: cursor_checksum(cursor)?,
        };
        let temp = self.file_path.with_extension("cursor.tmp");
        fs::write(&temp, serde_json::to_vec(&record)?)?;
        fs::rename(&temp, &self.file_path)?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct CursorRecord {
    version: u8,
    cursor: String,
    checksum: String,
}

fn cursor_checksum(cursor: &str) -> Result<String> {
    use sha2::Digest;
    Ok(hex::encode(sha2::Sha256::digest(cursor.as_bytes())))
}

pub struct NdjsonSink {
    file_path: PathBuf,
    cursor_store: FileCursorStore,
}

impl NdjsonSink {
    pub fn new(config: &NdjsonSinkConfig) -> Self {
        let file_path = PathBuf::from(&config.file_path);
        let cursor_path = if let Some(ref c) = config.cursor_file {
            PathBuf::from(c)
        } else {
            file_path.with_extension("cursor")
        };
        Self {
            file_path,
            cursor_store: FileCursorStore::new(cursor_path),
        }
    }
}

#[async_trait]
impl EventSink for NdjsonSink {
    async fn process_batch(&mut self, events: &[SorobanEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.file_path)?;
        for event in events {
            let json = serde_json::to_string(event)?;
            writeln!(file, "{}", json)?;
        }
        Ok(())
    }

    async fn get_cursor(&self) -> Result<Option<String>> {
        self.cursor_store.get_cursor()
    }

    async fn save_cursor(&mut self, cursor: String) -> Result<()> {
        self.cursor_store.save_cursor(&cursor)
    }
}

pub struct WebhookSink {
    client: Client,
    url: String,
    hmac_secret: String,
    cursor_store: FileCursorStore,
}

impl WebhookSink {
    pub fn new(config: &WebhookSinkConfig) -> Self {
        let cursor_path = if let Some(ref c) = config.cursor_file {
            PathBuf::from(c)
        } else {
            crate::utils::config::config_dir().join("webhook_cursor")
        };
        Self {
            client: Client::new(),
            url: config.url.clone(),
            hmac_secret: config.hmac_secret.clone(),
            cursor_store: FileCursorStore::new(cursor_path),
        }
    }
}

#[async_trait]
impl EventSink for WebhookSink {
    async fn process_batch(&mut self, events: &[SorobanEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }

        let payload = serde_json::to_string(events)?;

        let mut mac = HmacSha256::new_from_slice(self.hmac_secret.as_bytes())
            .context("Invalid HMAC secret")?;
        mac.update(payload.as_bytes());
        let signature = hex::encode(mac.finalize().into_bytes());

        let res = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("X-Signature", signature)
            .body(payload)
            .send()
            .await?;

        if !res.status().is_success() {
            anyhow::bail!("Webhook returned error status: {}", res.status());
        }
        Ok(())
    }

    async fn get_cursor(&self) -> Result<Option<String>> {
        self.cursor_store.get_cursor()
    }

    async fn save_cursor(&mut self, cursor: String) -> Result<()> {
        self.cursor_store.save_cursor(&cursor)
    }
}

pub struct PostgresSink {
    client: tokio_postgres::Client,
    sink_id: String,
}

impl PostgresSink {
    pub async fn new(config: &PostgresSinkConfig) -> Result<Self> {
        let (client, connection) =
            tokio_postgres::connect(&config.connection_string, NoTls).await?;
        tokio::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("connection error: {}", e);
            }
        });

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                ledger BIGINT NOT NULL,
                event_type TEXT NOT NULL,
                topic TEXT[] NOT NULL,
                value JSONB NOT NULL,
                created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
            )",
                &[],
            )
            .await?;

        client
            .execute(
                "CREATE TABLE IF NOT EXISTS event_cursors (
                sink_id TEXT PRIMARY KEY,
                last_ledger BIGINT NOT NULL DEFAULT 0,
                cursor_id TEXT
            )",
                &[],
            )
            .await?;
        client
            .execute(
                "ALTER TABLE event_cursors ADD COLUMN IF NOT EXISTS cursor_id TEXT",
                &[],
            )
            .await?;

        Ok(Self {
            client,
            sink_id: config.sink_id.clone(),
        })
    }
}

#[async_trait]
impl EventSink for PostgresSink {
    async fn process_batch(&mut self, events: &[SorobanEvent]) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let tx = self.client.transaction().await?;
        for event in events {
            let topic: Vec<&str> = event.topic.iter().map(|s| s.as_str()).collect();
            let value = serde_json::to_value(&event.value)?;
            tx.execute(
                "INSERT INTO events (id, ledger, event_type, topic, value) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (id) DO NOTHING",
                &[&event.id, &(event.ledger as i64), &event.event_type, &topic, &value],
            ).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    async fn get_cursor(&self) -> Result<Option<String>> {
        let row = self
            .client
            .query_opt(
                "SELECT cursor_id FROM event_cursors WHERE sink_id = $1 AND cursor_id IS NOT NULL",
                &[&self.sink_id],
            )
            .await?;
        if let Some(row) = row {
            let cursor: String = row.get(0);
            Ok(Some(cursor))
        } else {
            Ok(None)
        }
    }
    async fn save_cursor(&mut self, cursor: String) -> Result<()> {
        self.client.execute(
            "INSERT INTO event_cursors (sink_id, last_ledger, cursor_id) VALUES ($1, 0, $2) ON CONFLICT (sink_id) DO UPDATE SET cursor_id = EXCLUDED.cursor_id",
            &[&self.sink_id, &cursor],
        ).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_cursor_survives_restart_and_detects_tampering() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("events.cursor");
        FileCursorStore::new(path.clone())
            .save_cursor("opaque-rpc-cursor")
            .unwrap();
        assert_eq!(
            FileCursorStore::new(path.clone())
                .get_cursor()
                .unwrap()
                .as_deref(),
            Some("opaque-rpc-cursor")
        );

        let mut record: CursorRecord = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record.cursor = "changed".to_owned();
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        let error = FileCursorStore::new(path).get_cursor().unwrap_err();
        assert!(error.to_string().contains("integrity check failed"));
    }
}
