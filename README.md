# ⚡ starforge

> A developer productivity CLI for Stellar and Soroban workflows — built in Rust.

![License: MIT](https://img.shields.io/badge/License-MIT-cyan.svg)
![Language: Rust](https://img.shields.io/badge/Language-Rust-orange.svg)
![Network: Stellar](https://img.shields.io/badge/Network-Stellar-blue.svg)
![Status: Active](https://img.shields.io/badge/Status-Active-green.svg)
![Stellar Wave](https://img.shields.io/badge/Stellar-Wave%20Program-blueviolet.svg)
![CI](https://github.com/Nanle-code/StarForge/actions/workflows/ci.yml/badge.svg)](https://github.com/Nanle-code/StarForge/actions/workflows/ci.yml)
![Release](https://github.com/Nanle-code/StarForge/actions/workflows/release.yml/badge.svg)](https://github.com/Nanle-code/StarForge/actions/workflows/release.yml)

---

## Overview

**starforge** is a free, open-source command-line toolkit for developers building on the Stellar network. It brings together the most common Stellar and Soroban developer workflows — wallet management, project scaffolding, and contract deployment — into a single fast, ergonomic CLI.

Think of it as the "Hardhat or Foundry" experience for the Stellar ecosystem, built in Rust for speed and reliability.

This project is actively maintained and participates in the [Stellar Wave Program](https://www.drips.network/wave/stellar) on Drips — a monthly open-source contribution sprint where contributors earn rewards for merged pull requests.

---

## Features

### 🔑 Wallet Management
Create and manage Stellar keypairs locally. Fund testnet accounts via Friendbot, list all saved wallets, inspect live on-chain balances, and securely store keys in `~/.starforge/config.toml`.

### ◻ Project Scaffolding
Scaffold new Soroban smart contract projects from battle-tested templates with one command. Available templates: `hello-world`, `token`, `nft`, and `voting`. Also scaffolds full Stellar dApp frontends (Vite + React).

### 🚀 Contract Deployment
Validate, size-check, and deploy compiled Soroban `.wasm` files to Testnet or Mainnet. Verifies account balance on-chain, calculates WASM hash, and generates the exact `stellar contract deploy` command to complete the deployment.

---

## Installation

### Prerequisites

- Rust ≥ 1.80 ([install via rustup](https://rustup.rs))

### Build from source

```bash
git clone https://github.com/YOUR_USERNAME/starforge.git
cd starforge
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

### Wallet commands

```bash
# Create a new keypair
starforge wallet create alice

# Create and fund immediately (testnet only)
starforge wallet create deployer --fund

# List all saved wallets
starforge wallet list

# Show wallet details + live balance
starforge wallet show alice

# Reveal secret key
starforge wallet show alice --reveal

# Fund an existing wallet via Friendbot
starforge wallet fund alice

# Remove a wallet
starforge wallet remove alice
```

### Scaffold commands

```bash
# Scaffold a Soroban contract (hello-world template)
starforge new contract my-contract

# Scaffold with a specific template
starforge new contract my-token --template token
starforge new contract my-nft --template nft
starforge new contract my-vote --template voting

# Scaffold a Stellar dApp frontend (Vite + React)
starforge new dapp my-dapp
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
```

### Contract commands

```bash
# Inspect a deployed contract instance
starforge contract inspect CCPYZFKEAXHHS5VVW5J45TOU7S2EODJ7TZNJIA5LKDVL3PESCES6FNCI

# Inspect on a specific network
starforge contract inspect CCPYZFKEAXHHS5VVW5J45TOU7S2EODJ7TZNJIA5LKDVL3PESCES6FNCI --network mainnet
```

### Environment info

```bash
starforge info
```

### Shell completions

```bash
# Bash — add to ~/.bashrc
source <(starforge completions bash)

# Zsh — add to ~/.zshrc
source <(starforge completions zsh)

# Fish — save to fish completions directory
starforge completions fish > ~/.config/fish/completions/starforge.fish
```

After adding the line to your shell config, restart your shell or run `source ~/.bashrc` / `source ~/.zshrc`. Tab-completion for all subcommands and flags will then be active.

---

## Project Structure

```
starforge/
├── Cargo.toml
└── src/
    ├── main.rs                  # CLI entry point + banner
    ├── commands/
    │   ├── mod.rs
    │   ├── wallet.rs            # wallet create/list/show/fund/remove
    │   ├── new.rs               # project scaffolding + templates
    │   ├── contract.rs          # contract inspect + invoke
    │   ├── deploy.rs            # contract deployment
    │   └── info.rs              # environment info
    └── utils/
        ├── mod.rs
        ├── config.rs            # ~/.starforge/config.toml read/write
        ├── horizon.rs           # Horizon API + Friendbot HTTP calls
        ├── soroban.rs           # Soroban RPC helpers
        └── print.rs             # Consistent CLI output helpers
```

---

## Configuration

starforge stores all data in `~/.starforge/config.toml`:

```toml
network = "testnet"

[[wallets]]
name = "alice"
public_key = "GABC...XYZ"
secret_key = "SABC...XYZ"
network = "testnet"
created_at = "2025-01-01T00:00:00Z"
funded = true
```

> ⚠️ Secret keys are stored in plaintext. Do not use wallets containing real mainnet funds for development purposes.

---

## Contract Templates

| Template | Description |
|----------|-------------|
| `hello-world` | Basic contract with a `hello(to)` function. Great starting point. |
| `token` | Fungible token scaffold with `initialize`, `mint`, `balance`, `transfer`. |
| `nft` | Non-fungible token scaffold with `mint`, `owner_of`, `transfer`. |
| `voting` | Proposal and voting contract with `create_proposal`, `vote`, `results`. |

All templates include a working test suite and a README with build/deploy instructions.

---

## Contributing

This project participates in the **[Stellar Wave Program](https://www.drips.network/wave/stellar)** on Drips. Contributors who resolve issues during an active Wave earn Points that translate to real USDC rewards.

**Read the [Terms & Rules](https://docs.drips.network/wave/terms-and-rules) before contributing.**

### How to contribute

1. Fork the repository
2. Create a branch: `git checkout -b feat/your-feature`
3. Make your changes and commit: `git commit -m "feat: description"`
4. Push and open a Pull Request against `main`

Please keep PRs scoped to a single issue and include a clear description of what changed and why.

---
## License

MIT © 2025 — See [LICENSE](./LICENSE) for details.

---

## Acknowledgements

Built for the Stellar ecosystem.
Participates in the [Stellar Wave Program](https://www.drips.network/wave/stellar) via [Drips](https://www.drips.network).
Powered by the [Stellar Horizon API](https://developers.stellar.org/api/horizon) and [Soroban](https://soroban.stellar.org).
