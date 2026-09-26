# StarForge

**Scaffold, deploy and operate Soroban smart contracts from one fast Rust CLI:
templates, encrypted wallets and deployment safety checks for Stellar.**

[![CI](https://github.com/Nanle-code/StarForge/actions/workflows/ci.yml/badge.svg)](https://github.com/Nanle-code/StarForge/actions/workflows/ci.yml)
![License: MIT](https://img.shields.io/badge/License-MIT-cyan.svg)
![Status: beta](https://img.shields.io/badge/status-beta-yellow.svg)
![Stellar Wave](https://img.shields.io/badge/Stellar-Wave%20Program-blueviolet.svg)

---

## Overview

**starforge** is a free, open-source command-line toolkit for developers building on the Stellar network. It brings together the most common Stellar and Soroban developer workflows — wallet management, project scaffolding, and contract deployment — into a single fast, ergonomic CLI.

It provides a Hardhat/Foundry-like experience for the Stellar ecosystem while prioritizing reproducibility and security.

This project is actively maintained and participates in the [Stellar Wave Program](https://www.drips.network/wave/stellar) on Drips — a monthly open-source contribution sprint where contributors earn rewards for merged pull requests.

Security architecture and trust-boundary assumptions are documented in the
[StarForge threat model](./docs/SECURITY_THREAT_MODEL.md). Report newly discovered
security gaps as issues tagged `security` and include the affected boundary.

---

## Features

### 🔐 Wallet Management
Create and manage Stellar ed25519 keypairs locally. Generate cryptographically secure keys using proper Stellar strkey encoding (G... for public, S... for secret). Optionally encrypt keys at rest with AES-256-GCM. Fund testnet accounts via Friendbot, list all saved wallets, inspect live on-chain balances, and securely store keys in `~/.starforge/config.toml`.

### 🧩 Project Scaffolding
Scaffold new Soroban smart contract projects from battle-tested templates with one command. Choose from: `hello-world`, `token`, `nft`, and `voting`. Use interactive mode (`--interactive`) to customize contract options like author, license, storage type, and test inclusion. Also scaffolds full Stellar dApp frontends (Vite + React).

**NEW: Template Marketplace** - Discover and use community-contributed templates:
```bash
# Search for templates
starforge template search defi

# Use a marketplace template
starforge new contract my-dex --template uniswap-v2 --from marketplace

# Publish your own template
starforge template publish ./my-template
```

### 🚀 Contract Deployment
Validate, size-check, and deploy compiled Soroban `.wasm` files to Testnet or Mainnet. Verifies account balance on-chain, calculates the Soroban WASM hash as a SHA-256 digest of the raw file bytes, and generates the exact `stellar contract deploy` command to complete the deployment.

The local hash shown by `starforge deploy` is intended to match the value reported by `stellar contract inspect --wasm <file>` for the same bytecode. StarForge now computes that hash through a shared helper that validates the WASM payload, rejects empty or malformed input, and fails explicitly on unsupported build environments instead of silently producing a different result.

For contributors, the hash is intentionally defined as the SHA-256 digest of the raw `.wasm` bytecode. The implementation currently supports Linux, Windows, and macOS hosts; other environments are rejected with a clear error so reproducibility checks do not silently drift.

---

## Installation

### Quick Install (macOS / Linux)

You can install the latest release binary using the installation script:

```bash
curl -sL https://raw.githubusercontent.com/Nanle-code/StarForge/main/install.sh | bash
```

The script automatically:
1. Detects your OS and CPU architecture
2. Downloads the correct release archive from GitHub
3. **Verifies the SHA-256 checksum** against the published `SHA256SUMS.txt` before extracting
4. Installs the binary to `/usr/local/bin` (or a custom path — see below)
5. Cleans up all temporary files on exit

> **Security note**: Never pipe an untrusted script to `bash` without reviewing it first.
> You can read [`install.sh`](./install.sh) before running it, or download the binary
> directly from the [Releases page](https://github.com/Nanle-code/StarForge/releases) and verify the checksum manually:
>
> ```bash
> sha256sum -c SHA256SUMS.txt
> ```

#### Platform compatibility

| OS | Architecture | Supported |
|---|---|---|
| Linux | x86\_64 | ✅ |
| Linux | aarch64 | ✅ |
| macOS | x86\_64 | ✅ |
| macOS | aarch64 (Apple Silicon) | ✅ |
| Windows | x86\_64 | ✅ (`.zip` from [Releases](https://github.com/Nanle-code/StarForge/releases)) |
| FreeBSD / other | — | Not supported |

Windows binaries are built and smoke-tested in CI on every push and pull
request: CI verifies that `starforge.exe` starts and that its core `--help` and
`config doctor` surface works ([`tests/installer/windows_smoke.ps1`](tests/installer/windows_smoke.ps1)).
The release pipeline refuses to publish a Windows binary that fails these
checks, so a healthy download always starts on a supported Windows version.

#### Custom install directory

Override the default `/usr/local/bin` destination:

```bash
INSTALL_DIR="$HOME/.local/bin" \
  curl -sL https://raw.githubusercontent.com/Nanle-code/StarForge/main/install.sh | bash
```

#### Uninstall

```bash
rm -f /usr/local/bin/starforge
# or, if you used a custom INSTALL_DIR:
rm -f "$INSTALL_DIR/starforge"
```

### Homebrew (macOS / Linux)

A draft Homebrew formula is available for testing:

```bash
brew install Nanle-code/starforge/starforge
```

### Docker

Multi-arch (`linux/amd64`, `linux/arm64`) images are published to the GitHub
Container Registry on every tagged release, signed with build provenance
attestation (verifiable via `gh attestation verify`):

```bash
docker pull ghcr.io/nanle-code/starforge:latest
docker run --rm ghcr.io/nanle-code/starforge:latest --version

# Or pin to a specific release:
docker run --rm ghcr.io/nanle-code/starforge:0.1.0 --version
```

Recommended for CI: pin the tag (not `:latest`) so a build is reproducible,
and mount a workspace directory so `starforge`'s output persists outside the
container:

```yaml
- name: Run StarForge in CI
  run: |
    docker run --rm -v "$PWD:/workspace" -w /workspace \
      ghcr.io/nanle-code/starforge:0.1.0 doctor
```

### Linux packages (.deb / .rpm)

Every tagged release publishes `starforge-amd64.deb` and
`starforge-x86_64.rpm` alongside the tarballs, built by `cargo-deb` /
`cargo-generate-rpm` from `[package.metadata.deb]` /
`[package.metadata.generate-rpm]` in `Cargo.toml`. Both packages install the
binary, the top-level man page, and bash/zsh/fish completions.

```bash
# Debian / Ubuntu
curl -LO https://github.com/Nanle-code/StarForge/releases/latest/download/starforge-amd64.deb
sudo apt install ./starforge-amd64.deb

# Fedora / RHEL
curl -LO https://github.com/Nanle-code/StarForge/releases/latest/download/starforge-x86_64.rpm
sudo dnf install ./starforge-x86_64.rpm
```

Verify the SHA-256 checksum against `SHA256SUMS.txt` (also published with
every release) before installing, the same as for the tarball archives.

### Build from source

**Prerequisites:**
- Rust >= 1.80 ([install via rustup](https://rustup.rs))

```bash
git clone https://github.com/Nanle-code/StarForge.git
cd StarForge
cargo build --release

# Move the binary to your PATH
cp target/release/starforge ~/.local/bin/
# or on macOS:
cp target/release/starforge /usr/local/bin/
```

### Verify installation

```bash
starforge --version
# starforge 0.1.0

starforge info
```

---

## Usage

### Repeatable invocation scripts

Store ordered contract calls in YAML or JSON. Scripts use schema version `1`, reject unknown fields, and support `${NAME}` interpolation from the script's `env` map or the CI process environment:

```yaml
version: 1
env:
  CONTRACT_ID: CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
steps:
  - name: read-state
    contract_id: ${CONTRACT_ID}
    function: get_value
    wallet: ci
    network: testnet
    args:
      - type: string
        value: deployment
    assertions:
      - contains: ready
  - name: write-state
    contract_id: ${CONTRACT_ID}
    function: set_value
    wallet: ci
    network: testnet
    args:
      - type: string
        value: deployment
      - type: string
        value: ${VALUE}
```

Validate the complete sequence without contacting RPC or submitting transactions:

```bash
starforge contract invoke-script ops.yaml --dry-run
```

Run it in CI after configuring the `ci` wallet and environment variables. Steps execute sequentially, and a failed assertion stops the script:

```yaml
# .github/workflows/invoke.yml
- name: Run contract operations
  run: starforge contract invoke-script ops.yaml
  env:
    VALUE: production
```

### Stable JSON output contract

Use the global `--json` flag or set `STARFORGE_OUTPUT_JSON=1` to request machine-readable output from supported commands.

Every success response uses the same envelope shape:

```json
{
  "version": 1,
  "ok": true,
  "data": {
    "name": "wallet",
    "count": 2
  }
}
```

Failures use a versioned error envelope:

```json
{
  "version": 1,
  "ok": false,
  "error": {
    "code": "command_error",
    "message": "unsupported network"
  }
}
```

This is a global contract so automation can parse output consistently across commands without depending on per-command ad hoc schemas.

### Wallet commands

```bash
# Create a new keypair
starforge wallet create alice

# Create a wallet with encrypted storage (prompts for passphrase)
starforge wallet create alice --encrypt

# Create and fund immediately (testnet only)
starforge wallet create deployer --fund

# List all saved wallets
starforge wallet list

# Show wallet details + live balance
starforge wallet show alice

# Reveal secret key (prompts for passphrase if encrypted)
starforge wallet show alice --reveal

# Fund an existing wallet via Friendbot
starforge wallet fund alice

# Remove a wallet
starforge wallet remove alice

# Rotate a wallet but keep the same local name
starforge wallet rotate alice --fund
```

Wallet rotation keeps the same local wallet name in `~/.starforge/config.toml`, but it creates a brand-new on-chain Stellar account keypair. Any scripts, signer sets, or deployment flows that referenced the previous public key still need to be updated separately.

### Network commands

```bash
# Show current network and available networks
starforge network show

# Switch to mainnet
starforge network switch mainnet

# Add a custom network
starforge network add mynet \
  --horizon-url https://my-horizon.example.com \
  --soroban-rpc-url https://my-soroban.example.com

# Switch to custom network
starforge network switch mynet

# Test network connectivity
starforge network test
starforge network test mainnet
```

### Configuration commands

```bash
# Show all configuration settings
starforge config show

# Get a specific setting
starforge config get telemetry
starforge config get network

# Set a configuration value
starforge config set telemetry false
starforge config set network mainnet
```

Common settings:
- **telemetry**: Enable/disable anonymous usage telemetry (`true` or `false`)
- **network**: Set the default network (`testnet`, `mainnet`, or custom network name)

For privacy information, see [Telemetry & Privacy](#telemetry--privacy).

### Configuration schema migrations

starforge stores its configuration in `~/.starforge/config.toml`. When a new
release introduces a new schema version the CLI migrates the file automatically
on first run.

**What happens during a migration:**

1. A timestamped backup is written **before** any change is made:
   ```
   ~/.starforge/config.backup.v0.1753000000.toml
   ```
2. Each migration step is applied in order (v0 → v1, v1 → v2, …).
3. The updated config is persisted.

If the migration fails you can manually restore from the backup:

```bash
cp ~/.starforge/config.backup.v0.<timestamp>.toml ~/.starforge/config.toml
```

**Error types and what they mean:**

| Error                                                          | Cause                                                 | Fix                                                               |
| -------------------------------------------------------------- | ----------------------------------------------------- | ----------------------------------------------------------------- |
| `Config schema version 'X' is newer than this binary supports` | Config was written by a newer `starforge`             | Upgrade `starforge`                                               |
| `Unrecognised config schema version 'X'`                       | Config version field was manually edited or corrupted | Restore from backup or delete `~/.starforge/config.toml` to reset |
| `Failed to create backup of config vX before migration`        | Backup write failed (disk full, permissions)          | Free disk space or fix directory permissions                      |

**For contributors — adding a new migration step:**

1. Bump `CURRENT_CONFIG_VERSION` in `src/utils/config.rs`.
2. Add a `fn migrate_vN_to_vM(config: &mut Config)` function.
3. Append a `ConfigMigrationStep` entry to the `MIGRATION_STEPS` slice.
4. Add tests to `tests/config_migrations.rs` covering the new step.



### Scaffold commands

```bash
# Scaffold a Soroban contract (hello-world template)
starforge new contract my-contract

# Scaffold interactively with custom options
starforge new contract my-contract --interactive

# Scaffold with a specific template
starforge new contract my-token --template token
starforge new contract my-nft --template nft
starforge new contract my-vote --template voting

# Search marketplace templates
starforge template search defi
starforge new contract --search lending --tags defi

# Use a marketplace template
starforge new contract my-dex --template uniswap-v2 --from marketplace

# Scaffold a Stellar dApp frontend (Vite + React)
starforge new dapp my-dapp
```

### Local AI assistant

StarForge can use a locally running [Ollama](https://ollama.ai/) instance for
Soroban development help. Install Ollama, start the daemon, and pull the
recommended model before using the assistant:

```bash
ollama serve
starforge ai pull codellama:7b
```

```bash
# Check Ollama and locally installed models
starforge ai status
starforge ai models

# Ask a Soroban development question
starforge ai ask "How should I store an expiring value?"

# Review or explain a contract source file
starforge ai audit src/lib.rs
starforge ai explain src/lib.rs

# Generate tests or find gas optimisation opportunities
starforge ai test src/lib.rs
starforge ai optimise src/lib.rs
```

All requests remain local to Ollama at `http://localhost:11434`; no contract
source or prompts are sent to a cloud provider. Use `starforge ai status` to
diagnose an installation or runtime problem.

### Template marketplace commands

```bash
# Initialize marketplace with example templates
starforge template init

# Search for templates
starforge template search defi
starforge template search --tags dex,amm

# List all templates
starforge template list

# View template details
starforge template show uniswap-v2

# Publish your own template
starforge template publish ./my-template \
  --name my-awesome-template \
  --description "An awesome contract" \
  --author "Your Name" \
  --tags "defi,custom"

# Remove a template
starforge template remove my-template
```

macOS and Linux (x86\_64, aarch64). Windows `.zip`, checksums and
build-from-source: [installation guide](docs/INSTALL.md).

![StarForge demo: scaffold, deploy and invoke a Soroban contract on testnet](docs/assets/demo.gif)

<sub>A real testnet run, recorded with [`scripts/record-demo.py`](scripts/record-demo.py)
([cast file](docs/assets/demo.cast)).</sub>

## 30-second tour

These commands run offline in a throwaway `HOME`, and CI executes them on
every PR:

```bash run
starforge new contract hello              # scaffold from a template
starforge wallet create alice             # local keypair (add --encrypt to protect it)
starforge network show                    # testnet, mainnet, or your own
starforge template search defi            # community templates
```

Then deploy to testnet. StarForge works alongside
[stellar-cli](https://developers.stellar.org/docs/tools/cli), which compiles the
contract and signs the final transactions:

```bash norun
cd hello && stellar contract build
stellar keys generate deployer                          # or reuse an existing identity
starforge wallet import --from-stellar-cli deployer     # same wallet, now in StarForge
starforge wallet fund deployer
starforge deploy --wasm target/wasm32v1-none/release/hello.wasm --wallet deployer --dry-run
starforge deploy --wasm target/wasm32v1-none/release/hello.wasm --wallet deployer --yes --execute
stellar contract invoke --id <CONTRACT_ID> --source deployer --network testnet -- hello --to Stellar
```

## Highlights

|                         |                                                                                                                                                                                                                                       |
| ----------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Scaffolding**         | `hello-world`, `token`, `nft` and `voting` templates, a template marketplace, and Vite + React dApp frontends. See [Usage](docs/USAGE.md#scaffold-commands).                                                                          |
| **Wallets**             | Keys encrypted at rest (Argon2id + AES-256-GCM), BIP39, backups and recovery shares, Ledger/Trezor, import from stellar-cli. See [Usage](docs/USAGE.md#wallet-commands) and [wallet import security](docs/WALLET_IMPORT_SECURITY.md). |
| **Safe deploys**        | WASM validation, balance and fee simulation, dry-run plans, deploy policies, checkpoints, history and rollback. See [Deploy policy](docs/DEPLOY_POLICY.md) and [checkpoints](docs/DEPLOYMENT_CHECKPOINTS.md).                         |
| **Automation**          | A stable `--json` envelope, YAML invocation scripts with assertions, and non-interactive mode for CI. See [JSON stability](docs/CLI_JSON_STABILITY.md) and [Usage](docs/USAGE.md#repeatable-invocation-scripts).                      |
| **Local AI (optional)** | Audit, explain and test contracts with a local Ollama model. Nothing leaves your machine. See [Offline AI](docs/OFFLINE_AI.md).                                                                                                       |

Coming from stellar-cli? Read
**[Migrating from stellar-cli](docs/MIGRATING_FROM_STELLAR_CLI.md)** for a
command-by-command mapping, how to import identities, and what stellar-cli
still does better.

## Documentation

- [Installation](docs/INSTALL.md) · [Usage guide](docs/USAGE.md) · [Command reference](docs/COMMAND_REFERENCE.md) · [Cheat sheet](docs/COMMAND_CHEATSHEET.md)
- [Configuration](docs/CONFIGURATION.md) · [Architecture](ARCHITECTURE.md) · [All documentation](docs/README.md)
- Docs site: <https://nanle-code.github.io/StarForge/> (built from [`docs/`](docs/))

## Status and stability

StarForge is **beta** (`0.x`). The core wallet, scaffold and deploy workflows
are covered by CI on Linux, macOS and Windows. Command names and flags can
still change between minor releases; breaking changes are listed in the
release notes. The `--json` output envelope is versioned and stable
([policy](docs/CLI_JSON_STABILITY.md)). Several advanced command groups
(AI-assisted tooling, orchestration, governance) are experimental.

## Security

- Report vulnerabilities privately: see [SECURITY.md](docs/SECURITY.md).
- Trust boundaries and assumptions: [threat model](./docs/SECURITY_THREAT_MODEL.md).
- Install only from this repository. Releases ship with `SHA256SUMS.txt`, and
  the installer verifies it.
- Plaintext wallets are for testnet. Use `--encrypt` or a hardware wallet for
  real funds.
- Telemetry is **off by default** and local-only until you opt in
  ([details](docs/TELEMETRY_PRIVACY.md)).

## Contributing

Contributions are welcome. Start with [CONTRIBUTING.md](CONTRIBUTING.md) and
the [quick reference](CONTRIBUTOR_QUICK_REFERENCE.md). CI runs formatting,
clippy, tests, canonical-link checks and the runnable docs examples
([annotation guide](CONTRIBUTING.md#documentation-snippets)).

StarForge takes part in the [Stellar Wave Program](https://www.drips.network/wave/stellar)
on Drips, where merged contributions earn rewards. Read the
[terms](https://docs.drips.network/wave/terms-and-rules) first.

## License

MIT Â© 2025 â€” See [LICENSE](./LICENSE) for details.

---

## Acknowledgements

Built for the Stellar ecosystem.
Participates in the [Stellar Wave Program](https://www.drips.network/wave/stellar) via [Drips](https://www.drips.network).
Powered by the [Stellar Horizon API](https://developers.stellar.org/api/horizon) and [Soroban](https://soroban.stellar.org).

---

## Documentation

StarForge has comprehensive documentation covering all aspects of the project:

### ?? Core Documentation
- **[README.md](README.md)** - This file, quick start and overview
- **[Documentation.md](Documentation.md)** - Extended documentation with architecture overview
- **[ARCHITECTURE.md](ARCHITECTURE.md)** - Complete system architecture and design
- **[DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md)** - Contributing and development guide
- **[API_REFERENCE.md](API_REFERENCE.md)** - Complete command reference
- **[docs/COMMAND_REFERENCE.md](docs/COMMAND_REFERENCE.md)** - Navigable CLI command index
- **[docs/COMMAND_CHEATSHEET.md](docs/COMMAND_CHEATSHEET.md)** - Auto-generated CLI command cheat sheet

### ?? Feature Documentation
- **[TEMPLATE_MARKETPLACE.md](TEMPLATE_MARKETPLACE.md)** - Template marketplace feature
- **[QUICK_START_TEMPLATES.md](QUICK_START_TEMPLATES.md)** - Template quick start guide
- **[docs/SIMULATION_RESOURCES.md](docs/SIMULATION_RESOURCES.md)** - CPU, memory, footprint, and minimum resource fees from simulation
- **[docs/CORRELATION_IDS.md](docs/CORRELATION_IDS.md)** - Correlating structured logs across one invocation
- **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)** - Config parsing, overlay merging, and validation rules
- **[docs/WALLET_IMPORT_SECURITY.md](docs/WALLET_IMPORT_SECURITY.md)** - Limits enforced on untrusted wallet backups
- **[docs/DEPLOYMENT_CHECKPOINTS.md](docs/DEPLOYMENT_CHECKPOINTS.md)** - Resumable and idempotent deployment operations, session checkpointing, and staleness detection
- **[docs/DATABASE_MIGRATIONS.md](docs/DATABASE_MIGRATIONS.md)** - SQLite schema migrations, corruption detection, backup-before-migrate, and recovery
- **[docs/CLI_ACCESSIBILITY.md](docs/CLI_ACCESSIBILITY.md)** - `--plain` mode, `$NO_COLOR`, and screen-reader-friendly output
- **[FUZZING_GUIDE.md](FUZZING_GUIDE.md)** - Property-based tests, fuzz targets, mutation testing

### ?? Navigation
- **[DOCUMENTATION_INDEX.md](DOCUMENTATION_INDEX.md)** - Complete documentation index
- **[DOCUMENTATION_SUMMARY.md](DOCUMENTATION_SUMMARY.md)** - Documentation overview

### ?? Examples
- **[examples/template_marketplace_usage.md](examples/template_marketplace_usage.md)** - Practical examples
- **[tutorials/hello-world/](tutorials/hello-world/)** - Beginner tutorial

**Total**: 17 documentation files with 7,700+ lines covering architecture, development, API reference, and examples.

For a complete overview, see [DOCUMENTATION_INDEX.md](DOCUMENTATION_INDEX.md).


# Remove a template
starforge template remove my-template

# Remove template + delete all local files
starforge template remove my-template --purge
## Enhanced Binding Generator (Issue #336)

The binding generator now provides comprehensive type-safe interfaces for contract interaction:

### Features:
- **Multi-language support**: Rust, TypeScript, Python, Go
- **Type-safe interfaces**: Proper type annotations for all parameters
- **Event type definitions**: Extract and generate event types from contract metadata
- **Complex type support**: Options, Results, Vectors, Maps, custom UDTs
- **Comprehensive testing**: Full test coverage for all languages

### Usage:
```bash
# Generate Rust bindings
starforge contract generate-bindings ./contract.wasm --lang rust > client.rs

# Generate TypeScript bindings  
starforge contract generate-bindings ./contract.wasm --lang ts > client.ts

# Generate Python bindings
starforge contract generate-bindings ./contract.wasm --lang python > client.py

# Generate Go bindings
starforge contract generate-bindings ./contract.wasm --lang go > client.go
```

### Example Generated Rust Code:
```rust
pub struct ContractClient {
    pub contract_id: String,
    pub network: String,
    pub wallet: Option<String>,
}

impl ContractClient {
    pub fn transfer(&self, from: String, to: String, amount: u128) -> Result<()> {
        // Type-safe method implementation
    }
}

// Generated event types
pub struct TransferEvent {
    pub from: String,
    pub to: String,
    pub amount: String,
}
```

See `examples/binding_generator_example.md` for complete examples.

### Terminal UI
The `starforge ui` command provides a live TUI (Terminal User Interface) overview of your project, showing balances, deployed contracts, TTLs, recent transactions, and a live event tail.
![StarForge UI](https://raw.githubusercontent.com/Nanle-code/StarForge/main/docs/ui-screenshot.png)
