#![no_std]

//! Token vesting and streaming payment contract.
//!
//! Tokens vest linearly from `start` through `end`, with an optional cliff.
//! Nothing is claimable before the cliff. The schedule can optionally be
//! revoked by the admin, and the beneficiary can be changed by the current
//! beneficiary.

use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env,
};

#[contracttype]
#[derive(Clone)]
pub struct VestingConfig {
    pub admin: Address,
    pub beneficiary: Address,
    pub token: Address,
    pub amount: i128,
    pub start: u64,
    pub cliff: u64,
    pub end: u64,
    pub revocable: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Config,
    Claimed,
    Revoked,
    RevokedVested,
}

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    /// Initialize a vesting schedule.
    ///
    /// The caller must fund the contract separately with `fund`.
    pub fn initialize(
        env: Env,
        admin: Address,
        beneficiary: Address,
        token: Address,
        amount: i128,
        start: u64,
        cliff: u64,
        end: u64,
        revocable: bool,
    ) {
        if env.storage().instance().has(&DataKey::Config) {
            panic!("already initialized");
        }
        if amount <= 0 {
            panic!("amount must be positive");
        }
        if cliff < start {
            panic!("cliff before start");
        }
        if end < cliff {
            panic!("end before cliff");
        }

        admin.require_auth();

        let config = VestingConfig {
            admin,
            beneficiary,
            token,
            amount,
            start,
            cliff,
            end,
            revocable,
        };

        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::Claimed, &0i128);
        env.storage().instance().set(&DataKey::Revoked, &false);
        env.storage().instance().set(&DataKey::RevokedVested, &0i128);
    }

    /// Fund the vesting schedule with its configured token amount.
    pub fn fund(env: Env) {
        let config = Self::config(&env);
        config.admin.require_auth();

        let token_client = token::Client::new(&env, &config.token);
        token_client.transfer(
            &config.admin,
            &env.current_contract_address(),
            &config.amount,
        );
    }

    /// Claim all tokens currently vested for the beneficiary.
    pub fn claim(env: Env) -> i128 {
        let config = Self::config(&env);
        config.beneficiary.require_auth();

        let vested = Self::vested_amount(&env, &config);
        let claimed: i128 = env
            .storage()
            .instance()
            .get(&DataKey::Claimed)
            .unwrap_or(0);

        let claimable = vested - claimed;
        if claimable <= 0 {
            return 0;
        }

        env.storage()
            .instance()
            .set(&DataKey::Claimed, &vested);

        token::Client::new(&env, &config.token).transfer(
            &env.current_contract_address(),
            &config.beneficiary,
            &claimable,
        );

        env.events().publish(
            (symbol_short!("claim"), config.beneficiary.clone()),
            claimable,
        );

        claimable
    }

    /// Change the beneficiary. Only the current beneficiary may do this.
    pub fn change_beneficiary(env: Env, new_beneficiary: Address) {
        let mut config = Self::config(&env);
        config.beneficiary.require_auth();

        if env.storage().instance().get(&DataKey::Revoked).unwrap_or(false) {
            panic!("vesting revoked");
        }

        let old_beneficiary = config.beneficiary.clone();
        config.beneficiary = new_beneficiary.clone();

        env.storage().instance().set(&DataKey::Config, &config);

        env.events().publish(
            (symbol_short!("beneficiary"), old_beneficiary),
            new_beneficiary,
        );
    }

    /// Revoke the schedule and return all unvested tokens to the admin.
    pub fn revoke(env: Env) {
        let config = Self::config(&env);

        if !config.revocable {
            panic!("vesting not revocable");
        }

        config.admin.require_auth();

        if env.storage().instance().get(&DataKey::Revoked).unwrap_or(false) {
            panic!("already revoked");
        }

        let vested = Self::vested_amount(&env, &config);
        let claimed: i128 = env
            .storage()
            .instance()
            .get(&DataKey::Claimed)
            .unwrap_or(0);

        let unvested = config.amount - vested;
        if unvested > 0 {
            token::Client::new(&env, &config.token).transfer(
                &env.current_contract_address(),
                &config.admin,
                &unvested,
            );
        }

        env.storage().instance().set(&DataKey::Revoked, &true);
        env.storage().instance().set(&DataKey::RevokedVested, &vested);

        env.events().publish(
            (symbol_short!("revoke"), config.admin.clone()),
            unvested,
        );

        // Preserve the amount already vested so the beneficiary can claim it.
        env.storage().instance().set(&DataKey::Claimed, &claimed);
    }

    /// Return the configured vesting schedule.
    pub fn get_config(env: Env) -> VestingConfig {
        Self::config(&env)
    }

    /// Return the amount already claimed.
    pub fn get_claimed(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::Claimed)
            .unwrap_or(0)
    }

    /// Return the total amount vested as of the current ledger timestamp.
    pub fn get_vested(env: Env) -> i128 {
        let config = Self::config(&env);
        Self::vested_amount(&env, &config)
    }

    /// Return the amount currently available to claim.
    pub fn get_claimable(env: Env) -> i128 {
        let vested = Self::get_vested(env.clone());
        let claimed = Self::get_claimed(env);
        core::cmp::max(vested - claimed, 0)
    }

    /// Return whether the schedule has been revoked.
    pub fn is_revoked(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Revoked)
            .unwrap_or(false)
    }

    fn config(env: &Env) -> VestingConfig {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .expect("not initialized")
    }

    fn vested_amount(env: &Env, config: &VestingConfig) -> i128 {
        if env
            .storage()
            .instance()
            .get(&DataKey::Revoked)
            .unwrap_or(false)
        {
            // Revocation freezes vesting at the time of revocation. The
            // claimed amount remains separately tracked.
            return env
                .storage()
                .instance()
                .get(&DataKey::RevokedVested)
                .unwrap_or(0);
        }

        let now = env.ledger().timestamp();

        if now < config.cliff {
            return 0;
        }

        if config.start == config.end {
            return config.amount;
        }

        if now >= config.end {
            return config.amount;
        }

        let elapsed = now.saturating_sub(config.start) as i128;
        let duration = (config.end - config.start) as i128;

        config.amount * elapsed / duration
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::{StellarAssetClient, TokenClient};

    fn set_time(env: &Env, timestamp: u64) {
        env.ledger().with_mut(|ledger| {
            ledger.timestamp = timestamp;
        });
    }

    fn setup(
        env: &Env,
        amount: i128,
        start: u64,
        cliff: u64,
        end: u64,
        revocable: bool,
    ) -> (Address, Address, Address, TokenClient, {{PROJECT_NAME_PASCAL}}Client<'_>) {
        let admin = Address::generate(env);
        let beneficiary = Address::generate(env);
        let token_address = env.register_stellar_asset_contract_v2(admin.clone()).address();

        StellarAssetClient::new(env, &token_address).mint(&admin, &amount);

        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(env, &contract_id);

        client.initialize(
            &admin,
            &beneficiary,
            &token_address,
            &amount,
            &start,
            &cliff,
            &end,
            &revocable,
        );
        client.fund();

        let token_client = TokenClient::new(env, &token_address);

        (admin, beneficiary, token_address, token_client, client)
    }

    #[test]
    fn test_linear_vesting_after_cliff() {
        let env = Env::default();
        env.mock_all_auths();

        let (_admin, beneficiary, _token, token, client) =
            setup(&env, 1_000, 100, 200, 1_100, true);

        set_time(&env, 150);
        assert_eq!(client.get_vested(), 0);

        set_time(&env, 200);
        assert_eq!(client.get_vested(), 100);

        set_time(&env, 600);
        assert_eq!(client.get_vested(), 500);

        assert_eq!(client.claim(), 500);
        assert_eq!(token.balance(&beneficiary), 500);
    }

    #[test]
    fn test_zero_duration() {
        let env = Env::default();
        env.mock_all_auths();

        let (_admin, beneficiary, _token, token, client) =
            setup(&env, 1_000, 500, 500, 500, false);

        set_time(&env, 499);
        assert_eq!(client.get_vested(), 0);

        set_time(&env, 500);
        assert_eq!(client.get_vested(), 1_000);
        assert_eq!(client.claim(), 1_000);
        assert_eq!(token.balance(&beneficiary), 1_000);
    }

    #[test]
    fn test_past_start() {
        let env = Env::default();
        env.mock_all_auths();

        let (_admin, _beneficiary, _token, _token_client, client) =
            setup(&env, 1_000, 100, 100, 1_100, false);

        set_time(&env, 600);
        assert_eq!(client.get_vested(), 500);
    }

    #[test]
    fn test_change_beneficiary() {
        let env = Env::default();
        env.mock_all_auths();

        let (_admin, beneficiary, _token, token, client) =
            setup(&env, 1_000, 100, 100, 1_100, false);

        let new_beneficiary = Address::generate(&env);
        client.change_beneficiary(&new_beneficiary);

        set_time(&env, 1_100);
        assert_eq!(client.claim(), 1_000);
        assert_eq!(token.balance(&new_beneficiary), 1_000);
        assert_eq!(token.balance(&beneficiary), 0);
    }

    #[test]
    fn test_revoke_returns_unvested_tokens() {
        let env = Env::default();
        env.mock_all_auths();

        let (admin, beneficiary, _token, token, client) =
            setup(&env, 1_000, 100, 100, 1_100, true);

        set_time(&env, 600);
        assert_eq!(client.get_vested(), 500);

        client.revoke();

        assert!(client.is_revoked());
        assert_eq!(token.balance(&admin), 500);

        assert_eq!(client.claim(), 500);
        assert_eq!(token.balance(&beneficiary), 500);
    }
}
