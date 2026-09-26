# Template Compatibility Matrix for Soroban SDK Versions

StarForge templates now support compatibility constraints for both the StarForge CLI version and the Soroban SDK version. This ensures that templates are only used with compatible SDK versions, preventing broken scaffolds from SDK drift.

## Overview

The compatibility matrix is encoded in template metadata and validated at install/scaffold time. Templates can specify:

- **CLI version constraints**: Minimum and maximum StarForge CLI versions
- **SDK version constraints**: Minimum and maximum Soroban SDK versions

## Template Metadata

### TemplateEntry Structure

Templates now include the following compatibility fields:

```rust
pub struct TemplateEntry {
    // ... existing fields ...
    
    /// Minimum StarForge CLI version required (semver, e.g. "0.1.0")
    pub cli_version_min: Option<String>,
    
    /// Maximum StarForge CLI version supported (semver, e.g. "1.99.99")
    pub cli_version_max: Option<String>,
    
    /// Minimum Soroban SDK version required (semver, e.g. "22.0.0")
    pub soroban_sdk_min: Option<String>,
    
    /// Maximum Soroban SDK version supported (semver, e.g. "23.0.0")
    pub soroban_sdk_max: Option<String>,
    
    // ... other fields ...
}
```

## Compatibility Checking

### Compatibility Status

The compatibility check returns one of the following statuses:

- `Compatible`: Template is compatible with current CLI and SDK versions
- `TooOld`: Template requires a newer CLI version
- `TooNew`: Template is not compatible with current (too-new) CLI version
- `MalformedMetadata`: Template metadata contains malformed version strings
- `SorobanSdkIncompatible`: Template requires specific SDK version constraints

### SDK Version Detection

The system automatically detects the Soroban SDK version from a template's `Cargo.toml` file:

```toml
[dependencies]
soroban-sdk = "22.0.0"
```

Detection patterns support:
- Simple version: `soroban-sdk = "22.0.0"`
- With operators: `soroban-sdk = "^22.0.0"`
- Table format: `soroban-sdk = { version = "22.0.0", features = [...] }`

## Publishing Templates with SDK Constraints

### CLI Commands

Publish a template with SDK version constraints:

```bash
starforge template publish \
  --path ./my-template \
  --name my-template \
  --description "My awesome template" \
  --author "Your Name" \
  --version "1.0.0" \
  --soroban-sdk-min "22.0.0" \
  --soroban-sdk-max "23.0.0"
```

### API Usage

```rust
use starforge::utils::templates;

templates::publish_template_versioned(
    &template_path,
    "my-template".to_string(),
    "My awesome template".to_string(),
    "Your Name".to_string(),
    vec!["defi".to_string()],
    "1.0.0".to_string(),
    Some("0.1.0".to_string()),  // cli_version_min
    Some("1.0.0".to_string()),  // cli_version_max
    Some("22.0.0".to_string()), // soroban_sdk_min
    Some("23.0.0".to_string()), // soroban_sdk_max
    Some("MIT".to_string()),
    Some("https://github.com/example".to_string()),
    Some("https://example.com".to_string()),
    Some("https://docs.example.com".to_string()),
).await?;
```

## Validation

### Metadata Validation

The system validates version constraints at publish time:

- Version strings must be valid semver (major.minor.patch)
- Minimum version cannot be greater than maximum version
- Both CLI and SDK version constraints are validated

### Scaffold-Time Validation

When scaffolding a project from a template:

1. CLI version compatibility is checked first
2. SDK version compatibility is checked if constraints are present
3. SDK version is detected from the template's Cargo.toml
4. Scaffold fails clearly on incompatible combinations

### Error Messages

**CLI Version Incompatible:**
```
Error: Template requires StarForge CLI version >= 0.5.0, but running version is 0.1.0
Update StarForge: https://github.com/Nanle-code/StarForge
```

**SDK Version Incompatible:**
```
Error: Template requires Soroban SDK version 22.0.0-23.0.0, but found 21.0.0
Update Soroban SDK: https://github.com/stellar/rs-soroban-sdk
Template: my-template
SDK constraints: min=22.0.0, max=23.0.0
Detected version: 21.0.0
```

## Recommended SDK Constraints

### Common SDK Versions

| SDK Version | Release Date | Status | Recommended Constraint |
|-------------|--------------|--------|----------------------|
| 22.0.0      | 2024-01     | Stable | `soroban_sdk_min: "22.0.0"` |
| 23.0.0      | 2024-06     | Stable | `soroban_sdk_min: "23.0.0"` |
| 24.0.0      | 2024-12     | Latest | `soroban_sdk_min: "24.0.0"` |

### Best Practices

1. **Be Specific**: Use exact minimum versions rather than broad ranges
2. **Test Compatibility**: Test templates against declared SDK versions
3. **Document Changes**: Update SDK constraints when compatibility changes
4. **Graceful Degradation**: Provide fallback behavior when possible

## Example Template Manifest

```json
{
  "name": "defi-token",
  "version": "1.2.0",
  "description": "DeFi token contract template",
  "author": "StarForge Team",
  "cli_version_min": "0.1.0",
  "cli_version_max": "1.0.0",
  "soroban_sdk_min": "22.0.0",
  "soroban_sdk_max": "23.0.0",
  "tags": ["defi", "token", "standard"],
  "license": "MIT",
  "repository_url": "https://github.com/example/defi-token"
}
```

## CI Validation

The CI system validates example templates against declared compatibility matrices:

```yaml
# .github/workflows/template-compatibility.yml
name: Template Compatibility

on:
  push:
    paths:
      - 'templates/**'
      - 'src/utils/templates.rs'

jobs:
  compatibility:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - name: Test template compatibility
        run: |
          cargo test --test template_compatibility_test
```

## Migration Guide

### For Template Authors

1. **Add SDK constraints** to existing templates:
   ```bash
   starforge template publish \
     --soroban-sdk-min "22.0.0" \
     --soroban-sdk-max "23.0.0"
   ```

2. **Test compatibility** with different SDK versions
3. **Update documentation** with compatibility information
4. **Version bump** when compatibility changes

### For Template Users

1. **Check compatibility** before scaffolding:
   ```bash
   starforge template info my-template
   ```

2. **Update SDK** if needed:
   ```bash
   cargo update -p soroban-sdk
   ```

3. **Use compatible template versions**:
   ```bash
   starforge template fetch my-template --version 1.0.0
   ```

## Troubleshooting

### SDK Version Not Detected

If SDK version detection fails:

1. Ensure `Cargo.toml` exists in the template
2. Verify `soroban-sdk` dependency is properly declared
3. Check that version string is in supported format

### Compatibility Check Fails

If compatibility checks fail:

1. Verify version constraints are valid semver
2. Check that min <= max for both CLI and SDK versions
3. Ensure template is tested against declared versions
4. Review template changelog for compatibility notes

## Contributing

When adding new templates:

1. **Test** against multiple SDK versions
2. **Declare** appropriate SDK version constraints
3. **Document** any special compatibility requirements
4. **Update** CI to validate new templates
5. **Consider** backward compatibility when setting constraints
