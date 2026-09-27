#![no_std]
//! A token escrow contract for Soroban.
//!
//! A buyer locks tokens in the contract. A neutral arbiter then either releases
//! the funds to the seller (on successful delivery) or refunds the buyer (on a
//! dispute). This is a common building block for marketplaces, freelance
//! payments and over-the-counter trades.
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The immutable escrow configuration set at initialization.
    Config,
    /// Whether the buyer has funded the escrow.
    Funded,
    /// Whether the escrow has been settled (released or refunded).
    Settled,
}

#[contracttype]
#[derive(Clone)]
pub struct EscrowConfig {
    pub buyer: Address,
    pub seller: Address,
    pub arbiter: Address,
    pub token: Address,
    pub amount: i128,
}

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    // <starforge:add:methods>
    /// Initialize the escrow with its parties, token and amount.
    pub fn initialize(
        env: Env,
        buyer: Address,
        seller: Address,
        arbiter: Address,
        token: Address,
        amount: i128,
    ) {
        // <starforge:pause-check>
        if env.storage().instance().has(&DataKey::Config) {
            panic!("already initialized");
        }
        if amount <= 0 {
            panic!("amount must be positive");
        }
        let config = EscrowConfig {
            buyer,
            seller,
            arbiter,
            token,
            amount,
        };
        env.storage().instance().set(&DataKey::Config, &config);
        env.storage().instance().set(&DataKey::Funded, &false);
        env.storage().instance().set(&DataKey::Settled, &false);
    }

    /// The buyer deposits the agreed amount into the escrow.
    pub fn deposit(env: Env) {
        // <starforge:pause-check>
        let config = Self::config(&env);
        config.buyer.require_auth();

        if env.storage().instance().get(&DataKey::Funded).unwrap_or(false) {
            panic!("already funded");
        }

        let client = token::Client::new(&env, &config.token);
        client.transfer(
            &config.buyer,
            &env.current_contract_address(),
            &config.amount,
        );
        env.storage().instance().set(&DataKey::Funded, &true);
    }

    /// Release the escrowed funds to the seller.
    ///
    /// Authorized by either the buyer (confirming delivery) or the arbiter
    /// (resolving a dispute in the seller's favor).
    pub fn release(env: Env, caller: Address) {
        // <starforge:pause-check>
        let config = Self::config(&env);
        caller.require_auth();
        if caller != config.buyer && caller != config.arbiter {
            panic!("only buyer or arbiter can release");
        }
        Self::settle(&env, &config, &config.seller);
    }

    /// Refund the escrowed funds to the buyer.
    ///
    /// Authorized by either the seller (cancelling the deal) or the arbiter
    /// (resolving a dispute in the buyer's favor).
    pub fn refund(env: Env, caller: Address) {
        // <starforge:pause-check>
        let config = Self::config(&env);
        caller.require_auth();
        if caller != config.seller && caller != config.arbiter {
            panic!("only seller or arbiter can refund");
        }
        Self::settle(&env, &config, &config.buyer);
    }

    /// Return the escrow configuration.
    pub fn get_config(env: Env) -> EscrowConfig {
        Self::config(&env)
    }

    /// Return whether the escrow has been funded.
    pub fn is_funded(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Funded).unwrap_or(false)
    }

    /// Return whether the escrow has been settled.
    pub fn is_settled(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Settled).unwrap_or(false)
    }

    fn config(env: &Env) -> EscrowConfig {
        env.storage()
            .instance()
            .get(&DataKey::Config)
            .expect("not initialized")
    }

    fn settle(env: &Env, config: &EscrowConfig, recipient: &Address) {
        if !env.storage().instance().get(&DataKey::Funded).unwrap_or(false) {
            panic!("escrow not funded");
        }
        if env.storage().instance().get(&DataKey::Settled).unwrap_or(false) {
            panic!("escrow already settled");
        }
        let client = token::Client::new(env, &config.token);
        client.transfer(
            &env.current_contract_address(),
            recipient,
            &config.amount,
        );
        env.storage().instance().set(&DataKey::Settled, &true);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::{StellarAssetClient, TokenClient};

    fn create_token(env: &Env, admin: &Address) -> (Address, TokenClient) {
        let contract = env.register_stellar_asset_contract_v2(admin.clone());
        let address = contract.address();
        (address.clone(), TokenClient::new(env, &address))
    }

    #[test]
    fn test_release_flow() {
        let env = Env::default();
        env.mock_all_auths();

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let arbiter = Address::generate(&env);

        let (token_address, token) = create_token(&env, &buyer);
        StellarAssetClient::new(&env, &token_address).mint(&buyer, &1000);

        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        client.initialize(&buyer, &seller, &arbiter, &token_address, &500);
        client.deposit();
        assert!(client.is_funded());
        assert_eq!(token.balance(&contract_id), 500);

        client.release(&buyer);
        assert!(client.is_settled());
        assert_eq!(token.balance(&seller), 500);
    }

    #[test]
    fn test_refund_flow() {
        let env = Env::default();
        env.mock_all_auths();

        let buyer = Address::generate(&env);
        let seller = Address::generate(&env);
        let arbiter = Address::generate(&env);

        let (token_address, token) = create_token(&env, &buyer);
        StellarAssetClient::new(&env, &token_address).mint(&buyer, &1000);

        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        client.initialize(&buyer, &seller, &arbiter, &token_address, &500);
        client.deposit();

        client.refund(&arbiter);
        assert!(client.is_settled());
        assert_eq!(token.balance(&buyer), 1000);
    }
}
