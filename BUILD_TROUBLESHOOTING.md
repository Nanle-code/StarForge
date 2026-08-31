# Build Troubleshooting Guide

This guide helps diagnose and fix build issues in StarForge.

## Quick Checklist

Before diving into troubleshooting, verify:

```bash
# Check Rust version (need 1.80+)
rustc --version
cargo --version

# Update if needed
rustup update stable

# Clean and rebuild
cargo clean
cargo build --release

# Run tests
cargo test
```

If the above works, you're good! Otherwise, continue below.

---

## Common Issues and Solutions

### 0. Cargo.lock Reproducibility or Mutation Failures

**Problem**: CI fails with `git diff --exit-code Cargo.lock` or `starforge verify lockfile` reports violations.

**Cause**: `Cargo.toml` dependencies were modified without updating `Cargo.lock`, or platform-specific dependencies caused lockfile drift across operating systems.

**Solutions**:

```bash
# 1. Run starforge verify lockfile locally to inspect exact resolution errors or diffs
starforge verify lockfile

# 2. Update Cargo.lock to match Cargo.toml definitions
cargo check

# 3. Verify Cargo.lock immutability again
git diff --exit-code Cargo.lock
```

---

### 1. Network Connection Issues

**Problem**: `error: failed to get <crate> as a dependency...`

**Cause**: Network is offline or crates.io is unreachable

**Solutions**:

```bash
# Option A: Check your connection
ping crates.io

# Option B: Use offline mode (if dependencies are cached)
cargo build --offline

# Option C: Clear cargo cache and retry
cargo clean
rm -rf ~/.cargo/registry
cargo build

# Option D: Use different registry mirror (if in China)
# Edit ~/.cargo/config.toml:
# [source.crates-io]
# replace-with = "tsinghua"
# [source.tsinghua]
# registry = "https://mirrors.tsinghua.edu.cn/git/crates.io-index.git"
```

---

### 2. Rust Toolchain Issues

**Problem**: `error[E0514]: found crate ... compiled by an incompatible version of rustc`

**Cause**: Rust version mismatch

**Solutions**:

```bash
# Check which version you have
rustc --version

# Update to latest stable
rustup update stable

# Alternatively, use the exact version specified in rust-toolchain.toml
rustup install 1.80  # or whatever version is specified
rustup default 1.80

# Clean build artifacts and rebuild
cargo clean
cargo build
```

---

### 3. Build Cache Issues

**Problem**: `error: cannot find ... in this scope` (when it should exist)

**Cause**: Stale build cache or corrupted artifacts

**Solutions**:

```bash
# Complete clean rebuild
cargo clean
cargo build

# Or more aggressive clean
rm -rf target/
cargo build

# Check if specific module is missing
cargo build --lib wallet

# If issue persists, remove Cargo.lock and re-download deps
rm Cargo.lock
cargo build
```

---

### 4. Feature Flag Issues

**Problem**: `error[E0433]: cannot find ... in this scope`

**Cause**: Optional features not enabled

**Solutions**:

```bash
# Build with all features
cargo build --all-features

# Or specific features
cargo build --features hardware-wallet

# Check what features are available
cargo metadata --format-version 1 | grep -A 10 '"features"'
```

**`hardware-wallet` system dependencies**: this feature links `hidapi` (Ledger,
via USB HID) and `trezor-client` (Trezor, via `rusb`/libusb). Both are optional
so the default build never needs them, but building or testing with the
feature enabled requires the matching system headers:

```bash
# Debian/Ubuntu
sudo apt-get install -y libudev-dev libusb-1.0-0-dev

# Fedora
sudo dnf install -y systemd-devel libusbx-devel

# macOS
brew install libusb
```

Without a physical device, `wallet connect`, `wallet hw-status`, and
`wallet import --hardware` still build and run — they fail fast with a
"No Ledger/Trezor device detected" error instead of hanging, which is what
CI's `hardware-wallet` job exercises on every push (see `.github/workflows/ci.yml`).

---

### 5. Module Not Found Errors

**Problem**: `error[E0432]: unresolved import` or `no such module`

**Cause**: Module not properly declared in `mod.rs`

**Solutions**:

```bash
# Verify module structure
ls src/commands/      # Should see all .rs files
ls src/utils/         # Should see all utility modules

# Check src/commands/mod.rs for all declarations
cat src/commands/mod.rs

# Check src/utils/mod.rs for all declarations
cat src/utils/mod.rs

# If a module is missing, add it:
# echo "pub mod new_module;" >> src/commands/mod.rs
```

---

### 6. Compilation Errors After Recent Changes

**Problem**: `error[Exxx]: ...` after modifying code

**Cause**: Syntax error or broken reference in your change

**Solutions**:

```bash
# Check for syntax errors
cargo check

# Get more detailed error messages
cargo build -v

# Run clippy for lint warnings
cargo clippy -- -D warnings

# Run tests to see what broke
cargo test -- --test-threads=1 --nocapture

# Check format
cargo fmt --all --check
```

---

### 7. Plugin Loading Issues

**Problem**: `error: Plugin version incompatibility` or `Failed to load plugin`

**Cause**: Plugin built with different StarForge version

**Solutions**:

```bash
# Check StarForge version
starforge --version

# Rebuild plugins with current StarForge version
cargo build --release

# Check plugin compatibility
cargo build --test plugin_compatibility
cargo test --test plugin_compatibility -- --nocapture

# See BUILD_BASELINE_VERIFICATION.md for more info
cat BUILD_BASELINE_VERIFICATION.md
```

---

### 8. Test Failures

**Problem**: Tests fail to compile or run

**Solutions**:

