# Changelog

All notable changes to StarForge are documented in this file.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html)
as described in [STABILITY.md](./STABILITY.md).

> **Contributors:** Every user-facing PR must include an entry under
> `## [Unreleased]` in the appropriate section.  CI will fail if no entry
> is present for a PR that touches the stable surface (see `CI_ENFORCEMENT.md`).

---

## [Unreleased]

### Added

- `starforge bug-report` command — collects version, OS, active network,
  config path, and Stellar CLI presence into a prefilled GitHub issue
  template (`--output <file>` writes to disk) (#975).
- `STABILITY.md` — public stability policy, stable surface definition,
  semver rules, deprecation process, and 1.0 scope (#973).
- `CHANGELOG.md` — this file; establishes Keep-a-Changelog discipline
  going forward (#973).

### Changed

- Generated projects and bundled templates now target Soroban SDK `22.0.0`
  (previously `21.0.0`), matching the version StarForge itself builds against.
  The version lives in the new `SOROBAN_SDK_VERSION` and `STELLAR_XDR_VERSION`
  constants in `src/utils/templates.rs`, and `starforge new contract`, the
  example templates, and `templates/test-helpers` all read from that single
  source — no hard-coded SDK version strings remain in the generators (#910).
- Documented the SDK upgrade cadence in `docs/TEMPLATE_CONTRIBUTING.md` (#910).

---

## [0.1.0] — 2024-11-01

### Added

- Initial public release of StarForge.
- `starforge wallet` — create, list, fund (Friendbot), show balance, and
  remove named keypairs.
- `starforge new contract` / `starforge new dapp` — Soroban contract and
  dApp scaffolding.
- `starforge deploy` — deploy a compiled `.wasm` contract to testnet or
  mainnet.
- `starforge invoke` — invoke a deployed Soroban contract function.
- `starforge inspect` — inspect on-chain contract storage.
- `starforge contract` — core contract operations sub-commands.
- `starforge info` — environment summary (version, config, network status,
  Stellar CLI detection).
- `starforge config` — manage active network and telemetry settings.
- `starforge network` — view and switch active network.
- `starforge node` — Docker-based local Soroban devnet quickstart.
- `starforge tx` — fetch and display transaction details.
- `starforge completions` — generate shell completions for bash, zsh,
  fish, and PowerShell.
- `starforge monitor` *(experimental)* — live contract event monitoring.
- `starforge diagnostics` *(experimental)* — hardware wallet (Ledger /
  Trezor) connectivity diagnostics.
- Global flags: `--json`, `--quiet`, `--plain`, `--non-interactive`,
  `--log-format`, `--log-dir`, `--correlation-id`.

[Unreleased]: https://github.com/Nanle-code/StarForge/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/Nanle-code/StarForge/releases/tag/v0.1.0
