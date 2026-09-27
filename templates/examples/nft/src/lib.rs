#![no_std]
//! Non-fungible token (NFT) contract for Soroban.
//!
//! Each token is identified by a `u32` token ID. Supports minting (admin-only),
//! ownership transfer, per-token approval, URI metadata, and burning.
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    TotalSupply,
    Owner(u32),
    Uri(u32),
    Approved(u32),
}

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    // <starforge:add:methods>
    /// Initialize the contract. Can only be called once.
    pub fn initialize(env: Env, admin: Address) {
        // <starforge:pause-check>
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::TotalSupply, &0u32);
    }

    /// Mint a new token to `to` with the given `token_id` and metadata `uri`. Admin only.
    pub fn mint(env: Env, to: Address, token_id: u32, uri: String) {
        // <starforge:pause-check>
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();
        if env.storage().persistent().has(&DataKey::Owner(token_id)) {
            panic!("token already exists");
        }
        env.storage().persistent().set(&DataKey::Owner(token_id), &to);
        env.storage().persistent().set(&DataKey::Uri(token_id), &uri);
        let supply: u32 = env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalSupply, &(supply + 1));
    }

    /// Transfer `token_id` from `from` to `to`. Requires auth from `from`.
    pub fn transfer(env: Env, from: Address, to: Address, token_id: u32) {
        // <starforge:pause-check>
        from.require_auth();
        let owner: Address = env.storage().persistent().get(&DataKey::Owner(token_id)).expect("token not found");
        if owner != from {
            panic!("not token owner");
        }
        env.storage().persistent().set(&DataKey::Owner(token_id), &to);
        env.storage().persistent().remove(&DataKey::Approved(token_id));
    }

    /// Return the owner of `token_id`.
    pub fn owner_of(env: Env, token_id: u32) -> Address {
        env.storage().persistent().get(&DataKey::Owner(token_id)).expect("token not found")
    }

    /// Return the metadata URI of `token_id`.
    pub fn token_uri(env: Env, token_id: u32) -> String {
        env.storage().persistent().get(&DataKey::Uri(token_id)).expect("token not found")
    }

    /// Approve `spender` to transfer `token_id`. Must be called by the token owner.
    pub fn approve(env: Env, owner: Address, spender: Address, token_id: u32) {
        // <starforge:pause-check>
        owner.require_auth();
        let actual_owner: Address = env.storage().persistent().get(&DataKey::Owner(token_id)).expect("token not found");
        if actual_owner != owner {
            panic!("not token owner");
        }
        env.storage().persistent().set(&DataKey::Approved(token_id), &spender);
    }

    /// Return the approved address for `token_id`, if any.
    pub fn get_approved(env: Env, token_id: u32) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Approved(token_id))
    }

    /// Burn `token_id`. Must be called by the token owner.
    pub fn burn(env: Env, owner: Address, token_id: u32) {
        // <starforge:pause-check>
        owner.require_auth();
        let actual_owner: Address = env.storage().persistent().get(&DataKey::Owner(token_id)).expect("token not found");
        if actual_owner != owner {
            panic!("not token owner");
        }
        env.storage().persistent().remove(&DataKey::Owner(token_id));
        env.storage().persistent().remove(&DataKey::Uri(token_id));
        env.storage().persistent().remove(&DataKey::Approved(token_id));
        let supply: u32 = env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalSupply, &supply.saturating_sub(1));
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_mint_transfer_burn() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);

        let id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &id);

        client.initialize(&admin);
        client.mint(&alice, &1u32, &String::from_str(&env, "ipfs://token1"));

        assert_eq!(client.owner_of(&1u32), alice);
        assert_eq!(client.token_uri(&1u32), String::from_str(&env, "ipfs://token1"));

        client.transfer(&alice, &bob, &1u32);
        assert_eq!(client.owner_of(&1u32), bob);

        client.burn(&bob, &1u32);
    }

    #[test]
    fn test_approve() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);

        let id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &id);

        client.initialize(&admin);
        client.mint(&alice, &42u32, &String::from_str(&env, "ipfs://token42"));
        client.approve(&alice, &bob, &42u32);

        assert_eq!(client.get_approved(&42u32), Some(bob));
    }
}
