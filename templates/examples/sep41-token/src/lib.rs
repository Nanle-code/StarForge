#![no_std]
//! SEP-41 fungible token contract for Soroban.
//!
//! Implements the standard fungible-token interface described in SEP-41:
//! initialize, mint (admin-only), transfer, approve/transfer_from allowance
//! flow, burn/burn_from, and balance/allowance read helpers.
//!
//! Invariants upheld by every entry point (exercised by StarForge's property
//! tests in `tests/contract_property_tests.rs`):
//! - amounts are never negative, so no balance or allowance can go negative;
//! - transfers conserve the total supply, only `mint` and `burn*` change it;
//! - arithmetic is checked, so an overflow aborts instead of wrapping;
//! - every state-changing call requires auth from exactly the right party:
//!   the admin for `mint`, the owner for `transfer`/`approve`/`burn`, and the
//!   spender (not the owner) for `transfer_from`/`burn_from`.
use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Decimals,
    Name,
    Symbol,
    Balance(Address),
    Allowance(Address, Address),
}

fn check_nonnegative_amount(amount: i128) {
    if amount < 0 {
        panic!("negative amount is not allowed");
    }
}

fn read_balance(env: &Env, addr: &Address) -> i128 {
    env.storage()
        .persistent()
        .get(&DataKey::Balance(addr.clone()))
        .unwrap_or(0)
}

fn write_balance(env: &Env, addr: &Address, amount: i128) {
    env.storage()
        .persistent()
        .set(&DataKey::Balance(addr.clone()), &amount);
}

fn receive_balance(env: &Env, addr: &Address, amount: i128) {
    let balance = read_balance(env, addr)
        .checked_add(amount)
        .expect("balance overflow");
    write_balance(env, addr, balance);
}

fn spend_balance(env: &Env, addr: &Address, amount: i128) {
    let balance = read_balance(env, addr);
    if balance < amount {
        panic!("insufficient balance");
    }
    write_balance(env, addr, balance - amount);
}

fn spend_allowance(env: &Env, from: &Address, spender: &Address, amount: i128) {
    let key = DataKey::Allowance(from.clone(), spender.clone());
    let allowance: i128 = env.storage().persistent().get(&key).unwrap_or(0);
    if allowance < amount {
        panic!("insufficient allowance");
    }
    env.storage().persistent().set(&key, &(allowance - amount));
}

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    // <starforge:add:methods>
    /// Initialize the token. Can only be called once.
    pub fn initialize(env: Env, admin: Address, decimals: u32, name: String, symbol: String) {
        // <starforge:pause-check>
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Decimals, &decimals);
        env.storage().instance().set(&DataKey::Name, &name);
        env.storage().instance().set(&DataKey::Symbol, &symbol);
    }

    /// Mint `amount` tokens to `to`. Admin only.
    pub fn mint(env: Env, to: Address, amount: i128) {
        // <starforge:pause-check>
        check_nonnegative_amount(amount);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();
        receive_balance(&env, &to, amount);
    }

    /// Transfer `amount` tokens from `from` to `to`.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        // <starforge:pause-check>
        from.require_auth();
        check_nonnegative_amount(amount);
        spend_balance(&env, &from, amount);
        receive_balance(&env, &to, amount);
    }

    /// Return the token balance of `addr`.
    pub fn balance(env: Env, addr: Address) -> i128 {
        read_balance(&env, &addr)
    }

    /// Approve `spender` to spend `amount` on behalf of `from`.
    ///
    /// The new amount replaces any previous allowance.
    pub fn approve(env: Env, from: Address, spender: Address, amount: i128) {
        // <starforge:pause-check>
        from.require_auth();
        check_nonnegative_amount(amount);
        env.storage()
            .persistent()
            .set(&DataKey::Allowance(from, spender), &amount);
    }

    /// Return the amount `spender` is allowed to spend on behalf of `from`.
    pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Allowance(from, spender))
            .unwrap_or(0)
    }

    /// Transfer `amount` from `from` to `to` using `spender`'s allowance.
    ///
    /// Only `spender` authorizes this call; `from` authorized it earlier
    /// through `approve`.
    pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
        // <starforge:pause-check>
        spender.require_auth();
        check_nonnegative_amount(amount);
        spend_allowance(&env, &from, &spender, amount);
        spend_balance(&env, &from, amount);
        receive_balance(&env, &to, amount);
    }

    /// Burn `amount` tokens from `from`.
    pub fn burn(env: Env, from: Address, amount: i128) {
        // <starforge:pause-check>
        from.require_auth();
        check_nonnegative_amount(amount);
        spend_balance(&env, &from, amount);
    }

    /// Burn `amount` tokens from `from` using `spender`'s allowance.
    pub fn burn_from(env: Env, spender: Address, from: Address, amount: i128) {
        // <starforge:pause-check>
        spender.require_auth();
        check_nonnegative_amount(amount);
        spend_allowance(&env, &from, &spender, amount);
        spend_balance(&env, &from, amount);
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

        client.initialize(&admin, &7u32, &String::from_str(&env, "MyToken"), &String::from_str(&env, "MTK"));
        client.mint(&alice, &1000);
        assert_eq!(client.balance(&alice), 1000);

        client.transfer(&alice, &bob, &400);
        assert_eq!(client.balance(&alice), 600);
        assert_eq!(client.balance(&bob), 400);

        client.burn(&alice, &100);
        assert_eq!(client.balance(&alice), 500);
    }

    #[test]
    fn test_approve_transfer_from() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);
        let carol = Address::generate(&env);

        let id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &id);

        client.initialize(&admin, &7u32, &String::from_str(&env, "MyToken"), &String::from_str(&env, "MTK"));
        client.mint(&alice, &500);
        client.approve(&alice, &bob, &200);
        assert_eq!(client.allowance(&alice, &bob), 200);

        client.transfer_from(&bob, &alice, &carol, &150);
        assert_eq!(client.balance(&carol), 150);
        assert_eq!(client.allowance(&alice, &bob), 50);

        client.burn_from(&bob, &alice, &50);
        assert_eq!(client.balance(&alice), 300);
        assert_eq!(client.allowance(&alice, &bob), 0);
    }

    #[test]
    fn test_negative_amounts_rejected() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);

        let id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &id);

        client.initialize(&admin, &7u32, &String::from_str(&env, "MyToken"), &String::from_str(&env, "MTK"));
        client.mint(&alice, &100);
        assert!(client.try_transfer(&alice, &bob, &-1).is_err());
        assert!(client.try_approve(&alice, &bob, &-1).is_err());
        assert_eq!(client.balance(&alice), 100);
        assert_eq!(client.balance(&bob), 0);
    }
}
