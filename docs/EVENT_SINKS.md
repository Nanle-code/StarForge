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
    last_ledger BIGINT NOT NULL DEFAULT 0,
    cursor_id TEXT
);
`

### NDJSON File Sink

Appends events as Newline-Delimited JSON (NDJSON) to a specified file. 

`	oml
[event_sinks.ndjson]
file_path = "events.ndjson"
cursor_file = "events.cursor" # Optional
`

## Filtered subscriptions, cursors, and delivery semantics

`starforge monitor <CONTRACT>` subscribes to that contract's events. Use `--topic`
to restrict the RPC subscription by topic, with `--event-type` and `--value`
providing additional filters. Cursors are the opaque pagination tokens returned
by Soroban RPC; they are not ledger numbers or event IDs. File cursors are stored
as versioned JSON with a SHA-256 integrity checksum and replaced atomically.
Malformed or modified cursor files fail with an explicit error. Postgres stores
the same opaque token in `cursor_id`.

On restart the monitor resumes from the stored token. If multiple sinks are
configured, their stored cursors must match; the monitor refuses to choose an
arbitrary token when they differ. Keep a cursor store with the same contract
and subscription filters: the cursor is not portable across changed filters or
RPC providers.

Delivery is **at least once**. A crash after a sink accepts a batch but before
the cursor is saved can replay that batch. Consumers should deduplicate using
the event `id`. Exactly-once delivery is not guaranteed: sink writes and local
cursor persistence are not one atomic transaction. RPC retention limits may
also prevent resuming a cursor that has expired.
