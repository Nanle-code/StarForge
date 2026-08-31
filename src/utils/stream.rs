use crate::utils::http_client;
use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use stellar_xdr::curr::{Limited, Limits, ScSymbol, ScVal, WriteXdr};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

/// RPC and client-side filters for Soroban `getEvents`.
#[derive(Debug, Clone, Default)]
pub struct EventStreamFilters {
    pub event_type: Option<String>,
    pub topic_segments: Option<Vec<String>>,
    pub value_match: Option<String>,
}

/// Transport used for Soroban event streaming.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EventStreamTransport {
    /// Prefer a persistent WebSocket JSON-RPC connection, then fall back to HTTP polling.
    Auto,
    /// Use JSON-RPC over HTTP polling.
    #[default]
    Http,
    /// Use JSON-RPC over a persistent WebSocket connection.
    WebSocket,
}

impl EventStreamTransport {
    pub fn parse(value: &str) -> Result<Self> {
        match value.trim().to_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "http" | "poll" | "polling" => Ok(Self::Http),
            "ws" | "wss" | "websocket" => Ok(Self::WebSocket),
            other => anyhow::bail!(
                "unsupported event stream transport '{}'; use auto, websocket, or http",
                other
            ),
        }
    }
}

pub struct SorobanEventStream {
    rpc_url: String,
    websocket_url: String,
    contract_id: String,
    cursor: Option<String>,
    poll_interval: Duration,
    backoff: Backoff,
    filters: EventStreamFilters,
    transport: EventStreamTransport,
    websocket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    request_id: u64,
}

#[derive(Debug, Clone)]
struct Backoff {
    attempt: u32,
    base_ms: u64,
    max_ms: u64,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            attempt: 0,
            base_ms: 500,
            max_ms: 30_000,
        }
    }
}

impl Backoff {
    fn reset(&mut self) {
        self.attempt = 0;
    }

    fn next_delay(&mut self) -> Duration {
        let exp = self.attempt.min(6);
        self.attempt = self.attempt.saturating_add(1);
        let ms = (self.base_ms.saturating_mul(1_u64 << exp)).min(self.max_ms);
        Duration::from_millis(ms)
    }
}

impl SorobanEventStream {
    pub fn new(rpc_url: String, contract_id: String) -> Self {
        let websocket_url =
            websocket_url_from_rpc_url(&rpc_url).unwrap_or_else(|_| rpc_url.clone());
        Self {
            rpc_url,
            websocket_url,
            contract_id,
            cursor: None,
            poll_interval: Duration::from_secs(2),
            backoff: Backoff::default(),
            filters: EventStreamFilters::default(),
            transport: EventStreamTransport::default(),
            websocket: None,
            request_id: 0,
        }
    }

    pub fn with_poll_interval(mut self, seconds: u64) -> Self {
        self.poll_interval = Duration::from_secs(seconds.max(1));
        self
    }

    pub fn with_filters(mut self, filters: EventStreamFilters) -> Self {
        self.filters = filters;
        self
    }

    pub fn with_transport(mut self, transport: EventStreamTransport) -> Self {
        self.transport = transport;
        self
    }

    pub fn with_websocket_url(mut self, websocket_url: impl Into<String>) -> Self {
        self.websocket_url = websocket_url.into();
        self
    }

    pub fn with_event_type(mut self, event_type: impl Into<String>) -> Self {
        self.filters.event_type = Some(event_type.into());
        self
    }

    pub fn with_topic_segments(mut self, segments: Vec<String>) -> Self {
        self.filters.topic_segments = Some(segments);
        self
    }

    pub fn with_value_match(mut self, pattern: impl Into<String>) -> Self {
        self.filters.value_match = Some(pattern.into());
        self
    }

    pub fn transport(&self) -> EventStreamTransport {
        self.transport
    }

    pub fn websocket_url(&self) -> &str {
        &self.websocket_url
    }

    pub async fn next_batch(&mut self) -> Result<Vec<SorobanEvent>> {
        match self.transport {
            EventStreamTransport::Http => self.next_batch_http().await,
            EventStreamTransport::WebSocket => self.next_batch_websocket().await,
            EventStreamTransport::Auto => match self.next_batch_websocket().await {
                Ok(events) => Ok(events),
                Err(websocket_error) => {
                    self.websocket = None;
                    self.next_batch_http().await.with_context(|| {
                        format!(
                            "WebSocket stream failed ({}), and HTTP fallback also failed",
                            websocket_error
                        )
                    })
                }
            },
        }
    }

    async fn next_batch_http(&mut self) -> Result<Vec<SorobanEvent>> {
        let request = self.build_get_events_request();

        let client = http_client::get_client();
        let response: SorobanGetEventsResponse = client
            .post(&self.rpc_url)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("Soroban RPC request to {} failed", self.rpc_url))?
            .json()
            .await
            .with_context(|| "Failed to decode Soroban getEvents response")?;

