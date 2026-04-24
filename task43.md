# Task 43: Establish CI/CD Pipeline and Release Automation

**Priority:** Advanced  
**Assigned:** 2026-04-24  
**Repo:** Nanle-code/StarForge  
**Due:** 2026-05-03  
**Description:** Create a comprehensive CI/CD pipeline including automated testing, cross-platform binary building, security scanning, and automated releases.

---

## 1. Executive Summary

Establish end-to-end CI/CD automation for the StarForge Rust CLI project. The pipeline will validate code quality, run tests, build binaries for Linux/macOS/Windows, perform security audits, and publish releases to GitHub automatically.

---

## 2. Scope

### In Scope
- GitHub Actions workflow creation
- Multi-platform release builds (Linux, macOS x86_64/arm64, Windows)
- Automated test execution
- Security vulnerability scanning
- Release asset generation and publishing
- SHA256 checksum generation

### Out of Scope
- Code refactoring or feature development
- Docker container builds
- Third-party package manager distributions (Homebrew, AUR, etc.)
- Code signing certificates (can be added later)

---

## 3. Technical Requirements

| Requirement | Specification | Status |
|------------|---------------|--------|
| Multi-platform builds | Linux, macOS (x86_64 + arm64), Windows | Required |
| Automated testing | `cargo test` with 100% pass rate | Required |
| Code quality | `rustfmt` + `clippy` zero warnings | Required |
| Security scanning | `cargo audit` with deny warnings | Required |
| Release automation | GitHub Releases with assets | Required |
| Checksums | SHA256 for all binaries | Required |

---

## 4. Current State Analysis

### 4.1 Codebase
- **Language:** Rust (edition 2021)
- **Binary:** `starforge` (src/main.rs)
- **Modules:** 8 command modules + 4 utility modules
- **Dependencies:** 29 crates (clap, stellar-xdr, ed25519-dalek, etc.)
- **Tests:** 10 unit tests across 3 files

### 4.2 Existing Infrastructure
- No CI/CD workflows (`.github/workflows/` does not exist)
- No cross-compilation configuration
- No automated release process
- Manual builds only

### 4.3 External Dependencies
- Horizon Testnet API
- Friendbot (testnet faucet)
- Soroban RPC (testnet)

---

## 5. Implementation Plan

### Phase 1: CI Pipeline Foundation (Days 1-2)

**Objective:** Establish basic CI checks

**Tasks:**
1. Create `.github/workflows/ci.yml`
2. Configure Rust toolchain installation
3. Implement cargo cache strategy
4. Add formatting check (`cargo fmt -- --check`)
5. Add linting check (`cargo clippy -- -D warnings`)
6. Configure test execution (`cargo test --release`)
7. Set up PR status checks

**Deliverables:**
- `.github/workflows/ci.yml`
- Workflow runs on push and PR

**Success Criteria:**
- All checks pass on main branch
- PRs blocked if any check fails

---

### Phase 2: Security Scanning (Days 2-3)

**Objective:** Integrate vulnerability detection

**Tasks:**
1. Add audit job to CI workflow
2. Install `cargo-audit`
3. Configure `--deny-warnings` flag
4. Enable GitHub Secret Scanning
5. Review and resolve existing advisories

**Deliverables:**
- Audit job in CI
- Security policy document

**Success Criteria:**
- Zero high/critical vulnerabilities
- CI fails on new advisories

---

### Phase 3: Cross-Platform Build System (Days 3-5)

**Objective:** Enable compilation for all target platforms

**Tasks:**
1. Select cross-compilation tool (`cross` or `cargo-zigbuild`)
2. Create `.cargo/config.toml` with target configurations
3. Configure build matrix for:
   - `x86_64-unknown-linux-gnu`
   - `x86_64-pc-windows-gnu`
   - `x86_64-apple-darwin`
   - `aarch64-apple-darwin`
4. Implement binary stripping
5. Create packaging scripts (tar.gz for Unix, zip for Windows)
6. Test builds locally

**Deliverables:**
- `.cargo/config.toml`
- Build scripts
- 4 platform binaries (tested)

**Success Criteria:**
- All platforms build successfully
- Binaries execute on native hardware/emulation

---

### Phase 4: Release Automation (Days 5-7)

**Objective:** Automate publishing to GitHub Releases

