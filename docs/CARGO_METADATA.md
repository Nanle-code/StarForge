# Cargo Metadata & Package Links Correction Guide

This document explains how to detect, validate, and fix package metadata and link references in `Cargo.toml` files across StarForge repository and member crates.

---

## 1. Overview

Cargo requires accurate repository, homepage, and documentation URLs prior to publishing crates to `crates.io`. StarForge provides automated scanning and correction utilities (`CargoMetadataFixer` and `CargoMetadataValidator`) in `src/utils/cargo_metadata.rs`.

### Supported Metadata Fields

- `repository`: Source code repository URL (must use `http://` or `https://`).
- `homepage`: Project home page URL.
- `documentation`: Published API documentation URL (defaults to `https://docs.rs/<crate_name>`).
- `license` / `license-file`: Validates license identification or checks file existence relative to crate/repository root.
- `name`: Enforces crates.io alphanumeric and dash/underscore naming conventions.

---

## 2. Running Metadata Correction

### Programmatic Usage

```rust
use starforge::utils::cargo_metadata::{CargoMetadataFixer, CargoMetadataValidator};
use std::path::Path;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let repo_root = Path::new(".");
    let target_repo_url = "https://github.com/onajidavid87-web/StarForge";

    // Scan directory tree, update placeholders, and insert missing fields
    let fix_report = CargoMetadataFixer::locate_and_update_cargo_tomls(repo_root, target_repo_url)?;
    println!("Scanned {} files, updated {} files.", fix_report.files_scanned, fix_report.files_updated.len());

    // Validate a specific crate's metadata and package links
    let val_report = CargoMetadataValidator::validate_package_links(Path::new("Cargo.toml"))?;
    if val_report.is_valid {
        println!("All package links valid for {}", val_report.package_name);
    }

    Ok(())
}
```

---

## 3. Compatibility Notes

- **Rust Version:** Supported on Rust 1.80+ (`edition = "2021"`).
- **Cargo Spec:** Complies with Cargo manifest format specifications for published packages.
- **Monorepos:** Fully compatible with workspace roots and sub-crates.

---

## 4. Security Considerations

> [!IMPORTANT]
> **Pre-Publish Verification**: Always run link validation before invoking `cargo publish`. Publishing crates with broken or hijacked repository URLs can expose users to supply chain vulnerabilities.
> - Ensure all URLs use `https://` protocols.
> - Reject common placeholder patterns (`TODO`, `example.com`, `YOUR_USERNAME`).
> - Confirm that private crates explicitly declare `publish = false`.

---

## 5. Migration Guide & Before/After Examples

### Before Metadata Correction

```toml
[package]
name = "starforge-wasm"
version = "0.1.0"
edition = "2021"
rust-version = "1.80"
description = "WebAssembly API surface for StarForge"
license = "MIT"
repository = "https://github.com/YOUR_USERNAME/starforge"
keywords = ["stellar", "soroban", "wasm"]
```

### After Metadata Correction

```toml
[package]
name = "starforge-wasm"
version = "0.1.0"
edition = "2021"
rust-version = "1.80"
description = "WebAssembly API surface for StarForge"
license = "MIT"
repository = "https://github.com/onajidavid87-web/StarForge"
homepage = "https://github.com/onajidavid87-web/StarForge"
documentation = "https://docs.rs/starforge-wasm"
keywords = ["stellar", "soroban", "wasm"]
```
