# Quick Start: Template Marketplace

Get started with the StarForge Template Marketplace in 5 minutes.

## Installation

```bash
# Build StarForge with the new feature
cargo build --release

# Add to PATH (optional)
cp target/release/starforge ~/.local/bin/
```

## 1. Initialize (First Time Only)

```bash
starforge template init
```

This adds example templates to your local registry.

## 2. Discover Templates

### Search
```bash
# Search by keyword
starforge template search defi

# Search with tags
starforge template search dex --tags amm
```

### List All
```bash
starforge template list
```

### View Details
```bash
starforge template show uniswap-v2
```

## 3. Use a Template

```bash
# Create project from marketplace template
starforge new contract my-dex --template uniswap-v2 --from marketplace

# Navigate and build
cd my-dex
stellar contract build
```

## 4. Create Your Own Template

### Step 1: Create a Contract
```bash
starforge new contract my-template
cd my-template
```

### Step 2: Add Placeholders

Edit `Cargo.toml`:
```toml
[package]
name = "{{PROJECT_NAME}}"
```

Edit `src/lib.rs`:
```rust
#[contract]
pub struct {{PROJECT_NAME_PASCAL}};
```

### Step 3: Test It
```bash
cargo test
stellar contract build
```

### Step 4: Publish
```bash
cd ..
starforge template publish ./my-template \
  --name my-awesome-template \
  --description "Does something awesome" \
  --author "Your Name" \
  --tags "defi,awesome"
```

## 5. Share with Others

```bash
# Others can now use your template
starforge new contract test-project \
  --template my-awesome-template \
  --from marketplace
```

## Available Placeholders

| Placeholder | Input: "my-project" | Output |
|-------------|---------------------|--------|
| `{{PROJECT_NAME}}` | my-project | my-project |
| `{{PROJECT_NAME_SNAKE}}` | my-project | my_project |
| `{{PROJECT_NAME_PASCAL}}` | my-project | MyProject |

## Common Commands

```bash
# Search
starforge template search <query>
starforge template search <query> --tags <tag1,tag2>

# List
starforge template list

# Show
starforge template show <name>

# Use
starforge new contract <name> --template <template> --from marketplace

# Publish
starforge template publish <path> [options]

# Remove
starforge template remove <name>

# Initialize
starforge template init
```

## Example Workflow

```bash
# 1. Find a template
starforge template search lending

# 2. Check it out
starforge template show lending-pool

# 3. Use it
starforge new contract my-lending --template lending-pool --from marketplace

# 4. Build and deploy
cd my-lending
stellar contract build
starforge deploy --wasm target/wasm32-unknown-unknown/release/my_lending.wasm
```

## Tips

✅ **Use verified templates** - Look for the ✓ badge
✅ **Review code first** - Always check template source
✅ **Test locally** - Try templates in a safe environment
✅ **Add good tags** - Make your templates discoverable
✅ **Document well** - Include a comprehensive README

## Troubleshooting

**Template not found?**
```bash
starforge template list  # Check available templates
```

**Invalid structure?**
```bash
# Ensure your template has:
# - Cargo.toml
# - src/lib.rs
# - src/ directory
```

**Git clone failed?**
```bash
# Check if git is installed
git --version

# Verify template source URL
starforge template show <template-name>
```

## Built-in Templates

These templates are available without initializing the marketplace:

| Template | Command | Description |
|----------|---------|-------------|
| `hello-world` | `starforge new contract my-contract` | Basic contract with optional storage |
| `token` | `starforge new contract my-token --template token` | Fungible token with mint/burn/transfer |
| `nft` | `starforge new contract my-nft --template nft` | Non-fungible token with URI metadata |
| `voting` | `starforge new contract my-vote --template voting` | DAO proposal and voting contract |
| `stablecoin` | `starforge new contract my-stable --template stablecoin` | Pegged stablecoin with mint/burn |
| `escrow` | `starforge new contract my-escrow --template escrow` | Three-party escrow with arbiter release/refund |

## Next Steps

- Read the [full documentation](TEMPLATE_MARKETPLACE.md)
- Check out [usage examples](examples/template_marketplace_usage.md)
- Explore [example templates](templates/examples/)
- Join the community and share your templates!

## Support

- GitHub Issues: https://github.com/Josetic224/StarForge/issues
- Documentation: https://github.com/Josetic224/StarForge
