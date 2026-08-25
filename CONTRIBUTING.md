# Contributing to StarForge

Welcome to StarForge! This guide will help you get started contributing to the project. We appreciate your interest in making StarForge better.

## Table of Contents

- [Quick Start](#quick-start)
- [Prerequisites](#prerequisites)
- [Development Setup](#development-setup)
- [Building the Project](#building-the-project)
- [Running Tests](#running-tests)
- [Development Workflow](#development-workflow)
- [Code Quality](#code-quality)
- [Submitting a Pull Request](#submitting-a-pull-request)
- [Contributing to AI Features](#contributing-to-ai-features)
- [Common Issues & Troubleshooting](#common-issues--troubleshooting)
- [Questions & Support](#questions--support)

---

## Quick Start

1. **Fork and clone** the repository
2. **Install Rust** (if not already installed)
3. **Build the project**: `cargo build`
4. **Run tests**: `cargo test`
5. **Create a branch**: `git checkout -b feat/your-feature-name`
6. **Make changes** and commit with clear messages
7. **Push and open a Pull Request** against `master`

---

## Prerequisites

### Rust

StarForge requires **Rust 1.80 or later**. 

#### Install Rust

If you don't have Rust installed, use [rustup](https://rustup.rs) — the official Rust toolchain installer:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

After installation, verify your version:

```bash
rustc --version
cargo --version
```

#### Update Rust (if already installed)

```bash
rustup update stable
```

### Additional Tools

- **Git** for version control
- **A text editor or IDE** (VS Code, IntelliJ, Vim, etc.)

---

## Development Setup

### Clone the Repository

```bash
git clone https://github.com/Nanle-code/StarForge.git
cd StarForge
```

### Verify Your Setup

Run the info command to check your environment:

```bash
cargo build
cargo run -- info
```

You should see output with your Rust version and system information.

---

## Building the Project

### Build for Development (Debug)

```bash
cargo build
```

This produces an unoptimized binary in `target/debug/starforge`. Builds are fast, useful during development.

### Build for Release (Optimized)

```bash
cargo build --release
```

This produces an optimized binary in `target/release/starforge`. Builds are slower but the binary is much faster.

### Install Locally

After building, you can install the binary to your PATH:

```bash
# From debug build
cp target/debug/starforge ~/.local/bin/

# Or from release build
cp target/release/starforge ~/.local/bin/

# Then verify installation
starforge --version
```

---

## Running Tests

### Run All Tests

```bash
cargo test
```

This runs all unit tests, integration tests, and doc tests.

### Run Tests with Output

If you want to see `println!` output from passing tests:

```bash
cargo test -- --nocapture
```

### Run Specific Tests

```bash
# Run a single test
cargo test test_wallet_create

# Run tests matching a pattern
cargo test wallet

# Run integration tests from a specific file
cargo test --test wallet_lifecycle_e2e
```

### Run Tests in Parallel

Tests run in parallel by default. To run sequentially (useful for debugging):

```bash
cargo test -- --test-threads=1 --nocapture
```

### Run Smoke Tests

The project includes quick smoke tests to verify basic functionality:

```bash
cargo test --test cli_smoke
```

### Run Optional-Feature Tests (hardware wallets)

Ledger and Trezor support lives behind the `hardware-wallet` Cargo feature
and is skipped by the default `cargo test` above. CI compiles and tests it
in a dedicated `hardware-wallet` job on every push, so run the same command
locally before touching `src/utils/hardware_wallet.rs`:

```bash
# Linux: apt-get install -y libudev-dev libusb-1.0-0-dev first
cargo build --locked --features hardware-wallet
cargo test --locked --features hardware-wallet
```

No physical device is required — the tests assert the approval, rejection,
unsupported-envelope, and disconnected/no-device paths, either against pure
APDU-parsing logic or against the real `hidapi`/`trezor-client` backends'
"no device found" behavior. See
[BUILD_TROUBLESHOOTING.md](BUILD_TROUBLESHOOTING.md#4-feature-flag-issues)
for per-OS system dependencies.

### Check Code Quality

The CI pipeline runs several quality checks. Run them locally:

```bash
# Format check
cargo fmt --all --check

# Linter check
cargo clippy -- -D warnings

# Dependency security check (requires cargo-deny)
cargo install cargo-deny
cargo deny check
```

---

## Development Workflow

### 1. Create a Feature Branch

Use a descriptive branch name:

```bash
git checkout -b feat/issue-XXX-description
```

Naming conventions:
- `feat/` for new features
- `fix/` for bug fixes
- `docs/` for documentation improvements
- `refactor/` for code refactoring
- `test/` for test additions or improvements

### 2. Make Changes

Edit files in your preferred editor. The project structure:

```
src/
├── main.rs                # CLI entry point
├── commands/              # Command implementations
│   ├── wallet.rs
│   ├── new.rs
│   ├── deploy.rs
│   ├── contract.rs
│   └── ...
└── utils/                 # Helper utilities
    ├── config.rs
    ├── horizon.rs
    ├── soroban.rs
    └── print.rs
```

### 3. Write/Update Tests

When adding features, include tests:

```bash
# Add unit tests in the same file
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_feature() {
        // Your test here
    }
}
```

For integration tests, create a new file in `tests/`:

```bash
# tests/my_feature.rs
#[test]
fn test_my_feature() {
    // Your test here
}
```

### 4. Run Tests Locally

```bash
cargo test
```

### 5. Check Code Quality

```bash
cargo fmt --all
cargo clippy -- -D warnings
```

### 6. Commit Changes

Use clear, descriptive commit messages:

```bash
git add .
git commit -m "feat: add wallet encryption support"
```

Commit message format:
- Start with type: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`
- Use lowercase
- Be specific and concise
- Example: `fix: resolve panic in contract deployment with large files`

### 7. Push and Create a Pull Request

```bash
git push origin feat/issue-XXX-description
```

Then open a Pull Request on GitHub. Use the provided template and follow the checklist.

---

## Code Quality and Security Logging

StarForge enforces consistent code quality through automated CI checks. See [CI_ENFORCEMENT.md](CI_ENFORCEMENT.md) for full details.

### Security Logging Requirements

All security-relevant operations must be properly logged for auditability and debugging. See [SECURITY_LOGGING_GUIDE.md](SECURITY_LOGGING_GUIDE.md) for detailed requirements. Key principles:

- **Log all security operations** - Wallet creation, encryption, deployment, plugin loading, etc.
- **Never log secrets** - Private keys, passphrases, encryption keys must be redacted
- **Include context** - Operation type, outcome, timestamp, and relevant details
- **Use structured logging** - JSON format for machine parsing and aggregation
- **Verify in tests** - Security logging behavior should be tested

Before submitting a PR with security-relevant changes:
1. Check [SECURITY_LOGGING_GUIDE.md](SECURITY_LOGGING_GUIDE.md) for what should be logged
2. Review [SECURITY_LOGGING_BEST_PRACTICES.md](SECURITY_LOGGING_BEST_PRACTICES.md) for implementation patterns
3. Ensure logs don't contain secrets or sensitive data
4. Test that logs provide useful audit trail information

### Formatting

Use Rust's built-in formatter:

```bash
cargo fmt --all
```

This is automatically checked in CI. All code must pass `cargo fmt --all --check`.

**Pre-commit tip**: Format before every commit:
```bash
cargo fmt --all && git add .
```

### Linting

Use Clippy to catch common mistakes:

```bash
cargo clippy --locked -- -D warnings
```

Fix any warnings before submitting a PR. All code must pass this check in CI.

**Pre-commit tip**: Run locally before pushing:
```bash
cargo clippy --locked -- -D warnings
```

### Code Style Standards

For detailed code style expectations, see [CODE_STYLE_STANDARDS.md](CODE_STYLE_STANDARDS.md). This covers:

- Naming conventions (functions, variables, constants, types)
- Documentation requirements
- Error handling patterns
- Testing expectations
- Common Clippy violations and how to fix them

### Documentation

- Add doc comments to all public functions and types:

```rust
/// Brief description of what this function does.
///
/// More detailed explanation if needed.
///
/// # Arguments
/// * `arg1` - description
///
/// # Returns
/// Description of return value
///
/// # Example
/// ```
/// let result = my_function(42);
/// assert_eq!(result, 43);
/// ```
pub fn my_function(arg1: i32) -> i32 {
    arg1 + 1
}
```

- Keep README and other docs up-to-date with your changes
- Update CHANGELOG if your change is user-facing

### Pre-Commit Validation

Run this before every commit to catch issues early:

```bash
cargo fmt --all && \
  cargo build --locked && \
  cargo test --locked && \
  cargo clippy --locked -- -D warnings
```

All of these are checked in CI.

---

## Submitting a Pull Request

### Before Submitting

- [ ] Fork and clone the repository
- [ ] Create a feature branch
- [ ] Make your changes
- [ ] Add/update tests
- [ ] Run `cargo test` and verify all tests pass
- [ ] Run `cargo fmt --all`
- [ ] Run `cargo clippy -- -D warnings`
- [ ] Update relevant documentation
- [ ] Commit with clear messages
- [ ] Push to your fork

### Pull Request Checklist

When opening a PR, fill out the template with:

- **Description**: Clear explanation of what changed and why
- **Type**: feat, fix, docs, refactor, test
- **Related Issues**: Link to issue(s) being resolved (e.g., `closes #208`)
- **Tests**: Describe any tests added/modified
- **Checklist**:
  - [ ] Code follows style guidelines
  - [ ] Self-reviewed own code
  - [ ] Added tests for new functionality
  - [ ] All tests pass locally (`cargo test`)
  - [ ] Updated documentation if needed
  - [ ] No breaking changes (or clearly documented)

### PR Guidelines

- **Keep PRs focused**: One issue per PR when possible
- **Keep PRs scoped**: Smaller, focused PRs are easier to review and merge faster
- **Write clear descriptions**: Explain the "why" not just the "what"
- **Reference issues**: Use `closes #XXX` to automatically link issues
- **Test thoroughly**: Include test cases for both happy path and edge cases
- **Update docs**: If your changes affect user-facing behavior, update docs

---

## Contributing to AI Features

StarForge integrates AI features for smart contract generation and automated documentation. When contributing to these features:

### Guidelines
1. **Mock External Calls in Tests**: Ensure all unit and integration tests do not query live OpenAI or Anthropic endpoints. Use mocked endpoints or verify inputs/outputs via local unit tests.
2. **Prompts Verification**: If you modify the prompt configurations in `src/utils/ollama.rs` or `src/commands/generate.rs`, verify that the system context still enforces strictly compilable Soroban Rust code and adheres to `#![no_std]`.
3. **Structured Response Formats**: When building tools that parse LLM output programmatically, enforce JSON output structures using formatting schemas or system templates rather than trusting loose markdown responses.
4. **Local-First Precedence**: Keep local Ollama support up-to-date. Ensure fallback warnings correctly instruct developers on how to pull and serve local models.

For more information, see [ARCHITECTURE_AI.md](ARCHITECTURE_AI.md), [AI_INTEGRATION_GUIDE.md](AI_INTEGRATION_GUIDE.md), and [AI_PROMPT_GUIDE.md](AI_PROMPT_GUIDE.md).

---

## Common Issues & Troubleshooting

For detailed troubleshooting, see [BUILD_TROUBLESHOOTING.md](BUILD_TROUBLESHOOTING.md).

### "rustc version mismatch"

Ensure you're on the correct Rust version:

```bash
rustup update stable
rustc --version  # Should be 1.80 or later
```

### "cargo: command not found"

Rust and Cargo weren't installed correctly. Reinstall using rustup:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source $HOME/.cargo/env
```

### Build fails with "dependency not found"

Clear the build cache and rebuild:

```bash
cargo clean
cargo build
```

### Tests fail with "permission denied"

On macOS/Linux, make scripts executable:

```bash
chmod +x scripts/e2e-smoke.sh
```

### "Cannot connect to Horizon API"

Some tests require network access. If tests fail due to network issues:

```bash
# Run only local tests (no network)
cargo test --lib

# Run tests with retries
cargo test -- --test-threads=1
```

### Wallet tests fail with "STARFORGE_TEST_SECRET_KEY not set"

Some tests require a test secret key. Set it:

```bash
export STARFORGE_TEST_SECRET_KEY="SXXX..."  # Your test key
cargo test
```

### Clippy warnings won't go away

Update Clippy:

```bash
rustup update stable
cargo clean
cargo clippy -- -D warnings
```

### Build Baseline Status

For a complete verification of the project's build status, see [BUILD_BASELINE_VERIFICATION.md](BUILD_BASELINE_VERIFICATION.md).

This document confirms:
- ✅ All 22 command handlers are properly implemented
- ✅ All 24 utility modules are properly declared
- ✅ Zero unresolved imports across 74 source files
- ✅ All test files are ready to execute
- ✅ The baseline is clean and ready for development

---

## Questions & Support

- **Documentation**: See [DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md) for in-depth development topics
- **Issues**: Open a [GitHub issue](https://github.com/Nanle-code/StarForge/issues) with questions or bugs
- **Discussions**: Use [GitHub Discussions](https://github.com/Nanle-code/StarForge/discussions) for general questions
- **Stellar Docs**: See [Stellar Developer Docs](https://developers.stellar.org)
- **Soroban Docs**: See [Soroban Documentation](https://soroban.stellar.org)

---

## Recognition

Contributors are recognized in the project and may participate in the [Stellar Wave Program](https://www.drips.network/wave/stellar) for monetary rewards.

Thank you for contributing to StarForge! 🚀