```bash
# Run individual test
cargo test test_name -- --nocapture

# Run all tests with output
cargo test -- --nocapture --test-threads=1

# Run specific test file
cargo test --test cli_smoke

# Run only unit tests
cargo test --lib

# Run only integration tests
cargo test --test '*'

# Get verbose output on failure
RUST_BACKTRACE=1 cargo test -- --nocapture
```

---

### 9. Lock File Issues

**Problem**: Cargo.lock conflicts or version mismatch

**Solutions**:

```bash
# Use locked dependencies (recommended)
cargo build --locked

# Or update lock file
cargo update

# Or remove and regenerate
rm Cargo.lock
cargo build
```

---

### 10. Environment Variable Issues

**Problem**: `error: environment variable X not defined at compile time`

**Cause**: Build script didn't run or environment missing

**Solutions**:

```bash
# Force build script to run
cargo clean
cargo build

# Check if build.rs exists
ls -la build.rs

# Ensure CARGO_MANIFEST_DIR is set (usually automatic)
echo $CARGO_MANIFEST_DIR

# Run build explicitly
cargo build -vvv  # Very verbose output
```

---

## Diagnostic Commands

Use these to diagnose build issues:

```bash
# Check project structure
cargo tree

# List all dependencies
cargo tree --depth 3

# Check for outdated deps
cargo outdated

# Check for security issues
cargo audit

# Validate Cargo.toml
cargo check --all-targets

# Get environment info
starforge info

# Check Rust and toolchain
rustc --version --verbose
rustup show

# Run linter
cargo clippy --all-targets -- -D warnings

# Check formatting
cargo fmt --all -- --check
```

---

## Build Pipeline Verification

To verify the full build pipeline works:

```bash
# 1. Format check (as CI does)
cargo fmt --all --check

# 2. Dependency security check
cargo deny check

# 3. Build project
cargo build --locked

# 4. Run all tests
cargo test --locked

# 5. Run linter (CI checks)
cargo clippy --locked -- -D warnings

# 6. Build and test the optional hardware-wallet backends (requires
#    libudev-dev + libusb-1.0-0-dev, see "Feature Flag Issues" above)
cargo build --locked --features hardware-wallet
cargo test --locked --features hardware-wallet

# All together (simulates CI)
cargo fmt --all --check && \
  cargo build --locked && \
  cargo test --locked && \
  cargo clippy --locked -- -D warnings && \
  cargo build --locked --features hardware-wallet && \
  cargo test --locked --features hardware-wallet
```

---

## Getting Help

If none of these solutions work:

1. **Check existing issues**: https://github.com/Nanle-code/StarForge/issues
2. **Review CONTRIBUTING.md**: Setup and development guide
3. **Check BUILD_BASELINE_VERIFICATION.md**: Baseline status
4. **Search discussions**: https://github.com/Nanle-code/StarForge/discussions
5. **Read error messages carefully**: They usually tell you exactly what's wrong
6. **Check DEVELOPER_GUIDE.md**: Deep dive into project structure

---

## Advanced Debugging

### Enable verbose output:

```bash
# Very verbose build
RUST_LOG=debug cargo build -vvv

# Very verbose tests
RUST_BACKTRACE=full cargo test -- --nocapture

# Show all compiler passes
cargo rustc -- -C debug-info=full
```

### Check what's being compiled:

```bash
# See exactly what gets compiled
cargo build -v

# See linker commands
cargo rustc -- -C link-args=-verbose

# Generate assembly
cargo rustc --release -- --emit asm
```

### Inspect artifacts:

```bash
# List binary name
cargo build --message-format=json | jq .executable

# Find binary location
find target/ -name "starforge" -type f

# Check binary size
ls -lh target/release/starforge
```

---

## Performance Tips

If builds are slow:

```bash
# Use incremental compilation (default, but explicit)
CARGO_INCREMENTAL=1 cargo build

# Use ramdisk for target/ (Linux/Mac)
# sudo mount -t tmpfs -o size=10G tmpfs ~/StarForge/target

# Use mold linker (Linux, faster linking)
cargo rustc -- -C link-arg=-fuse-ld=mold

# Check what's taking time
cargo build --timings
```

---

## Platform-Specific Issues

### macOS

```bash
# If you get certificate issues
cert=$(security find-certificate -c "DigiCert" /Library/Keychains/System.keychain | \
       awk '/alis=/ {print $0}' | sed 's/alis=//' | tr -d '"')
security add-trusted-cert -d -r trustAsRoot -k /Library/Keychains/System.keychain "$cert"
```

### Windows

```bash
# Use cargo with MSVC
rustup default stable-msvc

# Or use GNU (might need manual setup)
rustup default stable-gnu

# Check linked libraries
dumpbin /dependents target\release\starforge.exe
```

### Linux

```bash
# On some systems, you might need dev tools
sudo apt-get install build-essential  # Debian/Ubuntu
sudo yum install gcc  # RHEL/CentOS

# Check glibc compatibility
ldd target/release/starforge
```

---

## Before Opening an Issue

1. Run `cargo clean && cargo build --release`
2. Run `cargo test` and capture full output
3. Run `rustc --version` and `cargo --version`
4. Check that you're on the latest master: `git pull origin master`
5. Include the full error message, not just the summary
6. Include your OS, Rust version, and step-by-step reproduction

---

## Still Stuck?

Please open an issue with:
- Full error message
- Output of `rustc --version`
- Output of `cargo --version`
- Steps to reproduce
- Your OS and environment

See [CONTRIBUTING.md](CONTRIBUTING.md) for how to open good issues.

---

*Last Updated: 2026-06-01*
