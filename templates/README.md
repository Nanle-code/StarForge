# StarForge Template Marketplace

This directory contains the template registry for the StarForge CLI marketplace feature.

## Overview

The template marketplace allows developers to discover, share, and use community-contributed Soroban smart contract templates.

## Registry Structure

The `registry.json` file contains metadata for all available templates:

```json
{
  "version": "1",
  "templates": [
    {
      "name": "template-name",
      "version": "1.0.0",
      "description": "Template description",
      "author": "Author Name",
      "tags": ["tag1", "tag2"],
      "source": {
        "type": "git",
        "url": "https://github.com/user/repo",
        "branch": "main"
      },
      "created_at": "2025-01-01T00:00:00Z",
      "updated_at": "2025-01-01T00:00:00Z",
      "downloads": 0,
      "verified": false
    }
  ]
}
```

## Registry Validation

`registry.schema.json` is the authoritative description of a registry document,
and StarForge checks a registry against it **before using it** — whether that
registry is the one bundled with the binary, one fetched from the marketplace,
or the local cache at `~/.starforge/templates/registry.json`. A malformed
template therefore fails immediately, naming the field at fault:

```text
templates[3].version: 'v1.2' is not valid semver (expected major.minor.patch, e.g. "1.2.0")
templates[3].source.url: required field is missing
templates[4].maintenance: 'archived' is not one of: active, maintained, deprecated, unknown
```

Check a registry yourself with:

```bash
# validate the registry the CLI would load
starforge template validate

# validate a specific registry file, or a file holding one template entry
starforge template validate templates/registry.json
starforge template validate ./my-template.json

# machine-readable report (exit status is non-zero when invalid)
starforge template validate --json
```

### What is checked

Beyond types and required fields, the schema carries semantic checks that
plain JSON Schema cannot express (declared as `x-format`):

| Check | Applies to | Rule |
|---|---|---|
| `semver` | `version`, `cli_version_min`, `cli_version_max`, `changelog[].version` | Exactly `major.minor.patch`, all numeric |
| `rfc3339` | `created_at`, `updated_at`, `security_review.audited_at` | RFC 3339 timestamp; the empty string means "unset" |
| `date` | `changelog[].date` | `YYYY-MM-DD` |
| `url` | `repository`, `homepage`, `documentation` | Absolute `http(s)://` URL |
| `git-url` | `source.url` | `https://`, `http://`, `git://`, `ssh://` or `git@host:path` |
| `template-name` | `name`, builtin `source.id` | No path separators, whitespace or control characters — the name becomes a directory under the template store |

Two further rules apply across a whole registry: `cli_version_min` may not
exceed `cli_version_max`, and no two entries may share a `name` **and**
`version` (the same template at different versions is fine).

### Where validation runs

| Point | Behaviour on failure |
|---|---|
| Remote registry fetch | Rejected **before** it is cached, so a broken marketplace index cannot replace a working local cache |
| Local registry read | Fails with the offending fields rather than an opaque parse error |
| Bundled registry fallback | Same check — the offline fallback is held to the schema too |
| `template install` / `publish` / `remove` | The registry is validated before it is written, so a malformed entry never reaches disk |
| `template install` / `fetch` | The derived template name is checked before any file is fetched |

### Compatibility

Unknown fields are **not** an error: an older CLI can read a registry written
by a newer one. They are reported as warnings by `starforge template validate`
so authors still catch misspelled field names.

> **Migration note:** `security_review.findings` is a *number* (the count of
> findings), matching the schema and the published registry. A hand-written
> registry that quoted it (`"findings": "0"`) must drop the quotes.

## Template Sources

Templates can come from three sources:

1. **Git Repository**: Clone from a Git URL
2. **Local Path**: Copy from a local directory
3. **Built-in**: Pre-packaged templates in StarForge

## Using Templates

### Search for templates
```bash
starforge template search defi
starforge template search --tags defi,dex
```

### List all templates
```bash
starforge template list
```

### View template details
```bash
starforge template show uniswap-v2
```

### Use a template
```bash
starforge new contract my-dex --template uniswap-v2 --from marketplace
```

### Publish your own template
```bash
starforge template publish ./my-template \
  --name my-awesome-template \
  --description "An awesome Soroban contract" \
  --author "Your Name" \
  --tags "defi,custom" \
  --version "1.0.0"
```

## Template Requirements

To be valid, a template must contain:

- `Cargo.toml` - Rust package manifest
- `src/` directory - Source code
- `src/lib.rs` - Main contract file

## Example Templates

Built-in example templates are provided under `templates/examples/`:

- `simple-counter`: A basic smart contract demonstrating storage usage by incrementing, getting, and resetting a counter.
- `token-allowlist`: A smart contract for managing an allowlist of approved addresses, controlled by an administrator.
- `escrow`: A DeFi token escrow with buyer, seller, and arbiter roles for marketplaces, freelance payments, and OTC trades.
- `dao-governance`: A minimal DAO governance contract with member proposals and one-member-one-vote tallying.
- `multisig-vault`: A threshold (M-of-N) multi-signature vault for shared-custody token transfers and treasuries.

## Template Placeholders

Templates can use placeholders that will be replaced during scaffolding:

- `{{PROJECT_NAME}}` - Project name (e.g., "my-project")
- `{{PROJECT_NAME_SNAKE}}` - Snake case (e.g., "my_project")
- `{{PROJECT_NAME_PASCAL}}` - Pascal case (e.g., "MyProject")

## Contributing Templates

To contribute a template to the official registry:

1. Create your template following the structure requirements
2. Test it locally with `starforge template publish`
3. Submit a PR to add it to `templates/registry.json`
4. Include documentation and examples

## Verified Templates

Templates marked as `verified: true` have been reviewed by the StarForge maintainers for:

- Code quality and security
- Proper documentation
- Working examples
- Best practices compliance
