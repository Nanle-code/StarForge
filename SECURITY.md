# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in StarForge, please open a
[GitHub issue](https://github.com/Nanle-code/StarForge/issues) or contact
the maintainers directly rather than disclosing it publicly, so a fix can
be prepared before details are shared widely.

## Secrets & PII handling

For a full inventory of where secrets (private keys, API tokens, auth
credentials) and PII/metadata (local paths, usernames, IP addresses, AI
prompt text) can flow within the CLI — config files, logs, telemetry
payloads, AI prompt requests, and stdout/stderr — along with the controls
applied at each point and known gaps, see
[docs/DATA_FLOW_INVENTORY.md](docs/DATA_FLOW_INVENTORY.md).

## Related documentation

- [docs/WALLET_IMPORT_SECURITY.md](docs/WALLET_IMPORT_SECURITY.md) — parser hardening for untrusted wallet backups
- [docs/RECOVERY_SHARES_SECURITY.md](docs/RECOVERY_SHARES_SECURITY.md) — threat model for Shamir recovery shares
- [SECURITY_LOGGING_GUIDE.md](SECURITY_LOGGING_GUIDE.md) — what may and may not be logged
- [TELEMETRY_PRIVACY.md](TELEMETRY_PRIVACY.md) — what telemetry collects and how to disable it
