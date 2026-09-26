# Using StarForge

A task-oriented tour of the most-used commands. For every command and flag see the
[command reference](COMMAND_REFERENCE.md); `starforge <command> --help` is always authoritative.
Moving over from stellar-cli? See [Migrating from stellar-cli](MIGRATING_FROM_STELLAR_CLI.md).

## Repeatable invocation scripts

Store ordered contract calls in YAML or JSON. Scripts use schema version `1`, reject unknown fields, and support `${NAME}` interpolation from the script's `env` map or the CI process environment:

```yaml file=ops.yaml
version: 1
env:
  CONTRACT_ID: CXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX
steps:
  - name: read-state
    contract_id: ${CONTRACT_ID}
    function: get_value
    wallet: ci
    network: testnet
    args:
      - type: string
        value: deployment
    assertions:
      - contains: ready
  - name: write-state
    contract_id: ${CONTRACT_ID}
    function: set_value
    wallet: ci
    network: testnet
    args:
      - type: string
        value: deployment
      - type: string
        value: ${VALUE}
```

Validate the complete sequence without contacting RPC or submitting transactions.
Every `${NAME}` must resolve, so set `VALUE` here too:

```bash run
VALUE=production starforge contract invoke-script ops.yaml --dry-run
```

Run it in CI after configuring the `ci` wallet and environment variables. Steps execute sequentially, and a failed assertion stops the script:

```yaml
# .github/workflows/invoke.yml
- name: Run contract operations
  run: starforge contract invoke-script ops.yaml
  env:
    VALUE: production
```

## Stable JSON output contract

Use the global `--json` flag or set `STARFORGE_OUTPUT_JSON=1` to request machine-readable output from supported commands.

Every success response uses the same envelope shape:

```json
{
  "version": 1,
  "ok": true,
  "data": {
    "name": "wallet",
    "count": 2
  }
}
```

Failures use a versioned error envelope:

```json
{
  "version": 1,
  "ok": false,
  "error": {
    "code": "command_error",
    "message": "unsupported network"
  }
}
```

This is a global contract so automation can parse output consistently across commands without depending on per-command ad hoc schemas.

## Wallet commands

```bash run
# Create a keypair (stored in plaintext: fine for testnet experiments)
starforge wallet create alice

# List saved wallets
starforge wallet list

# Rename or remove a wallet
starforge wallet rename alice bob
starforge wallet remove bob
```

Encrypt keys at rest with `--encrypt`. It prompts for a passphrase
interactively. In CI, supply the passphrase through `STARFORGE_PASSPHRASE`:

```bash run
STARFORGE_PASSPHRASE='correct horse battery staple 42!' \
  starforge wallet create vault --encrypt
```

These talk to the network or reveal secrets:

```bash norun
starforge wallet create deployer --fund   # create + fund via Friendbot (testnet)
starforge wallet fund alice               # fund an existing wallet
starforge wallet show alice               # details + live balance
starforge wallet show alice --reveal      # print the secret key
starforge wallet rotate alice --fund      # new keypair, same local name
```

Rotating keeps the local wallet name but creates a brand-new on-chain account.
Anything that referenced the old public key (scripts, signer sets) has to be
updated separately.

Already using stellar-cli? Import its identities instead of creating new ones:

```bash norun
starforge wallet import --from-stellar-cli alice
```

See [Migrating from stellar-cli](MIGRATING_FROM_STELLAR_CLI.md).

## Network commands

```bash run
# Show the active network and the configured ones
starforge network show

# Add a custom network and switch to it
starforge network add mynet \
  --horizon-url https://my-horizon.example.com \
  --soroban-rpc-url https://my-soroban.example.com
starforge network switch mynet

# Back to testnet, and clean up
starforge network switch testnet
starforge network remove mynet
```

```bash norun
starforge network test            # connectivity check for the active network
starforge network test mainnet
starforge network switch mainnet  # real funds from here on
```

## Configuration commands

