# Contributor Quick Reference

Fast lookup guide for common development tasks in StarForge.

## One-Minute Setup

```bash
# 1. Install Rust (if needed)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Clone the repo
git clone https://github.com/Nanle-code/StarForge.git
cd StarForge

# 3. Build and test
cargo build
cargo test

# Done! You're ready to contribute.
```

## Common Commands

| Task | Command |
|------|---------|
| Run preflight gates | `./scripts/preflight-pr.sh` |
| Build (debug) | `cargo build` |
| Build (release) | `cargo build --release` |
| Run tests | `cargo test` |
| Run doctests | `cargo test --doc` |
| Run with output | `cargo test -- --nocapture` |
| Format code | `cargo fmt --all` |
| Lint code | `cargo clippy -- -D warnings` |
| Check security | `cargo deny check` |
| Create branch | `git checkout -b feat/issue-XXX-description` |
| Run smoke tests | `cargo test --test cli_smoke` |

## Before Submitting a PR

StarForge requires all CI status checks to pass and branches to be conflict-free against `master`.

### Fast Path: Local Preflight Script

Run the automated preflight script to verify all merge gates locally:

```bash
# Standard merge gates (formatting, compilation, clippy, JSON contracts, unit & smoke tests)
./scripts/preflight-pr.sh

# Quick mode during development
./scripts/preflight-pr.sh --quick

# Full workspace test suite
./scripts/preflight-pr.sh --all
```

### Manual Step-by-Step Verification

```bash
# 1. Ensure branch is rebased on master (conflict-free)
git fetch origin
git rebase origin/master

# 2. Format your code (required)
cargo fmt --all

# 3. Run all tests (required)
cargo test --locked

# 4. Run doctests (required — verifies documentation examples)
cargo test --doc --locked

# 5. Check for linting issues (required)
cargo clippy --locked -- -D warnings

# 6. Check dependency security (required in CI)
cargo deny check

# 7. Verify smoke tests pass
cargo test --test cli_smoke --locked

# 8. Verify the app runs
cargo run -- --version

# 9. Commit and push
git add .
git commit -m "feat: your change"
git push origin feat/issue-XXX-description
```

## Project Structure

```
src/
├── main.rs              # CLI entry point
├── commands/            # Command modules
│   ├── wallet.rs        # Wallet operations
│   ├── new.rs           # Scaffolding
│   ├── deploy.rs        # Contract deployment
│   ├── contract.rs      # Contract inspection
│   └── ...
└── utils/               # Utilities
    ├── config.rs        # Config file handling
    ├── horizon.rs       # Horizon API client
    ├── soroban.rs       # Soroban RPC client
    └── print.rs         # CLI output formatting

tests/                  # Integration tests
├── cli_smoke.rs
├── wallet_*.rs
├── deploy_*.rs
└── ...

.github/
├── workflows/
│   ├── ci.yml          # Main CI pipeline
│   └── release.yml
└── pull_request_template.md
```

## Testing Patterns

### Unit Tests (in src/)

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_my_function() {
        let result = my_function(42);
        assert_eq!(result, 43);
    }
}
```

### Integration Tests (in tests/)

```rust
// tests/my_test.rs
#[test]
fn test_integration() {
    // Test that requires multiple modules
}
```

### Running Tests

```bash
# All tests
cargo test

# Specific test
cargo test test_name

# With output
cargo test -- --nocapture --test-threads=1

# Integration test file
cargo test --test cli_smoke
```

## Git Workflow

```bash
# 1. Create a branch
git checkout -b feat/issue-208-contributor-guide

# 2. Make changes
vim src/commands/wallet.rs

# 3. Commit
git add src/commands/wallet.rs
git commit -m "feat: add wallet encryption support"

# 4. Push to your fork
git push origin feat/issue-208-contributor-guide

# 5. Open PR on GitHub
```

## Branch Naming Convention

| Type | Pattern | Example |
|------|---------|---------|
| Feature | `feat/issue-XXX-description` | `feat/issue-208-contributor-guide` |
| Bug fix | `fix/issue-XXX-description` | `fix/issue-205-wallet-panic` |
| Documentation | `docs/description` | `docs/api-reference-update` |
| Refactor | `refactor/description` | `refactor/config-module` |
| Tests | `test/description` | `test/wallet-integration` |

## Code Style

### Formatting

```bash
# Auto-format all code
cargo fmt --all

# Check format (no changes)
cargo fmt --all --check
```

### Documentation Comments

```rust
/// Brief description (one line).
///
/// More detailed explanation (optional).
///
/// # Arguments
/// * `param1` - description
///
/// # Returns
/// Description of return value
///
/// # Example
/// ```
/// let result = function(42);
/// ```
pub fn function(param1: i32) -> i32 {
    param1 + 1
}
```

## Common Issues

| Problem | Solution |
|---------|----------|
| `rustc version mismatch` | `rustup update stable` |
| Build fails | `cargo clean && cargo build` |
| Tests fail (network) | Some tests need internet; retry or skip |
| Permission denied on scripts | `chmod +x scripts/*.sh` |
| Clippy warnings | `cargo clippy --all -- -D warnings` |

## Configuration Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Project manifest and dependencies |
| `Cargo.lock` | Dependency lock file (commit this) |
| `.rustfmt.toml` | Code formatting rules |
| `.github/workflows/ci.yml` | Continuous integration pipeline |
| `rust-toolchain.toml` | Required Rust version |

## Resources

- **CONTRIBUTING.md** — Full contribution guide
- **CI_ENFORCEMENT.md** — CI pipeline and code quality enforcement
- **CODE_STYLE_STANDARDS.md** — Detailed code style and linting rules
- **BUILD_BASELINE_VERIFICATION.md** — Project build status verification
- **BUILD_TROUBLESHOOTING.md** — Solutions for build issues
- **DEVELOPER_GUIDE.md** — In-depth development documentation
- **README.md** — Project overview
- **API_REFERENCE.md** — Complete command reference
- **ARCHITECTURE.md** — System design and architecture

## Getting Help

- Check existing [issues](https://github.com/Nanle-code/StarForge/issues)
- Search [discussions](https://github.com/Nanle-code/StarForge/discussions)
- Read DEVELOPER_GUIDE.md for deep dives
- Ask in a new issue or discussion

## CI Pipeline

The GitHub Actions pipeline runs on every push and PR:

1. **Rustfmt** — Code formatting check
2. **Cargo Deny** — Dependency security audit
3. **Documentation Tests** — Doc examples compile and pass
4. **Build, Test & Clippy** — Compilation, tests, and linting
5. **CLI Smoke Tests** — End-to-end functionality tests

All must pass for a PR to be mergeable.

## Debugging Tips

### Print debugging
```bash
cargo test -- --nocapture  # See println! output
```

### Run single-threaded
```bash
cargo test -- --test-threads=1  # Easier to read output
```

### Check what changed
```bash
git diff              # Unstaged changes
git diff --cached     # Staged changes
git log --oneline -5  # Recent commits
```

### Inspect build
```bash
cargo build -v  # Verbose build output
cargo tree      # Dependency tree
cargo check     # Fast syntax check (no linking)
```

---

**Ready to contribute?** Start with [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide.
