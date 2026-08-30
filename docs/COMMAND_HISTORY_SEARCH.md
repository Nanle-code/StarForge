# Command History Search

The `utils::history_search` query API filters persisted command history by
command text, `--network`, correlation ID, and inclusive UTC time bounds.
Returned commands pass through the same secret redaction used by the REPL.

Applications embedding StarForge can compose these filters to investigate a
deploy or invoke without exposing tokens in search output.