        self.apply_response(response)
    }

    async fn next_batch_websocket(&mut self) -> Result<Vec<SorobanEvent>> {
        let request = self.build_get_events_request();
        let response = self.websocket_request(request).await?;
        self.apply_response(response)
    }

    pub async fn sleep(&self) {
        tokio::time::sleep(self.poll_interval).await;
    }

    pub async fn sleep_backoff(&mut self) {
        tokio::time::sleep(self.backoff.next_delay()).await;
    }

    fn build_get_events_request(&mut self) -> serde_json::Value {
        self.request_id = self.request_id.saturating_add(1).max(1);
        let filter = self.build_rpc_filter();
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": self.request_id,
            "method": "getEvents",
            "params": {
                "filters": [filter],
                "pagination": {
                    "cursor": self.cursor.clone(),
                    "limit": 50
                }
            }
        })
    }

    fn apply_response(&mut self, response: SorobanGetEventsResponse) -> Result<Vec<SorobanEvent>> {
        if let Some(error) = response.error {
            anyhow::bail!(
                "Soroban RPC getEvents failed: {}",
                error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("unknown error")
            );
        }

        let result = response
            .result
            .ok_or_else(|| anyhow::anyhow!("Soroban RPC getEvents returned no result"))?;

        self.cursor = result.cursor;
        self.backoff.reset();

        let events = result
            .events
            .into_iter()
            .filter(|event| event_matches_value(event, &self.filters))
            .collect();

        Ok(events)
    }

    async fn ensure_websocket(&mut self) -> Result<()> {
        if self.websocket.is_some() {
            return Ok(());
        }

        let (websocket, _) = connect_async(self.websocket_url.as_str())
            .await
            .with_context(|| {
                format!(
                    "failed to connect to Soroban RPC WebSocket {}",
                    self.websocket_url
                )
            })?;
        self.websocket = Some(websocket);
        Ok(())
    }

    async fn websocket_request(
        &mut self,
        request: serde_json::Value,
    ) -> Result<SorobanGetEventsResponse> {
        self.ensure_websocket().await?;

        let send_result = {
            let websocket = self
                .websocket
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("WebSocket connection is not available"))?;
            websocket.send(Message::Text(request.to_string())).await
        };

        if let Err(err) = send_result {
            self.websocket = None;
            return Err(err).with_context(|| "failed to send Soroban RPC WebSocket request");
        }

        loop {
            let message = {
                let websocket = self
                    .websocket
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("WebSocket connection is not available"))?;
                let maybe_message = tokio::time::timeout(Duration::from_secs(30), websocket.next())
                    .await
                    .with_context(|| "timed out waiting for Soroban RPC WebSocket response")?;
                maybe_message
                    .ok_or_else(|| anyhow::anyhow!("Soroban RPC WebSocket closed unexpectedly"))?
            };

            let message = match message {
                Ok(message) => message,
                Err(err) => {
                    self.websocket = None;
                    return Err(err).with_context(|| "Soroban RPC WebSocket read failed");
                }
            };

            match message {
                Message::Text(text) => {
                    if let Some(response) = decode_websocket_response(text.as_ref())? {
                        return Ok(response);
                    }
                }
                Message::Binary(bytes) => {
                    let text = std::str::from_utf8(bytes.as_ref())
                        .with_context(|| "Soroban RPC WebSocket returned non-UTF8 binary data")?;
                    if let Some(response) = decode_websocket_response(text)? {
                        return Ok(response);
                    }
                }
                Message::Ping(payload) => {
                    if let Some(websocket) = self.websocket.as_mut() {
                        websocket.send(Message::Pong(payload)).await?;
                    }
                }
                Message::Pong(_) => {}
                Message::Frame(_) => {}
                Message::Close(frame) => {
                    self.websocket = None;
                    anyhow::bail!("Soroban RPC WebSocket closed: {:?}", frame);
                }
            }
        }
    }

    fn build_rpc_filter(&self) -> serde_json::Value {
        let event_type = self.filters.event_type.as_deref().unwrap_or("contract");

        let mut filter = serde_json::json!({
            "type": event_type,
            "contractIds": [self.contract_id],
        });

        if let Some(ref segments) = self.filters.topic_segments {
            let encoded: Result<Vec<String>> =
                segments.iter().map(|s| encode_topic_segment(s)).collect();
            if let Ok(topic_row) = encoded {
                if !topic_row.is_empty() {
                    filter["topics"] = serde_json::json!([topic_row]);
                }
            }
        }

        filter
    }
}

