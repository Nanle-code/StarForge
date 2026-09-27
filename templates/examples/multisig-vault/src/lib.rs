#![no_std]
//! A threshold multi-signature vault for Soroban.
//!
//! A set of owners controls a token balance held by the contract. Any owner can
//! propose a payment; once at least `threshold` distinct owners have approved
//! it, any owner can execute the transfer. This is the standard pattern behind
//! treasury and shared-custody wallets.
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Vec};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// The owners authorized to propose, approve and execute.
    Owners,
    /// Number of approvals required to execute a transaction.
    Threshold,
    /// The next transaction id to assign.
    NextId,
    /// A stored transaction, keyed by id.
    Tx(u32),
    /// Whether `(tx_id, owner)` has already approved.
    Approved(u32, Address),
}

#[contracttype]
#[derive(Clone)]
pub struct Transaction {
    pub id: u32,
    pub token: Address,
    pub to: Address,
    pub amount: i128,
    pub approvals: u32,
    pub executed: bool,
}

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    // <starforge:add:methods>
    /// Initialize the vault with its owners and approval threshold.
    pub fn initialize(env: Env, owners: Vec<Address>, threshold: u32) {
        // <starforge:pause-check>
        if env.storage().instance().has(&DataKey::Owners) {
            panic!("already initialized");
        }
        if owners.is_empty() {
            panic!("at least one owner is required");
        }
        if threshold == 0 || threshold > owners.len() {
            panic!("threshold must be between 1 and the number of owners");
        }
        env.storage().instance().set(&DataKey::Owners, &owners);
        env.storage().instance().set(&DataKey::Threshold, &threshold);
        env.storage().instance().set(&DataKey::NextId, &0u32);
    }

    /// Propose a token transfer from the vault. Counts as the proposer's approval.
    pub fn propose(env: Env, proposer: Address, token: Address, to: Address, amount: i128) -> u32 {
        // <starforge:pause-check>
        proposer.require_auth();
        Self::require_owner(&env, &proposer);
        if amount <= 0 {
            panic!("amount must be positive");
        }

        let id: u32 = env.storage().instance().get(&DataKey::NextId).unwrap_or(0);
        let tx = Transaction {
            id,
            token,
            to,
            amount,
            approvals: 1,
            executed: false,
        };
        env.storage().persistent().set(&DataKey::Tx(id), &tx);
        env.storage()
            .persistent()
            .set(&DataKey::Approved(id, proposer), &true);
        env.storage().instance().set(&DataKey::NextId, &(id + 1));
        id
    }

    /// Approve a pending transaction. Each owner may approve once.
    pub fn approve(env: Env, owner: Address, tx_id: u32) {
        // <starforge:pause-check>
        owner.require_auth();
        Self::require_owner(&env, &owner);

        let mut tx = Self::transaction(&env, tx_id);
        if tx.executed {
            panic!("transaction already executed");
        }

        let approved_key = DataKey::Approved(tx_id, owner.clone());
        if env.storage().persistent().get(&approved_key).unwrap_or(false) {
            panic!("already approved");
        }

        tx.approvals += 1;
        env.storage().persistent().set(&approved_key, &true);
        env.storage().persistent().set(&DataKey::Tx(tx_id), &tx);
    }

    /// Execute a transaction once it has reached the approval threshold.
    pub fn execute(env: Env, owner: Address, tx_id: u32) {
        // <starforge:pause-check>
        owner.require_auth();
        Self::require_owner(&env, &owner);

        let mut tx = Self::transaction(&env, tx_id);
        if tx.executed {
            panic!("transaction already executed");
        }
        let threshold: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Threshold)
            .expect("not initialized");
        if tx.approvals < threshold {
            panic!("not enough approvals");
        }

        let client = token::Client::new(&env, &tx.token);
        client.transfer(&env.current_contract_address(), &tx.to, &tx.amount);

        tx.executed = true;
        env.storage().persistent().set(&DataKey::Tx(tx_id), &tx);
    }

    /// Return a transaction by id.
    pub fn get_transaction(env: Env, tx_id: u32) -> Transaction {
        Self::transaction(&env, tx_id)
    }

    /// Return the approval threshold.
    pub fn get_threshold(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::Threshold)
            .expect("not initialized")
    }

    fn transaction(env: &Env, tx_id: u32) -> Transaction {
        env.storage()
            .persistent()
            .get(&DataKey::Tx(tx_id))
            .expect("transaction not found")
    }

    fn require_owner(env: &Env, address: &Address) {
        let owners: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::Owners)
            .expect("not initialized");
        if !owners.contains(address) {
            panic!("caller is not an owner");
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::token::{StellarAssetClient, TokenClient};

    #[test]
    fn test_multisig_execute() {
        let env = Env::default();
        env.mock_all_auths();

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let carol = Address::generate(&env);
        let recipient = Address::generate(&env);

        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        let mut owners = Vec::new(&env);
        owners.push_back(alice.clone());
        owners.push_back(bob.clone());
        owners.push_back(carol.clone());
        client.initialize(&owners, &2);

        // Fund the vault with a token.
        let issuer = Address::generate(&env);
        let token_contract = env.register_stellar_asset_contract_v2(issuer.clone());
        let token_address = token_contract.address();
        StellarAssetClient::new(&env, &token_address).mint(&contract_id, &1000);

        // Propose (1 approval) then approve to reach the threshold of 2.
        let id = client.propose(&alice, &token_address, &recipient, &400);
        client.approve(&bob, &id);

        client.execute(&alice, &id);

        let token = TokenClient::new(&env, &token_address);
        assert_eq!(token.balance(&recipient), 400);
        assert_eq!(token.balance(&contract_id), 600);
        assert!(client.get_transaction(&id).executed);
    }

    #[test]
    #[should_panic(expected = "not enough approvals")]
    fn test_execute_below_threshold_rejected() {
        let env = Env::default();
        env.mock_all_auths();

        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let recipient = Address::generate(&env);

        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        let mut owners = Vec::new(&env);
        owners.push_back(alice.clone());
        owners.push_back(bob.clone());
        client.initialize(&owners, &2);

        let issuer = Address::generate(&env);
        let token_contract = env.register_stellar_asset_contract_v2(issuer.clone());
        let token_address = token_contract.address();

        let id = client.propose(&alice, &token_address, &recipient, &100);
        client.execute(&alice, &id);
    }
}