```bash run
# Show all settings
starforge config show

# Change a setting (supported keys: telemetry.enabled, privacy.mode)
starforge config set telemetry.enabled false
starforge config set privacy.mode false
```

Use `starforge network switch <name>` to change the default network. For
privacy details, see [Privacy & Telemetry](#privacy--telemetry).

## Configuration schema migrations

When a release changes the config schema, StarForge migrates your stored
configuration automatically on first run.

**What happens during a migration:**

1. A timestamped backup is written **before** anything changes, for example
   `~/.starforge/config.backup.v0.1753000000.toml`.
2. The migration steps run in order (v0 → v1, v1 → v2, …).
3. The updated configuration is saved.

If a migration fails, restore the backup by hand:

```bash norun
cp ~/.starforge/config.backup.v0.<timestamp>.toml ~/.starforge/config.toml
```

**Errors and what they mean:**

| Error | Cause | Fix |
|---|---|---|
| `Config schema version 'X' is newer than this binary supports` | The config was written by a newer `starforge` | Upgrade `starforge` |
| `Unrecognised config schema version 'X'` | The version field was edited by hand or corrupted | Restore from the backup, or delete the file to reset |
| `Failed to create backup of config vX before migration` | The backup write failed (disk full, permissions) | Free disk space or fix directory permissions |

Contributors adding a migration step: see
[CONFIGURATION_MIGRATIONS.md](CONFIGURATION_MIGRATIONS.md).

## Scaffold commands

```bash run
# Scaffold a Soroban contract (hello-world template)
starforge new contract my-contract

# Other built-in templates
starforge new contract my-token --template token
starforge new contract my-nft --template nft
starforge new contract my-vote --template voting

# Search the template marketplace
starforge template search defi
starforge new contract --search lending --tags defi

# Scaffold a Stellar dApp frontend (Vite + React)
starforge new dapp my-dapp
```

```bash norun
# Answer prompts for author, license, storage type and tests
starforge new contract my-contract --interactive

# Use a marketplace template (downloads the template source)
starforge new contract my-dex --template uniswap-v2 --from marketplace
```

Build the result with `stellar contract build` (or `cargo build --target
wasm32v1-none --release`).

## Template marketplace commands

```bash run
starforge template init              # seed the local marketplace with examples
starforge template list
starforge template search defi
starforge template show uniswap-v2
```

```bash norun
# Publish your own template directory
starforge template publish ./my-template \
  --name my-awesome-template \
  --description "An awesome contract" \
  --author "Your Name" \
  --tags "defi,custom"

starforge template remove my-awesome-template
```

## Deploy commands

`starforge deploy` validates and size-checks the `.wasm`, checks the deployer's
balance and prints the deployment plan. `--execute` submits it through
stellar-cli, which signs with **its identity of the same name as the StarForge
wallet**. The easiest way to get a matching pair is
`starforge wallet import --from-stellar-cli <name>`.

```bash norun
WASM=target/wasm32v1-none/release/my_contract.wasm

# Plan only: validate, simulate and estimate fees; submits nothing
starforge deploy --wasm "$WASM" --wallet deployer --dry-run

# Deploy for real (testnet), without the confirmation prompt
starforge deploy --wasm "$WASM" --wallet deployer --yes --execute

# Optimise the WASM first
starforge deploy --wasm "$WASM" --wallet deployer --optimize --execute

# Mainnet
starforge deploy --wasm "$WASM" --wallet deployer --network mainnet --execute
```

## Contract commands

```bash norun
# Build a contract with provenance metadata
starforge contract build

# Build without StarForge/source provenance metadata
starforge contract build --no-provenance

# Inspect a deployed contract instance
starforge contract inspect <CONTRACT_ID>

# Inspect a local WASM's build metadata
starforge contract inspect --wasm ./my_contract.wasm

# Inspect deployed contract metadata as JSON
starforge contract inspect <CONTRACT_ID> --json

# Inspect local WASM metadata as JSON
starforge contract inspect --wasm ./my_contract.wasm --json

# Generate typed clients from a contract's embedded metadata
starforge contract generate-bindings ./my_contract.wasm --lang rust
starforge contract generate-bindings ./my_contract.wasm --lang ts
```

> **Build provenance:** `contract build` embeds the Git repository URL, commit SHA,
> and StarForge version in the WASM metadata. Because a repository URL may identify
> a private project, use `--no-provenance` when that information should not be
> embedded in the contract.

> **Invoking contracts:** `starforge contract invoke` doesn't yet decode real
> return values from simulation, and `--submit` doesn't sign with local wallets
> yet. Use `stellar contract invoke` for now; see
> [what stellar-cli does that StarForge doesn't](MIGRATING_FROM_STELLAR_CLI.md#what-stellar-cli-does-that-starforge-doesnt).
> Scripted, repeatable call plans are available through
> [`contract invoke-script`](#repeatable-invocation-scripts).

## Local AI assistant

StarForge can use a local [Ollama](https://ollama.ai/) instance for Soroban
development help. Requests stay on `http://localhost:11434`: no contract source
or prompts go to a cloud provider.

```bash norun
ollama serve
starforge ai pull codellama:7b

starforge ai status                    # diagnose the installation
starforge ai models
starforge ai ask "How should I store an expiring value?"
starforge ai audit src/lib.rs
starforge ai explain src/lib.rs
starforge ai test src/lib.rs
starforge ai optimise src/lib.rs
```

## Rollback safety testing

```bash norun
# Check that an upgraded contract can be rolled back without losing critical state
starforge test \
  --wasm target/wasm32v1-none/release/my_contract_v2.wasm \
  --rollback \
  --previous-wasm target/wasm32v1-none/release/my_contract_v1.wasm \
  --rollback-scenario tests/rollback/token-balances.json \
  --rollback-performance-budget-ms 1000 \
  --report json
```

The harness checks state preservation, rollback scenarios, data-integrity
invariants and performance budgets. See
[ROLLBACK_TESTING.md](https://github.com/Nanle-code/StarForge/blob/master/ROLLBACK_TESTING.md) for the scenario schema and CI
examples.

## Environment info

```bash run
starforge info
```

## Shell completions

`starforge completions <shell>` supports four shells: `bash`, `zsh`, `fish`, and `powershell`.

```bash run
# Bash: add to ~/.bashrc
source <(starforge completions bash)

# Fish: save to the fish completions directory
mkdir -p ~/.config/fish/completions
starforge completions fish > ~/.config/fish/completions/starforge.fish
```

```zsh norun
# Zsh: add to ~/.zshrc
source <(starforge completions zsh)
```

```powershell norun
# PowerShell: add to your $PROFILE
starforge completions powershell | Out-String | Invoke-Expression

# Or save it once and dot-source it from your profile:
starforge completions powershell > starforge-completions.ps1
# then add to $PROFILE: . /path/to/starforge-completions.ps1
```

After adding the line to your shell config, restart your shell (or `source` the config file / reload `$PROFILE`). Tab-completion for all subcommands and flags will then be active.

**Compatibility**: completion scripts are generated from the CLI's own command definitions via [`clap_complete`](https://docs.rs/clap_complete), so they always match the flags and subcommands of the `starforge` binary you're running -- there's no separately-maintained completion file to fall out of sync. Supported shell/OS combinations: Bash and Zsh on Linux/macOS, Fish on Linux/macOS/Windows, and PowerShell (5.1+ / PowerShell Core) on Windows, Linux, and macOS.

**Security note**: if you use `starforge plugin` to install third-party plugins, their command names and descriptions can appear in the generated completion script, which you typically `source` directly into your shell. `starforge` only interpolates plugin command names that look like plain identifiers (letters, digits, `-`, `_`, `:`); anything else (quotes, whitespace, shell metacharacters) is dropped from the script rather than escaped and embedded, so a malicious or corrupted plugin registry entry can't inject shell commands into your completion setup. Regenerate your completion script after installing or removing plugins to pick up the change.

**Migration note**: PowerShell support was added in this release -- existing Bash/Zsh/Fish completion setups are unaffected. If you previously worked around the lack of PowerShell completions with a custom script, you can remove it and switch to `starforge completions powershell`.

## Where StarForge keeps its data

Everything lives under `~/.starforge/`. Wallets, networks, settings and
deployment history are stored in a local SQLite database, `starforge.db`. A
legacy `config.toml` there is still read and migrated automatically. The
directory is resolved from `HOME` (`USERPROFILE` on Windows), so pointing
`HOME` at a temporary directory gives CI jobs and experiments a clean,
isolated state. [CONFIGURATION.md](CONFIGURATION.md) covers overlays,
validation and the JSON/TOML formats.

## Security

Secret keys can be stored **encrypted at rest** using the `--encrypt` flag during wallet creation:

```bash norun
starforge wallet create mykey --encrypt
# You will be prompted to set a secure passphrase
```

Encryption uses:
- **AES-256-GCM** for authenticated encryption
- **Argon2** for key derivation from your passphrase
- **Random salt and nonce** for each encryption operation

When revealing an encrypted key, you must provide the correct passphrase:

```bash norun
starforge wallet show mykey --reveal
# You will be prompted for the passphrase
```

Unencrypted keys (without `--encrypt`) are stored in plaintext and are suitable only for testnet or throwaway accounts. **Do not use plaintext keys on mainnet with real funds.**

## Privacy & Telemetry

StarForge values your privacy.

### Default: off, auditable and resettable
StarForge does not collect telemetry by default. It only records local anonymous usage data after you explicitly opt in. The exact payload is stored locally at `~/.starforge/data/telemetry.log`, and it is never sent anywhere without a separate explicit opt-in.

### Opting in
Enable telemetry at any time with one of these methods:

- `starforge config set telemetry.enabled true`
- `starforge telemetry enable`
- `export STARFORGE_TELEMETRY=true` (or `1`) in your shell profile

Inspect, erase or turn it back off at any time:

```bash run
starforge telemetry status
starforge telemetry payload      # the exact last recorded payload
starforge telemetry reset        # erase local telemetry and the anonymous ID
starforge telemetry disable
```

Details: [TELEMETRY_PRIVACY.md](https://github.com/Nanle-code/StarForge/blob/master/TELEMETRY_PRIVACY.md).

## Typed contract bindings

`starforge contract generate-bindings` produces typed clients from the metadata embedded in a contract `.wasm`:

### Features
- **Multi-language support**: Rust, TypeScript, Python, Go
- **Type-safe interfaces**: Proper type annotations for all parameters
- **Event type definitions**: Extract and generate event types from contract metadata
- **Complex type support**: Options, Results, Vectors, Maps, custom UDTs
- **Comprehensive testing**: Full test coverage for all languages

### Usage
```bash norun
# Generate Rust bindings
starforge contract generate-bindings ./contract.wasm --lang rust > client.rs

# Generate TypeScript bindings  
starforge contract generate-bindings ./contract.wasm --lang ts > client.ts

# Generate Python bindings
starforge contract generate-bindings ./contract.wasm --lang python > client.py

# Generate Go bindings
starforge contract generate-bindings ./contract.wasm --lang go > client.go
```

### Example generated Rust code
```rust
pub struct ContractClient {
    pub contract_id: String,
    pub network: String,
    pub wallet: Option<String>,
}

impl ContractClient {
    pub fn transfer(&self, from: String, to: String, amount: u128) -> Result<()> {
        // Type-safe method implementation
    }
}

// Generated event types
pub struct TransferEvent {
    pub from: String,
    pub to: String,
    pub amount: String,
}
```

See [examples/binding_generator_example.md](https://github.com/Nanle-code/StarForge/blob/master/examples/binding_generator_example.md) for complete examples.
