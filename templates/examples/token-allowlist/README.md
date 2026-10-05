# {{PROJECT_NAME}}

A token allowlist smart contract for Soroban. It enables managing a list of approved addresses that are permitted to perform actions (like transfer/receive tokens, or participate in a DAO).

## Features

- Initialize contract with an admin
- Check if an address is allowlisted
- Add addresses to the allowlist (admin only)
- Remove addresses from the allowlist (admin only)
- Update admin address (admin only)

## Build

```bash
stellar contract build
```

## Test

```bash
cargo test
```

## Deploy

```bash
starforge deploy \
  --wasm target/wasm32v1-none/release/{{PROJECT_NAME_SNAKE}}.wasm \
  --network testnet
```

## Usage

```bash
# Initialize the contract
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- initialize \
  --admin <ADMIN_ADDRESS>

# Add user to allowlist
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- add \
  --address <USER_ADDRESS>

# Check allowlist status
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- is_allowed \
  --address <USER_ADDRESS>
```
