# StarForge threat model

This document is the living security baseline for the StarForge CLI. Review it
at least once per release and whenever a new network, plugin, marketplace, or
AI capability is added. Security-sensitive changes should link the relevant
asset and boundary below in their design or pull request.

## Assets

- Wallet secret keys, encrypted key material, recovery shares, and signing
  requests.
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
accidentally approve unsafe commands or disclose secrets through prompts.

## Trust boundaries and controls

| Boundary | Main risk | Existing controls | Remaining gap |
| --- | --- | --- | --- |
| Wallet files → signer | Key theft or misuse | Encrypted-at-rest options, validation, explicit wallet selection | OS account compromise remains outside the CLI’s control |
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

## Review cadence and gap tracking

Maintainers should review this model during each release and after changes to
wallet handling, plugin loading, marketplace fetching, AI integrations, or RPC
code. New gaps should become issues tagged `security` and link back here;
closing a gap should update the corresponding boundary row and add a regression
test where feasible.