pub fn websocket_url_from_rpc_url(rpc_url: &str) -> Result<String> {
    let trimmed = rpc_url.trim();
    if trimmed.starts_with("wss://") || trimmed.starts_with("ws://") {
        return Ok(trimmed.to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("https://") {
        return Ok(format!("wss://{}", rest));
    }
    if let Some(rest) = trimmed.strip_prefix("http://") {
        return Ok(format!("ws://{}", rest));
    }
    anyhow::bail!("cannot derive WebSocket URL from '{}'", rpc_url)
}

fn event_matches_value(event: &SorobanEvent, filters: &EventStreamFilters) -> bool {
    let Some(ref pattern) = filters.value_match else {
        return true;
    };
    if pattern.is_empty() {
        return true;
    }
    let haystack = event.value.to_string().to_lowercase();
    haystack.contains(&pattern.to_lowercase())
}

fn encode_topic_segment(segment: &str) -> Result<String> {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        anyhow::bail!("topic segment cannot be empty");
    }
    if trimmed == "*" || trimmed == "**" {
        return Ok(trimmed.to_string());
    }
    if looks_like_base64(trimmed) {
        return Ok(trimmed.to_string());
    }

    let symbol = ScSymbol(
        trimmed
            .as_bytes()
            .try_into()
            .with_context(|| format!("invalid topic symbol '{}'", trimmed))?,
    );
    let scval = ScVal::Symbol(symbol);
    let mut bytes = Vec::new();
    scval.write_xdr(&mut Limited::new(&mut bytes, Limits::none()))?;
    Ok(BASE64.encode(bytes))
}

fn looks_like_base64(value: &str) -> bool {
    value.len() >= 8
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '/' || c == '=')
}

fn decode_websocket_response(text: &str) -> Result<Option<SorobanGetEventsResponse>> {
    let value: serde_json::Value = serde_json::from_str(text)
        .with_context(|| "failed to decode Soroban RPC WebSocket JSON message")?;

    if value.get("result").is_some() || value.get("error").is_some() {
        return Ok(Some(serde_json::from_value(value)?));
    }

    if let Some(result) = value.pointer("/params/result") {
        return Ok(Some(serde_json::from_value(serde_json::json!({
            "result": result.clone()
        }))?));
    }

    if let Some(result) = value.get("params") {
        if result.get("events").is_some() || result.get("cursor").is_some() {
            return Ok(Some(serde_json::from_value(serde_json::json!({
                "result": result.clone()
            }))?));
        }
    }

    // Ignore unrelated subscription acknowledgements or keep-alive messages.
    Ok(None)
}

#[derive(Debug, Deserialize)]
struct SorobanGetEventsResponse {
    result: Option<SorobanGetEventsResult>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct SorobanGetEventsResult {
    cursor: Option<String>,
    events: Vec<SorobanEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SorobanEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub ledger: u32,
    pub id: String,
    #[serde(default)]
    pub topic: Vec<String>,
    pub value: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::net::TcpListener;
    use tokio_tungstenite::accept_async;

    #[test]
    fn derives_websocket_urls_from_http_rpc_urls() {
        assert_eq!(
            websocket_url_from_rpc_url("https://soroban-testnet.stellar.org").unwrap(),
            "wss://soroban-testnet.stellar.org"
        );
        assert_eq!(
            websocket_url_from_rpc_url("http://localhost:8000/rpc").unwrap(),
            "ws://localhost:8000/rpc"
        );
        assert_eq!(
            websocket_url_from_rpc_url("wss://example.test/rpc").unwrap(),
            "wss://example.test/rpc"
        );
    }

    #[test]
    fn decodes_subscription_style_websocket_events() {
        let message = json!({
            "method": "events",
            "params": {
                "cursor": "abc",
                "events": [{
                    "type": "contract",
                    "ledger": 7,
                    "id": "event-1",
                    "topic": ["topic"],
                    "value": {"ok": true}
                }]
            }
        });

        let response = decode_websocket_response(&message.to_string())
            .unwrap()
            .unwrap();
        let result = response.result.unwrap();
        assert_eq!(result.cursor.as_deref(), Some("abc"));
        assert_eq!(result.events.len(), 1);
    }

    #[test]
    fn decodes_direct_result_websocket_events() {
        let message = json!({
            "jsonrpc": "2.0",
            "result": {
                "cursor": "xyz",
                "events": [{
                    "type": "contract",
                    "ledger": 9,
                    "id": "event-2",
                    "topic": ["mint"],
                    "value": {"ok": true}
                }]
            }
        });

        let response = decode_websocket_response(&message.to_string())
            .unwrap()
            .unwrap();
        let result = response.result.unwrap();
        assert_eq!(result.cursor.as_deref(), Some("xyz"));
        assert_eq!(result.events[0].id, "event-2");
    }

    #[test]
    fn websocket_url_derivation_rejects_unrelated_inputs() {
        assert!(websocket_url_from_rpc_url("notaurl").is_err());
    }

    #[tokio::test]
    async fn websocket_transport_round_trips_get_events() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            let message = socket.next().await.unwrap().unwrap();
            let request: serde_json::Value =
                serde_json::from_str(message.to_text().unwrap()).unwrap();
            assert_eq!(request["method"], "getEvents");
            socket
                .send(Message::Text(
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "result": {
                            "cursor": "mock-cursor",
                            "events": [{
                                "type": "contract",
                                "ledger": 12,
                                "id": "mock-event",
                                "topic": ["mint"],
                                "value": {"amount": 10}
                            }]
                        }
                    })
                    .to_string()
                    .into(),
                ))
                .await
                .unwrap();
        });

        let mut stream = SorobanEventStream::new(format!("http://{}", address), "C123".to_string())
            .with_transport(EventStreamTransport::WebSocket)
            .with_websocket_url(format!("ws://{}", address));
        let events = stream.next_batch().await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, "mock-event");
        server.await.unwrap();
    }
}
