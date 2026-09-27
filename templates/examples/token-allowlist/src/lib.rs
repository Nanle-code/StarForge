#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Allowed(Address),
}

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    // <starforge:add:methods>
    /// Initialize the contract with an admin address
    pub fn initialize(env: Env, admin: Address) {
        // <starforge:pause-check>
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
    }

    /// Check if an address is in the allowlist
    pub fn is_allowed(env: Env, address: Address) -> bool {
        env.storage().persistent().get(&DataKey::Allowed(address)).unwrap_or(false)
    }

    /// Add an address to the allowlist (admin only)
    pub fn add(env: Env, address: Address) {
        // <starforge:pause-check>
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();
        env.storage().persistent().set(&DataKey::Allowed(address), &true);
    }

    /// Remove an address from the allowlist (admin only)
    pub fn remove(env: Env, address: Address) {
        // <starforge:pause-check>
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();
        env.storage().persistent().set(&DataKey::Allowed(address), &false);
    }

    /// Update the admin address (admin only)
    pub fn set_admin(env: Env, new_admin: Address) {
        // <starforge:pause-check>
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &new_admin);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    #[test]
    fn test_allowlist_flow() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);

        // Initialize contract
        client.initialize(&admin);

        // Should not be allowed initially
        assert!(!client.is_allowed(&user1));
        assert!(!client.is_allowed(&user2));

        // Add user1 to allowlist
        client.add(&user1);
        assert!(client.is_allowed(&user1));
        assert!(!client.is_allowed(&user2));

        // Add user2 to allowlist
        client.add(&user2);
        assert!(client.is_allowed(&user2));

        // Remove user1
        client.remove(&user1);
        assert!(!client.is_allowed(&user1));
        assert!(client.is_allowed(&user2));
    }
}
