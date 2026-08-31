# ? starforge

> A developer productivity CLI for Stellar and Soroban workflows â€” built in Rust.

![License: MIT](https://img.shields.io/badge/License-MIT-cyan.svg)
![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)
![Network: Stellar](https://img.shields.io/badge/Network-Stellar-blue.svg)
![Status: Active](https://img.shields.io/badge/Status-Active-green.svg)
![Stellar Wave](https://img.shields.io/badge/Stellar-Wave%20Program-blueviolet.svg)

---

## Overview

**starforge** is a free, open-source command-line toolkit for developers building on the Stellar network. It brings together the most common Stellar and Soroban developer workflows â€” wallet management, project scaffolding, and contract deployment â€” into a single fast, ergonomic CLI.

Think of it as the "Hardhat or Foundry" experience for the Stellar ecosystem, built in Rust for speed and reliability.

This project is actively maintained and participates in the [Stellar Wave Program](https://www.drips.network/wave/stellar) on Drips â€” a monthly open-source contribution sprint where contributors earn rewards for merged pull requests.

Configuration schema ownership and migration guidance are documented in
[Configuration schema migrations](./docs/CONFIGURATION_MIGRATIONS.md).

---

## Features

### ?? Wallet Management
Create and manage Stellar ed25519 keypairs locally. Generate cryptographically secure keys using proper Stellar strkey encoding (G... for public, S... for secret). Optionally encrypt keys at rest with AES-256-GCM. Fund testnet accounts via Friendbot, list all saved wallets, inspect live on-chain balances, and securely store keys in `~/.starforge/config.toml`.

### ? Project Scaffolding
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
curl -sL https://raw.githubusercontent.com/Josetic224/StarForge/main/install.sh | bash
```

The script automatically:
1. Detects your OS and CPU architecture
2. Downloads the correct release archive from GitHub
3. **Verifies the SHA-256 checksum** against the published `SHA256SUMS.txt` before extracting
4. Installs the binary to `/usr/local/bin` (or a custom path — see below)
5. Cleans up all temporary files on exit

> **Security note**: Never pipe an untrusted script to `bash` without reviewing it first.
> You can read [`install.sh`](./install.sh) before running it, or download the binary
> directly from the [Releases page](https://github.com/Josetic224/StarForge/releases) and verify the checksum manually:
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
| Windows | x86\_64 | ✅ (`.zip` from [Releases](https://github.com/Josetic224/StarForge/releases)) |
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
  curl -sL https://raw.githubusercontent.com/Josetic224/StarForge/main/install.sh | bash
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
brew install Josetic224/starforge/starforge
```

### Build from source

**Prerequisites:**
- Rust >= 1.80 ([install via rustup](https://rustup.rs))

```bash
git clone https://github.com/Josetic224/StarForge.git
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

| Error | Cause | Fix |
|---|---|---|
| `Config schema version 'X' is newer than this binary supports` | Config was written by a newer `starforge` | Upgrade `starforge` |
| `Unrecognised config schema version 'X'` | Config version field was manually edited or corrupted | Restore from backup or delete `~/.starforge/config.toml` to reset |
| `Failed to create backup of config vX before migration` | Backup write failed (disk full, permissions) | Free disk space or fix directory permissions |

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

### Deploy commands

```bash
# Deploy a compiled contract
starforge deploy --wasm target/wasm32-unknown-unknown/release/my_contract.wasm

# Deploy to mainnet using a specific wallet
starforge deploy \
  --wasm target/wasm32-unknown-unknown/release/my_contract.wasm \
  --network mainnet \
  --wallet deployer

# Skip confirmation prompt (for CI)
starforge deploy --wasm ./my_contract.wasm --yes

# Optimize with soroban-optimize before deployment
starforge deploy --wasm ./my_contract.wasm --optimize
```

### Contract commands

```bash
# Inspect a deployed contract instance
starforge contract inspect CCPYZFKEAXHHS5VVW5J45TOU7S2EODJ7TZNJIA5LKDVL3PESCES6FNCI

# Inspect on a specific network
starforge contract inspect CCPYZFKEAXHHS5VVW5J45TOU7S2EODJ7TZNJIA5LKDVL3PESCES6FNCI --network mainnet

# Generate typed wrappers from embedded contract metadata
starforge contract generate-bindings ./my_contract.wasm --lang rust
starforge contract generate-bindings ./my_contract.wasm --lang ts
```

### Rollback safety testing

```bash
# Validate that an upgraded contract can be rolled back without losing critical state
starforge test \
  --wasm target/wasm32-unknown-unknown/release/my_contract_v2.wasm \
  --rollback \
  --previous-wasm target/wasm32-unknown-unknown/release/my_contract_v1.wasm \
  --rollback-scenario tests/rollback/token-balances.json \
  --rollback-performance-budget-ms 1000 \
  --report json
```

The rollback harness checks state preservation, rollback scenarios, data integrity invariants, and rollback performance budgets. See [ROLLBACK_TESTING.md](ROLLBACK_TESTING.md) for scenario schema and CI examples.

### Environment info

```bash
starforge info
```

### Shell completions

```bash
# Bash â€” add to ~/.bashrc
source <(starforge completions bash)

# Zsh â€” add to ~/.zshrc
source <(starforge completions zsh)

# Fish â€” save to fish completions directory
starforge completions fish > ~/.config/fish/completions/starforge.fish
```

After adding the line to your shell config, restart your shell or run `source ~/.bashrc` / `source ~/.zshrc`. Tab-completion for all subcommands and flags will then be active.

---

## Project Structure

```
starforge/
+-- Cargo.toml
+-- src/
    +-- main.rs                  # CLI entry point + banner
    +-- commands/
    Â¦   +-- mod.rs
    Â¦   +-- wallet.rs            # wallet create/list/show/fund/remove
    Â¦   +-- new.rs               # project scaffolding + templates
    Â¦   +-- contract.rs          # contract inspect + invoke
    Â¦   +-- deploy.rs            # contract deployment
    Â¦   +-- info.rs              # environment info
    +-- utils/
        +-- mod.rs
        +-- config.rs            # ~/.starforge/config.toml read/write
        +-- horizon.rs           # Horizon API + Friendbot HTTP calls
        +-- soroban.rs           # Soroban RPC helpers
        +-- print.rs             # Consistent CLI output helpers
```

---

## Privacy & Telemetry

StarForge values your privacy.

### Local-Only Telemetry Guarantee
To help improve CLI usability, starforge collects anonymous usage telemetry (such as command names and execution times). This telemetry data is **stored purely locally** at `~/.starforge/data/telemetry.log`. **No network requests are ever made** for telemetry transmission; your telemetry data never leaves your machine.

### Explicit Opt-Out Methods
You can easily disable telemetry at any time using one of three methods:

1. **Config Command:**
   ```bash
   starforge config set telemetry.enabled false
   ```

2. **Telemetry Subcommand:**
   ```bash
   starforge telemetry disable
   ```

3. **Environment Variable:**
   Set the `STARFORGE_TELEMETRY` environment variable to `false` or `0` in your shell profile:
   ```bash
   export STARFORGE_TELEMETRY=false
   ```

To inspect your current telemetry status:
```bash
starforge telemetry status
```

---


## Configuration

starforge stores all data in `~/.starforge/config.toml`:

```toml
network = "testnet"

[[wallets]]
name = "alice"
public_key = "GABC...XYZ"
secret_key = "SABC...XYZ"  # plaintext or encrypted (see Security section)
network = "testnet"
created_at = "2025-01-01T00:00:00Z"
funded = true

[networks.testnet]
horizon_url = "https://horizon-testnet.stellar.org"
soroban_rpc_url = "https://soroban-testnet.stellar.org"
```

### Security

Secret keys can be stored **encrypted at rest** using the `--encrypt` flag during wallet creation:

```bash
starforge wallet create mykey --encrypt
# You will be prompted to set a secure passphrase
```

Encryption uses:
- **AES-256-GCM** for authenticated encryption
- **Argon2** for key derivation from your passphrase
- **Random salt and nonce** for each encryption operation

When revealing an encrypted key, you must provide the correct passphrase:

```bash
starforge wallet show mykey --reveal
# You will be prompted for the passphrase
```

Unencrypted keys (without `--encrypt`) are stored in plaintext and are suitable only for testnet or throwaway accounts. **Do not use plaintext keys on mainnet with real funds.**

### Test Environment Secret

Some tests validate secret-key parsing without embedding a secret in the repository. Set the value at runtime before running the test suite:

```powershell
$env:STARFORGE_TEST_SECRET_KEY = "S..."  # 56-character Stellar secret key
cargo test
```

Generate this value outside the codebase using your preferred secure workflow, such as a local Stellar key generation command or an existing throwaway test wallet. The key should live only in your shell environment or secret manager, not in source control.

### Telemetry & Privacy

starforge collects **anonymous telemetry** to help us improve the CLI. **No personal data is collected** — only command names, success/failure status, and execution time.

#### Disable Telemetry

If you prefer not to participate:

```bash
# Permanently disable telemetry
starforge config set telemetry false

# Or use an environment variable (useful for CI/CD)
export STARFORGE_TELEMETRY=0
```

**What's collected**: Command name, timestamp, success status, duration (milliseconds), and a random anonymous ID.

**What's NOT collected**: Wallet addresses, secret keys, contract code, configuration values, error messages, or personal information.

For detailed information, see [TELEMETRY_PRIVACY.md](./TELEMETRY_PRIVACY.md).

---

| Template | Description |
|----------|-------------|
| `hello-world` | Basic contract with a `hello(to)` function. Great starting point. |
| `token` | Fungible token scaffold with `initialize`, `mint`, `balance`, `transfer`. |
| `nft` | Non-fungible token scaffold with `mint`, `owner_of`, `transfer`. |
| `voting` | Proposal and voting contract with `create_proposal`, `vote`, `results`. |

All templates include a working test suite and a README with build/deploy instructions.

---

## Contributing

We welcome contributions from developers of all experience levels! Whether you're fixing a bug, adding a feature, or improving documentation, your work helps the Stellar ecosystem.

**New contributor?** Start here: [CONTRIBUTING.md](CONTRIBUTING.md) has everything you need to get set up and submit your first PR.

**Need a quick reference?** Check out [CONTRIBUTOR_QUICK_REFERENCE.md](CONTRIBUTOR_QUICK_REFERENCE.md) for common commands and patterns.

### Key Contribution Resources

| Resource | What it covers |
|----------|---|
| [CONTRIBUTING.md](CONTRIBUTING.md) | **Full contributor guide** — setup, building, testing, PR process |
| [CONTRIBUTOR_QUICK_REFERENCE.md](CONTRIBUTOR_QUICK_REFERENCE.md) | **Quick lookup** — common commands, project structure, troubleshooting |
| [CI_ENFORCEMENT.md](CI_ENFORCEMENT.md) | **CI pipeline** — formatting, linting, security, and test requirements |
| [CODE_STYLE_STANDARDS.md](CODE_STYLE_STANDARDS.md) | **Code style** — naming, documentation, linting rules, IDE setup |
| [DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md) | **Deep dive** — architecture, adding features, release process |

### Quick Start

1. Fork and clone the repository
2. Follow [CONTRIBUTING.md](CONTRIBUTING.md) to set up Rust and the project
3. Create a branch: `git checkout -b feat/issue-XXX-description`
4. Make your changes and run `cargo test`
5. Push and open a Pull Request with a clear description

### Rewards

This project participates in the **[Stellar Wave Program](https://www.drips.network/wave/stellar)** on Drips. Contributors who resolve issues during an active Wave earn Points that translate to real USDC rewards.

**Read the [Terms & Rules](https://docs.drips.network/wave/terms-and-rules) before contributing.**

---
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

### ?? Feature Documentation
- **[TEMPLATE_MARKETPLACE.md](TEMPLATE_MARKETPLACE.md)** - Template marketplace feature
- **[QUICK_START_TEMPLATES.md](QUICK_START_TEMPLATES.md)** - Template quick start guide
- **[docs/SIMULATION_RESOURCES.md](docs/SIMULATION_RESOURCES.md)** - CPU, memory, footprint, and minimum resource fees from simulation
- **[docs/CORRELATION_IDS.md](docs/CORRELATION_IDS.md)** - Correlating structured logs across one invocation
- **[docs/CONFIGURATION.md](docs/CONFIGURATION.md)** - Config parsing, overlay merging, and validation rules
- **[docs/WALLET_IMPORT_SECURITY.md](docs/WALLET_IMPORT_SECURITY.md)** - Limits enforced on untrusted wallet backups
- **[docs/DEPLOYMENT_CHECKPOINTS.md](docs/DEPLOYMENT_CHECKPOINTS.md)** - Resumable and idempotent deployment operations, session checkpointing, and staleness detection
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
