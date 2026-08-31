# CI Enforcement and Code Quality Standards

This document describes how StarForge enforces code quality through continuous integration.

## Overview

StarForge uses an automated CI pipeline to ensure consistent code quality. Every push and pull request is validated against:

1. **Code Formatting** - Rust standard formatting via `cargo fmt`
2. **Code Linting** - Best practices and correctness via `cargo clippy`
3. **Dependency Security** - Supply chain security via `cargo deny`
4. **Compilation** - Successful builds with no errors
5. **Tests** - All tests pass without failures
6. **Smoke Tests** - Basic CLI functionality works end-to-end

---

## CI Pipeline Overview

### Job: Rustfmt (Code Formatting)

**Purpose**: Ensure all Rust code follows standard formatting conventions  
**Trigger**: Every push and pull request  
**Status**: ✅ Required (must pass)

```bash
cargo fmt --all --check
```

**What it checks:**
- Indentation (4 spaces)
- Line length and wrapping
- Spacing around operators and delimiters
- Import organization
- Comment formatting

**Local equivalent:**
```bash
# Check if code is formatted
cargo fmt --all --check

# Auto-format all code
cargo fmt --all
```

---

### Job: Cargo Deny (Dependency Security)

**Purpose**: Audit dependencies for security vulnerabilities and license issues  
**Trigger**: Every push and pull request  
**Status**: ✅ Required (must pass)

```bash
cargo deny check --all-features
```

**What it checks:**
- Known security advisories in dependencies (via RustSec advisory database)
- Only approved open-source licenses are present in the dependency tree
- Duplicate crate versions across the dependency graph
- Only dependencies from crates.io are allowed; untrusted registries and git sources are rejected

**Configuration:** The policy is defined in `deny.toml` at the repository root. Every ignored advisory must have a documented rationale.

**Local equivalent:**
```bash
# Install cargo-deny (if not present)
cargo install cargo-deny

# Run security auditcargo deny check
# Run all supply-chain checks
cargo deny check

# Run individual checks
cargo deny check advisories
cargo deny check licenses
cargo deny check bans
cargo deny check sources
```

**Failure behavior:**
- cargo-deny exits non-zero on any policy violation, which fails the CI job.
- The `continue-on-error` flag is **not** used — all violations block the PR.
- Output includes the specific advisory ID, crate name, and license that caused the failure.

**Unsupported environments:**
- If cargo-deny cannot be installed or run in the CI environment, the job fails rather than silently skipping.
- The CI configuration uses the official `EmbarkStudios/cargo-deny-action@v2` action, which handles Rust toolchain setup automatically.

**Handling new violations:**
- **Advisory**: Update the dependency or add a documented ignore to `[advisories].ignore` in `deny.toml`.
- **License**: Add the license to `[licenses].allow` in `deny.toml` if it is compatible, or replace the dependency.
- **Duplicate**: Investigate whether the duplicate can be eliminated; duplicates are currently warned (not denied) to avoid breaking changes.
- **Source**: No new registries or git sources are allowed without explicit approval.

---

### Job: Documentation Tests

**Purpose**: Ensure documentation examples (doctests) compile and pass  
**Trigger**: Every push and pull request  
**Status**: ✅ Required (must pass)

```bash
cargo test --doc --locked
```

**What it checks:**
- All ```` ``` ```` and ```` ```no_run ```` doc examples compile successfully
- Pure-logic examples execute and pass assertions
- Public utility APIs stay documented with accurate examples

**Local equivalent:**
```bash
cargo test --doc --locked
```

See [DOCTEST_GUIDELINES.md](DOCTEST_GUIDELINES.md) for how to write doctests.

---

### Job: Secure Defaults Audit

**Purpose**: Verify that StarForge ships with secure, privacy-respecting defaults  
**Trigger**: Every push and pull request  
**Status**: ✅ Required (must pass)

```bash
cargo test --test secure_defaults_audit --locked
```

**What it checks:**
- Telemetry opt-out is respected (defaults to enabled)
- AI telemetry cloud aggregation is disabled by default
- Friendbot is absent on mainnet, present on testnet
- Default network is testnet
- Plugin trust sources match known repos only
- Wallet encryption is opt-in
- File permissions are restricted (0600)
- Network passphrases are correct

