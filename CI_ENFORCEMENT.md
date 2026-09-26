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
7. **Code Coverage** - LLVM source-based coverage of the full test suite
8. **Static Analysis** - CodeQL for Rust, TypeScript and the workflows themselves

The complete pipeline (testing, coverage, multi-platform builds, security
scanning and releases) is mapped in [CI/CD Pipeline Map](#cicd-pipeline-map)
below.

---

## CI Pipeline Overview

### Job: Rustfmt (Code Formatting)

**Purpose**: Ensure all Rust code follows standard formatting conventions  
**Trigger**: Every push and pull request  
**Status**: ⚠️ Currently non-blocking. `master` still has unformatted code in
a handful of files, so the job emits a `rustfmt` warning annotation instead of
failing. Reformatting everything at once would conflict with every open branch;
once `cargo fmt --all --check` passes on `master`, the step in `ci.yml` should
be switched back to a hard gate (the comment in the workflow says how). Please
still run `cargo fmt --all` on the files you touch.

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

### Job: Plugin Capability Enforcement

**Purpose**: Prove that plugin filesystem/network capability boundaries are enforced  
**Trigger**: Every push and pull request

```bash
cargo test --test plugin_capability_integration --locked
```

**What it checks:**
- A malicious sample plugin with no declared capabilities is denied `fs:read`, `fs:write` and `network`
- Its WASM modules importing WASI filesystem/socket functions are rejected by the sandbox before running
- A benign sample plugin is granted exactly the capabilities it declares and runs successfully

See [docs/plugins/capabilities.md](docs/plugins/capabilities.md#5-testing-capability-enforcement) for the harness and fixtures.

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
`.zip` from [Releases](https://github.com/Nanle-code/StarForge/releases).
Windows binaries are built and smoke-tested in CI on every push and pull
request, and the release pipeline refuses to publish a Windows binary that
fails these startup/help checks.

### Job: Canonical Repository URLs

**What it checks**: every tracked file links only to the canonical repository,
`Nanle-code/StarForge`. That covers GitHub web and clone URLs,
`raw.githubusercontent.com`, `api.github.com/repos`, Homebrew taps and
installer `REPO=` lines. Unfilled `YOUR_USERNAME` placeholders in GitHub URLs
fail too.

**Why**: a link to a fork in an install command or in release metadata can
make users install binaries built by someone else.

```bash
bash scripts/check-canonical-urls.sh
```

The installer's end-to-end test
([`tests/installer/test_install_e2e.sh`](tests/installer/test_install_e2e.sh))
runs the unmodified `install.sh` against a fake GitHub and fails if it
requests any URL outside the canonical repository.

### Job: Docs Snippets (`docs.yml`)

**What it checks**: every shell block in `README.md` and `docs/` is annotated
`run` or `norun`. `run` blocks execute against the freshly built binary in a
temporary `HOME`, and a failure is reported as `path:line`. A second job
runs the `run local` blocks against a `stellar/quickstart` service container.

```bash
cargo build && python3 scripts/docs-snippets.py
```

See [CONTRIBUTING.md](CONTRIBUTING.md#documentation-snippets) for the
annotation rules. The same workflow builds the mdBook docs site from `docs/`
and publishes it to GitHub Pages from `master`.

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

1. **Required Status Checks**: All CI workflow jobs (`fmt`, `msrv`, `deny`, `secure-defaults`, `doctests`, `build-and-test`, `docs-cheatsheet`, `hardware-wallet`, `clippy`, `smoke`, `cli-macos`, `cli-windows`, `reproducible-wasm`) must pass on the latest commit before a pull request can be merged. See [docs/BRANCH_PROTECTION.md](docs/BRANCH_PROTECTION.md) for the full list of required check names.
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

## CI/CD Pipeline Map

Issue #44 asked for a comprehensive pipeline covering testing, coverage,
multi-platform builds, security scanning and automated releases. This is where
each of those lives:

| Concern | Workflow | Jobs / notes | Blocking? |
| --- | --- | --- | --- |
| Formatting, lint | `ci.yml` | Rustfmt, Clippy (`-D warnings`, all features) | Clippy yes; Rustfmt warns only (see above) |
| MSRV | `ci.yml` | `cargo check --locked --workspace` on Rust 1.80.0 | yes |
| Tests (Linux) | `ci.yml` | Build and Test, Doctests, Secure Defaults Audit, Hardware Wallet, Smoke, Docs Cheat Sheet | yes |
| Tests (macOS, Windows) | `ci.yml` | macOS CLI Tests, Windows CLI Tests | yes |
| Code coverage | `coverage.yml` | `cargo llvm-cov` over the whole suite: LCOV + JSON + HTML artifact (`coverage-report`), totals in the job summary, optional Codecov upload | Fails if tests fail; a minimum % is enforced only when the `COVERAGE_THRESHOLD` repository variable is set |
| Property tests, fuzzing, mutation | `fuzzing.yml` | proptest, fuzz harness build, nightly fuzz smoke, weekly cargo-mutants | yes (mutants informational) |
| Dependency advisories / licenses / sources | `ci.yml` (Cargo Deny), `audit.yml` (Cargo Audit, weekly + PR) | curated exceptions in `deny.toml` / `audit.toml` | yes |
| Dependency review | `audit.yml` (Dependency Review, PRs only) | GitHub advisory DB diff of the PR | no (`continue-on-error`; cargo-deny/audit are the gate) |
| SAST | `codeql.yml` | CodeQL for `actions`, `javascript-typescript`, `rust` (weekly + PR) | yes, except `rust` which is informational |
| Dependency updates | `.github/dependabot.yml` | weekly grouped PRs for cargo, npm (`registry-api/`), GitHub Actions; MSRV-sensitive exact pins ignored | n/a |
| Release binaries | `release.yml` | Linux x86_64 + aarch64, macOS x86_64 + arm64, Windows x86_64; smoke-tested, packaged, provenance-attested, `SHA256SUMS.txt` | yes |
| Release publishing | `release.yml` | GitHub Release with generated notes on `v*` tags; Homebrew formula PR | yes |
| Release dry run | `release.yml` (`workflow_dispatch`) | builds and packages everything and uploads `release-files-dry-run`, without publishing or attesting | n/a |
| Deployment | `deployment.yml` | manual, environment-protected, reuses `ci.yml` as its quality gate | yes |

### Pipeline conventions

- **Least privilege**: every workflow declares `permissions: contents: read` at
  the top level; jobs that need more (release publishing, attestations,
  code-scanning uploads, audit check runs) opt in at the job level.
- **Concurrency**: superseded runs on pull requests and feature branches are
  cancelled. Pushes to `master`, releases, and deployments are never cancelled.
- **Caching**: CI and coverage jobs use `Swatinem/rust-cache`. Release builds
  and the Docs Cheat Sheet job are deliberately uncached (clean release
  artifacts; `build.rs` must really run).
- **Secrets in conditions**: the `secrets` context is not allowed in a step's
  `if:`, so optional integrations (Slack, Codecov) are exposed through job
  `env:` and tested as `env.NAME != ''`.
- **Validation**: workflow changes should pass
  [`actionlint`](https://github.com/rhysd/actionlint) before they are pushed.

### Cutting a release

1. Optionally run **Release** from the Actions tab (`workflow_dispatch`) on
   `master` and download `release-files-dry-run` to check the archives and
   `SHA256SUMS.txt`.
2. Push a `vX.Y.Z` tag. `release.yml` runs the secure defaults audit, builds
   all five platform archives, attests them, publishes the GitHub Release, and
   opens a Homebrew formula PR.
3. The archive names are fixed (`starforge-{linux,darwin}-{x86_64,aarch64}.tar.gz`,
   `starforge-windows-x86_64.zip`) because `install.sh` and the Homebrew
   formula download them by name. `tests/release_artifacts_test.py` fails if
   the build matrix and `scripts/release_artifacts.py` drift apart.

## CI Configuration Files

### Main CI Pipeline
- Location: `.github/workflows/ci.yml`
- Triggers: Every push and PR (also callable via `workflow_call`)
- Jobs: fmt, msrv, deny, secure-defaults, doctests, build-and-test,
  docs-cheatsheet, hardware-wallet, clippy, smoke, cli-macos, cli-windows

### Coverage
- Location: `.github/workflows/coverage.yml`
- Local equivalent: `scripts/coverage.sh` or
  `cargo llvm-cov --locked --html -- --test-threads=1`

### Dependency Security
- Managed by: `cargo deny` (`deny.toml`, checked with `--all-features`) and
  `cargo audit` (`audit.toml`; the ignore list is duplicated in `audit.yml`)
- Updates: `.github/dependabot.yml`

### Static Analysis
- Location: `.github/workflows/codeql.yml`

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
A: `Cargo.lock` pins versions. Dependabot opens grouped weekly update PRs (see `.github/dependabot.yml`), which must pass the same CI, including the MSRV job. Exact-pinned crates are bumped by hand.

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