**Tasks:**
1. Create `.github/workflows/release.yml`
2. Configure tag-based trigger (`v*` pattern)
3. Set up artifact collection from build matrix
4. Implement SHA256 checksum generation
5. Configure GitHub Release creation
6. Add release notes generation
7. Test with draft release

**Deliverables:**
- `.github/workflows/release.yml`
- Release template

**Success Criteria:**
- Tag push creates release automatically
- All assets included
- Checksums verified

---

### Phase 5: Testing & Validation (Days 7-8)

**Objective:** Ensure pipeline reliability

**Tasks:**
1. Run full CI pipeline on feature branch
2. Test release workflow with `v0.1.1-rc.1` tag
3. Validate all platform binaries
4. Verify checksums
5. Test installation from release assets
6. Document edge cases

**Deliverables:**
- Test report
- Known issues list

**Success Criteria:**
- All tests pass
- Binaries install and run correctly

---

### Phase 6: Documentation (Day 9)

**Objective:** Document the new workflows

**Tasks:**
1. Update README.md with CI/CD badges
2. Add "Contributing" section
3. Document release process
4. Create troubleshooting guide
5. Add workflow diagrams

**Deliverables:**
- Updated README.md
- CONTRIBUTING.md

**Success Criteria:**
- Clear documentation for maintainers

---

## 6. Workflow Specifications

### 6.1 CI Workflow (`ci.yml`)

**Triggers:**
- Push to: `main`, `develop`
- Pull request to: `main`

**Jobs:**
1. **format** - Check code formatting
2. **lint** - Run Clippy lints
3. **test** - Execute unit tests
4. **audit** - Security vulnerability scan

**Environment:** Ubuntu latest

---

### 6.2 Release Workflow (`release.yml`)

**Triggers:**
- Tag push: `v*`
- Manual dispatch

**Jobs:**
1. **build** (matrix) - Cross-compile for all platforms
2. **release** - Create GitHub Release with assets

**Environment:** Ubuntu latest (build), macOS latest (if needed for signing)

**Note on Windows Code Signing:** Authenticode signing for Windows binaries requires a code-signing certificate (e.g., from DigiCert, Sectigo) and `osslsigncode` or `signtool`. This incurs **monetary cost** (~$70–$300/year for EV/standard certificates) and requires a Windows runner or cross-signing setup. Unsigned Windows binaries may trigger SmartScreen/AV warnings. See Risk Assessment for details.

---

## 7. Build Matrix & Platform Strategy

| Platform | Target Triple | Output Binary | Strategy |
|----------|---------------|---------------|----------|
| Linux | `x86_64-unknown-linux-gnu` | `starforge-linux-x86_64` | Native cross-compile via `cross` (Docker) or `cargo build --target`. Static linking with `x86_64-linux-gnu` + musl optional for maximum portability. |
| Windows | `x86_64-pc-windows-gnu` (MinGW) | `starforge-windows-x86_64.exe` | Use MinGW target (`x86_64-pc-windows-gnu`) on Linux via `cross`/Mingw-w64 toolchain. Avoids Visual Studio dependency. **Alternative:** `x86_64-pc-windows-msvc` requires Visual Studio Build Tools (Windows runner only). |
| macOS Intel | `x86_64-apple-darwin` | `starforge-macos-x86_64` | **Cannot cross-compile from Linux.** Use macOS GitHub runner (see below). |
| macOS ARM | `aarch64-apple-darwin` | `starforge-macos-aarch64` | **Cannot cross-compile from Linux.** Use macOS GitHub runner (see below). |

**Best Industry Standard Approaches:**

- **Linux:** Use `cross` (Docker-based) with `x86_64-unknown-linux-gnu` target. Fast, reproducible, no host dependencies. Optionally produce musl builds (`x86_64-unknown-linux-musl`) for fully static binaries.
- **Windows:** Use `cross` with `x86_64-pc-windows-gnu` (MinGW) target. Provides working binaries without Windows licenses or Visual Studio. For MSVC ABI compatibility, use a Windows GitHub runner and `x86_64-pc-windows-msvc`.
- **macOS (Intel & ARM):** **No reliable cross-compilation from Linux.** Industry standard is to use **macOS GitHub Actions runners** (`macos-latest` or `macos-14`) which provide native `rustc` for both `x86_64-apple-darwin` and `aarch64-apple-darwin`. Universal (fat) binaries can be created with `lipo` if needed.

