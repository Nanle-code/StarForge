# Plugin Trust Model and Lifecycle

StarForge supports third-party plugins through a shared-library extension
system. This document describes the trust model, compatibility requirements,
and the full plugin lifecycle.

---

## Trust levels

Every installed plugin is assigned one of three trust levels at install time:

| Level     | When assigned                                 | StarForge behaviour                                  |
| --------- | --------------------------------------------- | ---------------------------------------------------- |
| `local`   | Plugin installed via `--path` (no source URL) | Always loaded without warnings                       |
| `trusted` | Source URL matches a known trusted prefix     | Loaded without warnings                              |
| `unknown` | Source URL provided but not in the allow-list | Warning shown on load; `--force` required to install |

### Trusted sources

Trusted sources are defined in the StarForge configuration (`~/.starforge/config.toml`). By default, it includes:

- `https://github.com/Nanle-code/starforge-*`
- `https://github.com/StarForge-Labs/*`
- `https://crates.io/crates/starforge-plugin-*`

Any other source is `unknown` unless explicitly added to the `plugin_trust.trusted_sources` list in your configuration.

---

## Supported-Version Policy and Compatibility Requirements

Plugins are native shared libraries loaded at runtime via `libloading`.
To guarantee host stability, type safety, and memory safety, StarForge enforces a strict **Supported-Version Policy** and pre-load binary verification:

### Pre-load Manifest Verification
To prevent OS-level dynamic library linker crashes or undefined behavior when loading incompatible binaries, the plugin loader inspects and validates the plugin manifest (`starforge-plugin.toml`) **before** invoking `Library::new()`.

### Compatibility Rules

1. **rustc ABI Alignment** — The plugin binary must be compiled with the exact same `rustc` toolchain version as the host StarForge executable.
2. **Major Version Alignment** — The plugin's `starforge_version` major number must match the running StarForge major version (e.g. `0.x.y` plugins are incompatible with `1.x.y` StarForge CLI hosts).
3. **SemVer Range Bounds** — Plugins may declare `starforge_version_min` and/or `starforge_version_max` in `starforge-plugin.toml`. The running StarForge CLI host must fall within `[min, max]`.

### Manifest Schema Example (`starforge-plugin.toml`)

```toml
name = "my-plugin"
version = "1.0.0"
starforge_version = "0.1.0"
starforge_version_min = "0.1.0"
starforge_version_max = "0.9.9"
description = "StarForge compatible plugin"
```

### Migration Guidance
- **Updating Plugins for New StarForge Versions**: Bump `starforge_version` in `starforge-plugin.toml` and rebuild using the matching Rust toolchain (`rustup override set <toolchain>`).
- **Handling Version Rejections**: When a plugin is rejected with `PluginLoadError::ManifestIncompatible` or `UnsupportedCoreVersion`, check the error diagnostic for the expected core version and rebuild instructions.

---

## Plugin lifecycle

### Install

```bash
# From a local path (always trusted)
starforge plugin install my-plugin --path ./libstarforge_my_plugin.so

# From a trusted source URL
starforge plugin install my-plugin --source https://github.com/Nanle-code/starforge-my-plugin

# From an unknown source (requires --force)
starforge plugin install my-plugin \
    --path ./libstarforge_my_plugin.so \
    --source https://example.com/my-plugin \
    --force
```

### List

```bash
starforge plugin list
```

Shows all installed plugins with their path, trust level, and source.

### Load and execute

```bash
starforge plugin load          # loads and reports all installed plugins
starforge my-plugin <args>     # execute a loaded plugin as an external subcommand
```

### Crash isolation and diagnostics

StarForge isolates third-party plugin crashes at the host boundary. Runtime panics during plugin registration or execution are caught before they unwind into the CLI process, and the user receives a structured diagnostic instead of a corrupted host state.

This applies to:

- invalid plugin paths and corrupted shared libraries
- unsupported runtime environments or incompatible plugin versions
- runtime panics in `on_load()` and `execute()`
- failure paths that return actionable guidance instead of aborting the host CLI

### Verify

```bash
starforge plugin verify              # verify all installed plugins
starforge plugin verify my-plugin    # verify a specific plugin
```

Checks:

- Library file exists on disk at the registered path
- Trust level is `local` or `trusted`

### Uninstall

```bash
starforge plugin uninstall my-plugin
```

Removes the plugin from the registry. The library file on disk is **not**
deleted — remove it manually if desired.

---

## Building a plugin

Use the `starforge-plugin-sdk` crate:

```toml
# Cargo.toml
[dependencies]
starforge-plugin-sdk = { path = "crates/starforge-plugin-sdk" }

[lib]
crate-type = ["cdylib"]
```

```rust
use starforge_plugin_sdk::{export_plugin, Plugin, PluginRegistrar};

struct MyPlugin;

impl Plugin for MyPlugin {
    fn name(&self) -> &'static str { "my-plugin" }
    fn version(&self) -> &'static str { "0.1.0" }
    fn description(&self) -> &'static str { "My StarForge plugin" }
    fn execute(&self, args: &[String]) -> Result<(), String> {
        println!("Hello from my-plugin! args={:?}", args);
        Ok(())
    }
}

fn register(registrar: &mut dyn PluginRegistrar) {
    registrar.register_plugin(Box::new(MyPlugin));
}

export_plugin!(register);
```

Build with the **same** Rust toolchain used to build StarForge:

```bash
cargo build --release
```

---

## Publisher Authentication and Signature Verification

StarForge supports cryptographic publisher authentication using Ed25519 signatures. Plugin authors can sign the compiled shared library binary with a Stellar public key (`G...` address or 32-byte hex public key) and distribute the signature in `starforge-plugin.toml`.

### Publisher Signature Manifest Schema

```toml
# starforge-plugin.toml
name = "my-plugin"
version = "1.0.0"
starforge_version = "0.1.0"
description = "My signed plugin"

publisher = "StarForge Labs"
publisher_key = "GABC1234567890EXAMPLEPUBLICKEY..."
signature = "hex_or_base64_ed25519_signature_over_sha256_binary_digest..."
```

### Verification Statuses

When a plugin is verified, StarForge computes the SHA-256 digest of the shared library binary on disk and checks the Ed25519 signature:

| Verification Status | Meaning |
| ------------------- | ------- |
| `verified` | Signature successfully verified against binary checksum using publisher key |
| `unsigned` | No signature provided in `starforge-plugin.toml` |
| `invalid_signature` | Signature check failed (binary was tampered with or key mismatch) |
| `untrusted_publisher` | Signature valid, but publisher key is not in `trusted_publishers` allowlist |
| `malformed_key` | `publisher_key` is not a valid Stellar G-address or hex public key |
| `malformed_signature` | `signature` string format is invalid hex/base64 |

### Configuring Publisher Allowlist & Signature Policy

In `~/.starforge/config.toml`:

```toml
[plugin_trust]
# List of trusted publisher Stellar public keys or hex public keys
trusted_publishers = [
    "GABC1234567890EXAMPLEPUBLICKEY...",
]

# Strict signature enforcement: reject unsigned plugins during install and load
require_signatures = false
```

---

## Security considerations

- Never load plugins from sources you do not control.
- Prefer `--path` installs from artifacts you have reviewed or plugins with verified publisher signatures (`verified`).
- If `require_signatures = true` is set, all plugins must contain a valid Ed25519 signature from a recognized publisher key.
- The `--force` flag bypasses source trust and signature warnings for administrative troubleshooting, but does **not** bypass ABI or major-version compatibility checks.

