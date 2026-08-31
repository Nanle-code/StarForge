# Custom Network Validation and Passphrase Security

## 1. Overview
When adding custom Stellar networks via `starforge network add`, endpoints and network passphrases are strictly validated.

## 2. Validation Rules
- **URL Syntax**: All URLs (`--horizon-url`, `--soroban-rpc-url`, `--friendbot-url`) must be valid, well-formed RFC 3986 URIs starting with `http://` or `https://` with a valid host.
- **Passphrase Integrity**: If `--passphrase` is provided, empty or whitespace-only values are rejected.
- **Trailing Slashes**: URLs are automatically trimmed of trailing slashes to guarantee consistent downstream endpoint construction.
