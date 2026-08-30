# REPL Session Persistence and Replay

The interactive shell stores history at `~/.starforge/history` by default.
History can be disabled with `starforge shell --no-history`.

History entries are redacted before persistence when they contain values for
`--secret`, `--secret-key`, `--token`, `--api-key`, or their `name=value`
forms. Entries containing `[REDACTED]` are skipped by `:history replay`.

Use `:history replay` to replay recorded invocations. Read-only commands run
immediately; commands named `deploy`, `invoke`, `tx`, `upgrade`, or `migrate`
require an explicit `y` confirmation for every execution.
