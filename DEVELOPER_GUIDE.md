# StarForge Developer Guide

Complete guide for developers contributing to or extending StarForge.

## Table of Contents

1. [Plugin Version Compatibility](#plugin-version-compatibility)
2. [Getting Started](#getting-started)
3. [Development Setup](#development-setup)
4. [Project Structure](#project-structure)
5. [Code Style Guide](#code-style-guide)
6. [Adding New Features](#adding-new-features)
7. [Cargo.lock Reproducibility & Cross-Platform Lock](#cargolock-reproducibility--cross-platform-lock)
8. [Testing](#testing)
9. [Documentation](#documentation)
10. [Common Tasks](#common-tasks)
11. [Debugging](#debugging)
12. [Release Process](#release-process)
13. [Database Migrations](#database-migrations)

---

## Cargo.lock Reproducibility & Cross-Platform Lock

StarForge strictly enforces `Cargo.lock` reproducibility across all supported operating systems (Linux, macOS, Windows).

### Requirements & Principles

1. **Deterministic Builds**: Locked builds (`cargo build --locked` / `cargo check --locked`) must resolve identical dependency versions across Linux, macOS, and Windows.
2. **No Mutating Builds**: Running standard CI steps or local build commands must never mutate `Cargo.lock`.
3. **Out-of-Sync Prevention**: Modifying dependencies in `Cargo.toml` without updating `Cargo.lock` via `cargo update -p <crate>` will fail CI quality checks.

### Verification CLI Command

Developers can verify lockfile reproducibility locally prior to committing:

```bash
# Verify lockfile reproducibility for the current directory
starforge verify lockfile

# Verify lockfile in a specific workspace path with JSON output
starforge verify lockfile --path ./my-workspace --json
```

---

## Plugin Version Compatibility

StarForge enforces version compatibility when loading plugins to prevent subtle
runtime failures caused by ABI or API mismatches.

### How it works

Every plugin shared library must export a `PLUGIN_DECLARATION` symbol (provided
automatically by the `export_plugin!` macro).  When `starforge plugin load` runs,
the loader checks two fields in that declaration:

| Field | What is checked | Failure behaviour |
|---|---|---|
| `rustc_version` | Must match the exact rustc version used to build StarForge | Hard error — load aborted |
| `core_version` | **Major** version must match StarForge's own `CARGO_PKG_VERSION` | Hard error — load aborted |

The compatibility rule for `core_version` follows semantic versioning:

- `0.x.y` plugins are **only** compatible with a `0.x.y` StarForge core (major `0`).
- `1.x.y` plugins are **only** compatible with a `1.x.y` StarForge core (major `1`).
- Minor and patch bumps within the same major are considered backwards-compatible.

### Error messages

When a plugin fails the version check you will see a clear message, for example:

```
Error: Plugin version incompatibility in 'libmy_plugin.so':
  Plugin was built for StarForge 0.1.0
  Running StarForge 1.0.0

  The major version must match. Rebuild the plugin against
  StarForge 1.0.0 or install a compatible StarForge version.
  See DEVELOPER_GUIDE.md § "Plugin Version Compatibility" for details.
```

### Writing a compatible plugin

1. **Pin the StarForge version** in your plugin's `Cargo.toml`:

   ```toml
   [dependencies]
   # Use the same major version as the StarForge binary your users will run.
   starforge = "0.1"
   ```

2. **Use the `export_plugin!` macro** — it embeds both `rustc_version` and
   `core_version` automatically at compile time:

   ```rust
   use starforge::export_plugin;

   export_plugin!(register);

   fn register(registrar: &mut dyn starforge::plugins::PluginRegistrar) {
       registrar.register_plugin(Box::new(MyPlugin));
   }
   ```

3. **Rebuild when StarForge's major version changes.**  Check the running version
   with `starforge --version` and compare it to the version your plugin was built
   against (shown in `starforge plugin load` output under "Built for StarForge").

4. **Use the same Rust toolchain** as the StarForge binary.  The easiest way is
   to keep a `rust-toolchain.toml` in your plugin repo that mirrors the one in
   the StarForge repo.

### Checking compatibility without loading

```bash
# See which StarForge version is running
starforge --version

# See which version each installed plugin was built for
starforge plugin load
```

The `load` command prints a "Built for StarForge" line for every successfully
loaded plugin, and a descriptive error for any that fail the check.

---

## Getting Started

### Prerequisites

- **Rust**: 1.80 or higher ([install via rustup](https://rustup.rs))
- **Git**: For version control
- **Stellar CLI**: For contract operations (optional)
- **Docker**: For containerized development (optional)

### Clone and Build

```bash
# Clone repository
git clone https://github.com/YOUR_USERNAME/starforge.git
cd starforge

# Build in debug mode
cargo build

# Build in release mode
cargo build --release

# Run tests
cargo test

# Run with logging
RUST_LOG=debug cargo run -- wallet list
```

---

## Development Setup

### IDE Configuration

#### VS Code

Recommended extensions:

- `rust-analyzer` - Rust language support
- `crates` - Cargo.toml dependency management
- `Better TOML` - TOML syntax highlighting
- `Error Lens` - Inline error display

`.vscode/settings.json`:

```json
{
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.cargo.features": "all",
  "editor.formatOnSave": true
}
```

#### IntelliJ IDEA / CLion

Install the Rust plugin and configure:

- Enable Clippy for code analysis
- Set rustfmt as formatter
- Enable external linter

### Environment Variables

```bash
# Enable debug logging
export RUST_LOG=debug

# Disable telemetry during development
export STARFORGE_TELEMETRY=false

# Use custom config directory
export STARFORGE_CONFIG_DIR=~/.starforge-dev
```

### Secret Redaction & Security Logging
StarForge enforces centralized secret redaction via `crate::utils::redaction::redact_secrets`. Tracing output streams (`RUST_LOG`) and CLI error output streams automatically sanitize Stellar secret keys (`S...`), hex private keys, BIP-39 mnemonic seed phrases, auth tokens (`Bearer`, `ghp_`, `sk-`), signed XDR transaction payloads, and embedded URL credentials before output. Existing helper functions (`redact_public_key`, `redact_secret_value`, `redact_signed_xdr`) delegate to this centralized engine.

### Password-Based Encryption & KDF Parameter Tuning

StarForge encrypts Stellar secret keys at rest using **Argon2id** key derivation and **AES-256-GCM** authenticated encryption.

#### KDF Versioning & Schema Formats

- **Version 1 (`KDF_VERSION_1 = 1`)**: Argon2id + AES-256-GCM.
- **Bundle Formats**:
  - Legacy 3-part: `salt:nonce:ciphertext` (library defaults: 32,768 KiB memory, 3 iterations, 1 parallelism thread).
  - 5-part: `salt:nonce:ciphertext:mem:iterations` (custom memory cost and iteration count).
  - 6-part: `salt:nonce:ciphertext:mem:iterations:parallelism` (custom memory, iterations, and parallelism).
  - Versioned 7-part: `v1:salt:nonce:ciphertext:mem:iterations:parallelism` (explicit version prefixing for modern tuned bundles).

#### Parameter Bounds & Safety Constraints

- **Memory Cost (`mem`)**: Min 8,192 KiB (8 MiB), Max 2,097,152 KiB (2 GiB).
- **Iterations (`iterations`)**: Min 1, Max 100.
- **Parallelism (`parallelism`)**: Min 1, Max 64 threads.

#### Per-Wallet Metadata & Safe Upgrades

KDF parameters are stored per wallet (`WalletEntry.kdf_options` and metadata embedded in `secret_key`). Wallet encryption parameters can be tuned or upgraded safely without data loss using:

```bash
# Tune KDF parameters for a specific wallet
starforge wallet tune-kdf alice --mem 65536 --iterations 4 --parallelism 2

# Upgrade wallet KDF to global configuration settings
starforge wallet tune-kdf alice --use-global
```

The upgrade procedure enforces zero-data-loss safety:
1. Validates existing password against current bundle before making any changes.
2. Validates new KDF parameters against security bounds.
3. Re-encrypts secret key with new parameters.
4. Performs a verification decryption round-trip on the new bundle before persisting changes to disk and database.
5. If any validation or decryption step fails, the original encrypted secret and metadata remain completely unchanged.


### Development Workflow

```bash
# 1. Create feature branch
git checkout -b feature/my-feature

# 2. Make changes
# ... edit files ...

# 3. Run tests
cargo test

# 4. Check formatting
cargo fmt --check

# 5. Run clippy
cargo clippy -- -D warnings

# 6. Build
cargo build

# 7. Test manually
cargo run -- <command>

# 8. Run smoke tests (optional but recommended)
./scripts/e2e-smoke.sh

# 9. Commit
git add .
git commit -m "feat: add my feature"

# 10. Push and create PR
git push origin feature/my-feature
```

---

## Project Structure

### Source Code Organization

```
src/
├── main.rs              # Entry point
├── commands/            # User-facing commands
│   ├── mod.rs          # Module exports
│   ├── wallet.rs       # Wallet operations
│   ├── template.rs     # Template marketplace
│   └── ...
├── utils/               # Shared utilities
│   ├── mod.rs          # Module exports
│   ├── config.rs       # Configuration
│   ├── templates.rs    # Template system
│   └── ...
└── plugins/             # Plugin system
    ├── mod.rs          # Module exports
    ├── interface.rs    # Plugin traits
    └── ...
```

### File Naming Conventions

- **Commands**: `<noun>.rs` (e.g., `wallet.rs`, `network.rs`)
- **Utilities**: `<function>.rs` (e.g., `config.rs`, `crypto.rs`)
- **Tests**: `<module>_test.rs` or inline `#[cfg(test)] mod tests`

### Module Organization

Each module should follow this structure:

```rust
// 1. Imports
use crate::utils::config;
use anyhow::Result;

// 2. Type definitions
pub struct MyStruct { /* ... */ }
pub enum MyEnum { /* ... */ }

// 3. Public API
pub fn public_function() -> Result<()> { /* ... */ }

// 4. Private helpers
fn private_helper() { /* ... */ }

// 5. Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() { /* ... */ }
}
```

---

## Code Style Guide

### Rust Style

Follow the [Rust Style Guide](https://doc.rust-lang.org/1.0.0/style/):

```rust
// ✅ Good
pub fn create_wallet(name: String, encrypt: bool) -> Result<()> {
    validate_name(&name)?;
    let keypair = generate_keypair();
    save_wallet(name, keypair, encrypt)
}

// ❌ Bad
pub fn CreateWallet(Name: String, Encrypt: bool) -> Result<()> {
    ValidateName(&Name)?;
    let KeyPair = GenerateKeypair();
    SaveWallet(Name, KeyPair, Encrypt)
}
```

### Naming Conventions

| Type      | Convention             | Example           |
| --------- | ---------------------- | ----------------- |
| Functions | `snake_case`           | `fetch_account()` |
| Types     | `PascalCase`           | `WalletEntry`     |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_RETRIES`     |
| Modules   | `snake_case`           | `hardware_wallet` |
| Lifetimes | `'lowercase`           | `'a`, `'static`   |

### Error Handling

```rust
// ✅ Use Result and ? operator
pub fn operation() -> Result<()> {
    let data = load_data()?;
    process_data(data)?;
    Ok(())
}

// ✅ Add context to errors
fs::read_to_string(&path)
    .with_context(|| format!("Failed to read {}", path.display()))?

// ❌ Don't use unwrap() in production code
let data = load_data().unwrap(); // Bad!

// ✅ Use expect() with clear message for programmer errors
let data = load_data()
    .expect("Config should be initialized in main()");
```

### Documentation

````rust
/// Fetches account information from Horizon API.
///
/// # Arguments
///
/// * `public_key` - The Stellar public key (G...)
/// * `network` - Network name ("testnet" or "mainnet")
///
/// # Returns
///
/// Returns `AccountResponse` with balance and sequence information.
///
/// # Errors
///
/// Returns error if:
/// - Account doesn't exist on the network
/// - Network is unreachable
/// - Response parsing fails
///
/// # Example
///
/// ```
/// let account = fetch_account("GABC...", "testnet")?;
/// println!("Balance: {}", account.balances[0].balance);
/// ```
pub fn fetch_account(public_key: &str, network: &str) -> Result<AccountResponse> {
    // Implementation
}
````

### Comments

```rust
// ✅ Explain WHY, not WHAT
// Use shallow clone to reduce bandwidth and disk usage
git_clone(&url, "--depth", "1");

// ❌ Don't state the obvious
// Clone the repository
git_clone(&url);

// ✅ TODO comments with context
// TODO(username): Add retry logic after implementing exponential backoff

// ❌ Vague TODOs
// TODO: fix this
```

---

## Adding New Features

### 1. Adding a New Command

**Step 1**: Create command file

```bash
touch src/commands/mycommand.rs
```

**Step 2**: Define command structure

```rust
// src/commands/mycommand.rs
use anyhow::Result;
use clap::Subcommand;
use crate::utils::print as p;

#[derive(Subcommand)]
pub enum MyCommands {
    /// Do something useful
    Action {
        /// Input parameter
        #[arg(long)]
        input: String,
    },
}

pub fn handle(cmd: MyCommands) -> Result<()> {
    match cmd {
        MyCommands::Action { input } => action(input),
    }
}

fn action(input: String) -> Result<()> {
    p::header("My Command");
    p::kv("Input", &input);

    // Your logic here

    p::success("Done!");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action() {
        // Test your command
    }
}
```

**Step 3**: Register in mod.rs

```rust
// src/commands/mod.rs
pub mod mycommand;
```

**Step 4**: Add to main CLI

```rust
// src/main.rs
#[derive(Subcommand)]
enum Commands {
    // ... existing commands

    /// My new command
    #[command(subcommand)]
    MyCommand(commands::mycommand::MyCommands),
}

// In main():
let result = match cli.command {
    // ... existing matches
    Commands::MyCommand(cmd) => commands::mycommand::handle(cmd),
};
```

**Step 5**: Update documentation

```bash
# Update README.md with new command
# Add examples to examples/ directory
# Update ARCHITECTURE.md if needed
```

### 2. Adding a New Utility Module

**Step 1**: Create utility file

```bash
touch src/utils/myutil.rs
```

**Step 2**: Implement functionality

```rust
// src/utils/myutil.rs
use anyhow::Result;

/// Does something useful
pub fn do_something(input: &str) -> Result<String> {
    // Implementation
    Ok(format!("Processed: {}", input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_do_something() {
        let result = do_something("test").unwrap();
        assert_eq!(result, "Processed: test");
    }
}
```

**Step 3**: Register in mod.rs

```rust
// src/utils/mod.rs
pub mod myutil;
```

**Step 4**: Use in commands

```rust
use crate::utils::myutil;

fn my_command() -> Result<()> {
    let result = myutil::do_something("input")?;
    println!("{}", result);
    Ok(())
}
```

### 3. Adding Template Support

**Step 1**: Create template directory

```bash
mkdir -p templates/examples/my-template/src
```

**Step 2**: Add template files

```toml
# templates/examples/my-template/Cargo.toml
[package]
name = "{{PROJECT_NAME}}"
version = "0.1.0"
edition = "2021"

[dependencies]
soroban-sdk = "21.0.0"
```

```rust
// templates/examples/my-template/src/lib.rs
#![no_std]
use soroban_sdk::{contract, contractimpl, Env};

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    pub fn hello(env: Env) -> String {
        String::from_str(&env, "Hello from {{PROJECT_NAME}}")
    }
}
```

**Step 3**: Add to registry

```json
// templates/registry.json
{
  "templates": [
    {
      "name": "my-template",
      "version": "1.0.0",
      "description": "My awesome template",
      "author": "Your Name",
      "tags": ["example"],
      "source": {
        "type": "local",
        "path": "templates/examples/my-template"
      },
      "created_at": "2025-01-01T00:00:00Z",
      "updated_at": "2025-01-01T00:00:00Z",
      "downloads": 0,
      "verified": false
    }
  ]
}
```

---

## Testing

### Unit Tests

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_functionality() {
        let result = my_function("input");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "expected");
    }

    #[test]
    fn test_error_case() {
        let result = my_function("");
        assert!(result.is_err());
    }

    #[test]
    #[should_panic(expected = "Invalid input")]
    fn test_panic() {
        panic_function();
    }
}
```

### Integration Tests

```rust
// tests/integration_test.rs
use starforge::utils::config;
use tempfile::TempDir;

#[test]
fn test_config_lifecycle() {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("config.toml");

    // Test save
    let config = config::Config::default();
    config::save(&config).unwrap();

    // Test load
    let loaded = config::load().unwrap();
    assert_eq!(loaded.version, config.version);
}
```

### CLI smoke tests

Fast regression checks for core commands live in `tests/cli_smoke.rs` and
`scripts/e2e-smoke.sh`. CI runs both after every build:

```bash
cargo test --test cli_smoke
./scripts/e2e-smoke.sh
STARFORGE_E2E=1 ./scripts/e2e-smoke.sh   # optional network checks
```

### Running Tests

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run tests with output
cargo test -- --nocapture

# Run tests in specific module
cargo test config::tests

# Run integration tests only
cargo test --test integration_test

# Run with coverage (requires tarpaulin)
cargo tarpaulin --out Html
```

### End-to-End Smoke Tests

StarForge includes an end-to-end smoke test script that verifies basic functionality across all major commands.

**Location**: `scripts/e2e-smoke.sh`

**Running Smoke Tests**:

```bash
# Build the project first
cargo build --release

# Run smoke tests (without network tests)
./scripts/e2e-smoke.sh

# Run smoke tests with network tests (requires internet)
STARFORGE_E2E=1 ./scripts/e2e-smoke.sh
```

**What the smoke test covers**:

1. **Basic Commands**
   - `starforge info` - System information
   - `starforge --version` - Version display
   - `starforge --help` - Help text

2. **Wallet Operations**
   - `wallet create` - Create test wallet
   - `wallet list` - List wallets
   - `wallet show` - Display wallet details

3. **Network Operations**
   - `network show` - Display network configuration
   - `network test` - Test network connectivity (requires `STARFORGE_E2E=1`)
   - `wallet fund` - Fund testnet wallet (requires `STARFORGE_E2E=1`)

4. **Template Operations**
   - `template list` - List available templates
   - `template search` - Search templates

5. **Other Commands**
   - `completions` - Generate shell completions

**Network Test Gating**:

Network tests are gated behind the `STARFORGE_E2E=1` environment variable because they:
- Require internet connectivity
- Depend on external services (Stellar testnet, Friendbot)
- May be slow or flaky in CI environments
- Can hit rate limits

To skip network tests in CI:

```yaml
# .github/workflows/ci.yml
- name: Run smoke tests
  run: ./scripts/e2e-smoke.sh  # Skips network tests by default
```

To run full tests locally:

```bash
STARFORGE_E2E=1 ./scripts/e2e-smoke.sh
```

**Exit Codes**:
- `0` - All tests passed
- `1` - One or more tests failed

**Cleanup**:

The smoke test automatically cleans up test wallets on exit. If cleanup fails, you may need to manually remove test wallets:

```bash
# List wallets to find test wallets
starforge wallet list

# Remove test wallet (when delete command is implemented)
# starforge wallet delete smoke-test-<timestamp>
```

### Test Organization

```
tests/
├── integration_test.rs      # Integration tests
├── template_test.rs          # Template-specific tests
└── common/
    └── mod.rs               # Shared test utilities
```

---

## Documentation

### Code Documentation

```rust
/// Module-level documentation
///
/// This module handles wallet operations including creation,
/// listing, and management of Stellar keypairs.

/// Function documentation
///
/// Creates a new wallet with the given name.
///
/// # Arguments
///
/// * `name` - Wallet identifier
/// * `encrypt` - Whether to encrypt the secret key
///
/// # Returns
///
/// Returns `Ok(())` on success, or an error if:
/// - Wallet name already exists
/// - Keypair generation fails
/// - Config save fails
pub fn create_wallet(name: String, encrypt: bool) -> Result<()> {
    // Implementation
}
```

### User Documentation

Update these files when adding features:

1. **README.md** - Main documentation, usage examples
2. **ARCHITECTURE.md** - Architecture and design decisions
3. **DEVELOPER_GUIDE.md** - This file
4. **Feature-specific docs** - Detailed feature documentation

### Documentation Standards

- Use clear, concise language
- Include code examples
- Explain WHY, not just WHAT
- Keep examples up-to-date
- Add diagrams for complex flows
- Update [docs/COMMAND_REFERENCE.md](docs/COMMAND_REFERENCE.md) when adding or renaming CLI subcommands

### Command cheat sheet (auto-generated)

[docs/COMMAND_CHEATSHEET.md](docs/COMMAND_CHEATSHEET.md) is **auto-generated from clap
command metadata** by the crate's `build.rs`. It is committed so
it can be linked from the README and the docs site, but you must **never edit it by hand**.

**Regenerating the cheat sheet**

When you add, rename, or remove a top-level subcommand, or change its one-line
description, update the clap metadata (the `Commands` enum and `MAJOR_SUBCOMMANDS`
table in `build.rs`) and then regenerate:

```bash
cargo build          # build.rs rewrites docs/COMMAND_CHEATSHEET.md
git add docs/COMMAND_CHEATSHEET.md build.rs
git commit
```

> If the committed cheat sheet is out of date, CI fails the
> `Docs Cheat Sheet (anti-drift)` check with a `git diff --exit-code` error.
> Note: hidden commands (`#[command(hide)]`) and internal commands listed in
> `INTERNAL_COMMANDS` (`external`, `autocomplete`, `man`, `feature-flags`, `help`)
> are excluded from the cheat sheet consistently.

---

## Common Tasks

### Adding a Dependency

```bash
# Add to Cargo.toml
cargo add <crate-name>

# Add with specific version
cargo add <crate-name>@1.0.0

# Add with features
cargo add <crate-name> --features feature1,feature2

# Add as dev dependency
cargo add --dev <crate-name>
```

### Updating Dependencies

```bash
# Update all dependencies
cargo update

# Update specific dependency
cargo update <crate-name>

# Check for outdated dependencies
cargo outdated
```

### Running Clippy

```bash
# Run clippy
cargo clippy

# Deny all warnings
cargo clippy -- -D warnings

# Fix automatically (when possible)
cargo clippy --fix
```

### Formatting Code

```bash
# Format all code
cargo fmt

# Check formatting without changing
cargo fmt --check

# Format specific file
rustfmt src/main.rs
```

### Regenerating Shell Completions

Shell completion scripts are generated by `build.rs` into the `completions/` directory.

```bash
# Regenerate completions (bash/zsh/fish)
cargo build
```

### Building Documentation

```bash
# Build docs
cargo doc

# Build and open in browser
cargo doc --open

# Include private items
cargo doc --document-private-items
```

### Benchmarking

```bash
# Run benchmarks
cargo bench

# Run specific benchmark
cargo bench benchmark_name

# Save baseline
cargo bench -- --save-baseline my-baseline

# Compare to baseline
cargo bench -- --baseline my-baseline
```

### Running with Docker Soroban Sandbox

The `shell` command supports connecting to a local Soroban sandbox via Docker:

```bash
# Start the interactive shell against a local Docker Soroban sandbox
starforge shell --contract ./target/wasm32-unknown-unknown/release/my_contract.wasm --network docker-testnet
```

When `--network docker-testnet` is used, StarForge:
1. Ensures the Docker containers defined in `docker-compose.yml` are running (includes `stellar-testnet` and `soroban-rpc`)
2. Runs contract invocations inside the Docker network where the Soroban RPC is available at `http://soroban-rpc:8000`
3. Routes all RPC calls through the local sandbox instead of Stellar testnet

The `docker-compose.yml` at the project root defines:
- **stellar-testnet**: A full Stellar + Soroban RPC node on `localhost:8000`
- **soroban-rpc**: Dedicated Soroban RPC endpoint on `localhost:8001`

Prerequisites:
- Docker and docker-compose installed
- Docker daemon running

---

## Debugging

### Debug Logging

```rust
// Add to Cargo.toml
[dependencies]
log = "0.4"
env_logger = "0.10"

// In main.rs
env_logger::init();

// In code
use log::{debug, info, warn, error};

debug!("Debug message: {:?}", value);
info!("Info message");
warn!("Warning message");
error!("Error message");
```

### Running with Debug Output

```bash
# Enable all debug logs
RUST_LOG=debug cargo run -- wallet list

# Enable specific module
RUST_LOG=starforge::commands::wallet=debug cargo run -- wallet list

# Multiple modules
RUST_LOG=starforge::commands=debug,starforge::utils=info cargo run
```

### Using rust-gdb

```bash
# Build with debug symbols
cargo build

# Run with gdb
rust-gdb target/debug/starforge

# Set breakpoint
(gdb) break src/main.rs:42

# Run
(gdb) run wallet list

# Step through
(gdb) step
(gdb) next

# Print variable
(gdb) print variable_name
```

### Common Issues

**Issue**: Compilation errors after updating dependencies

```bash
# Solution: Clean and rebuild
cargo clean
cargo build
```

**Issue**: Tests failing intermittently

```bash
# Solution: Run tests serially
cargo test -- --test-threads=1
```

**Issue**: Slow compilation

```bash
# Solution: Use sccache
cargo install sccache
export RUSTC_WRAPPER=sccache
```

---

## Release Process

### Version Bumping

1. Update version in `Cargo.toml`
2. Update version in `src/main.rs` (if hardcoded)
3. Update CHANGELOG.md
4. Commit: `git commit -m "chore: bump version to X.Y.Z"`

### Creating a Release

```bash
# 1. Tag the release
git tag -a v0.2.0 -m "Release v0.2.0"

# 2. Push tag
git push origin v0.2.0

# 3. Build release binaries
cargo build --release

# 4. Create GitHub release
# - Go to GitHub releases
# - Create new release from tag
# - Upload binaries
# - Add release notes
```

### Release Checklist

- [ ] All tests passing
- [ ] Documentation updated
- [ ] CHANGELOG.md updated
- [ ] Version bumped
- [ ] Release notes prepared
- [ ] Binaries built for all platforms
- [ ] GitHub release created
- [ ] Announcement posted

---

## Best Practices

### 1. Error Handling

```rust
// ✅ Use Result for fallible operations
pub fn operation() -> Result<()> {
    let data = load_data()?;
    process(data)?;
    Ok(())
}

// ✅ Add context to errors
load_data()
    .with_context(|| "Failed to load configuration")?

// ✅ Create custom error types for complex cases
#[derive(Debug, thiserror::Error)]
pub enum MyError {
    #[error("Invalid input: {0}")]
    InvalidInput(String),
    #[error("Network error")]
    Network(#[from] ureq::Error),
}
```

### 2. Configuration Management

```rust
// ✅ Load config once, pass as reference
let config = config::load()?;
process_with_config(&config)?;

// ❌ Don't reload config repeatedly
fn process() {
    let config = config::load().unwrap(); // Bad!
    // ...
}
```

### 3. User Feedback

```rust
// ✅ Provide progress indicators
p::step(1, 3, "Loading configuration...");
p::step(2, 3, "Processing data...");
p::step(3, 3, "Saving results...");

// ✅ Show helpful error messages
anyhow::bail!(
    "Wallet '{}' not found.\n\nTry: starforge wallet list",
    name
);
```

### 4. Testing

```rust
// ✅ Test edge cases
#[test]
fn test_empty_input() { /* ... */ }

#[test]
fn test_invalid_input() { /* ... */ }

#[test]
fn test_boundary_conditions() { /* ... */ }

// ✅ Use descriptive test names
#[test]
fn creates_wallet_with_encrypted_key_when_encrypt_flag_is_true() {
    // ...
}
```

---

## Database Migrations

StarForge uses a transactional database migration system to manage schema changes over time. This ensures that database upgrades are safe, reversible, and can be rolled back if needed.

### Overview

The migration system is implemented in `src/utils/database.rs` and provides:

- **Version tracking**: Each schema change has a unique version number
- **Transaction safety**: Migrations run within SQLite transactions for atomicity
- **Rollback capability**: Failed migrations can be rolled back to the previous version
- **Migration history**: All applied migrations are recorded in the `schema_migrations` table
- **Checksum validation**: Each migration has a checksum to detect changes

### Current Schema Version

The current schema version is defined by `CURRENT_SCHEMA_VERSION` constant in `src/utils/database.rs`.

### Adding a New Migration

When you need to modify the database schema, follow these steps:

1. **Increment the schema version**

   Update `CURRENT_SCHEMA_VERSION` in `src/utils/database.rs`:

   ```rust
   pub const CURRENT_SCHEMA_VERSION: i64 = 2; // Increment from 1 to 2
   ```

2. **Implement the migration struct**

   Add a new migration struct that implements the `Migration` trait:

   ```rust
   struct MigrationV2;

   impl Migration for MigrationV2 {
       fn version(&self) -> i64 {
           2
       }
       
       fn description(&self) -> &str {
           "add_wallet_encryption_index"
       }
       
       fn up(&self, conn: &mut Connection) -> Result<()> {
           // Apply schema changes
           conn.execute(
               "CREATE INDEX IF NOT EXISTS idx_wallets_encryption ON wallets(encryption_status)",
               [],
           )?;
           Ok(())
       }
       
       fn down(&self, conn: &mut Connection) -> Result<()> {
           // Rollback schema changes
           conn.execute(
               "DROP INDEX IF EXISTS idx_wallets_encryption",
               [],
           )?;
           Ok(())
       }
   }
   ```

3. **Register the migration**

   Add the migration to the `get_migration` method:

   ```rust
   fn get_migration(&self, version: i64) -> Option<Box<dyn Migration>> {
       match version {
           1 => Some(Box::new(MigrationV1 {})),
           2 => Some(Box::new(MigrationV2 {})), // Add this line
           _ => None,
       }
   }
   ```

4. **Write tests**

   Add tests for your migration in the `tests` module:

   ```rust
   #[test]
   fn migration_v2_adds_index() {
       let db = in_memory_db();
       let migration = MigrationV2 {};
       let mut conn = db.conn;
       
       migration.up(&mut conn).unwrap();
       
       // Verify the index exists
       let index_exists: bool = conn
           .query_row(
               "SELECT COUNT(*) > 0 FROM sqlite_master WHERE type='index' AND name='idx_wallets_encryption'",
               [],
               |row| row.get(0),
           )
           .unwrap();
       assert!(index_exists);
   }
   ```

### Migration Lifecycle

1. **Fresh Database**: When a new database is created, it starts at the current schema version
2. **Existing Database**: On startup, the system checks the current version and applies any pending migrations
3. **Migration Application**: Each migration runs in a transaction. If it fails, the transaction is rolled back
4. **Migration Recording**: Successful migrations are recorded in `schema_migrations` table
5. **Rollback**: The latest migration can be rolled back using `rollback_migration()`

### Migration API

Key methods in the `Database` struct:

- `get_current_schema_version()` - Returns the current schema version
- `get_applied_migrations()` - Returns all applied migrations
- `run_migrations()` - Applies pending migrations to reach current version
- `rollback_migration(version)` - Rolls back a specific migration

### Best Practices

- **Always implement `down()`**: Every migration must have a rollback implementation
- **Keep migrations idempotent**: Use `IF NOT EXISTS` and similar patterns
- **Test thoroughly**: Test both `up()` and `down()` methods
- **Document changes**: Update this guide when adding migrations
- **Version monotonicity**: Versions must always increase, never reuse old numbers
- **Transaction safety**: All schema changes should be within the migration transaction

### Troubleshooting

**Migration fails to apply**:
- Check the error message for specific SQL or logic errors
- Verify the migration's `up()` method is correct
- Ensure the database is not locked by another process

**Rollback fails**:
- Ensure you're rolling back the latest migration only
- Check that the `down()` method correctly reverses the `up()` changes
- Verify no data constraints prevent rollback

**Schema version mismatch**:
- Check `CURRENT_SCHEMA_VERSION` matches your expectations
- Verify all migrations are properly registered in `get_migration()`
- Review the `schema_migrations` table for applied versions

### Testing Migrations

Run migration-specific tests:

```bash
cargo test --lib database::tests::migration
```

Test with a real database:

```bash
# Backup your database first
cp ~/.starforge/starforge.db ~/.starforge/starforge.db.backup

# Run the application to trigger migrations
starforge wallet list

# Check the schema version
starforge db stats
```

---

## Configuration Schema Migrations

StarForge configuration stored in `~/.starforge/config.toml` uses explicit, versioned schema migrations managed by `src/utils/config.rs`.

### Architecture

- `CURRENT_CONFIG_VERSION`: Constant (`"1"`) defining the latest supported schema version.
- `run_config_migrations()`: Entry point that compares the config version with `CURRENT_CONFIG_VERSION`.
- `ConfigMigrationError`: Custom error enum with `FromFuture`, `UnknownVersion`, `StepFailed`, and `BackupFailed` variants.
- `MigrationReport`: Detailed report returned with `from_version`, `to_version`, `steps_applied`, and `backup_path`.

### Safe Execution & Backup Policy

Before any migration steps run, a timestamped backup is automatically created:
`~/.starforge/config.backup.v<version>.<timestamp>.toml`. If backup creation fails, migration is immediately aborted to guarantee zero data loss.

### Adding a New Migration Step

1. Update `CURRENT_CONFIG_VERSION` in `src/utils/config.rs`.
2. Implement `fn migrate_vN_to_vM(config: &mut Config)`.
3. Add a new `ConfigMigrationStep` entry to `MIGRATION_STEPS` in `src/utils/config.rs`.
4. Add integration tests in `tests/config_migrations.rs`.

### Testing Config Migrations

Run the integration test suite:

```bash
cargo test --test config_migrations
```

## Contributing Guidelines

### Pull Request Process

1. **Fork** the repository
2. **Create** a feature branch
3. **Make** your changes
4. **Test** thoroughly
5. **Document** your changes
6. **Submit** a pull request

### PR Checklist

- [ ] Code follows style guide
- [ ] Tests added/updated
- [ ] Documentation updated
- [ ] Commit messages follow convention
- [ ] No merge conflicts
- [ ] CI passes

### Commit Message Convention

```
<type>(<scope>): <subject>

<body>

<footer>
```

Types:

- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation
- `style`: Formatting
- `refactor`: Code restructuring
- `test`: Adding tests
- `chore`: Maintenance

Examples:

```
feat(wallet): add hardware wallet support

Implements Ledger and Trezor integration for secure key storage.

Closes #123
```

---

## Resources

### Documentation

- [Rust Book](https://doc.rust-lang.org/book/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [Stellar Documentation](https://developers.stellar.org/)
- [Soroban Documentation](https://soroban.stellar.org/)

### Tools

- [rust-analyzer](https://rust-analyzer.github.io/) - IDE support
- [clippy](https://github.com/rust-lang/rust-clippy) - Linter
- [rustfmt](https://github.com/rust-lang/rustfmt) - Formatter
- [cargo-edit](https://github.com/killercup/cargo-edit) - Dependency management

### Community

- [Stellar Discord](https://discord.gg/stellar)
- [Rust Users Forum](https://users.rust-lang.org/)
- [GitHub Discussions](https://github.com/YOUR_USERNAME/starforge/discussions)

---

## Getting Help

- **Issues**: [GitHub Issues](https://github.com/YOUR_USERNAME/starforge/issues)
- **Discussions**: [GitHub Discussions](https://github.com/YOUR_USERNAME/starforge/discussions)
- **Discord**: Join the Stellar Discord
- **Email**: maintainer@example.com

---

**Happy Coding! 🚀**
