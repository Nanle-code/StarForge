# Event Sinks

StarForge provides pluggable event sinks that allow you to turn the event stream into a lightweight indexer for small projects. You can forward events to a Webhook (with HMAC signatures), a Postgres database, or an NDJSON file. 

## Configuration

Event sinks are configured via your project manifest (starforge-project.toml).

### Webhook Sink

Sends an HTTP POST to the specified URL with a JSON payload of matching events. The request includes an X-Signature header with an HMAC-SHA256 signature of the payload.

`	oml
[event_sinks.webhook]
url = "https://example.com/webhook"
hmac_secret = "your_secret_key"
cursor_file = ".starforge/webhook_cursor" # Optional
`

### Postgres Sink

Inserts events directly into a Postgres database. StarForge will automatically create the required schema (events and event_cursors tables) if they do not exist.

`	oml
[event_sinks.postgres]
connection_string = "postgresql://user:password@localhost/dbname"
sink_id = "my_project_sink"
`

#### Schema
The following schema is provided and automatically created:

`sql
CREATE TABLE IF NOT EXISTS events (
    id TEXT PRIMARY KEY,
    ledger BIGINT NOT NULL,
    event_type TEXT NOT NULL,
    topic TEXT[] NOT NULL,
    value JSONB NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS event_cursors (
    sink_id TEXT PRIMARY KEY,
    cursor_id TEXT NOT NULL
);
`

### NDJSON File Sink

Appends events as Newline-Delimited JSON (NDJSON) to a specified file. 

`	oml
[event_sinks.ndjson]
file_path = "events.ndjson"
cursor_file = "events.cursor" # Optional
`

## At-Least-Once Delivery and Cursors

All sinks guarantee at-least-once delivery with durable cursors. When you run starforge monitor, the CLI reads the highest cursor stored across all configured sinks and automatically resumes the stream from that ledger point, ensuring no events are missed across restarts.
