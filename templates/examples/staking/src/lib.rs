#![no_std]
//! Simple staking / yield contract for Soroban.
//!
//! Users stake a `stake_token` and earn `reward_token` at a configurable
//! `reward_rate` expressed in basis points per 1 000 ledgers.
//! Rewards are calculated as: `stake * ledger_diff * reward_rate / 10_000`.
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    StakeToken,
    RewardToken,
    RewardRate,
    Stake(Address),
    StakedAt(Address),
}

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    // <starforge:add:methods>
    /// Initialize the staking contract. Can only be called once.
    pub fn initialize(
        env: Env,
        admin: Address,
        stake_token: Address,
        reward_token: Address,
        reward_rate: i128,
    ) {
        // <starforge:pause-check>
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::StakeToken, &stake_token);
        env.storage().instance().set(&DataKey::RewardToken, &reward_token);
        env.storage().instance().set(&DataKey::RewardRate, &reward_rate);
    }

    /// Stake `amount` of stake tokens. Adds to any existing stake.
    pub fn stake(env: Env, staker: Address, amount: i128) {
        // <starforge:pause-check>
        staker.require_auth();
        // Settle any accrued rewards before changing the stake.
        Self::settle_rewards(&env, &staker);

        let stake_token: Address = env.storage().instance().get(&DataKey::StakeToken).expect("not initialized");
        token::Client::new(&env, &stake_token).transfer(&staker, &env.current_contract_address(), &amount);

        let current: i128 = env.storage().persistent().get(&DataKey::Stake(staker.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Stake(staker.clone()), &(current + amount));
        env.storage().persistent().set(&DataKey::StakedAt(staker), &env.ledger().sequence());
    }

    /// Unstake `amount` of stake tokens and return them to `staker`.
    pub fn unstake(env: Env, staker: Address, amount: i128) {
        // <starforge:pause-check>
        staker.require_auth();
        let current: i128 = env.storage().persistent().get(&DataKey::Stake(staker.clone())).unwrap_or(0);
        if current < amount {
            panic!("insufficient stake");
        }
        // Settle rewards before reducing stake.
        Self::settle_rewards(&env, &staker);

        let stake_token: Address = env.storage().instance().get(&DataKey::StakeToken).expect("not initialized");
        token::Client::new(&env, &stake_token).transfer(&env.current_contract_address(), &staker, &amount);

        env.storage().persistent().set(&DataKey::Stake(staker.clone()), &(current - amount));
        env.storage().persistent().set(&DataKey::StakedAt(staker), &env.ledger().sequence());
    }

    /// Claim all accrued rewards. Returns the reward amount transferred.
    pub fn claim_rewards(env: Env, staker: Address) -> i128 {
        // <starforge:pause-check>
        staker.require_auth();
        let rewards = Self::get_rewards(env.clone(), staker.clone());
        if rewards > 0 {
            let reward_token: Address = env.storage().instance().get(&DataKey::RewardToken).expect("not initialized");
            token::Client::new(&env, &reward_token).transfer(&env.current_contract_address(), &staker, &rewards);
        }
        // Reset the staked-at ledger so rewards don't double-count.
        env.storage().persistent().set(&DataKey::StakedAt(staker), &env.ledger().sequence());
        rewards
    }

    /// Return the staked amount for `staker`.
    pub fn get_stake(env: Env, staker: Address) -> i128 {
        env.storage().persistent().get(&DataKey::Stake(staker)).unwrap_or(0)
    }

    /// Return the pending reward for `staker` (not yet claimed).
    pub fn get_rewards(env: Env, staker: Address) -> i128 {
        let stake: i128 = env.storage().persistent().get(&DataKey::Stake(staker.clone())).unwrap_or(0);
        if stake == 0 {
            return 0;
        }
        let staked_at: u32 = env.storage().persistent().get(&DataKey::StakedAt(staker)).unwrap_or(env.ledger().sequence());
        let ledger_diff = (env.ledger().sequence() - staked_at) as i128;
        let rate: i128 = env.storage().instance().get(&DataKey::RewardRate).unwrap_or(0);
        stake * ledger_diff * rate / (10_000 * 1_000)
    }

    // Settle pending rewards into the staker's reward balance (internal helper).
    fn settle_rewards(env: &Env, staker: &Address) {
        let rewards = Self::get_rewards(env.clone(), staker.clone());
        if rewards > 0 {
            let reward_token: Address = env.storage().instance().get(&DataKey::RewardToken).expect("not initialized");
            token::Client::new(env, &reward_token).transfer(&env.current_contract_address(), staker, &rewards);
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::StellarAssetClient;

    fn setup(env: &Env) -> (Address, Address, Address, Address, Address) {
        let admin = Address::generate(env);
        let staker = Address::generate(env);
        let stake_tok = env.register_stellar_asset_contract_v2(admin.clone()).address();
        let reward_tok = env.register_stellar_asset_contract_v2(admin.clone()).address();
        StellarAssetClient::new(env, &stake_tok).mint(&staker, &10_000);
        // Pre-fund the contract with reward tokens (simulates a reward pool).
        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        StellarAssetClient::new(env, &reward_tok).mint(&contract_id, &1_000_000);
        (admin, staker, stake_tok, reward_tok, contract_id)
    }

    #[test]
    fn test_stake_and_unstake() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, staker, stake_tok, reward_tok, contract_id) = setup(&env);
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        client.initialize(&admin, &stake_tok, &reward_tok, &100i128);
        client.stake(&staker, &1_000);
        assert_eq!(client.get_stake(&staker), 1_000);

        client.unstake(&staker, &500);
        assert_eq!(client.get_stake(&staker), 500);
    }

    #[test]
    fn test_rewards_accrue() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, staker, stake_tok, reward_tok, contract_id) = setup(&env);
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        client.initialize(&admin, &stake_tok, &reward_tok, &100i128);
        client.stake(&staker, &10_000);

        // Advance the ledger sequence to simulate time passing.
        env.ledger().with_mut(|l| l.sequence_number += 1_000);

        let rewards = client.get_rewards(&staker);
        // 10_000 * 1_000 ledgers * 100 bps / (10_000 * 1_000) = 10
        assert_eq!(rewards, 10);

        client.claim_rewards(&staker);
        // After claiming, pending rewards reset to 0.
        assert_eq!(client.get_rewards(&staker), 0);
    }
}
