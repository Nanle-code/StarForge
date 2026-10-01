# {{PROJECT_NAME}}

A minimal DAO governance smart contract for Soroban.

Members create proposals and cast one-member-one-vote ballots. A proposal passes
when it has more votes for than against. This demonstrates the core governance
loop (propose → vote → tally) that most on-chain DAOs build upon.

## Features

- Initialize the DAO with a set of founding members
- Members create titled proposals
- One-member-one-vote, enforced per proposal
- Close proposals to end voting
- Tally results and check whether a proposal has passed

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
# Initialize with members
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- initialize --members '["<ADDR_1>","<ADDR_2>"]'

# Create a proposal
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- propose --proposer <ADDR_1> --title "Fund the treasury"

# Vote on proposal 0
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- vote --voter <ADDR_2> --proposal_id 0 --support true

# Check whether it passed
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- has_passed --proposal_id 0
```