See [SECURE_DEFAULTS_AUDIT.md](SECURE_DEFAULTS_AUDIT.md) for the full checklist.

---

### Job: Build, Test & Clippy

**Purpose**: Compile the project, run tests, and check for common mistakes  
**Trigger**: Every push and pull request  
**Status**: ✅ Required (must pass)

**Steps:**

1. **Build**
   ```bash
   cargo build --locked
   ```
   Compiles the entire project with locked dependencies

2. **Test**
   ```bash
   cargo test --locked
   ```
   Runs all unit and integration tests

3. **Clippy (Linting)**
   ```bash
   cargo clippy --locked -- -D warnings
   ```
   Checks for common mistakes and best practices, treating warnings as errors

**What Clippy checks:**
- Unnecessary complexity or redundant code
- Incorrect use of standard library functions
- Performance anti-patterns
- Memory safety issues
- Unused variables or imports
- Common pitfalls and idioms

**Local equivalent:**
```bash
# Check for Clippy warnings
cargo clippy --all-targets

# Apply auto-fixes (when available)
cargo clippy --fix --allow-dirty --allow-staged
```

---

### Job: Cargo.lock Reproducibility & Immutability Verification

**Purpose**: Ensure locked builds do not mutate dependency resolution on Linux, macOS, or Windows.  
**Trigger**: Every push and pull request across all OS matrix targets  
**Status**: ✅ Required (must pass)

```bash
# Enforce lockfile immutability
git diff --exit-code Cargo.lock

# Verify Cargo.lock reproducibility with StarForge
starforge verify lockfile
```

**What it checks:**
- `Cargo.lock` exact deterministic resolution across supported operating systems (Linux, macOS, Windows).
- That locked compilation (`--locked`) does not modify `Cargo.lock` or require dependency resolution updates.
- Detection of out-of-sync dependency specifications between `Cargo.toml` and `Cargo.lock`.

---

### Job: CLI Smoke Tests

**Purpose**: Validate basic CLI functionality works end-to-end  
**Trigger**: Every push and pull request  
**Status**: ✅ Required (must pass)

```bash
cargo test --test cli_smoke --locked
./scripts/e2e-smoke.sh
```

**What it tests:**
- `starforge info` exits cleanly
- `starforge --version` shows version
- `starforge --help` lists commands
- `starforge wallet list` works
- `starforge network show` works
- `starforge template list` works
- `starforge deploy --help` documents flags

---

### Job: Windows Binary Startup Smoke Tests

**Purpose**: Validate that the Windows binary starts and its core help/doctor
surface works, mirroring what users get from the shipping `.zip`  
**Trigger**: Every push and pull request (`ci.yml` → `cli-windows`) and on
installer changes (`installer-tests.yml` → `installer-windows`)  
**Status**: ✅ Required (must pass) — also release-blocking via `release.yml`

```powershell
pwsh -NoProfile -ExecutionPolicy Bypass -File tests/installer/windows_smoke.ps1 -Binary target\release\starforge.exe
```

**What it tests:**
- `starforge --version` exits 0
- `starforge --help` exits 0
- `starforge info` exits 0 (binary startup)
- `starforge config --help` exits 0 and lists `doctor`
- `starforge config doctor` runs diagnostically in an isolated
  `STARFORGE_CONFIG_DIR`; the offline `schema` finding must pass while
  network/toolchain findings (Horizon, Soroban RPC, Stellar CLI on PATH) are
  reported without failing the job

**Failure visibility:** each failing check prints the exact command, its exit
code, and captured output. Full output is teed to `windows-smoke.log` and
uploaded as a CI artifact on failure.

