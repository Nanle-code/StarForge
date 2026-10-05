# SEP-41 Token

A SEP-41 compliant fungible token contract for Soroban.

## Functions

| Function | Description |
|----------|-------------|
| `initialize(admin, decimals, name, symbol)` | Set up the token (once only) |
| `mint(to, amount)` | Mint tokens to an address — admin only |
| `transfer(from, to, amount)` | Move tokens between accounts |
| `balance(addr)` | Query token balance |
| `approve(from, spender, amount)` | Authorise a spender |
| `allowance(from, spender)` | Query remaining allowance |
| `transfer_from(spender, from, to, amount)` | Spend an allowance |
| `burn(from, amount)` | Destroy tokens |
| `burn_from(spender, from, amount)` | Destroy tokens using an allowance |

Negative amounts are rejected, balance arithmetic is checked, and
`transfer_from`/`burn_from` require only the spender's authorization.

## Usage

```bash
# Scaffold a new project from this template
starforge new contract my-token --template sep41-token

# Build
cargo build --target wasm32-unknown-unknown --release

# Test
cargo test
```

## Using a classic asset via its SAC

Many DeFi flows take a SEP-41 token contract id. For a classic asset, resolve or
wrap the Stellar Asset Contract first:

```bash
# Deterministic id (no transaction)
starforge asset contract-id USDC:G... --network testnet

# Deploy the SAC if missing, then pass the C… id into token clients
starforge asset wrap USDC:G... --wallet deployer --network testnet --yes
```

Native XLM uses `starforge asset contract-id XLM` (same as `native`).
