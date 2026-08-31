# Confirmation UX and Destructive Action Safety

StarForge uses layered confirmation prompts for operations that move real funds,
reveal secrets, or deploy to mainnet. This document covers challenge phrases,
automation bypass rules, and audit logging.

---

## Risk levels

| Level | Typical operations | Prompt style |
|-------|-------------------|--------------|
| Low | Read-only previews, dry-runs | `[y/N]` |
| Medium | Testnet deploys, non-destructive writes | `[y/N]` |
| High / destructive | Mainnet deploy, secret reveal, account merge | Typed challenge phrase |

---

## Challenge phrases

Destructive actions require typing an **exact, case-sensitive** phrase. Whitespace
at the start or end is trimmed; shortcuts like `yes` or pasted multiline text are
rejected.

| Action | Phrase | Commands |
|--------|--------|----------|
| Mainnet deploy | `deploy-mainnet` | `starforge deploy --network mainnet --execute` |
| Secret reveal | `reveal-secret` | `starforge wallet show <NAME> --reveal` |
| Account merge | wallet name (override) | `starforge wallet merge --from …` |
| Mainnet transaction | `send-mainnet` | `starforge tx send --network mainnet` |
| Mainnet contract invoke | `invoke-mainnet` | `starforge contract invoke --network mainnet` |

Account merge uses the **source wallet name** as the challenge phrase so operators
must deliberately name the account being closed.

---

## Automation and CI

Non-interactive environments (CI, piped stdin, `--non-interactive`) fail fast
with a clear error instead of blocking on stdin.

| Scenario | Behavior |
|----------|----------|
| Medium-risk + `--yes` | Confirmation skipped (unchanged) |
| Destructive + `--yes` only | **Blocked** — requires unsafe opt-in |
| Destructive + `--yes` + unsafe env | Allowed with a logged warning |

### Unsafe bypass (automation only)

To skip destructive confirmations in controlled automation, set **both**:

```bash
export STARFORGE_UNSAFE_SKIP_CONFIRMATION=1
starforge deploy --network mainnet --execute --yes …
```

This bypass is:

- **Explicit** — the env var name signals danger
- **Logged** — every outcome is recorded via structured logging (no secrets)
- **Documented** — flagged here and in [SECURITY_LOGGING_GUIDE.md](../SECURITY_LOGGING_GUIDE.md)

Do **not** set `STARFORGE_UNSAFE_SKIP_CONFIRMATION` in developer shells or shared
CI secrets unless the pipeline is scoped to non-production automation.

---

## Audit logging

Confirmation results are logged at `INFO` with:

- `confirmation_action` — e.g. `mainnet_deploy`, `secret_reveal`
- `confirmation_network`
- `confirmation_outcome` — `Confirmed`, `Cancelled`, `SkippedDryRun`, `SkippedUnsafeBypass`
- `confirmation_unsafe_bypass` — whether the unsafe env var was set

User input (typed phrases, secrets) is **never** logged.

---

## Related docs

- [COMMAND_REFERENCE.md](COMMAND_REFERENCE.md) — command flags
- [SECURITY_LOGGING_GUIDE.md](../SECURITY_LOGGING_GUIDE.md) — logging standards
- [DEPLOY_POLICY.md](DEPLOY_POLICY.md) — organization deploy gates