**Recommended Build Matrix for StarForge (Linux + Windows + macOS):**

```yaml
strategy:
  matrix:
    include:
      # Linux - Fast, free, on Ubuntu
      - target: x86_64-unknown-linux-gnu
        os: ubuntu-latest
        use-cross: true
        archive: tar.gz
      
      # Windows (MinGW) - Cross-compile on Ubuntu, free
      - target: x86_64-pc-windows-gnu
        os: ubuntu-latest
        use-cross: true
        archive: zip
      
      # macOS Intel - Native on macOS runner
      - target: x86_64-apple-darwin
        os: macos-latest
        use-cross: false
        archive: tar.gz
      
      # macOS ARM - Native on macOS runner
      - target: aarch64-apple-darwin
        os: macos-latest
        use-cross: false
        archive: tar.gz
```

**This combination is FREE for public repositories** and covers all three platforms you requested.

---

## 8. Resource Requirements

### GitHub Actions
- **Compute:** Ubuntu, macOS, Windows runners
- **Storage:** Release assets retained 90 days for artifacts
- **Minutes:** Public repository (unlimited)

### Tools & Actions
- `actions/checkout@v4`
- `dtolnay/rust-toolchain@stable`
- `taiki-e/install-action` (for cross, cargo-audit)
- `actions/cache@v3`
- `softprops/action-gh-release@v1`

---

## 9. Risk Assessment

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| macOS cross-compilation from Linux | High (certain failure) | High | Use macOS GitHub runners natively; cannot cross-compile |
| Windows AV false positives / SmartScreen | Medium | Medium | Purchase code-signing certificate (~$70–$300/yr) and `osslsigncode`; use Windows runner for signing |
| Friendbot/Horizon rate limits | High | High | Mock network layer in tests; use testnet sparingly; add exponential backoff; fund wallets once per test suite |
| Dependency vulnerabilities (RustSec) | High | High | `cargo audit --deny-warnings` in CI; weekly `cargo update`; pin vulnerable deps; allow-list low-severity only |
| GitHub Actions macOS/Windows quota exhaustion | Medium | High | Limit matrix concurrency; use `max-parallel: 2`; cache aggressively; skip macOS on PR from forks |
| Clippy/format failures on existing code | High | Medium | Run `cargo fmt` and `cargo clippy --fix` before CI; commit fixes first |
| WASM build toolchain missing | Medium | Medium | Install `rustup target add wasm32-unknown-unknown`; verify `stellar contract build` in CI if scaffolding tested |
| Build timeouts (>10m) | Low | Medium | Use `cross` + Docker layer caching; cache `target/` and `~/.cargo`; use `sccache` |
| No rollback mechanism for bad releases | Medium | Medium | Tag releases immutably; document manual rollback: delete tag, revert commit, re-release |
| Secrets leakage (GH_TOKEN) | Low | Critical | Use GitHub-provided `GITHUB_TOKEN` with minimal scopes; never use personal access tokens; rotate if leaked |

---

## 10. Success Metrics

### Quantitative
- CI pipeline duration: < 15 minutes (Linux), < 20 minutes (macOS/Windows)
- Build success rate: 100% across all 4 platforms
- Test pass rate: 100% (10/10 unit tests)
- Zero high/critical vulnerabilities (cargo audit)
- Zero clippy warnings (after initial fix pass)
- Zero rustfmt violations
- Release automation: 100% (no manual steps beyond git tag push)

### Qualitative
- Consistent, reproducible builds across platforms
- Automated quality and security gates
- Secure by default (deny warnings, no secrets in repo)
- Well-documented workflows and troubleshooting
- Clear rollback procedure for bad releases

---

## 11. Maintenance Plan

### Ongoing Tasks
- Monitor CI runs weekly
- Review dependency updates (Dependabot)
- Update workflows for Rust toolchain changes
- Rotate secrets if used
- Archive old releases quarterly

### Ownership
- Primary: DevOps/Build Engineer
- Secondary: Project Maintainers

---

## 12. Acceptance Criteria

