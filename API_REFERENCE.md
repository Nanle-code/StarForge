# StarForge API Reference

Complete reference for all StarForge commands, options, and utilities.

> For a concise, navigable index of every CLI subcommand see [docs/COMMAND_REFERENCE.md](docs/COMMAND_REFERENCE.md).

## Table of Contents

1. [Command Line Interface](#command-line-interface)
2. [Wallet Commands](#wallet-commands)
3. [Template Commands](#template-commands)
4. [Contract Commands](#contract-commands)
5. [Network Commands](#network-commands)
6. [Transaction Commands](#transaction-commands)
7. [Utility Commands](#utility-commands)
8. [Configuration](#configuration)
9. [Exit Codes](#exit-codes)

---

## Command Line Interface

### Global Options

```bash
starforge [OPTIONS] <COMMAND>
```

| Option | Description |
|--------|-------------|
| `-q, --quiet` | Suppress ASCII banner and decorative output |
| `-h, --help` | Print help information |
| `-V, --version` | Print version information |

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `STARFORGE_CONFIG_DIR` | Configuration directory | `~/.starforge` |
| `STARFORGE_TELEMETRY` | Enable/disable telemetry | `true` |
| `RUST_LOG` | Logging level | `info` |

---

## Wallet Commands

### `starforge wallet create`

Create a new Stellar keypair and save it locally.

**Usage:**
```bash
starforge wallet create <NAME> [OPTIONS]
```

**Arguments:**
- `<NAME>` - Friendly name for the wallet (alphanumeric, dash, underscore)

**Options:**
- `--fund` - Fund the wallet via Friendbot immediately (testnet only)
- `--network <NETWORK>` - Network to associate with wallet (`testnet`, `mainnet`)
- `--encrypt` - Encrypt the secret key with a passphrase

**Examples:**
```bash
# Create basic wallet
starforge wallet create alice

# Create and fund on testnet
starforge wallet create deployer --fund

# Create with encryption
starforge wallet create secure-wallet --encrypt

# Create on mainnet
starforge wallet create mainnet-wallet --network mainnet
```

**Output:**
```
◆ Creating wallet 'alice'

[1/2] Generating keypair…

Public Key     : GABC...XYZ
Secret Key     : Stored in plaintext (not recommended for mainnet).

[2/2] Saving to ~/.starforge/config.toml…

✓ Wallet 'alice' created and saved!

View it with: starforge wallet show alice
```

---

### `starforge wallet list`

List all saved wallets.

**Usage:**
```bash
starforge wallet list
```

**Output:**
```
◆ Saved Wallets
─────────────────────────────────────────────────────────────

  1. alice [funded]
     Key : GABC...XYZ
     Net : testnet

  2. bob [unfunded]
     Key : GDEF...ABC
     Net : testnet

─────────────────────────────────────────────────────────────
2 wallet(s) on testnet — ~/.starforge/config.toml
```

---

### `starforge wallet show`

Show details of a saved wallet including live balance.

**Usage:**
```bash
starforge wallet show <NAME> [OPTIONS]
```

**Arguments:**
- `<NAME>` - Wallet name

**Options:**
- `--reveal` - Show the secret key in plaintext

**Examples:**
```bash
# Show wallet details
starforge wallet show alice

# Reveal secret key
starforge wallet show alice --reveal
```

**Output:**
```
◆ Wallet: alice
─────────────────────────────────────────────────────────────

Public Key     : GABC...XYZ
Secret Key     : ******************** (--reveal to show)
Network        : testnet
Funded         : yes
Created        : 2025-01-01T00:00:00Z

─────────────────────────────────────────────────────────────

Fetching live balance on testnet…

XLM            : 10000.0000000 XLM
```

---

### `starforge wallet fund`

Fund a wallet via Friendbot (testnet only).

**Usage:**
```bash
starforge wallet fund <NAME>
```

**Arguments:**
- `<NAME>` - Wallet name to fund

**Example:**
```bash
starforge wallet fund alice
```

---

### `starforge wallet remove`

Remove a wallet from local storage.

**Usage:**
```bash
starforge wallet remove <NAME>
```

**Arguments:**
- `<NAME>` - Wallet name to remove

**Example:**
```bash
starforge wallet remove alice
```

---

### `starforge wallet rename`

Rename a wallet.

**Usage:**
```bash
starforge wallet rename <OLD_NAME> <NEW_NAME>
```

**Arguments:**
- `<OLD_NAME>` - Current wallet name
- `<NEW_NAME>` - New wallet name

**Example:**
```bash
starforge wallet rename alice alice-testnet
```

---

### `starforge wallet export`

Export a wallet to a JSON backup file.

**Usage:**
```bash
starforge wallet export <NAME> --output <FILE>
```

**Arguments:**
- `<NAME>` - Wallet name to export

**Options:**
- `--output <FILE>` - Output file path for the backup JSON

**Example:**
```bash
starforge wallet export alice --output ./wallet-backup.json
```

**Notes:**
- Secrets are written only to the backup file and are never printed to stdout.

---

### `starforge wallet import`

Import wallets from a JSON backup file.

**Usage:**
```bash
starforge wallet import --file <FILE>
```

**Options:**
- `--file <FILE>` - Path to a wallet backup JSON file

**Example:**
```bash
starforge wallet import --file ./wallet-backup.json
```

---

### `starforge wallet sign`

Sign an arbitrary message using a wallet.

**Usage:**
```bash
starforge wallet sign <NAME> <MESSAGE> [OPTIONS]
```

**Arguments:**
- `<NAME>` - Wallet name to use for signing
- `<MESSAGE>` - Message to sign (UTF-8 string)

**Options:**
- `--hardware <DEVICE>` - Use hardware wallet (`ledger`, `trezor`)

**Examples:**
```bash
# Sign with local key
starforge wallet sign alice "Hello, Stellar!"

# Sign with hardware wallet
starforge wallet sign alice "Transaction data" --hardware ledger
```

---

### `starforge wallet multisig`

Multi-signature account management.

#### `starforge wallet multisig create`

Create a multi-sig configuration.

**Usage:**
```bash
starforge wallet multisig create <NAME> --threshold <N> --signers <WALLETS>
```

**Arguments:**
- `<NAME>` - Multi-sig account name

**Options:**
- `--threshold <N>` - Required signature weight
- `--signers <WALLETS>` - Comma-separated wallet names
- `--network <NETWORK>` - Network override

**Example:**
```bash
starforge wallet multisig create treasury \
  --threshold 2 \
  --signers alice,bob,charlie
```

#### `starforge wallet multisig sign`

Sign a multi-sig transaction.

**Usage:**
```bash
starforge wallet multisig sign <NAME> --transaction <FILE> [OPTIONS]
```

**Arguments:**
- `<NAME>` - Multi-sig account name

**Options:**
- `--transaction <FILE>` - Path to transaction JSON
- `--output <FILE>` - Output file (defaults to in-place update)

**Example:**
```bash
starforge wallet multisig sign treasury --transaction tx.json
```

#### `starforge wallet multisig list`

List multi-sig accounts.

**Usage:**
```bash
starforge wallet multisig list
```

#### `starforge wallet multisig show`

Show multi-sig account details.

**Usage:**
```bash
starforge wallet multisig show <NAME>
```

#### `starforge wallet multisig submit`

Submit a fully-signed multi-sig transaction.

**Usage:**
```bash
starforge wallet multisig submit <NAME> --transaction <FILE> [OPTIONS]
```

**Options:**
- `--transaction <FILE>` - Path to signed transaction JSON
- `--network <NETWORK>` - Network to submit on

---

## Template Commands

### `starforge template init`

Initialize template registry with example templates.

**Usage:**
```bash
starforge template init
```

**Output:**
```
◆ Initialize Template Registry

Adding example templates to the marketplace...

✓ Added: uniswap-v2
✓ Added: lending-pool
✓ Added: governance
✓ Added: multisig-wallet

✓ Template registry initialized with 4 example templates

Browse templates:
  starforge template list
  starforge template search defi
```

---

### `starforge template search`

Search for templates in the marketplace.

**Usage:**
```bash
starforge template search <QUERY> [OPTIONS]
```

**Arguments:**
- `<QUERY>` - Search query (matches name, description, tags)

**Options:**
- `--tags <TAGS>` - Filter by tags (comma-separated)

**Examples:**
```bash
# Search by keyword
starforge template search defi

# Search with tag filter
starforge template search dex --tags amm,swap

# Search for governance
starforge template search governance
```

**Output:**
```
◆ Template Marketplace — Search
Query          : defi
─────────────────────────────────────────────────────────────

Found 2 template(s):

1. uniswap-v2 ✓
   Uniswap V2 style automated market maker (AMM) DEX implementation
   1.0.0 • Stellar Community • 42 downloads
   Tags: defi, dex, amm, swap

2. lending-pool ✓
   Decentralized lending and borrowing protocol
   1.0.0 • Stellar Community • 28 downloads
   Tags: defi, lending, borrowing

─────────────────────────────────────────────────────────────

Use a template:
  starforge new contract my-project --template uniswap-v2 --from marketplace
```

---

### `starforge template list`

List all available templates.

**Usage:**
```bash
starforge template list
```

---

### `starforge template show`

Show details of a specific template.

**Usage:**
```bash
starforge template show <NAME>
```

**Arguments:**
- `<NAME>` - Template name

**Example:**
```bash
starforge template show uniswap-v2
```

**Output:**
```
◆ Template: uniswap-v2
─────────────────────────────────────────────────────────────

uniswap-v2 ✓

Description    : Uniswap V2 style automated market maker (AMM) DEX
Version        : 1.0.0
Author         : Stellar Community
Downloads      : 42
Tags           : defi, dex, amm, swap

Created        : 2025-01-01T00:00:00Z
Updated        : 2025-01-15T00:00:00Z

Source         : Git Repository
URL            : https://github.com/stellar/soroban-examples
Branch         : main

─────────────────────────────────────────────────────────────

Use this template:
  starforge new contract my-project --template uniswap-v2 --from marketplace
```

---

### `starforge template publish`

Publish a template to the local marketplace.

**Usage:**
```bash
starforge template publish <PATH> [OPTIONS]
```

**Arguments:**
- `<PATH>` - Path to template directory

**Options:**
- `--name <NAME>` - Template name
- `--description <DESC>` - Template description
- `--author <AUTHOR>` - Author name
- `--tags <TAGS>` - Comma-separated tags
- `--version <VERSION>` - Version (default: 1.0.0)

**Examples:**
```bash
# Publish with all options
starforge template publish ./my-template \
  --name my-awesome-template \
  --description "An awesome contract" \
  --author "Your Name" \
  --tags "defi,custom" \
  --version "1.0.0"

# Interactive publish (prompts for missing info)
starforge template publish ./my-template
```

---

### `starforge template remove`

Remove a template from the local marketplace.

**Usage:**
```bash
starforge template remove <NAME>
```

**Arguments:**
- `<NAME>` - Template name to remove

**Example:**
```bash
starforge template remove my-template
```

---

## Contract Commands

### `starforge new contract`

Scaffold a new Soroban smart contract project.

**Usage:**
```bash
starforge new contract <NAME> [OPTIONS]
```

**Arguments:**
- `<NAME>` - Project name

**Options:**
- `--template <TEMPLATE>` - Template to use (`hello-world`, `token`, `nft`, `voting`)
- `--interactive` - Interactively customize the contract
- `--from <SOURCE>` - Use template from source (`marketplace`)
- `--search <QUERY>` - Search for templates
- `--tags <TAGS>` - Filter templates by tags
- `--ci` - Generate `.github/workflows/stellar-ci.yml` (cargo test + WASM size checks)

**Examples:**
```bash
# Basic contract
starforge new contract my-contract

# Use specific template
starforge new contract my-token --template token

# Interactive mode
starforge new contract my-contract --interactive

# From marketplace
starforge new contract my-dex --template uniswap-v2 --from marketplace

# Search templates
starforge new contract --search defi --tags dex

# Include GitHub Actions CI
starforge new contract my-contract --ci
```

---

### `starforge contract inspect`

Inspect a deployed contract instance.

**Usage:**
```bash
starforge contract inspect <CONTRACT_ID> [OPTIONS]
```

**Arguments:**
- `<CONTRACT_ID>` - Contract ID (starts with 'C')

**Options:**
- `--network <NETWORK>` - Network to use
- `--json` - Print machine-readable JSON output

**Example:**
```bash
starforge contract inspect CCPYZFKEAXHHS5VVW5J45TOU7S2EODJ7TZNJIA5LKDVL3PESCES6FNCI
```

**JSON schema (`--json`):**
- `contract_id` (string)
- `executable` (string)
- `wasm_hash` (string|null)
- `storage_durability` (string)
- `latest_ledger` (number)
- `last_modified_ledger_seq` (number|null)
- `live_until_ledger_seq` (number|null)
- `instance_storage` (array of objects): `{ "key": string, "value": string }`

---

### `starforge contract generate-bindings`

Generate typed client wrappers from Soroban contract metadata embedded in a WASM file.

**Usage:**
```bash
starforge contract generate-bindings <WASM_FILE> --lang <LANG>
```

**Arguments:**
- `<WASM_FILE>` - Path to a compiled Soroban WASM file with `contractspecv0` metadata

**Options:**
- `--lang <LANG>` - Output language (`rust`, `ts`)

**Examples:**
```bash
starforge contract generate-bindings ./target/wasm32-unknown-unknown/release/my_contract.wasm --lang rust
starforge contract generate-bindings ./target/wasm32-unknown-unknown/release/my_contract.wasm --lang ts
```

---

### `starforge deploy`

Deploy a compiled Soroban contract.

**Usage:**
```bash
starforge deploy --wasm <FILE> [OPTIONS]
```

**Options:**
- `--wasm <FILE>` - Path to compiled .wasm file (required)
- `--network <NETWORK>` - Network to deploy to (`testnet`, `mainnet`)
- `--wallet <NAME>` - Wallet name to use for deployment
- `--optimize` - Run `soroban-optimize`/Stellar CLI optimization before deployment prep and show size reduction
- `--simulate` - Simulate deploy via Soroban RPC (fee estimate, error check)
- `--yes` - Skip confirmation prompt
- `--execute` - Execute `stellar contract deploy ...` when `stellar` CLI is on PATH (default is dry-run)

**Examples:**
```bash
# Deploy to testnet
starforge deploy --wasm target/wasm32-unknown-unknown/release/my_contract.wasm

# Deploy to mainnet with specific wallet
starforge deploy \
  --wasm ./my_contract.wasm \
  --network mainnet \
  --wallet deployer

# Skip confirmation (for CI)
starforge deploy --wasm ./my_contract.wasm --yes

# Simulate fees before confirming
starforge deploy --wasm ./my_contract.wasm --simulate --wallet deployer

# Execute immediately (requires stellar CLI on PATH)
starforge deploy --wasm ./my_contract.wasm --execute

# Optimize before deployment
starforge deploy --wasm ./my_contract.wasm --optimize
```

---

## Network Commands

### `starforge network show`

Show current network and available networks.

**Usage:**
```bash
starforge network show
```

---

### `starforge network switch`

Switch the active network.

**Usage:**
```bash
starforge network switch <NETWORK>
```

**Arguments:**
- `<NETWORK>` - Target network (`testnet`, `mainnet`, or custom)

**Example:**
```bash
starforge network switch mainnet
```

---

### `starforge network add`

Add a custom network endpoint.

**Usage:**
```bash
starforge network add <NAME> --horizon-url <URL> [OPTIONS]
```

**Arguments:**
- `<NAME>` - Network name

**Options:**
- `--horizon-url <URL>` - Horizon API URL (required)
- `--soroban-rpc-url <URL>` - Soroban RPC URL (optional)

**Example:**
```bash
starforge network add mynet \
  --horizon-url https://my-horizon.example.com \
  --soroban-rpc-url https://my-soroban.example.com
```

---

### `starforge network test`

Test connectivity to a network.

**Usage:**
```bash
starforge network test [NETWORK]
```

**Arguments:**
- `[NETWORK]` - Network to test (defaults to current)

**Example:**
```bash
starforge network test mainnet
```

---

## Transaction Commands

### `starforge tx send`

Send a Stellar payment transaction.

**Usage:**
```bash
starforge tx send --from <WALLET> --to <ADDRESS> --amount <AMOUNT> [OPTIONS]
```

**Options:**
- `--from <WALLET>` - Source wallet name (required)
- `--to <ADDRESS>` - Destination public key (required)
- `--amount <AMOUNT>` - Amount to send (required)
- `--asset <ASSET>` - Asset to send (default: XLM, format: CODE:ISSUER)
- `--network <NETWORK>` - Network to use
- `--yes` - Skip confirmation prompt

**Examples:**
```bash
# Send XLM
starforge tx send --from alice --to GDEF... --amount 100

# Send custom asset
starforge tx send \
  --from alice \
  --to GDEF... \
  --amount 50 \
  --asset USDC:GABC...

# Skip confirmation
starforge tx send --from alice --to GDEF... --amount 10 --yes
```

---

### `starforge tx batch`

Submit multiple Stellar operations in a single transaction from a JSON file.

**Usage:**
```bash
starforge tx batch --file <FILE> --from <WALLET> [OPTIONS]
```

**Options:**
- `--file <FILE>` - Path to operations JSON (required)
- `--from <WALLET>` - Source wallet name (required)
- `--network <NETWORK>` - Network to use (`testnet` or `mainnet`, default: `testnet`)
- `--yes` - Skip confirmation prompt

**Operations file schema:**
```json
{
  "operations": [
    {
      "type": "payment",
      "to": "GDEF...",
      "amount": "100",
      "asset": "XLM"
    }
  ]
}
```

Supported operation types: `payment` (`to`, `amount`, optional `asset` as `XLM` or `CODE:ISSUER`).

**Examples:**
```bash
starforge tx batch --file operations.json --from alice
starforge tx batch --file ops.json --from alice --network testnet --yes
```

---

### `starforge tx history`

Fetch and display recent transactions.

**Usage:**
```bash
starforge tx history <PUBLIC_KEY> [OPTIONS]
```

**Arguments:**
- `<PUBLIC_KEY>` - Account public key

**Options:**
- `-l, --limit <N>` - Number of transactions (max 200, default: 10)
- `-n, --network <NETWORK>` - Network to use
- `--cursor <CURSOR>` - Pagination cursor
- `--after <DATE>` - Show transactions after date (ISO 8601)
- `--before <DATE>` - Show transactions before date (ISO 8601)
- `--successful` - Show only successful transactions
- `--details` - Show full transaction details

**Examples:**
```bash
# Recent transactions
starforge tx history GABC...

# Last 50 transactions
starforge tx history GABC... --limit 50

# Filter by date
starforge tx history GABC... --after 2025-01-01 --before 2025-01-31

# Only successful
starforge tx history GABC... --successful

# With details
starforge tx history GABC... --details
```

---

## Utility Commands

### `starforge info`

Show starforge config and environment info.

**Usage:**
```bash
starforge info
```

---

### `starforge completions`

Generate shell completions.

**Usage:**
```bash
starforge completions <SHELL>
```

**Arguments:**
- `<SHELL>` - Shell type (`bash`, `zsh`, `fish`)

**Examples:**
```bash
# Bash
starforge completions bash > ~/.bash_completion.d/starforge

# Zsh
starforge completions zsh > ~/.zsh/completions/_starforge

# Fish
starforge completions fish > ~/.config/fish/completions/starforge.fish
```

---

### `starforge shell`

Interactive REPL for local contract testing.

**Usage:**
```bash
starforge shell --contract <WASM>
```

**Options:**
- `--contract <WASM>` - Path to compiled contract
- `--no-history` - Disable persistent history for this session
- `--history-max-lines <N>` - Max lines to keep in `~/.starforge/history` (default: 1000)

**Example:**
```bash
starforge shell --contract target/wasm32-unknown-unknown/release/my_contract.wasm
```

---

### `starforge monitor`

Live monitoring of contracts or wallets.

**Usage:**
```bash
starforge monitor [OPTIONS]
```

**Options:**
- `--contract <ID>` - Contract ID to monitor
- `--events <EVENTS>` - Comma-separated event names to filter
- `--wallet <NAME>` - Wallet name to monitor
- `--threshold <AMOUNT>` - XLM threshold for notifications
- `--network <NETWORK>` - Network to use
- `--interval <SECONDS>` - Poll interval (default: 2)

**Examples:**
```bash
# Monitor contract events
starforge monitor --contract CCPYZ... --events transfer,mint

# Monitor wallet balance
starforge monitor --wallet alice --threshold 1000
```

---

### `starforge test`

Run contract tests.

**Usage:**
```bash
starforge test --wasm <FILE> [OPTIONS]
```

**Options:**
- `--wasm <FILE>` - Path to compiled wasm (required)
- `--coverage` - Collect coverage report
- `--report <FORMAT>` - Output report format (`html`, `json`)

**Example:**
```bash
starforge test --wasm target/wasm32-unknown-unknown/release/my_contract.wasm --coverage
```

---

### `starforge gas`

Gas analysis and optimization.

#### `starforge gas analyze`

Analyze gas costs, resource usage, and optimization opportunities.

**Usage:**
```bash
starforge gas analyze --wasm <FILE> [OPTIONS]
```

**Options:**
- `--wasm <FILE>` - Path to wasm file (required)
- `--network <NETWORK>` - Network to use

**Output includes:**
- WASM size and SHA256 fingerprint
- Heuristic score and gas risk level
- Estimated CPU instructions, memory bytes, storage bytes, and fee in stroops
- Host-call and control-flow operation counts
- Optimization suggestions for large binaries, panic strings, debug printing, and expensive host-call patterns

#### `starforge gas optimize`

Optimize wasm for gas efficiency.

**Usage:**
```bash
starforge gas optimize --target <INPUT> --output <OUTPUT>
```

**Options:**
- `--target <INPUT>` - Input wasm file (required)
- `--output <OUTPUT>` - Output wasm file (required)

#### `starforge gas diff`

Compare two wasm builds side-by-side and detect estimated gas regressions.

**Usage:**
```bash
starforge gas diff <OLD_WASM> <NEW_WASM>
```

**Arguments:**
- `<OLD_WASM>` - Baseline wasm file
- `<NEW_WASM>` - Candidate wasm file

**Output includes:**
- Old/new wasm size
- Old/new estimated fee in stroops
- Old/new estimated CPU instructions
- Old/new gas risk level
- Delta, percentage change, and regression classification
- Profiling timings per analysis step

---

### `starforge inspect storage`

List decoded storage entries for a contract scope.

**Usage:**
```bash
starforge inspect storage <CONTRACT_ID> [OPTIONS]
```

**Options:**
- `--scope <SCOPE>` - `instance`, `persistent`, or `temporary`
- `--network <NETWORK>` - Network to use (`testnet`, `mainnet`)
- `--json` - Print machine-readable JSON output

**JSON schema (`--json`):**
- `contract_id` (string)
- `scope` (string)
- `entries` (array of objects): `{ "key": string, "value": string }`

---

### `starforge benchmark`

Performance benchmarking.

**Usage:**
```bash
starforge benchmark [OPTIONS]
```

---

### `starforge plugin`

Manage third-party plugins.

#### `starforge plugin install`

Install a plugin.

**Usage:**
```bash
starforge plugin install <NAME> --path <LIB>
```

#### `starforge plugin list`

List installed plugins.

**Usage:**
```bash
starforge plugin list
```

#### `starforge plugin load`

Load and verify plugins.

**Usage:**
```bash
starforge plugin load
```

---

## Configuration

### Config File Location

`~/.starforge/config.toml`

### Config Structure

```toml
version = "1"
network = "testnet"
telemetry_enabled = true

[[wallets]]
name = "alice"
public_key = "GABC...XYZ"
secret_key = "SABC...XYZ"  # or encrypted: "salt:nonce:ciphertext"
network = "testnet"
created_at = "2025-01-01T00:00:00Z"
funded = true

[networks.testnet]
horizon_url = "https://horizon-testnet.stellar.org"
soroban_rpc_url = "https://soroban-testnet.stellar.org"

[networks.mainnet]
horizon_url = "https://horizon.stellar.org"
soroban_rpc_url = "https://mainnet.sorobanrpc.com"
```

---

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | General error |
| 2 | Invalid arguments |
| 3 | Configuration error |
| 4 | Network error |
| 5 | Validation error |

---

## Template Placeholders

When creating templates, use these placeholders:

| Placeholder | Description | Example |
|-------------|-------------|---------|
| `{{PROJECT_NAME}}` | Original project name | my-project |
| `{{PROJECT_NAME_SNAKE}}` | Snake case | my_project |
| `{{PROJECT_NAME_PASCAL}}` | Pascal case | MyProject |

---

## Error Messages

### Common Errors

**Wallet not found:**
```
✗ Error: Wallet 'alice' not found

Try: starforge wallet list
```

**Network unreachable:**
```
✗ Error: Failed to reach Horizon on testnet

Check your internet connection or try: starforge network test
```

**Invalid public key:**
```
✗ Error: Invalid public key: must start with 'G'

A valid Stellar public key looks like: GABC...XYZ (56 characters)
```

---

## Best Practices

1. **Always encrypt mainnet wallets**: Use `--encrypt` flag
2. **Test on testnet first**: Verify everything works before mainnet
3. **Use descriptive wallet names**: e.g., `deployer-testnet`, `treasury-mainnet`
4. **Keep backups**: Export and securely store secret keys
5. **Review templates**: Always review template code before using
6. **Use verified templates**: Look for the ✓ badge

---

## Support

- **Documentation**: https://github.com/Josetic224/StarForge
- **Issues**: https://github.com/Josetic224/StarForge/issues
- **Discord**: Join the Stellar Discord
