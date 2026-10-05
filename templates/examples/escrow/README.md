# {{PROJECT_NAME}}

A token escrow smart contract for Soroban.

A buyer locks tokens in the contract; a neutral arbiter then either releases the
funds to the seller (on successful delivery) or refunds the buyer (on a
dispute). This is a common building block for marketplaces, freelance payments
and over-the-counter trades.

## Roles

- **Buyer** — funds the escrow and can release the funds to the seller.
- **Seller** — receives the funds on release and can refund the buyer.
- **Arbiter** — neutral third party who can release or refund to resolve a dispute.

## Features

- Initialize an escrow with buyer, seller, arbiter, token and amount
- Fund the escrow from the buyer
- Release funds to the seller (buyer or arbiter)
- Refund funds to the buyer (seller or arbiter)
- Inspect funded / settled state

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
# Initialize the escrow
stellar contract invoke \
  --id <CONTRACT_ID> \
  --network testnet \
  -- initialize \
  --buyer <BUYER_ADDRESS> \
  --seller <SELLER_ADDRESS> \
  --arbiter <ARBITER_ADDRESS> \
  --token <TOKEN_ADDRESS> \
  --amount 500

# Buyer deposits the funds
stellar contract invoke --id <CONTRACT_ID> --network testnet -- deposit

# Release funds to the seller (called by buyer or arbiter)
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- release --caller <BUYER_OR_ARBITER_ADDRESS>

# Refund funds to the buyer (called by seller or arbiter)
stellar contract invoke \
  --id <CONTRACT_ID> --network testnet \
  -- refund --caller <SELLER_OR_ARBITER_ADDRESS>
```
