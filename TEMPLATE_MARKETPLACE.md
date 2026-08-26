# Template Marketplace Feature

## Overview

The Template Marketplace feature enables community-contributed Soroban smart contract templates with versioning, discovery, and easy scaffolding. This allows developers to quickly bootstrap projects using battle-tested templates from the community.

## Features

### 1. Template Discovery
Search and browse available templates by name, description, or tags:

```bash
# Search for DeFi templates
starforge template search defi

# Search with tag filtering
starforge new contract --search defi --tags dex,amm

# List all available templates
starforge template list

# View detailed information about a template
starforge template show uniswap-v2
```

#### Relevance ranking, filters and explanations

Search ranks results by **text relevance** first (name matches outweigh tag
matches, which outweigh description matches), then by quality score, then by
downloads. Each result explains *why* it matched so the list is easy to scan.

```bash
# Rank by relevance to the query
starforge template search token

# Require specific tags (a template must have all of them)
starforge template search "" --tags defi,dex

# Only verified templates
starforge template search wallet --verified

# Only high-quality templates (score 0-100)
starforge template search defi --min-quality 70

# List everything ranked by quality (empty query)
starforge template search
```

Each result shows the matched fields and a relevance value, e.g.:

```
   1. uniswap-v2@1.0.0  [quality 92/100]  ✓ Verified  📖 Documented  ★ Popular (1240 downloads)
      Matched: name, tag: defi (relevance 70)
```

### 2. Template Usage
Scaffold new projects using marketplace templates:

```bash
# Use a marketplace template
starforge new contract my-dex --template uniswap-v2 --from marketplace

# Search and use in one workflow
starforge new contract --search lending
# Then use the found template
starforge new contract my-lending --template lending-pool --from marketplace
```

### 3. Template Publishing
Share your templates with the community:

```bash
# Publish a template
starforge template publish ./my-template \
  --name my-awesome-template \
  --description "An awesome Soroban contract" \
  --author "Your Name" \
  --tags "defi,custom" \
  --version "1.0.0"

# Interactive publishing (prompts for missing info)
starforge template publish ./my-template
```

### 4. Template Management
Manage your local template registry:

```bash
# Initialize registry with example templates
starforge template init

# Remove a template
starforge template remove my-template
```

## Template Structure

### Required Files
Every template must contain:
- `Cargo.toml` - Rust package manifest
- `src/` directory - Source code
- `src/lib.rs` - Main contract file

### Optional Files
- `README.md` - Documentation
- `.cargo/config.toml` - Cargo configuration
- `tests/` - Test files

### Template Placeholders
Templates support automatic variable substitution:

| Placeholder | Example Input | Example Output |
|-------------|---------------|----------------|
| `{{PROJECT_NAME}}` | my-project | my-project |
| `{{PROJECT_NAME_SNAKE}}` | my-project | my_project |
| `{{PROJECT_NAME_PASCAL}}` | my-project | MyProject |

Example usage in `Cargo.toml`:
```toml
[package]
name = "{{PROJECT_NAME}}"
version = "0.1.0"
```

Example usage in `src/lib.rs`:
```rust
#[contract]
pub struct {{PROJECT_NAME_PASCAL}};
```

## Template Sources

Templates can be sourced from three locations:

### 1. Git Repository
```json
{
  "source": {
    "type": "git",
    "url": "https://github.com/user/repo",
    "branch": "main"
  }
}
```

### 2. Local Path
```json
{
  "source": {
    "type": "local",
    "path": "/path/to/template"
  }
}
```

### 3. Built-in
```json
{
  "source": {
    "type": "builtin",
    "id": "hello-world"
  }
}
```

## Registry Format

The template registry is stored in `~/.starforge/templates/registry.json`:

```json
{
  "version": "1",
  "templates": [
    {
      "name": "uniswap-v2",
      "version": "1.0.0",
      "description": "Uniswap V2 style AMM DEX",
      "author": "Stellar Community",
      "tags": ["defi", "dex", "amm"],
      "source": {
        "type": "git",
        "url": "https://github.com/stellar/soroban-examples",
        "branch": "main"
      },
      "created_at": "2025-01-01T00:00:00Z",
      "updated_at": "2025-01-01T00:00:00Z",
      "downloads": 42,
      "verified": true
    }
  ]
}
```

## Template Verification

Templates can be marked as `verified: true` by maintainers. Verified templates:
- Have been reviewed for security and quality
- Follow Soroban best practices
- Include proper documentation
- Have working tests

Verified templates appear first in search results with a ✓ badge.

## Example Workflow

### For Template Users

1. **Discover templates:**
   ```bash
   starforge template search defi
   ```

2. **View template details:**
   ```bash
   starforge template show uniswap-v2
   ```

3. **Create project from template:**
   ```bash
   starforge new contract my-dex --template uniswap-v2 --from marketplace
   ```

4. **Build and deploy:**
   ```bash
   cd my-dex
   stellar contract build
   starforge deploy --wasm target/wasm32-unknown-unknown/release/my_dex.wasm
   ```

### For Template Authors

1. **Create your template:**
   ```bash
   # Create a new contract as usual
   starforge new contract my-template
   cd my-template
   
   # Add your custom logic
   # Replace hardcoded names with placeholders
   ```

2. **Add placeholders:**
   ```rust
   // In src/lib.rs
   #[contract]
   pub struct {{PROJECT_NAME_PASCAL}};
   ```

3. **Test the template:**
   ```bash
   cargo test
   stellar contract build
   ```

4. **Publish to marketplace:**
   ```bash
   cd ..
   starforge template publish ./my-template \
     --name my-awesome-template \
     --description "Does something awesome" \
     --author "Your Name" \
     --tags "defi,awesome" \
     --version "1.0.0"
   ```