**Windows support status:** StarForge ships Windows `x86_64` binaries as a
`.zip` from [Releases](https://github.com/Josetic224/StarForge/releases).
Windows binaries are built and smoke-tested in CI on every push and pull
request, and the release pipeline refuses to publish a Windows binary that
fails these startup/help checks.

---

## Acceptance Criteria Compliance

### ✅ CI Fails Clearly on Regressions

Each job has clear, descriptive names and output:

| Regression Type | Job | Failure Visibility |
|---|---|---|
| Formatting errors | Rustfmt | ❌ Clear diff of formatting issues |
| Lint violations | Build, Test & Clippy | ❌ Specific warning messages |
| Security issues | Cargo Deny | ❌ Advisory ID and description |
| Test failures | Build, Test & Clippy | ❌ Test name and assertion |
| Secure default regressions | Secure Defaults Audit | ❌ Which default changed |
| Broken doc examples | Documentation Tests | ❌ Compilation error or assertion failure |
| Broken CLI | CLI Smoke Tests | ❌ Which command failed |
| Broken Windows binary | Windows Binary Startup Smoke Tests | ❌ Exact command, exit code, and output (log artifact) |

**Example failure output:**
```
error: code must be formatted
...
Run `cargo fmt --all` to format your code
```

---

### ✅ Documented Standard for Contributors

This enforcement is documented in:

- **[CONTRIBUTING.md](CONTRIBUTING.md)** - Full contribution guide with code quality section
- **[CONTRIBUTOR_QUICK_REFERENCE.md](CONTRIBUTOR_QUICK_REFERENCE.md)** - Quick lookup for common commands
- **[CODE_STYLE_STANDARDS.md](CODE_STYLE_STANDARDS.md)** - Detailed code style and linting rules
- **This file** - CI enforcement and pipeline details

All new contributors see these documents in the onboarding flow.

---

### ✅ Codebase Remains Consistent

Enforcing these checks ensures:

1. **No format drift** - All code formatted identically via `cargo fmt`
2. **No style regressions** - Linting catches anti-patterns before merge
3. **No security issues** - Dependencies audited automatically
4. **No broken functionality** - Tests and smoke tests run on every change
5. **No hidden complexity** - Clippy enforces readability and maintainability

---

## Development Workflow

### Before Committing

Run these commands locally to match what CI checks:

```bash
# 1. Format code
cargo fmt --all

# 2. Build project
cargo build --locked

# 3. Run tests
cargo test --locked

# 4. Check secure defaults
cargo test --test secure_defaults_audit --locked
# 4. Check doctests
cargo test --doc --locked

# 5. Check linting
cargo clippy --locked -- -D warnings

# 6. Verify smoke tests
cargo test --test cli_smoke --locked

# 7. Verify CLI JSON output stability contracts
cargo test --test json_contract_stability --locked
```

Or run all at once (simulates CI):

```bash
cargo fmt --all --check && \
  cargo build --locked && \
  cargo test --locked && \
  cargo test --doc --locked && \
  cargo clippy --locked -- -D warnings && \
  cargo test --test cli_smoke --locked && \
  cargo test --test json_contract_stability --locked
```

The JSON contract stability check prevents stable `--json` fields listed in
`tests/fixtures/json_contracts/stable-fields-baseline.json` from disappearing
from `docs/contracts/cli-json-fields.json` unless they are first marked
`deprecated`.

---

### Branch Protections & Merge Requirements

StarForge enforces GitHub branch protections on the `master` branch:

1. **Required Status Checks**: All CI workflow jobs (`fmt`, `msrv`, `deny`, `secure-defaults`, `build-and-test`, `clippy`, `smoke`, `cli-macos`, `cli-windows`) must pass before a pull request can be merged.
1. **Required Status Checks**: All CI workflow jobs (`fmt`, `msrv`, `deny`, `doctests`, `build-and-test`, `clippy`, `smoke`, `cli-macos`, `cli-windows`) must pass before a pull request can be merged.
2. **Conflict-Free Enforcement**: Pull requests with merge conflicts are blocked from merging. Branches must be cleanly rebased against `master`.
3. **Approved Reviews**: PRs require maintainer review and approval with all conversational threads resolved.

### Pre-PR Verification with Preflight Script

To verify all merge gates locally before pushing and opening a PR, use the preflight script:

```bash
# 1. Run standard preflight merge gates
./scripts/preflight-pr.sh

# 2. Fast subset check during active development
./scripts/preflight-pr.sh --quick

# 3. Full suite check before final submission
./scripts/preflight-pr.sh --all
```

The script exits with a non-zero exit code if any gate fails, pinpointing the issue immediately.

You can also run individual gate commands manually:

```bash
# 1. Ensure your branch is up to date and conflict-free
git fetch origin
git rebase origin/master

# 2. Run full validation
cargo fmt --all --check && \
  cargo deny check && \
  cargo build --locked && \
  cargo test --locked && \
  cargo clippy --locked -- -D warnings

# 3. Verify smoke tests
cargo test --test cli_smoke --locked

# 4. Push and open PR
git push origin feat/your-feature
# Open PR on GitHub
```

---

## IDE Integration

### VS Code

**Rust Analyzer extension** - automatically formats on save:

```json
{
  "[rust]": {
    "editor.formatOnSave": true,
    "editor.defaultFormatter": "rust-lang.rust-analyzer"
  }
}
```

**Clippy warnings in editor** - set in settings:

```json
{
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.checkOnSave.extraArgs": [
    "--",
    "-D",
    "warnings"
  ]
}
```

### IntelliJ IDEA / RustRover

**Built-in Rust support** automatically runs:
- `cargo fmt` checks (with auto-fix option)
- Clippy linting (with action hints)

Enable in **Settings → Languages & Frameworks → Rust → Rustfmt**

### Vim / Neovim

**rust.vim plugin** with formatting:

```vim
let g:rustfmt_autosave = 1
```

---

## Common Issues and Solutions

### "error: code must be formatted"

```bash
# Fix automatically
cargo fmt --all

# Verify
cargo fmt --all --check
```

### "warning: X could be written as Y" (Clippy)

```bash
# See what auto-fixes are available
cargo clippy --fix --allow-dirty --allow-staged

# Or manually review and apply suggestions
cargo clippy --locked -- -D warnings
```

### "Deny: advisory X found"

```bash
# Check which dependency has the issue
cargo deny fetch

# Update to a patched version
cargo update
```

### Tests fail locally but CI passes

```bash
# Use locked dependencies (what CI uses)
cargo test --locked

# Run in CI environment (single-threaded)
cargo test -- --test-threads=1
```

---

## Customization

### Formatting Rules

Formatting is controlled by `.rustfmt.toml`. Current defaults are stable and widely adopted. To customize:

```toml
# Example: change max line length
max_width = 120
```

However, changing these after merged code is not recommended as it affects blame and history.

### Linting Rules

Clippy rules are stable and enforced with `-D warnings` (deny). To suppress a specific warning:

```rust
#[allow(clippy::rule_name)]
fn my_function() {
    // ...
}
```

Document why the rule is suppressed in a comment.

---

## CI Configuration Files

### Main CI Pipeline
- Location: `.github/workflows/ci.yml`
- Triggers: Every push and PR
- Jobs: fmt, deny, test, smoke
- Duration: ~2-3 minutes

### Dependency Security
- Managed by: `cargo deny`
- Config: `deny.toml` (if present)
- Checked: With `--all-features`

---

## Monitoring CI Status

### For Contributors

- **On Pull Request**: Green checkmark ✅ means all checks passed
- **On Pull Request**: Red X ❌ means at least one check failed
- **Click "Details"**: Shows which job failed and why

### For Maintainers

Monitor the [Actions tab](https://github.com/Nanle-code/StarForge/actions) for:
- Flaky tests (inconsistent failures)
- New Clippy warnings introduced
- Dependency vulnerabilities discovered
- Performance regressions

---

## FAQ

**Q: Why enforce `-D warnings` in Clippy?**  
A: Warnings are future errors. Treating them as errors now prevents accumulation and keeps code quality high.

**Q: Can I skip CI checks?**  
A: No. All PRs must pass CI to merge. This ensures consistency and prevents breaking changes.

**Q: What if CI fails for an environmental reason?**  
A: Rerun the check via GitHub Actions UI or push a new commit to trigger re-run.

**Q: How often are dependencies updated?**  
A: `Cargo.lock` pins versions. Dependencies are updated manually via `cargo update` and tested before commit.

**Q: Why test on every push, not just PRs?**  
A: Catches issues before opening PR, saves review time, and ensures master is always deployable.

---

## Further Reading

- [CONTRIBUTING.md](CONTRIBUTING.md) - Contribution guidelines
- [CODE_STYLE_STANDARDS.md](CODE_STYLE_STANDARDS.md) - Code style and standards
- [DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md) - In-depth development guide
- [Clippy lint list](https://rust-lang.github.io/rust-clippy/) - All Clippy rules
- [Rustfmt configuration](https://rust-lang.github.io/rustfmt/) - Formatting options

---

*Last updated: 2026-06-01*  
*Issue #207: Enforce formatting and linting in CI*