### Must Have (Go-Live)
- [ ] CI workflow passes on main branch
- [ ] All tests pass (10/10)
- [ ] No clippy warnings
- [ ] Code formatted correctly
- [ ] Security audit passes
- [ ] All 4 platforms build successfully
- [ ] Release workflow creates assets
- [ ] SHA256 checksums generated
- [ ] Release published with all assets

### Should Have
- [ ] Code coverage reporting
- [ ] Build artifacts retained
- [ ] Documentation updated

### Nice to Have
- [ ] Automated changelog
- [ ] Docker images
- [ ] Package manager distributions

---

## 13. Additional Problems & Solutions (Not Previously Referenced)

### Problem 1: Existing Code Has Clippy/Format Violations
**Issue:** Running `cargo clippy -- -D warnings` or `cargo fmt --check` on the current codebase will fail immediately due to existing `unwrap()`, `expect()`, and formatting issues.

**Solution:** 
- Run `cargo fmt` on entire codebase first (commit separately)
- Run `cargo clippy --fix --allow-dirty --allow-no-vcs` to auto-fix warnings
- For remaining `unwrap()` in `horizon.rs` (line 28) and `config.rs` (line 68), replace with proper error handling using `anyhow::Context`
- Add `#![deny(warnings)]` to `src/lib.rs` and `src/main.rs` after fixes are complete

### Problem 2: No Integration/E2E Tests for CLI Commands
**Issue:** Only 10 unit tests exist. No tests verify `starforge wallet create` actually creates config entries, or that `starforge new contract` generates valid Cargo.toml files.

**Solution:**
- Add `tests/` directory with integration tests using `assert_cmd` and `predicates` crates
- Test CLI commands in isolated temp directories
- Mock network calls using `wiremock` or `mockito` for Horizon/Friendbot
- Add golden file tests for scaffolded output

### Problem 3: WASM Toolchain Not Installed in CI
**Issue:** `new.rs` scaffolds Soroban contracts with `crate-type = ["cdylib"]` requiring `wasm32-unknown-unknown` target. CI will fail when testing contract templates.

**Solution:**
- Add `rustup target add wasm32-unknown-unknown` to CI setup
- Install `stellar-cli` for `stellar contract build` validation
- Or skip template compilation tests in CI (mark as ignored)

### Problem 4: Friendbot Rate Limiting Breaks CI
**Issue:** `wallet.rs` tests and any test calling `horizon::fund_account` will hit Friendbot rate limits (1 request per IP per hour).

**Solution:**
- Add `#[cfg_attr(test, mockall::automock)]` to horizon module
- Use dependency injection for `fund_account` in wallet tests
- Or add `#[ignore]` to network-dependent tests
- Use `mockito` to mock HTTP responses in test environment

### Problem 5: No SBOM (Software Bill of Materials)
**Issue:** Modern CI/CD requires SBOM for compliance/security audits.

**Solution:**
- Add `cargo cyclonedx` or `cargo sbom` to CI
- Generate CycloneDX JSON SBOM as release artifact
- Upload as `starforge-sbom.json`

### Problem 6: Binary Size Bloat
**Issue:** No validation that release binaries stay within reasonable size limits.

**Solution:**
- Add CI step: `ls -lh target/*/release/starforge*`
- Fail build if binary > 20MB (uncompressed)
- Use `strip` and `upx` (Linux) for compression
- Enable `opt-level = 'z'` and `lto = true` in release profile

### Problem 7: No Semantic Versioning Automation
**Issue:** Manual version bumping in `Cargo.toml` is error-prone.

**Solution:**
- Use `cargo-release` to automate version bumps and tagging
- Or use GitHub Actions with `cargo semver-checks`
- Enforce conventional commits to auto-determine version bump type

### Problem 8: macOS Code Signing Not Configured
**Issue:** macOS binaries from CI will be "unsigned" and trigger Gatekeeper warnings.

**Solution:**
- Enroll in Apple Developer Program ($99/year)
- Use `codesign` on macOS runner with Developer ID certificate
- Store certificate in GitHub Secrets as base64
- Sign before creating release tarball

### Problem 9: Windows SmartScreen Warnings
**Issue:** New Windows binaries trigger "Windows protected your PC" SmartScreen warnings.

**Solution:**
- Purchase EV Code Signing Certificate (~$300/year)
- Sign with `osslsigncode` on Windows runner
- Submit to Microsoft SmartScreen for reputation (takes weeks)
- Alternative: Provide SHA256 hashes and instructions to bypass

