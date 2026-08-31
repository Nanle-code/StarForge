# Data-Flow Inventory: Secrets & PII in the CLI

Tracking issue: #795

This inventory enumerates where secrets (private keys, API tokens, auth
credentials) and PII/metadata (local paths, usernames, IP addresses, AI
prompt text) can flow to within StarForge, which controls apply to each
path today, and which gaps remain open. It is a point-in-time map intended
to be updated whenever a new sink (log stream, telemetry event, AI prompt
template) is added.

## 1. Data classes

| Class | Examples | Primary sources |
|---|---|---|
| **Secrets** | Stellar secret keys (`S...`), hex private keys, encryption/derived keys, API tokens (`Bearer`, `ghp_`, `sk-`), passphrases, BIP-39 mnemonics, signed XDR / transaction envelopes, Basic-Auth URL credentials | `starforge wallet` (create/import/export/rotate), `starforge deploy`, custom network config (RPC/Horizon URLs with embedded credentials) |
| **PII / metadata** | Local filesystem paths, OS usernames, hostnames, IP addresses, wallet/contract names, command arguments, AI prompt text (which may embed contract source, file paths, or error text) | Any CLI invocation, config file, AI-assisted commands (`starforge ai *`, `generate`, `audit`) |

## 2. Sinks and control mapping

| Sink | What can land there | Controls in place | Source |
|---|---|---|---|
| **Config store** (`~/.starforge/config.toml` legacy path; current config persisted via `Database::open()` / `config::save`) | Wallet `secret_key` (plaintext or encrypted bundle), custom network passphrases/URLs, wallet names | `validate_secret_key()` rejects malformed keys before storage; encrypted export bundles use Argon2 + AES-GCM (`salt:nonce:ciphertext`, see [WALLET_ENCRYPTION_FIX.md](../WALLET_ENCRYPTION_FIX.md)). Some artifacts (deployment checkpoints, `src/utils/deployment_checkpoint.rs`) are written with `0o600` file permissions. | `src/utils/config.rs`, `src/utils/database.rs`, `src/utils/deployment_checkpoint.rs` |
| **Wallet backup/export files** | `secret_key` (StrKey or encrypted bundle), `public_key`, wallet names, network | Backup format v2 has an HMAC-SHA256 integrity tag; parser rejects oversized/malformed input before touching secrets; error messages quote wallet names but never key material (see [WALLET_IMPORT_SECURITY.md](WALLET_IMPORT_SECURITY.md)); Shamir recovery shares are opt-in and information-theoretically safe below threshold (see [RECOVERY_SHARES_SECURITY.md](RECOVERY_SHARES_SECURITY.md)) | `src/utils/wallet_import.rs` |
| **Logs (`tracing` output, structured JSON)** | Operation metadata (wallet/contract names, network, durations), and — absent redaction — error text that could echo secrets | Centralized redaction engine `crate::utils::redaction::redact_secrets` runs on tracing log streams and CLI error streams; it pattern-matches Stellar secret keys, hex private keys, bearer/API tokens, key-value secret assignments, signed XDR, URL Basic-Auth and query-param credentials, and BIP-39 mnemonics before output. Sensitivity tiers (public / private / sensitive) are documented in [SECURITY_LOGGING_GUIDE.md](../SECURITY_LOGGING_GUIDE.md). | `src/utils/redaction.rs`, `SECURITY_LOGGING_GUIDE.md`, `SECURITY_LOGGING_BEST_PRACTICES.md` |
| **Telemetry payloads** | Command name, timestamp, success/failure, duration, anonymous UUID | Opt-out via `starforge config set telemetry false` or `STARFORGE_TELEMETRY=0`; documented exclusions (no wallet addresses, secret keys, contract code, config values, error messages, file paths, or identity); `sanitize_payload()` / `minimize_payload()` in `src/utils/privacy.rs` redact fields keyed `email`/`phone`/`name` and allow-list fields before a payload leaves the process. Local-only today — no network egress without future explicit opt-in. | `src/utils/telemetry.rs`, `src/utils/privacy.rs`, [TELEMETRY_PRIVACY.md](../TELEMETRY_PRIVACY.md) |
| **AI prompt requests** (local Ollama or remote model router) | Full contract source, file paths passed as context, error/debug text, user-authored prompt text; system prompts in `ollama.rs`/`generate.rs`/`ai_docs.rs` inject `[CONTRACT_CODE]` and related context verbatim | Prompts avoid requesting or echoing key material by convention (templates only reference contract/ABI content); no redaction pass is currently applied to prompt context before it is sent to a model. See [AI_PROMPT_GUIDE.md](../AI_PROMPT_GUIDE.md), [PROMPT_ENGINEERING.md](PROMPT_ENGINEERING.md). | `src/utils/ai*.rs` |
| **stdout / stderr** | Command output, interactive prompts, wallet `show --reveal` output, error messages | `redact_secrets()` applies to CLI error streams; secret-revealing commands (e.g. `wallet show --reveal`) are explicit, user-initiated opt-in actions rather than default output | `src/main.rs`, `src/commands/*.rs` |

