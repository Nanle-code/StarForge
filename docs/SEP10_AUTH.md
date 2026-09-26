# SEP-10 web authentication

[SEP-10](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0010.md)
is how a wallet proves ownership of a Stellar account to an anchor. The anchor
answers with a short-lived JWT, and that JWT is what SEP-24 and SEP-31 clients
send on every authenticated anchor request.

`starforge sep10 auth` performs the whole handshake from a local wallet and
prints the JWT.

## Quick start

```bash
starforge sep10 auth --domain testanchor.stellar.org --wallet alice
```

## The handshake

| Step | What StarForge does |
| --- | --- |
| 1 | Reads `https://<domain>/.well-known/stellar.toml` for `WEB_AUTH_ENDPOINT`, `SIGNING_KEY`, `WEB_AUTH_DOMAIN`, and `NETWORK_PASSPHRASE`. |
| 2 | Requests a challenge for the wallet's public key (`GET <WEB_AUTH_ENDPOINT>?account=G...&home_domain=<domain>`). |
| 3 | Validates the challenge locally before signing anything. |
| 4 | Signs the challenge with the wallet's secret key. |
| 5 | Submits the signed challenge and prints the JWT the anchor returns. |

Because the challenge is signed with a real key, StarForge never hands a
signature to an anchor that failed validation.

## What is validated

A challenge is rejected, with a specific error, when any of the following is
not true:

* The transaction is a v1 envelope signed by the anchor — v0 and fee-bump
  envelopes are refused.
* The source account is the account that is authenticating.
* The sequence number is `0`, so the anchor can never replay the transaction.
* `time_bounds` are present, `min_time <= now <= max_time`, and the window is at
  most 15 minutes.
* The transaction has exactly one operation: `manage_data` with the key
  `<home_domain> auth` and a non-empty nonce value.
* The transaction carries no memo.
* `stellar.toml`'s `WEB_AUTH_DOMAIN`, when set, matches the home domain that was
  requested.
* The challenge carries exactly one signature — the anchor's — and it is valid
  for `SIGNING_KEY` over the network passphrase the anchor declares.

The signature check uses `ed25519-dalek` over the SEP-10 transaction hash, so a
forged, replayed, or mismatched challenge fails locally and never reaches the
anchor.

## Options

| Flag | Purpose |
| --- | --- |
| `--domain <DOMAIN>` | Anchor home domain (required). |
| `--wallet <NAME>` | Local wallet to authenticate with (required). |
| `--network testnet\|mainnet` | Network to sign for (default: `testnet`). |
| `--toml-url <URL>` | Override the `stellar.toml` URL. Useful for staging anchors and local reference servers. |
| `--json` | Emit the machine-readable envelope instead of the human-readable report. |
| `--verbose` | Show every value of the handshake, including the challenge source account and nonce. |
| `--output <FILE>` | Also write the JWT to a file. |

## Testing against a local reference server

`--toml-url` points the client at a `stellar.toml` that is not served from the
well-known path, which is how the end-to-end tests in
`tests/sep10_reference_server.rs` run an anchor on `127.0.0.1`:

```bash
starforge sep10 auth \
  --domain 127.0.0.1:8080 \
  --wallet alice \
  --toml-url http://127.0.0.1:8080/.well-known/stellar.toml \
  --verbose
```

## JSON output

`--json` (or a globally enabled JSON mode) prints the standard StarForge
envelope. The fields are documented in
[`contracts/cli-json-fields.json`](contracts/cli-json-fields.json):

```json
{
  "version": 1,
  "ok": true,
  "data": {
    "home_domain": "testanchor.stellar.org",
    "web_auth_endpoint": "https://testanchor.stellar.org/auth",
    "account": "G...",
    "signing_key": "G...",
    "network_passphrase": "Test SDF Network ; September 2015",
    "challenge_data_name": "testanchor.stellar.org auth",
    "challenge_seconds_remaining": 300,
    "signatures_after_signing": 2,
    "jwt": "eyJ...",
    "jwt_claims": {
      "iss": "testanchor.stellar.org",
      "sub": "G...",
      "iat": 1700000000,
      "exp": 1700003600
    },
    "jwt_file": null
  }
}
```

## Errors

Failures exit non-zero and name the rule that was broken, for example:

```text
✗  Error: challenge signature is not valid for SIGNING_KEY G...
```

With `--json`, failures are reported on stderr as an error envelope
(`ok: false`) so a caller can tell a bad challenge from a bad network.

## See also

* [Command cheat sheet](COMMAND_CHEATSHEET.md)
* [Configuration](CONFIGURATION.md)
