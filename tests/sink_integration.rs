use anyhow::Result;
use starforge::utils::event_sinks::{
    EventSink, NdjsonSink, NdjsonSinkConfig, WebhookSink, WebhookSinkConfig,
};
use starforge::utils::stream::SorobanEvent;
use std::fs;
use tempfile::tempdir;

#[tokio::test]
async fn test_ndjson_sink_integration() -> Result<()> {
    let dir = tempdir()?;
    let path = dir.path().join("events.ndjson");
    let config = NdjsonSinkConfig {
        file_path: path.to_string_lossy().to_string(),
        cursor_file: None,
    };
    let mut sink = NdjsonSink::new(&config);

    let event = SorobanEvent {
        event_type: "contract".to_string(),
        ledger: 100,
        id: "0000000100-0000000001".to_string(),
        topic: vec!["transfer".to_string()],
        value: serde_json::json!({"amount": 5}),
    };

    sink.process_batch(&[event.clone()]).await?;
    sink.save_cursor(event.id.clone()).await?;

    let cursor = sink.get_cursor().await?;
    assert_eq!(cursor, Some(event.id));

    let content = fs::read_to_string(path)?;
    assert!(content.contains("0000000100-0000000001"));
    Ok(())
}

#[tokio::test]
async fn test_webhook_sink_config() {
    let config = WebhookSinkConfig {
        url: "http://localhost:9999".to_string(),
        hmac_secret: "secret".to_string(),
        cursor_file: None,
    };
    let sink = WebhookSink::new(&config);
}