## 3. Control summary

- **Redaction/masking**: `src/utils/redaction.rs` (logs, CLI error streams) and `src/utils/privacy.rs` (`sanitize_payload`, `anonymize_text`, `minimize_payload` for telemetry-shaped payloads).
- **File permissions**: `0o600` applied to deployment checkpoint files (`src/utils/deployment_checkpoint.rs`); not currently applied uniformly to the config database or wallet export files (see Gap 1 context and follow-ups below).
- **Opt-in/opt-out toggles**: telemetry is opt-out (`config set telemetry false`, `STARFORGE_TELEMETRY=0`); Shamir recovery shares and remote telemetry (future) are opt-in.
- **Integrity checks**: HMAC-SHA256 tag on v2 wallet backups; SHA-256 `secret_hash` on Shamir shares.
- **Input validation as a security boundary**: `wallet_import.rs` treats externally-sourced files as untrusted, enforcing size limits and format checks before parsing.

## 4. Gaps

The following gaps were identified while building this inventory and are tracked as follow-up issues rather than fixed here.

### Gap 1: In-memory secret retention

Secret keys, passphrases, and derived encryption keys are held in ordinary
`String`/`Vec<u8>` values (e.g. `Wallet.secret_key: Option<String>` in
`src/utils/config.rs`) with no use of zeroizing wrapper types. Rust's
ordinary heap allocations are not scrubbed on drop, so secret material can
remain resident in freed memory, swap, or a core dump after the value is
logically out of scope.

**Follow-up:** wrap secret-bearing fields in a zeroizing type (e.g. the
`zeroize`/`secrecy` crates) and audit `wallet`, `deploy`, and recovery-share
code paths for places a secret is copied into a non-zeroizing buffer.

### Gap 2: Unredacted panic/crash dumps

`redact_secrets()` is applied to tracing log streams and CLI error streams,
but there is no global panic hook (no `std::panic::set_hook` in
`src/main.rs`) that routes a panic message/backtrace through the same
redaction engine. A panic triggered while a secret is in a local variable
captured in the panic payload, or with `RUST_BACKTRACE=1` set, could print
unredacted material directly to stderr or into a crash report.

**Follow-up:** install a panic hook that redacts the panic message via
`crate::utils::redaction::redact_secrets` before printing, and evaluate
whether backtraces should be suppressed or scrubbed by default.

### Gap 3: AI prompt context sanitization

AI-assisted commands (`ai_context.rs`, `generate.rs`, `ai_docs.rs`, and
related modules under `src/utils/ai*.rs`) assemble prompt context —
contract source, file paths, and sometimes error text — and send it to a
local or remote model with no redaction pass and no user preview/confirm
step. Unlike the telemetry path (which runs payloads through
`sanitize_payload`/`minimize_payload`), prompt context is not checked for
embedded secrets (e.g. a `.env` value pasted into an error message) or for
local path/username disclosure before it leaves the process, and the user
is not shown what will be sent before it is sent.

**Follow-up:** run assembled AI prompt context through
`redaction::redact_secrets` (or an equivalent prompt-specific filter)
before dispatch, and add an opt-in preview/confirmation step for
commands that attach file paths or system metadata to a prompt.

## 5. Maintenance

Update this inventory whenever a new log stream, telemetry event, config
field, or AI prompt template is introduced, and whenever one of the gaps
above is closed by a linked follow-up issue.
