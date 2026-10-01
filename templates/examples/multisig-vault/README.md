# {{PROJECT_NAME}}

A threshold multi-signature vault smart contract for Soroban.

A set of owners controls a token balance held by the contract. Any owner can
propose a payment; once at least `threshold` distinct owners have approved it,
any owner can execute the transfer. This is the standard pattern behind treasury
and shared-custody wallets.

## Features

- Initialize with a set of owners and an approval threshold (M-of-N)
- Any owner can propose a token transfer (counts as their approval)
- One approval per owner per transaction
- Execute a transfer only once the threshold is met
- Inspect transactions and the configured threshold

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
# Initialize a 2-of-3 vault
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- initialize --owners '["<ADDR_1>","<ADDR_2>","<ADDR_3>"]' --threshold 2

# Propose a transfer (counts as the proposer's approval)
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- propose --proposer <ADDR_1> --token <TOKEN_ADDRESS> --to <RECIPIENT> --amount 400

# Second owner approves transaction 0
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- approve --owner <ADDR_2> --tx_id 0

# Execute once the threshold is reached
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- execute --owner <ADDR_1> --tx_id 0
```
