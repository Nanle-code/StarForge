# RWA Token Template

A real-world asset (RWA) token template for Soroban, implementing the SEP-41 token interface with additional compliance and administration features.

## Compliance Considerations

Real-world assets (RWAs) often require strict regulatory compliance, which standard unrestricted tokens cannot provide. This template includes features to satisfy these requirements:

- **KYC Allowlist**: Only addresses added to the allowlist can hold, send, or receive the token. This ensures all token holders have passed Know Your Customer (KYC) or Anti-Money Laundering (AML) checks.
- **Freezing**: Admins can freeze specific accounts, preventing them from transferring tokens. This is useful for responding to legal orders, suspected fraud, or compromised accounts.
- **Clawback**: Admins can revoke tokens from an account. This is essential for recovering tokens in cases of fraud, errors, or loss of private keys where regulatory recovery is mandated.
- **Forced Transfer**: Admins can force the transfer of tokens between accounts. This enables administrative reassignment of assets when ordered by courts or for estate management.

**Note**: All administrative actions (allowlisting, freezing, clawback, forced transfer) emit events on the ledger for transparency and auditability.

## Features

- Full SEP-41 standard implementation
- Role-based admin access control
- `set_allowed`: Admin can add/remove users from the KYC allowlist
- `set_frozen`: Admin can freeze/unfreeze accounts
- `clawback`: Admin can confiscate tokens
- `forced_transfer`: Admin can forcibly move tokens
- Events emitted on all admin actions

## Build

```bash
stellar contract build
```

## Test

```bash
cargo test
```

## Setup & Usage

```bash
# Initialize
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS> \
  --decimals 7 \
  --name "Real World Asset" \
  --symbol "RWA"

# Allowlist an address
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- set_allowed \
  --addr <USER_ADDRESS> \
  --allowed true

# Mint tokens
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- mint \
  --to <USER_ADDRESS> \
  --amount 1000

# Freeze an account
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- set_frozen \
  --addr <USER_ADDRESS> \
  --frozen true
```
