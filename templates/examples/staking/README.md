# Staking Contract

A simple staking / yield contract for Soroban. Users stake a `stake_token` and earn `reward_token` proportional to the amount staked and the time elapsed (measured in ledger sequences).

Reward formula: `stake × ledger_diff × reward_rate / (10_000 × 1_000)`

`reward_rate` is expressed in basis points per 1 000 ledgers (e.g. `100` = 1 % per 1 000 ledgers).

## Functions

| Function | Description |
|----------|-------------|
| `initialize(admin, stake_token, reward_token, reward_rate)` | Set up the contract (once only) |
| `stake(staker, amount)` | Deposit stake tokens |
| `unstake(staker, amount)` | Withdraw stake tokens |
| `claim_rewards(staker)` | Claim accrued reward tokens |
| `get_stake(staker)` | Query staked amount |
| `get_rewards(staker)` | Query pending rewards |

## Usage

```bash
# Scaffold a new project from this template
starforge new contract my-staking --template staking

# Build
cargo build --target wasm32v1-none --release

# Test
cargo test
```
