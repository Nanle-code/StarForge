## Reject Incompatible Plugins Safely and Publish Supported-Version Policy

### Description

Safely rejects incompatible plugins prior to binary execution and publishes a formal supported-version policy across the StarForge Plugin SDK, manifest schemas, and plugin loader engine.

### Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [x] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to change)
- [x] Documentation update

### Changes Made

- **Plugin SDK** (`crates/starforge-plugin-sdk/src/lib.rs`):
  - Updated `PluginMeta` struct to track target `starforge_version`.
  - Added constructors `PluginMeta::new()` and `PluginMeta::with_starforge_version()`.
  - Added helper `is_compatible_with()` to query target core version alignment.
- **Manifests & Version Policy Engine** (`src/plugins/manifest.rs`):
  - Implemented `SupportedVersionPolicy` struct with policy evaluation (`evaluate()`) and policy summary formatting (`policy_summary()`).
  - Added robust semver string parser (`parse_version_parts`) supporting semver tags (e.g., `v1.0.0`, `0.1.0-alpha`).
  - Enforced major version compatibility rules and version range bounds (`starforge_version_min` / `starforge_version_max`).
  - Enhanced `PluginManifest::validate()` and `validate_for_core()` to produce actionable error diagnostics.
- **Pre-Load Loader Safety** (`src/plugins/loader.rs`):
  - Shifted `starforge-plugin.toml` manifest discovery and compatibility validation to run **before** invoking `Library::new()`.
  - Guarantees incompatible plugin binaries are safely rejected prior to dynamic library loading or OS dynamic symbol linking.
- **Integration Test Suite** (`tests/plugin_version_compatibility_test.rs`):
  - Added test suite covering happy paths (compatible manifest validation & load inspection), boundary cases (exact `starforge_version_min` / `starforge_version_max` bounds matching `CORE_VERSION`, policy summary formatting), and failure paths (incompatible major version, min version violation, max version violation, missing required manifest fields, absent manifest error formatting).
- **User Guidance & Documentation** (`PLUGIN_TRUST.md`):
  - Published comprehensive "Supported-Version Policy and Compatibility Requirements" section.
  - Documented pre-load manifest verification, major version rules, version range bounds (`starforge_version_min`/`max`), manifest schema examples, and developer migration guidance.

### Testing

#### How has this been tested?

Executed dedicated integration test suite in `tests/plugin_version_compatibility_test.rs` via `cargo test --test plugin_version_compatibility_test`.

- [x] Unit tests added/updated
- [x] Integration tests added/updated
- [x] Manual testing performed

#### Test Coverage

- **Happy path**: Valid plugin manifest matching `CORE_VERSION` passes validation and loading inspection.
- **Edge cases**: Exact min/max bounds matching current `CORE_VERSION`, policy summary generation, semver parsing logic (`v1.0.0`, `0.1.0-alpha`).
- **Error handling**: Rejection of incompatible major versions (e.g. `1.0.0` vs `0.1.0`), minimum version constraint violations (`starforge_version_min`), maximum version constraint violations (`starforge_version_max`), missing required fields, absent manifest file errors.

### Code Quality Checklist

- [x] My code follows the style guidelines of this project
- [x] I have performed a self-review of my own code
- [x] I have commented my code, particularly in hard-to-understand areas
- [x] I have made corresponding changes to the documentation
- [x] My changes generate no new warnings/errors
- [x] I have added tests that prove my fix is effective or that my feature works
- [x] New and existing unit/integration tests pass locally (8/8 tests passing)
- [x] The CI checks pass

### Breaking Changes

- [x] No breaking changes (backward-compatible manifest validation and safe fallback checks).

### Documentation

- [x] `PLUGIN_TRUST.md` updated
