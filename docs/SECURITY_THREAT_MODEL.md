# StarForge threat model

This document is the living security baseline for the StarForge CLI. Review it
at least once per release and whenever a new network, plugin, marketplace, or
AI capability is added. Security-sensitive changes should link the relevant
asset and boundary below in their design or pull request.

## Assets

- Wallet secret keys, encrypted key material, recovery shares, and signing
  requests.
- Browser-wallet handoff state: the one-time localhost URL, its nonce, and the
  signed envelope XDR it returns.
- Contract WASM, deployment parameters, transaction payloads, and RPC results.
- Plugin binaries, manifests, requested capabilities, and marketplace
  metadata.
- Local configuration, downloaded templates, caches, telemetry, and command
  history.
- AI prompts, generated plans, and any source or transaction context supplied
  to an AI provider.

## Adversaries

We defend against a malicious local process or plugin, a compromised or
typosquatted marketplace source, a network attacker, a malicious RPC response,
and an attacker who obtains a stale cache or backup. Users may also
accidentally approve unsafe commands or disclose secrets through prompts. The
browser-wallet handoff adds a hostile-local-page and nonce-replay adversary.

## Trust boundaries and controls

| Boundary | Main risk | Existing controls | Remaining gap |
| --- | --- | --- | --- |
| Wallet files → signer | Key theft or misuse | Encrypted-at-rest options, validation, explicit wallet selection | OS account compromise remains outside the CLI’s control |
| Browser wallet → localhost handoff | Forged or replayed signature, hostile local page injecting script, secret exposure | Loopback-only (`127.0.0.1`) bind on an ephemeral port, single-use 32-byte cryptographic nonce, strict `Content-Security-Policy` with a script nonce, short timeout, base64 envelope validation before the signed XDR is accepted, no secret ever loaded into the CLI | The page loads a pinned Stellar Wallets Kit bundle from a CDN, and a compromised browser extension can still return a valid-but-wrong signature; users must compare the displayed digest with their wallet |
| CLI → Stellar/Horizon/Soroban RPC | Forged results or endpoint substitution | Configured network endpoints, simulation and deployment validation | TLS/DNS trust and endpoint availability must be monitored |
| Marketplace → local template | Supply-chain code execution | Source trust classification, checksum verification when supplied, staged installation | Registries should require signed metadata and mandatory digests |
| Plugin binary → CLI process | Arbitrary code and capability abuse | Manifest/version checks and trust levels | Plugins are native code and are not a sandbox |
| AI provider → CLI/user | Prompt injection or unsafe generated actions | Human confirmation and security-oriented command flows | Never treat model output as authorization or verified facts |
| Local cache/config → runtime | Poisoned, stale, or over-permissive data | Versioned config migration and validation | Cache integrity and permission checks must cover every artifact path |

## Security requirements

1. Never log secret keys, decrypted configuration, credentials, or complete
   transaction signing payloads.
2. Treat all downloaded content and RPC responses as untrusted until validated.
3. Require explicit user confirmation before signing, deploying, deleting, or
   executing generated content.
4. Reject unknown future configuration versions with an actionable upgrade
   message; do not silently reinterpret them.
5. Report a trust decision and the source URL for every external plugin or
   template before it is loaded or copied into a project.
6. Bind browser-handoff servers to `127.0.0.1` only, authorise every request
   with a single-use cryptographic nonce, serve a strict
   `Content-Security-Policy`, and shut the server down on a short timeout.
7. Never accept a browser-supplied signature before validating that it is a
   non-empty base64 envelope XDR, and never reuse or accept an already-consumed
   handoff nonce.

## Browser-wallet localhost handoff

`starforge wallet sign-tx --signer browser` keeps keys in the user's browser
wallet by moving the signature request, not the secret, into a one-time page.
The CLI:

- generates a fresh 32-byte nonce from the OS-backed RNG and binds an ephemeral
  port on `127.0.0.1` (never a public interface);
- serves the page under `default-src 'none'` with a `script-src` restricted to
  the nonce plus the pinned Stellar Wallets Kit CDN origin, `connect-src 'self'`,
  `frame-ancestors 'none'`, `base-uri 'none'`, and `form-action 'none'`;
- exposes the unsigned envelope, its SHA-256 digest, and a decoded summary so
  the user can compare them with what their wallet shows before approving;
- consumes the nonce exactly once, validates the returned signed XDR, and stops
  listening as soon as a signature arrives or the timeout expires.

Threats and mitigations:

| Threat | Mitigation |
| --- | --- |
| Another local process drives the CLI's handoff endpoint | Nonce required on every route; the port is ephemeral and loopback-only for the lifetime of the command |
| Replay of a captured signed payload | Nonce is single-use and consumed only after a well-formed signature is validated |
| Hostile local page injects script into the handoff page | CSP with a script nonce; inline `<script>` blocks without the nonce do not execute |
| Oversized or malformed request exhausts memory or crashes the CLI | `Content-Length` capped by `MAX_REQUEST_BYTES`; malformed bodies get a `400` without consuming the nonce |
| Stolen secrets | The CLI never reads a secret for `--signer browser`; only the signed XDR crosses the handoff |
| Malicious wallet or CDN returns a wrong signature | Out of the CLI's control; the page shows the envelope digest so the user can reject it, and the CLI never submits from this command |

## Review cadence and gap tracking

Maintainers should review this model during each release and after changes to
wallet handling, plugin loading, marketplace fetching, AI integrations, or RPC
code. New gaps should become issues tagged `security` and link back here;
closing a gap should update the corresponding boundary row and add a regression
test where feasible.