### Problem 10: No Dependency License Compliance Check
**Issue:** Corporate users need to know license compatibility.

**Solution:**
- Add `cargo deny check licenses` to CI
- Generate license report as artifact
- Configure allow-list (MIT, Apache-2.0, BSD-3-Clause)
- Fail on GPL/AGPL or custom licenses

---

## 14. Critical Path - What Must Be Fixed Before CI Works

1. **Fix clippy warnings** in existing code (unwrap/expect usage)
2. **Format code** with rustfmt
3. **Decide on Windows target**: MinGW (`gnu`) vs MSVC (requires VS Build Tools)
4. **Handle macOS builds**: Accept that cross-compilation from Linux is impossible; use macOS runners
5. **Address network dependencies**: Mock or ignore Friendbot/Horizon in tests
6. **Install WASM target**: If testing contract templates in CI

---

## 15. Recommended Minimal Viable CI (Week 1)

**Don't try to do everything at once. Start with:**

1. **Basic CI** (`.github/workflows/ci.yml`)
   - Ubuntu runner only
   - `cargo fmt --check`
   - `cargo clippy -- -D warnings` (after fixing existing code)
   - `cargo test`
   - `cargo audit`

2. **Single-platform release** (Linux x86_64 only)
   - Use `cross` for Linux builds
   - Simple GitHub Release
   - SHA256 checksums

3. **Fix existing codebase**
   - Run `cargo fmt`
   - Run `cargo clippy --fix`
   - Commit fixes

**Then expand to:**
- Windows (MinGW cross-compile)
- macOS (via runner)
- Multi-platform releases
- Advanced security scanning

---

## 16. Sign-Off

| Role | Name | Date | Signature |
|------|------|------|-----------|
| Project Manager | | | |
| DevOps Engineer | | | |
| Security Lead | | | |
| QA Lead | | | |

---

## 17. Reference: Paid Services & Cost Summary

> **Note:** This section lists items that **cannot be started for free**. All implementation steps in Phases 1-6 can be done at **zero cost** on public repositories.

| Item | Cost | Frequency | Notes | Can Do Free? |
|------|------|-----------|-------|--------------|
| **GitHub Actions (public repo)** | **$0** | Unlimited | Free for public repositories | ✅ YES |
| **GitHub Actions (private repo)** | ~$10-50/mo | Per 2,000 min over free tier | macOS/Windows runners cost more | ❌ NO |
| **Code Signing (Windows)** | $70-300/yr | One-time per year | EV certificate for SmartScreen | ❌ NO |
| **Code Signing (macOS)** | $99/yr | Annual | Apple Developer Program | ❌ NO |
| **Domain/URL Shortener** | $10-20/yr | Optional | For install.sh script | ❌ NO |
| **Storage/CDN** | $0-5/mo | Optional | For distributing releases | ✅ YES (use GitHub Releases) |
| **TOTAL (public, no signing)** | **$0** | | Free tier sufficient | ✅ YES |
| **TOTAL (private, with signing)** | **~$200-400/yr** | | Includes certs + extra CI minutes | ❌ NO |

### What You Can Do FREE Right Now:

✅ **CI Pipeline** (Linux + Windows MinGW on Ubuntu, macOS on macOS runner)
✅ **Automated Testing** (all 10 unit tests)
✅ **Security Scanning** (`cargo audit`, GitHub Secret Scanning)
✅ **Release Automation** (GitHub Releases with all 4 binaries)
✅ **SHA256 Checksums** (generated automatically)

### What Costs Money:

❌ **Windows SmartScreen fix** → Need EV Code Signing Certificate ($300/yr)
❌ **macOS Gatekeeper fix** → Need Apple Developer Program ($99/yr)
❌ **Private repository** → Need to pay for extra CI minutes beyond free tier

### Bottom Line:

**You can build the ENTIRE CI/CD pipeline for FREE** as long as:
1. The repository remains **public**
2. You skip code signing (users will see warnings, but binaries work)
3. You stay within free tier limits (2,000 Linux min/mo, 200 macOS min/mo)

**The ONLY things that cost money are optional improvements** (code signing to avoid OS warnings).

---

*Document Version: 1.1*  
*Last Updated: 2026-04-24*
