# Doctest Guidelines for StarForge

This document explains how to write, maintain, and enforce documentation tests (doctests) for StarForge's public utility APIs.

## Overview

Doctests are Rust code examples embedded in `///` doc comments that are compiled and optionally run as part of `cargo test --doc`. They serve as both documentation and regression tests, ensuring examples in the codebase stay accurate and compilable.

StarForge enforces doctests in CI for selected public utility modules. Broken examples will fail the build.

---

## Enforced Modules

The following modules have doctests enforced in CI (via the `Documentation Tests` job in `.github/workflows/ci.yml`):

| Module | Path | Doctest Status |
|--------|------|----------------|
| `logging` | `src/utils/logging.rs` | `no_run` — compiles but does not execute (initializes global state) |
| `print` | `src/utils/print.rs` | `text` — displays output format, not compiled |
| `doc_extractor` | `src/utils/doc_extractor.rs` | Compiled examples in tests |
| `doc_generator` | `src/utils/doc_generator.rs` | Compiled examples in tests |
| `ai_docs` | `src/utils/ai_docs.rs` | Compiled examples in tests |
| `contract_test_framework` | `src/utils/contract_test_framework.rs` | `ignore` — requires external setup |
| `starforge_plugin_sdk` | `crates/starforge-plugin-sdk/src/lib.rs` | `ignore` — requires `Default` impl and macro context |

To enforce doctests for additional modules, add them to the `Documentation Tests` CI job in `.github/workflows/ci.yml`.

---

## Writing Doctests

### Basic Syntax

Doc comments use `///` (item-level) or `//!` (module-level). Code blocks are fenced with triple backticks:

```rust
/// Add two numbers together.
///
/// # Examples
///
/// ```
/// let result = starforge::utils::add(2, 3);
/// assert_eq!(result, 5);
/// ```
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

### Attributes

Rustdoc supports several attributes on code fences:

| Attribute | When to use |
|-----------|------------|
| `` ``` `` | Default. Code must compile and pass. Use for pure-logic examples. |
| `` ```no_run `` | Code compiles but does **not** execute. Use when the example has side effects (file I/O, network, global state). |
| `` ```ignore `` | Code is **skipped** entirely by `cargo test --doc`. Use when the example cannot compile in isolation (needs external setup, mock context, or framework-specific initialization). |
| `` ```compile_fail `` | Code is expected to **fail** compilation. Use to demonstrate error cases. |
| `` ```text `` | Not treated as Rust code. Use for output format examples. |
| `` ```rust `` | Explicitly marks the block as Rust (same as default). Useful when combined with `ignore` or `no_run`. |

### Rules for Doctests

1. **All public items should have doc comments** with at least a one-line description.
2. **Public utility functions** (in `src/utils/`) should have `# Examples` sections with compilable doctests when practical.
3. **Do not use `ignore` unnecessarily.** If the example can be made to compile, prefer `no_run` or plain `` ``` ``.
4. **Use `no_run` for side-effectful code** — logging init, file writes, network calls, etc.
5. **Use `ignore` only when compilation is impossible** — e.g., the example requires types or setup not available in the doctest harness.
6. **Keep examples concise** — doctests are documentation first, tests second.
7. **Use `# ` hidden lines** for setup code that would clutter the example:
   ```rust
   /// ```
   /// # use starforge::utils::logging::{LogConfig, init};
   /// # fn example() -> anyhow::Result<()> {
   /// init(LogConfig::default())?;
   /// # Ok(())
   /// # }
   /// ```
   ```

### Importing the Crate

Doctests run as separate binaries. To use your crate's items:

```rust
/// ```
/// use starforge::utils::print;
/// print::success("Done!");
/// ```
```

Or use the crate name directly (Rust 2018+ edition):

```rust
/// ```
/// starforge::utils::print::success("Done!");
/// ```
```

---

## CI Enforcement

### How It Works

The `Documentation Tests` job in `.github/workflows/ci.yml` runs:

```bash
cargo test --doc --locked
```

This compiles and runs all non-`ignore` doctests in the library crate. If any doctest fails to compile or panics during execution, the CI job fails.

### What Gets Tested

- **Plain `` ``` `` blocks**: Compiled and executed.
- **`` ```no_run `` blocks**: Compiled but not executed.
- **`` ```ignore `` blocks**: Skipped entirely.
- **`` ```compile_fail `` blocks**: Expected to fail compilation.
- **`` ```text `` blocks**: Not compiled.

### Adding New Modules to Enforcement

1. Add a `# Examples` doctest section to the public item.
2. Ensure the example compiles with `cargo test --doc` locally.
3. The new doctest is automatically included in CI (since `cargo test --doc` runs all library doctests).

---

## Common Patterns

### Pattern: Simple Pure Function

```rust
/// Multiply two numbers.
///
/// # Examples
///
/// ```
/// assert_eq!(starforge::utils::multiply(3, 4), 12);
/// ```
pub fn multiply(a: i32, b: i32) -> i32 {
    a * b
}
```

### Pattern: Function with Side Effects

```rust
/// Initialize the global logging subscriber.
///
/// # Examples
///
/// ```no_run
/// use starforge::utils::logging::{LogConfig, LogFormat, init};
/// init(LogConfig { format: LogFormat::Json, ..Default::default() }).unwrap();
/// ```
pub fn init(config: LogConfig) -> Result<()> {
    // ...
}
```

### Pattern: Example That Needs Setup

```rust
/// Run the contract test framework.
///
/// ```rust,ignore
/// let result = ContractTestFramework::new(config)
///     .add_suite(my_suite)
///     .run()?;
/// ```
pub struct ContractTestFramework { /* ... */ }
```

### Pattern: Demonstrating Output Format

```rust
/// Print a structured CLI error to stderr.
///
/// Output format:
///
/// ```text
///   ✗  Error: <message>
///      Context: <context>
///
///   What to try:
///     → hint one
/// ```
pub fn cli_error(err: &anyhow::Error, hints: &[&str]) { /* ... */ }
```

---

## Troubleshooting

### "error[E0432]: unresolved import"

The doctest cannot find your crate's items. Use the full path:
```rust
/// ```
/// use starforge::utils::print;
/// ```
```

### "error[E0599]: no method named `foo` found"

The example uses an API that has changed. Update the example to match the current API.

### Doctest passes locally but fails in CI

Ensure you're running with `--locked` to match CI's dependency versions:
```bash
cargo test --doc --locked
```

### Doctest is slow or hangs

Mark it `no_run` if it has side effects, or `ignore` if it requires external resources.

---

## References

- [The Rust Reference — Documentation tests](https://doc.rust-lang.org/rustdoc/write-documentation/documentation-tests.html)
- [RFC 1574 — More API documentation conventions](https://github.com/rust-lang/rfcs/blob/master/text/1574-more-api-documentation-conventions.md)
- [CI Enforcement](CI_ENFORCEMENT.md)
- [Contributing Guide](CONTRIBUTING.md)

---

*Last updated: 2026-08-30 — Issue #793: Enforce documentation tests for public utility APIs*