5. **Share with others:**
   ```bash
   # Others can now use it
   starforge new contract test-project --template my-awesome-template --from marketplace
   ```

## Built-in Example Templates

The marketplace includes several example templates:

1. **uniswap-v2** - AMM DEX implementation
2. **lending-pool** - Lending and borrowing protocol
3. **governance** - DAO governance with voting
4. **multisig-wallet** - Multi-signature wallet

Initialize these with:
```bash
starforge template init
```

## Implementation Details

### File Structure
```
src/
├── commands/
│   ├── new.rs          # Extended with marketplace support
│   └── template.rs     # New template management commands
├── utils/
│   └── templates.rs    # Template registry and operations
templates/
├── registry.json       # Default template registry
├── README.md          # Template documentation
└── examples/
    └── simple-counter/ # Example template
```

### Key Functions

**Template Discovery:**
- `search_templates(query, tags)` - Search with filtering
- `get_template(name)` - Get specific template
- `load_registry()` - Load template registry

**Template Operations:**
- `fetch_template(entry, dest)` - Download/copy template
- `validate_template_structure(path)` - Validate template
- `publish_template(...)` - Publish new template

**Registry Management:**
- `add_template(entry)` - Add to registry
- `remove_template(name)` - Remove from registry
- `save_registry(registry)` - Persist changes

## Future Enhancements

Potential improvements for future versions:

1. **Remote Registry** - Central registry server for global template sharing
2. **Template Versioning** - Support multiple versions of the same template
3. **Template Updates** - Update existing templates to newer versions
4. **Template Dependencies** - Templates that depend on other templates
5. **Template Categories** - Organize templates into categories
6. **Template Ratings** - Community ratings and reviews
7. **Template Analytics** - Usage statistics and popularity metrics
8. **Template CI/CD** - Automated testing and verification
9. **Template Marketplace UI** - Web interface for browsing templates

## Security Considerations

When using templates from the marketplace:

1. **Review Code** - Always review template code before using
2. **Verify Source** - Check the template source (Git URL, author)
3. **Use Verified** - Prefer verified templates when available
4. **Test Thoroughly** - Test templates in a safe environment first
5. **Update Dependencies** - Keep template dependencies up to date

## Contributing

To contribute templates to the official registry:

1. Create a high-quality template following best practices
2. Test thoroughly with multiple project names
3. Add comprehensive documentation
4. Submit a PR adding your template to `templates/registry.json`
5. Include examples and usage instructions

## Template Cache

Marketplace templates are cloned with `--depth 1` (shallow clone) and stored in
`~/.starforge/template-cache/<name>/`.  On subsequent runs the cached copy is
reused, so no network round-trip occurs.

### Cache location

```
~/.starforge/template-cache/
├── token-standard/     ← cached after first use
├── lending-pool/
└── uniswap-v2/
```

### Force-refresh

Pass `--force-refresh` to delete the cached copy and re-clone:

```bash
starforge new contract my-token --template token-standard --force-refresh
```

This is useful when the upstream template has been updated and you want the
latest version.

### How it works

1. `fetch_template_cached` checks `~/.starforge/template-cache/<name>/`.
2. If the directory exists and `--force-refresh` is not set, it is used as-is.
3. If `--force-refresh` is set (or the directory does not exist), the old cache
   is removed and the template is re-cloned with `git clone --depth 1`.
4. `template_source_content` reads `src/lib.rs` from the cached directory and
   returns it to the scaffolding step.

## Quality Signals & Trust Indicators

To help users identify dependable templates in a growing community catalog,
each template carries lightweight quality metadata that is surfaced across the
`list`, `search` and `show` commands.

### Metadata fields

| Field         | Meaning                                                        |
|---------------|----------------------------------------------------------------|
| `verified`    | Template has been vetted by maintainers                        |
| `documented`  | Template ships user-facing documentation (e.g. a README)       |
| `maintenance` | Maintenance state: `active`, `maintained`, `deprecated`, `unknown` |
| `downloads`   | Usage metadata used as a proxy for community confidence        |

Example registry entry:

```json
{
  "name": "uniswap-v2",
  "version": "1.0.0",
  "verified": true,
  "documented": true,
  "maintenance": "active",
  "downloads": 1240
}
```

### Quality score

Each template is assigned a `0-100` quality score blending the signals above:

- Verified: `+40`
- Documented: `+20`
- Usage: up to `+30` (scaled by downloads, capped)
- Maintenance: `active +10`, `maintained +5`, `deprecated -25`

The score drives ranking in `starforge template search` (highest quality
first, with raw downloads breaking ties) and is shown alongside trust badges
such as `✓ Verified`, `📖 Documented`, `🟢 Actively maintained` and
`★ Popular`. This makes trusted, well-documented and well-maintained templates
easier to discover.

## Installation Progress & Recovery

Installing a marketplace template runs through three visible steps, each shown
with a spinner that resolves to a check mark:

```
✓ Fetched template 'uniswap-v2'
✓ Template structure is valid
✓ Installed into 'my-dex'
```

### Safe rollback

Installation is **atomic from the user's point of view**: if any step fails the
partially-written files are removed automatically, so you never end up with a
half-installed project directory.

- The download is staged in a temporary directory that is always cleaned up.
- The target project directory is only kept once every step succeeds; on
  failure it is rolled back.

### Actionable errors

When a step fails, the error explains what went wrong and how to recover, e.g.:

```
Failed to fetch template 'uniswap-v2' from git:https://github.com/...
  • Check your network connection and that `git` is installed.
  • The partial download was rolled back automatically.
```

## Support

For issues or questions:
- GitHub Issues: https://github.com/Josetic224/StarForge/issues
- Documentation: https://github.com/Josetic224/StarForge/blob/main/README.md
