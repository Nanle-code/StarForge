# Code Style and Linting Standards

This document defines the code style and linting standards for the StarForge project. All contributors must follow these guidelines to maintain code consistency, quality, and readability across the codebase.

## Table of Contents

1. [Overview](#overview)
2. [Rust Formatting Standards](#rust-formatting-standards)
3. [Clippy Lint Rules](#clippy-lint-rules)
4. [Pre-Commit Developer Checklist](#pre-commit-developer-checklist)
5. [CI Pipeline Expectations](#ci-pipeline-expectations)
6. [IDE/Editor Integration](#ideeditor-integration)
7. [Automated Enforcement](#automated-enforcement)
8. [Quick Reference](#quick-reference)

---

## Overview

StarForge uses **industry-standard Rust tooling** to enforce code quality:

- **rustfmt**: Automatic code formatting (enforced in CI)
- **Clippy**: Linter for catching common mistakes and idioms
- **cargo deny**: Security and license compliance checking
- **Custom lint rules**: Project-specific suppressions for known limitations

**Philosophy**: We automate all style enforcement so developers can focus on logic, not formatting. When in doubt, run the tools—they are the source of truth.

---

## Rust Formatting Standards

### rustfmt Overview

All Rust code must be formatted using [rustfmt](https://github.com/rust-lang/rustfmt), Rust's official formatter. This is non-negotiable and enforced in CI.

**What rustfmt does:**
- Normalizes indentation (4 spaces per level)
- Aligns imports and use statements
- Wraps long lines consistently
- Formats comments and doc strings
- Enforces brace placement and spacing

### Running rustfmt

```bash
# Format all code in the project
cargo fmt --all

# Check formatting without modifying (useful in CI)
cargo fmt --all --check

# Format a specific file
rustfmt src/main.rs

# Format with specific edition (2021)
cargo fmt -- --edition 2021
```

### Key rustfmt Rules for StarForge

These are the standards enforced by rustfmt in this project (Rust 2021 edition):

#### Indentation
- **4 spaces** per indentation level (not tabs)
- No trailing whitespace

```rust
// ✅ Correct
fn my_function(x: i32) -> i32 {
    if x > 0 {
        x + 1
    } else {
        x - 1
    }
}

// ❌ Incorrect (tabs or 2 spaces)
fn my_function(x: i32) -> i32 {
→   if x > 0 {
→   →   x + 1
```

#### Line Length
- **Prefer lines under 100 characters** (soft limit)
- Lines may exceed 100 chars if breaking them makes code less readable
- rustfmt will break long lines intelligently

```rust
// ✅ Long line that's clear
let result = perform_complex_calculation_with_meaningful_name(arg1, arg2, arg3)?;

// ✅ Broken into multiple lines for clarity
let result = perform_complex_calculation_with_meaningful_name(
    very_long_argument_one,
    very_long_argument_two,
    very_long_argument_three,
)?;

// ❌ Artificially broken for no reason
let result =
    perform_calculation(arg1, arg2, arg3)?;
```

#### Imports and Modules
- Group imports in this order: **std**, **external crates**, **internal crates**
- Separate groups with blank lines
- Use paths over wildcard imports (except for prelude)

```rust
// ✅ Correct grouping
use std::fs;
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::utils::config;
use crate::utils::print;

// ❌ Wrong: mixed groups
use anyhow::Result;
use std::fs;
use crate::utils::config;
use serde::Deserialize;

// ⚠️ Minimize wildcard imports (except std/prelude)
use anyhow::*;  // Avoid—be explicit
```

#### Naming and Case

| Item | Convention | Example |
|------|-----------|---------|
| Functions | `snake_case` | `create_wallet()` |
| Types/Structs | `PascalCase` | `WalletConfig` |
| Enums | `PascalCase` | `NetworkType::Testnet` |
| Constants | `SCREAMING_SNAKE_CASE` | `MAX_RETRIES = 3` |
| Lifetimes | `'lowercase` | `'a`, `'static` |
| Modules | `snake_case` | `mod wallet_manager;` |
| Type Parameters | `PascalCase` | `<T, U>` |

```rust
// ✅ Correct naming
const MAX_WALLET_SIZE: usize = 100;

struct WalletConfig {
    name: String,
    encrypted: bool,
}

fn create_wallet(config: WalletConfig) -> Result<Wallet> {
    // ...
}

enum NetworkType {
    Testnet,
    Mainnet,
    Development,
}

// ❌ Incorrect naming
const max_wallet_size: usize = 100;  // Should be SCREAMING_SNAKE_CASE
struct walletConfig {}                // Should be PascalCase
fn CreateWallet() {}                  // Should be snake_case
```

#### Whitespace and Brackets

- **Spaces around operators**: `let x = a + b;` (not `a+b`)
- **No space before colons in type annotations**: `x: i32` (not `x : i32`)
- **Space after keywords**: `if condition {` (not `if(condition) {`)
- **Closing braces on same line** as opening (K&R style for functions)

```rust
// ✅ Correct spacing
fn process(data: Vec<String>) -> Result<Output> {
    let x = 5 + 3;
    if x > 0 {
        process_positive(x)
    } else {
        process_negative(x)
    }
}

// ❌ Incorrect
fn process(data:Vec<String>)->Result<Output>{
    let x=5+3;
    if(x>0){
        process_positive(x);
    }else{
        process_negative(x);
    }
}
```

#### Comments and Doc Comments

- Use `//` for comments (not `/*...*/` for line comments)
- Use `///` for public function/type documentation
- Use `//!` for module-level documentation
- Doc comments must precede items they document

```rust
// ✅ Correct doc comment format
/// Fetches account information from the Horizon API.
///
/// This function queries the Stellar Horizon service for the given public key
/// and returns detailed account balance and sequence information.
///
/// # Arguments
///
/// * `public_key` - Stellar public key starting with 'G'
/// * `network` - Network identifier: "testnet" or "mainnet"
///
/// # Returns
///
/// Returns `Ok(AccountData)` with account details, or error if:
/// - Account doesn't exist
/// - Network is unreachable
/// - Response parsing fails
///
/// # Example
///
/// ```
/// let account = fetch_account("GAB...", "testnet")?;
/// println!("Balance: {}", account.balance);
/// ```
pub fn fetch_account(public_key: &str, network: &str) -> Result<AccountData> {
    // Implementation...
}

// ✅ Module-level doc comment
//! Network operations for Stellar integration.
//!
//! This module provides abstractions over the Horizon HTTP API
//! for querying account and transaction data.

// ✅ Inline comments explain WHY, not WHAT
// Use shallow clone to minimize memory footprint
git_clone("--depth", "1", &url);

// ❌ Don't state the obvious
// Clone the repository
git_clone(&url);

// ❌ Multiple-line comments for code (use // for consistency)
/* This does something */
```

#### Control Flow

- Braces always on same line as control structure (Allman style not used in Rust)
- No space before opening brace
- Closing brace on its own line for multi-line blocks

```rust
// ✅ Correct
if condition {
    do_something();
} else {
    do_other_thing();
}

for item in items {
    process(item)?;
}

match value {
    Some(x) => println!("{}", x),
    None => println!("Nothing"),
}

// ❌ Wrong: Allman style not used in Rust
if condition
{
    do_something();
}

// ❌ Wrong: no space before brace
if condition{
    do_something();
}
```

#### Trait Implementations and Generics

- Space after generic bounds with `where`
- Align multiple bounds for readability

```rust
// ✅ Correct generic/trait syntax
impl<T: Clone, U: Default> MyType<T, U> {
    fn method(&self) -> T {
        self.value.clone()
    }
}

impl<T> Iterator for MyIterator<T>
where
    T: Clone + Debug,
{
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        None
    }
}

// ❌ Wrong
impl<T:Clone,U:Default>MyType<T,U>{
    fn method(&self)->T{
        self.value.clone()
    }
}
```

### Viewing and Enforcing rustfmt Output

```bash
# See what would change (dry-run)
cargo fmt --all -- --check

# Apply all changes
cargo fmt --all

# Force a specific edition (2021)
cargo fmt --edition 2021

# Verbose mode (shows files being formatted)
cargo fmt --all --verbose
```

---

## Clippy Lint Rules

### Overview

[Clippy](https://github.com/rust-lang/rust-clippy) is the Rust linter that catches common mistakes, suggests idiomatic patterns, and improves performance. In CI, clippy runs with **all warnings treated as errors** (`-D warnings`), so clippy violations block merges.

**Key principle**: Write idiomatic Rust. When clippy complains, it's usually right.

### Common Clippy Rules

This is a selection of clippy rules you'll encounter in code review:

#### Error Handling

| Rule | Problem | Fix |
|------|---------|-----|
| `must_use_candidate` | Function result often needs checking | Add `#[must_use]` or document why not |
| `result_unit_err` | Error type is `()` | Use meaningful error types |
| `unwrap_used` | Called `unwrap()` in library code | Use `?` operator instead |
| `expect_used` | Called `expect()` | Use `?` operator; only use `expect()` for programmer errors |

```rust
// ✅ Good: Using ? operator
pub fn process_file(path: &str) -> Result<Data> {
    let contents = std::fs::read_to_string(path)?;
    parse_data(&contents)
}

// ❌ Bad: Using unwrap
pub fn process_file(path: &str) -> Result<Data> {
    let contents = std::fs::read_to_string(path).unwrap();  // CLIPPY: unwrap_used
    Ok(parse_data(&contents).unwrap())
}

// ✅ Good: expect() only for programmer errors
let config = config::load()
    .expect("Config should be initialized in main()");

// ✅ Good: add #[must_use] to functions whose return value matters
#[must_use]
pub fn validate_key(key: &str) -> bool {
    // ...
}

// ❌ Bad: silently ignoring Result
validate_key(&input);  // CLIPPY: must_use_candidate
```

#### Idioms and Performance

| Rule | Problem | Fix |
|------|---------|-----|
| `needless_clone` | Cloning when not needed | Use references instead |
| `needless_return` | Explicit `return` on last line | Remove; let value be implicit |
| `too_many_arguments` | Function has >7 parameters | Group into struct |
| `type_complexity` | Type is too complex to read | Extract into type alias |
| `redundant_closure` | Closure just forwards to function | Use function pointer directly |

```rust
// ✅ Good: implicit return
fn calculate(x: i32) -> i32 {
    x + 1
}

// ❌ Bad: explicit return on last line
fn calculate(x: i32) -> i32 {
    return x + 1;  // CLIPPY: needless_return
}

// ✅ Good: no unnecessary cloning
fn process(items: &[String]) {
    for item in items {
        println!("{}", item);
    }
}

// ❌ Bad: clone when not needed
fn process(items: &[String]) {
    for item in items.clone() {  // CLIPPY: needless_clone
        println!("{}", item);
    }
}

// ✅ Good: group many parameters
struct Config {
    host: String,
    port: u16,
    timeout: u64,
    retries: u32,
}

fn connect(config: &Config) -> Result<()> { }

// ❌ Bad: too many parameters
fn connect(
    host: &str,
    port: u16,
    timeout: u64,
    retries: u32,
) -> Result<()> { }  // CLIPPY: too_many_arguments
```

#### Style

| Rule | Problem | Fix |
|------|---------|-----|
| `manual_string_new` | Creating empty string inefficiently | Use `String::new()` |
| `filter_next` | Using `.filter().next()` | Use `.find()` instead |
| `map_flatten` | Using `.map().flatten()` | Use `.flat_map()` instead |
| `comparison_to_empty` | Comparing to empty string/vec | Use `.is_empty()` |

```rust
// ✅ Good: idiomatic
let empty = String::new();
let found = items.find(|x| x.is_valid());
let flattened: Vec<_> = items.flat_map(|x| x.children()).collect();
if name.is_empty() { }

// ❌ Bad: non-idiomatic
let empty = String::from("");  // CLIPPY: manual_string_new
let found = items.filter(|x| x.is_valid()).next();  // CLIPPY: filter_next
let flattened: Vec<_> = items.map(|x| x.children()).flatten().collect();  // CLIPPY: map_flatten
if name == "" { }  // CLIPPY: comparison_to_empty
```

### Running Clippy

```bash
# Run clippy with warnings only (non-blocking)
cargo clippy

# Run clippy and deny all warnings (what CI does)
cargo clippy -- -D warnings

# Run clippy on tests
cargo clippy --tests -- -D warnings

# Run clippy with all features
cargo clippy --all-features -- -D warnings

# Auto-fix some issues (when possible)
cargo clippy --fix

# Check specific lint
cargo clippy -- -W clippy::needless_clone
```

### Project-Specific Allowances

**There is no crate-wide lint allowlist.** `src/lib.rs` and `src/main.rs` used to
carry a blanket `#![allow(dead_code, unused, clippy::all)]`, and `Cargo.toml`
carried a matching `[lints]` table — together these silenced essentially every
default Clippy lint group (correctness, suspicious, style, complexity, perf)
across the *entire* ~40k-line crate, not just the handful of patterns the
comments claimed to cover. That's exactly backwards: it hid real bugs (see
below) behind the same blanket that was meant to excuse a few CLI functions
with many arguments.

Every remaining `#[allow(...)]` in the codebase is now **scoped to the single
item that needs it** — a function, struct, or field — with a comment
immediately above explaining *why*:

```rust
// Each parameter is an independent, named input (CLI flags / distinct config
// values); bundling them into a struct here would add indirection without
// reducing real complexity.
#[allow(clippy::too_many_arguments)]
async fn monitor_contract(
    contract_id: &str,
    events_filter: Option<&str>,
    // ...
) -> Result<()> {
```

**When to add a scoped allow:**
- Only on the specific item that triggers it — never on a module, and never
  crate-wide.
- Only for patterns that are genuinely **unavoidable or clearly intentional**
  for that one item, not as a shortcut past a warning you haven't looked at.
- Always with a comment explaining *why*, not just restating the lint name.
- `dead_code` is the one lint where "unavoidable" usually means "not wired up
  yet, but deleting it isn't this change's call to make" — that's a valid
  reason, but say so explicitly rather than leaving the allow unexplained.

**What restoring this signal found:** with the blanket removed,
`cargo clippy --all-features` went from silently clean to over 2000
warnings — 94% of them were the single mechanical `uninlined_format_args`
style lint (fixed via `cargo clippy --fix`), but the rest included real
defects the blanket had been hiding, e.g. a constructed template changelog
entry that was built and then silently discarded (`changelog: None` instead
of `Some(changelog)`) and unreachable branches. Local, documented exceptions
don't have that failure mode: each one is small enough to actually read.

**When NOT to add:**
- "I don't want to refactor" — do the refactor
- "This is faster" — measure it; clippy suggestions are usually equivalent
- "It's more readable" — it probably isn't; clippy catches real issues

### Interpreting Clippy Warnings

When clippy complains, read the **full message**—it includes:

1. **The rule name** (in brackets): `[clippy::rule_name]`
2. **Why it matters**: "This is inefficient" or "This is not idiomatic"
3. **The suggestion**: What to change

```
warning: using `clone` on type `String` which implements `Copy`
   --> src/main.rs:42:15
    |
42  |     let x = s.clone();
    |             ^^^^^^^^^^ help: try dereferencing: `*s`
    |
    = note: `#[warn(clippy::clone_on_copy)]` on by default
```

**What to do:**
1. Read the help text
2. Understand WHY it's suggested
3. Apply the fix or add `#[allow(clippy::rule_name)]` with a comment
4. Run `cargo clippy` again to verify

---

## Pre-Commit Developer Checklist

**Before pushing code, run this checklist locally.** It mirrors what CI will run.

### Quick Pre-Commit Checklist

```bash
#!/bin/bash
# Pre-commit checks before git push

echo "Checking code format..."
cargo fmt --all --check || {
    echo "❌ Format check failed. Run: cargo fmt --all"
    exit 1
}

echo "Running clippy..."
cargo clippy --all-targets -- -D warnings || {
    echo "❌ Clippy failed. Fix warnings or run: cargo clippy --fix"
    exit 1
}

echo "Running tests..."
cargo test --locked || {
    echo "❌ Tests failed"
    exit 1
}

echo "Running smoke tests..."
./scripts/e2e-smoke.sh || {
    echo "⚠️  Smoke tests failed (optional, but recommended)"
    exit 0
}

echo "✅ All checks passed! Ready to push."
```

### Step-by-Step Pre-Commit Process

#### 1. Format Your Code

```bash
# Format everything
cargo fmt --all

# Verify formatting
cargo fmt --all --check
```

After this step, all formatting violations should be fixed. rustfmt is non-negotiable.

#### 2. Run Clippy

```bash
# Run clippy and fix what you can automatically
cargo clippy --fix

# Run clippy and report remaining issues
cargo clippy --all-targets -- -D warnings
```

If clippy reports warnings after `--fix`, you need to manually address them:
- Read the warning message carefully
- Refactor if possible
- Add `#[allow(...)]` with a comment if absolutely necessary
- Re-run clippy to confirm

#### 3. Run All Tests

```bash
# Run unit and integration tests
cargo test --locked

# Run with verbose output to see failures
cargo test --locked -- --nocapture

# Run tests sequentially if debugging
cargo test --locked -- --test-threads=1
```

**All tests must pass locally before pushing.** If tests fail in CI, the PR is blocked.

#### 4. Check Dependencies

```bash
# Run cargo-deny (dependency security/license check)
cargo deny check

# If missing, install it first:
cargo install cargo-deny
cargo deny init  # Creates deny.toml
```

#### 5. Manual Code Review

Before committing:

- [ ] Code follows naming conventions (functions: `snake_case`, types: `PascalCase`)
- [ ] Error messages are clear and actionable
- [ ] No `unwrap()` or `panic!()` in library code (only in `main.rs` for exceptional cases)
- [ ] Public functions have doc comments
- [ ] No debug `println!()` statements left in
- [ ] No commented-out code blocks
- [ ] No `TODO`/`FIXME` without context (should reference an issue)
- [ ] Tests cover normal cases, error cases, and edge cases

#### 6. Commit with Clear Message

```bash
# Use semantic commit messages
git commit -m "feat: add wallet encryption support"

# Format: <type>(<scope>): <message>
# Types: feat, fix, docs, style, refactor, test, chore
# Scope: Optional, but recommended (e.g., wallet, network, contract)
# Message: Lowercase, imperative, no period
```

Good examples:
```
feat(wallet): add hardware wallet support
fix(deploy): handle large contract files correctly
docs(plugin): update plugin development guide
refactor(config): simplify configuration loading
test(network): add integration tests for Horizon API
chore(deps): update clap to 4.5.0
```

#### 7. Push and Verify CI

```bash
git push origin feat/your-feature

# Watch CI at:
# https://github.com/Nanle-code/StarForge/actions
```

CI will re-run:
- **rustfmt** check (all code must be formatted)
- **clippy** check (all warnings are errors)
- **cargo test** (all tests must pass)
- **cargo deny** (dependency audit)
- **smoke tests** (E2E command checks)

If any step fails, you'll see a detailed error message. Fix locally and push again.

### Automating Pre-Commit Checks

You can set up a local git hook to run checks automatically:

```bash
# Create .git/hooks/pre-commit
#!/bin/bash
set -e

echo "Running pre-commit checks..."

cargo fmt --all --check || {
    echo "Format check failed. Run: cargo fmt --all"
    exit 1
}

cargo clippy -- -D warnings || exit 1
cargo test --locked || exit 1

echo "✅ Pre-commit checks passed"
```

Make it executable:
```bash
chmod +x .git/hooks/pre-commit
```

Now `git commit` will automatically run these checks. Add `--no-verify` to skip (not recommended).

---

## CI Pipeline Expectations

This section describes what happens in CI and what will block a PR merge.

### GitHub Actions Workflow

**File**: `.github/workflows/ci.yml`

The CI pipeline runs on every push and pull request. It consists of these jobs:

#### Job 1: Rustfmt Check

```yaml
name: Rustfmt
runs-on: ubuntu-latest
- cargo fmt --all --check
```

**What it checks**: All Rust code is formatted according to rustfmt rules.

**Failure conditions**:
- Any file has formatting differences
- Imports are not grouped correctly
- Lines exceed rustfmt's wrapping preferences

**How to fix**: `cargo fmt --all` locally and push again.

```bash
cargo fmt --all
git add .
git commit -m "style: apply rustfmt"
git push
```

#### Job 2: Cargo Deny

```yaml
name: Cargo Deny
runs-on: ubuntu-latest
- cargo deny check
```

**What it checks**: Dependencies for:
- Known security vulnerabilities (advisory database)
- Copyleft/restricted licenses (GPL, AGPL)
- Duplicate dependencies
- Unused dependencies

**Failure conditions**:
- Any dependency has a known CVE
- Any dependency uses a banned license
- Dependency tree is broken or incomplete

**How to fix**:
1. Check the deny.toml file for rules
2. Update vulnerable dependencies: `cargo update <crate>`
3. If a license is problematic, discuss with maintainers
4. If a denial is incorrect, add an exception to `deny.toml`

```bash
# View deny configuration
cat deny.toml

# Update specific crate
cargo update clap

# Check again locally
cargo deny check
```

#### Job 3: Build, Test, and Clippy

```yaml
name: Build, Test & Clippy
runs-on: ubuntu-latest
- cargo build --locked
- cargo test --locked
- cargo clippy --locked -- -D warnings
```

**What it checks**:
1. **Build**: Project compiles without errors
2. **Tests**: All unit and integration tests pass
3. **Clippy**: No clippy warnings (all treated as errors with `-D warnings`)

**Failure conditions**:
- Compilation errors
- Any test fails
- Any clippy warning appears

**How to fix**:
```bash
# Fix compilation
cargo build --locked

# Fix tests
cargo test --locked -- --nocapture  # See output

# Fix clippy
cargo clippy --locked -- -D warnings
cargo clippy --fix  # Auto-fix when possible
```

#### Job 4: Smoke Tests

```yaml
name: CLI Smoke Tests
runs-on: ubuntu-latest
- cargo test --test cli_smoke --locked
- ./scripts/e2e-smoke.sh
```

**What it checks**:
1. **CLI smoke tests** (`tests/cli_smoke.rs`): Quick checks that main CLI commands work
2. **E2E smoke script** (`scripts/e2e-smoke.sh`): End-to-end verification of common workflows

**Failure conditions**:
- Any command returns unexpected exit code
- Any command output is missing expected text
- Script exits with code 1

**How to fix**:
```bash
# Run locally to see what's failing
cargo test --test cli_smoke --locked -- --nocapture
./scripts/e2e-smoke.sh

# Debug with environment variable
STARFORGE_E2E=1 ./scripts/e2e-smoke.sh  # Includes network tests
```

### CI Status and PR Requirements

**PR Status Requirements:**

All four CI jobs must pass before a PR can be merged:
1. ✅ Rustfmt
2. ✅ Cargo Deny  
3. ✅ Build, Test & Clippy
4. ✅ Smoke Tests

If any job fails, you'll see:
- A red ❌ on the PR
- A comment with the failure log
- A link to the CI run

**View CI logs:**
```
PR → "Checks" tab → Click job name → Scroll to see failure
```

**Re-running CI:**
- Push a new commit to update the PR
- Click "Re-run jobs" button if the failure was environmental (e.g., network timeout)

### Handling CI Failures

**Common CI failures and fixes:**

| Failure | Cause | Fix |
|---------|-------|-----|
| `cargo fmt --all --check` | Code not formatted | `cargo fmt --all` |
| `clippy` warnings | Non-idiomatic code | `cargo clippy --fix` or refactor manually |
| `cargo test` fails | Test assertion failed | Debug with `cargo test -- --nocapture` |
| `cargo deny` fails | Vulnerable/restricted dependency | Update dependency or add exception |
| Smoke test fails | Command output unexpected | Run locally with `./scripts/e2e-smoke.sh` |
| Timeout (>10 min) | Slow test or network issue | Can click "Re-run jobs" |

### Locked Dependencies

CI uses `--locked` flag for deterministic builds:

```bash
# Uses exact versions from Cargo.lock
cargo build --locked
cargo test --locked
cargo clippy --locked
```

This ensures:
- Same versions across environments
- No surprise breaking changes from dependencies
- Reproducible builds

**When to update Cargo.lock:**
```bash
# Update specific dependency
cargo update clap

# Or update all
cargo update

# Commit the updated Cargo.lock
git add Cargo.lock
git commit -m "chore: update dependencies"
```

---

## IDE/Editor Integration

### Visual Studio Code

#### Setup

1. **Install Rust Analyzer extension**:
   - Open VS Code Extensions (Ctrl+Shift+X)
   - Search for "Rust-analyzer"
   - Click "Install"

2. **Install rustfmt and clippy** (if not already installed):
   ```bash
   rustup component add rustfmt clippy
   ```

3. **Configure settings** (`.vscode/settings.json` in project root):

```json
{
  "editor.defaultFormatter": "rust-lang.rust-analyzer",
  "[rust]": {
    "editor.defaultFormatter": "rust-lang.rust-analyzer",
    "editor.formatOnSave": true,
    "editor.codeActionsOnSave": {
      "source.fixAll.clippy": "explicit"
    }
  },
  "rust-analyzer.checkOnSave.command": "clippy",
  "rust-analyzer.checkOnSave.extraArgs": ["--all-targets", "--", "-D", "warnings"],
  "rust-analyzer.diagnostics.enableExperimental": true,
  "rust-analyzer.inlayHints.maxLength": 80,
  "rust-analyzer.assist.emitMustUse": true
}
```

This configuration:
- Formats on save using rustfmt
- Runs clippy on save and shows violations
- Enables experimental diagnostics
- Shows type hints inline

#### Usage in VS Code

- **Format**: `Shift+Alt+F` (or manual `cargo fmt`)
- **Hover for diagnostics**: Hover over red squiggly lines
- **Quick fixes**: `Ctrl+.` when cursor is on warning
- **Go to definition**: `F12` or `Ctrl+Click`
- **Find usages**: `Shift+Alt+F12`

#### Recommended Extensions

```json
{
  "recommendations": [
    "rust-lang.rust-analyzer",
    "serayuzgur.crates",
    "bungcip.better-toml",
    "usernamehw.errorlens",
    "vadimcn.vscode-lldb"
  ]
}
```

Save as `.vscode/extensions.json`, then use "Extensions: Show Recommended" command.

### IntelliJ IDEA / CLion

#### Setup

1. **Install Rust plugin**:
   - Settings → Plugins → Search "Rust"
   - Install "Rust" by JetBrains

2. **Enable formatters and linters**:
   - Settings → Languages & Frameworks → Rust → Rustfmt
   - Check "Run rustfmt on Save"

3. **Enable Clippy**:
   - Settings → Languages & Frameworks → Rust → Clippy
   - Check "Run external linter"
   - Check "Show warnings"

4. **Configure code style**:
   - Settings → Editor → Code Style → Rust
   - Ensure 4-space indentation is set
   - Match inspection settings to clippy

#### Usage in IntelliJ

- **Format**: `Ctrl+Alt+L` (or `Cmd+Alt+L` on macOS)
- **Run inspections**: `Ctrl+Alt+I`
- **Fix with intention**: `Alt+Enter` on error
- **Run Clippy**: Tools → Run External Tools → Clippy

### Vim / Neovim

#### Setup with rust-analyzer

Install rust-analyzer (if not already via rustup):

```bash
rustup component add rust-analyzer
```

For **vim-lsp** users:

```vim
" .vimrc or init.vim
if executable('rust-analyzer')
  augroup lsp
    autocmd!
    autocmd User lsp_setup call lsp#register_server({
        \ 'name': 'rust-analyzer',
        \ 'cmd': {server_info->['rust-analyzer']},
        \ 'workspace_config': {'rust': {'checkOnSave': {'command': 'clippy'}}},
        \ 'allowlist': ['rust'],
        \ })
  augroup end
endif
```

For **Neovim with nvim-lspconfig**:

```lua
-- init.lua
local nvim_lsp = require('lspconfig')
nvim_lsp.rust_analyzer.setup {
  settings = {
    ['rust-analyzer'] = {
      checkOnSave = {
        command = 'clippy',
        extraArgs = { '--all-targets', '--', '-D', 'warnings' }
      }
    }
  }
}

-- Auto format on save
vim.api.nvim_create_autocmd('BufWritePre', {
  pattern = '*.rs',
  callback = function()
    vim.lsp.buf.format { async = false }
  end
})
```

#### Using External Tools

Configure Vim/Neovim to run `cargo fmt` and `cargo clippy` as external tools:

```vim
" Format with :Fmt
command! Fmt execute '!cargo fmt --all' | edit

" Lint with :Lint
command! Lint execute '!cargo clippy -- -D warnings'

" Run tests with :Test
command! Test execute '!cargo test'
```

### Emacs

For Emacs users with **rustic-mode**:

```elisp
(use-package rustic
  :ensure t
  :init
  (setq rustic-linter 'clippy
        rustic-format-on-save t
        rustic-lsp-client 'lsp-mode))

;; Optional: keybindings
(define-key rustic-mode-map (kbd "M-f") #'rustic-format-buffer)
(define-key rustic-mode-map (kbd "C-c C-c l") #'rustic-compile)
```

---

## Automated Enforcement

### Pre-Commit Hooks Setup

To prevent committing code that fails checks, set up git hooks:

```bash
# Create pre-commit hook
mkdir -p .git/hooks

cat > .git/hooks/pre-commit << 'EOF'
#!/bin/bash
set -e

echo "Checking format..."
cargo fmt --all --check || {
    echo "Format check failed. Run: cargo fmt --all"
    exit 1
}

echo "Running clippy..."
cargo clippy -- -D warnings || {
    echo "Clippy violations found."
    exit 1
}

echo "Running unit tests..."
cargo test --lib || exit 1

echo "Pre-commit checks passed!"
EOF

chmod +x .git/hooks/pre-commit
```

Now `git commit` will automatically run checks. To skip (not recommended):
```bash
git commit --no-verify
```

### GitHub Actions Re-run

If CI fails on a flaky test or timeout:

1. Go to the PR on GitHub
2. Click "Checks" or "Details" on a failed job
3. Click "Re-run jobs" button

This re-runs all CI jobs without requiring a new commit.

### Local CI Simulation

Before pushing, run all CI checks locally:

```bash
#!/bin/bash
# Run all CI checks locally

echo "Simulating CI pipeline..."

echo "1/4: Checking format..."
cargo fmt --all --check || exit 1

echo "2/4: Running cargo deny..."
cargo deny check || exit 1

echo "3/4: Building, testing, and clippy..."
cargo build --locked || exit 1
cargo test --locked || exit 1
cargo clippy --locked -- -D warnings || exit 1

echo "4/4: Running smoke tests..."
cargo test --test cli_smoke --locked || exit 1
./scripts/e2e-smoke.sh || exit 1

echo "All CI checks passed locally!"
```

Save as `scripts/ci-check.sh` and run before pushing:
```bash
chmod +x scripts/ci-check.sh
./scripts/ci-check.sh
```

---

## Quick Reference

### One-Liner Command Cheat Sheet

```bash
# Format all code
cargo fmt --all

# Check if formatted (no changes)
cargo fmt --all --check

# Run linter with auto-fix
cargo clippy --fix

# Run linter (deny warnings)
cargo clippy -- -D warnings

# Run all tests
cargo test --locked

# Run specific test
cargo test wallet_create -- --nocapture

# Build everything
cargo build --locked

# Security/license check
cargo deny check

# Run pre-commit checklist
cargo fmt --all --check && cargo clippy -- -D warnings && cargo test --locked

# Full CI simulation
./scripts/ci-check.sh
```

### Common Issues and Solutions

| Issue | Solution |
|-------|----------|
| Code not formatted in CI | `cargo fmt --all` and commit |
| Clippy warnings block PR | `cargo clippy --fix` or manually refactor |
| Tests fail locally but pass in CI | Run with `--locked`: `cargo test --locked` |
| Slow compilation | Use sccache: `export RUSTC_WRAPPER=sccache` |
| "denied by Cargo.lock" | Run `cargo update` and commit `Cargo.lock` |
| IDE not showing errors | Reload Rust-analyzer: `Ctrl+Shift+P` → "Reload Window" |

### Standards at a Glance

| Aspect | Standard | Enforced By |
|--------|----------|-------------|
| Formatting | 4-space indent, rustfmt rules | rustfmt + CI |
| Naming | `snake_case` functions, `PascalCase` types | Clippy + code review |
| Line length | ~100 chars (soft) | Manual review |
| Error handling | `Result<T>` and `?` operator | Clippy + code review |
| Doc comments | `///` on public items | Manual review |
| Imports | Grouped (std, external, internal) | rustfmt + manual review |
| Tests | Unit + integration, all must pass | CI test job |
| Dependencies | No vulnerable/copyleft crates | Cargo Deny |

---

## Summary

**StarForge Code Standards at a Glance:**

1. **Run `cargo fmt --all`** after every change (non-negotiable)
2. **Run `cargo clippy -- -D warnings`** and fix warnings before pushing
3. **Run `cargo test --locked`** to ensure tests pass
4. **Check your IDE is configured** to format and lint on save
5. **Read CI failure messages carefully**—they tell you exactly what to fix
6. **Use the pre-commit checklist** before pushing to avoid CI failures

**Philosophy**: We automate everything so you can focus on logic, not style. When in doubt, run the tools—they're the source of truth.

For questions or suggestions about these standards, open a GitHub issue or discussion.

---

**Last Updated**: 2025-06-01

**Maintainer**: StarForge Core Team

**Related Documents**: [CONTRIBUTING.md](CONTRIBUTING.md), [DEVELOPER_GUIDE.md](DEVELOPER_GUIDE.md), [ARCHITECTURE.md](ARCHITECTURE.md)
