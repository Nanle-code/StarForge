# NFT Contract

A non-fungible token (NFT) contract for Soroban. Each token has a unique `u32` ID and a URI pointing to its off-chain metadata.

## Functions

| Function | Description |
|----------|-------------|
| `initialize(admin)` | Set up the contract (once only) |
| `mint(to, token_id, uri)` | Mint a new token — admin only |
| `transfer(from, to, token_id)` | Transfer ownership |
| `owner_of(token_id)` | Query the owner |
| `token_uri(token_id)` | Query the metadata URI |
| `approve(owner, spender, token_id)` | Approve a single-token spender |
| `get_approved(token_id)` | Query the approved spender |
| `burn(owner, token_id)` | Burn a token — owner only |

## Usage

```bash
# Scaffold a new project from this template
starforge new contract my-nft --template nft

# Build
cargo build --target wasm32v1-none --release

# Test
cargo test
```
