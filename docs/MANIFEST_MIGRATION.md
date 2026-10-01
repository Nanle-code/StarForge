# Migration Guide: Adopting `starforge.toml` Project Manifests

This guide assists existing StarForge projects in migrating from ad-hoc command line flags, global user settings, and legacy lockfiles (`starforge-project.toml`) to the new unified `starforge.toml` project manifest.

---

## Why Migrate?

Prior to issue #938, project configuration was fragmented across:
- Command-line flags (`--wasm`, `--network`, `--wallet`)
- Global user settings (`~/.starforge/config.toml`)
- Legacy lockfiles (`starforge-project.toml`)

`starforge.toml` provides a single source of truth committed to version control, ensuring all team members and automated CI pipelines run with exact contract target definitions, custom network parameters, deployment targets, and task scripts.

---

## Step-by-Step Migration Process

### Step 1: Initialize `starforge.toml`

Run `starforge manifest init` in your project root directory:

```bash
starforge manifest init --name <your-project-name>
```

This creates a default `starforge.toml` with `version = "1"`, starter `[package]`, `[contracts]`, `[networks]`, `[deploy]`, and `[scripts]` sections.

### Step 2: Configure Contract Targets

Map your contracts and WASM build outputs under `[contracts]`:

```toml
[contracts.my_contract]
path = "contracts/my_contract"
build = "starforge contract build"
```

### Step 3: Define Custom Networks and Deploy Targets

Move custom RPC URLs and deployment target parameters into `[networks]` and `[deploy]`:

```toml
[networks.custom_testnet]
horizon_url = "https://horizon-testnet.stellar.org"
soroban_rpc_url = "https://soroban-testnet.stellar.org"

[deploy.testnet]
network = "custom_testnet"
contracts = ["my_contract"]
fee = 100
source_wallet = "deployer"
```

### Step 4: Validate Your Manifest

Run the validation command:

```bash
starforge manifest validate
```

### Step 5: Update CLI Workflows

Once `starforge.toml` is committed:
- **`starforge deploy`**: You no longer need `--wasm target/...`. Running `starforge deploy` automatically reads `[contracts]` and `[deploy]` targets from `starforge.toml`.
- **`starforge test`**: Running `starforge test` automatically discovers contract WASM targets from `starforge.toml`.
- **`starforge-project.toml`**: Existing `starforge-project.toml` team lockfiles remain supported for global user overlay overrides, but `starforge.toml` takes priority for contract and build targets.

---

## Verification

Confirm your setup by running:

```bash
starforge manifest show
```
