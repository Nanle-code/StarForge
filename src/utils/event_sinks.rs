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

    pub fn get_cursor(&self) -> Result<Option<u32>> {
        if !self.file_path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&self.file_path)?;
        let ledger: u32 = content.trim().parse()?;
        Ok(Some(ledger))
    }

    pub fn save_cursor(&self, ledger: u32) -> Result<()> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&self.file_path, ledger.to_string())?;
        Ok(())
    }
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

    async fn get_cursor(&self) -> Result<Option<u32>> {
        self.cursor_store.get_cursor()
    }

    async fn save_cursor(&mut self, ledger: u32) -> Result<()> {
        self.cursor_store.save_cursor(ledger)
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

    async fn get_cursor(&self) -> Result<Option<u32>> {
        self.cursor_store.get_cursor()
    }

    async fn save_cursor(&mut self, ledger: u32) -> Result<()> {
        self.cursor_store.save_cursor(ledger)
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
                last_ledger BIGINT NOT NULL
            )",
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
                "INSERT INTO events (id, ledger, event_type, topic, value) VALUES (, , , , ) ON CONFLICT (id) DO NOTHING",
                &[&event.id, &(event.ledger as i64), &event.event_type, &topic, &value],
            ).await?;
        }
        tx.commit().await?;
        Ok(())
    }
    async fn get_cursor(&self) -> Result<Option<u32>> {
        let row = self
            .client
            .query_opt(
                "SELECT last_ledger FROM event_cursors WHERE sink_id = ",
                &[&self.sink_id],
            )
            .await?;
        if let Some(row) = row {
            let ledger: i64 = row.get(0);
            Ok(Some(ledger as u32))
        } else {
            Ok(None)
        }
    }
    async fn save_cursor(&mut self, ledger: u32) -> Result<()> {
        self.client.execute(
            "INSERT INTO event_cursors (sink_id, last_ledger) VALUES (, ) ON CONFLICT (sink_id) DO UPDATE SET last_ledger = ",
            &[&self.sink_id, &(ledger as i64)],
        ).await?;
        Ok(())
    }
}
