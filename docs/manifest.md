# StarForge Project Manifest (`starforge.toml`)

`starforge.toml` is the single source of truth for StarForge projects. It describes smart contracts, network configurations, deployment targets, init arguments, and custom task scripts per project, similar to `Cargo.toml` or `hardhat.config.js`.

---

## Overview

Previously, StarForge settings were spread across command-line flags, global user configuration (`~/.starforge/config.toml`), and ad hoc files. `starforge.toml` provides a single, versioned, shareable manifest committed to your repository so every team member and CI pipeline runs against identical configurations.

### Key Capabilities

- **Single Source of Truth**: Standardizes contracts, networks, deployment environments, init parameters, and scripts.
- **Upward Discovery**: Commands (`deploy`, `test`, `manifest validate`, etc.) discover `starforge.toml` upward from your current working directory.
- **Auto Generation**: `starforge new` scaffolds contracts and dApps with a pre-configured `starforge.toml`.
- **JSON Schema Validation**: Standardized JSON Schema for IDE completion, linting, and automated validation via `starforge manifest validate`.

---

## Schema Reference

### Top-Level Fields

| Field | Type | Description |
|---|---|---|
| `version` | `string` | **Required.** Manifest schema version. Must be `"1"`. |
| `package` | `table` | Optional project metadata. |
| `wasm_target` | `string` | Optional Soroban WASM target override. Defaults to the target supported by the active Rust toolchain. |
| `contracts` | `table` | Smart contract target specifications. |
| `networks` | `table` | Custom network endpoints and overrides. |
| `deploy` | `table` | Target deployment environments (`deploy.testnet`, `deploy.mainnet`, etc.). |
| `scripts` | `table` | Custom task script shortcuts. |

---

## Example `starforge.toml`

```toml
version = "1"
wasm_target = "wasm32v1-none"

[package]
name = "my_soroban_project"
version = "0.1.0"
description = "Soroban smart contract project"
authors = ["Rindi <kwarpojonathanrindi@gmail.com>"]
license = "MIT"

[contracts.my_contract]
path = "."
build = "starforge contract build"

[networks.testnet]
horizon_url = "https://horizon-testnet.stellar.org"
soroban_rpc_url = "https://soroban-testnet.stellar.org"
friendbot_url = "https://friendbot-testnet.stellar.org"
network_passphrase = "Test SDF Network ; October 2015"

[deploy.testnet]
network = "testnet"
contracts = ["my_contract"]
fee = 100
source_wallet = "alice"

[scripts]
build = "starforge contract build"
test = "cargo test"
deploy = "starforge deploy"
```

---

## CLI Commands

### `starforge manifest validate`

Validates your `starforge.toml` manifest against the JSON Schema and structural constraints.

```bash
starforge manifest validate [--path <path>]
```

### `starforge manifest schema`

Outputs or saves the JSON Schema for `starforge.toml`.

```bash
starforge manifest schema [--out starforge.schema.json]
```

### `starforge manifest init`

Generates a starter `starforge.toml` in your current directory.

```bash
starforge manifest init [--name <project_name>]
```

### `starforge manifest show`

Displays a summary or JSON output of the discovered manifest.

```bash
starforge manifest show [--json]
```

---

## Command Integration

### `starforge deploy`

When `--wasm` is omitted, `starforge deploy` discovers `starforge.toml` in the working directory or parent directories and automatically extracts contract WASM paths, target networks, and wallet settings.

```bash
# Deploys using target settings in starforge.toml
starforge deploy --network testnet
```

### `starforge test`

When `--wasm` is omitted, `starforge test` reads contract WASM paths directly from `starforge.toml`.

```bash
# Runs test suite using contracts defined in starforge.toml
starforge test
```

---

## JSON Schema

The official JSON Schema is saved at `docs/starforge.schema.json` and accessible via `starforge manifest schema`.
