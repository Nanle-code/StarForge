#![no_std]

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
    Allowed(Address),
    Frozen(Address),
}

fn check_nonnegative_amount(amount: i128) {
    if amount < 0 {
        panic!("negative amount is not allowed");
    }
}

fn check_allowed(env: &Env, addr: &Address) {
    if !env.storage().persistent().get(&DataKey::Allowed(addr.clone())).unwrap_or(false) {
        panic!("address is not allowlisted");
    }
}

fn check_not_frozen(env: &Env, addr: &Address) {
    if env.storage().persistent().get(&DataKey::Frozen(addr.clone())).unwrap_or(false) {
        panic!("address is frozen");
    }
}

fn check_authorized(env: &Env, addr: &Address) {
    check_allowed(env, addr);
    check_not_frozen(env, addr);
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
    /// Initialize the token. Can only be called once.
    pub fn initialize(env: Env, admin: Address, decimals: u32, name: String, symbol: String) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::Decimals, &decimals);
        env.storage().instance().set(&DataKey::Name, &name);
        env.storage().instance().set(&DataKey::Symbol, &symbol);
    }

    /// Admin function to allowlist an address for KYC.
    pub fn set_allowed(env: Env, addr: Address, allowed: bool) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();
        env.storage().persistent().set(&DataKey::Allowed(addr.clone()), &allowed);
        env.events().publish(("set_allowed", addr), allowed);
    }

    /// Admin function to freeze an address.
    pub fn set_frozen(env: Env, addr: Address, frozen: bool) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();
        env.storage().persistent().set(&DataKey::Frozen(addr.clone()), &frozen);
        env.events().publish(("set_frozen", addr), frozen);
    }

    /// Admin function to clawback tokens from an address.
    pub fn clawback(env: Env, from: Address, amount: i128) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();
        check_nonnegative_amount(amount);
        spend_balance(&env, &from, amount);
        env.events().publish(("clawback", from), amount);
    }

    /// Admin function to force a transfer between addresses.
    pub fn forced_transfer(env: Env, from: Address, to: Address, amount: i128) {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        admin.require_auth();
        check_nonnegative_amount(amount);
        check_authorized(&env, &to);
        spend_balance(&env, &from, amount);
        receive_balance(&env, &to, amount);
        env.events().publish(("forced_transfer", from, to), amount);
    }

    /// Mint `amount` tokens to `to`. Admin only.
    pub fn mint(env: Env, to: Address, amount: i128) {
        check_nonnegative_amount(amount);
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();
        check_authorized(&env, &to);
        receive_balance(&env, &to, amount);
        env.events().publish(("mint", to), amount);
    }

    /// Transfer `amount` tokens from `from` to `to`.
    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        from.require_auth();
        check_nonnegative_amount(amount);
        check_authorized(&env, &from);
        check_authorized(&env, &to);
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
        from.require_auth();
        check_nonnegative_amount(amount);
        check_authorized(&env, &from);
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
        spender.require_auth();
        check_nonnegative_amount(amount);
        check_authorized(&env, &spender);
        check_authorized(&env, &from);
        check_authorized(&env, &to);
        spend_allowance(&env, &from, &spender, amount);
        spend_balance(&env, &from, amount);
        receive_balance(&env, &to, amount);
    }

    /// Burn `amount` tokens from `from`.
    pub fn burn(env: Env, from: Address, amount: i128) {
        from.require_auth();
        check_nonnegative_amount(amount);
        check_authorized(&env, &from);
        spend_balance(&env, &from, amount);
    }

    /// Burn `amount` tokens from `from` using `spender`'s allowance.
    pub fn burn_from(env: Env, spender: Address, from: Address, amount: i128) {
        spender.require_auth();
        check_nonnegative_amount(amount);
        check_authorized(&env, &spender);
        check_authorized(&env, &from);
        spend_allowance(&env, &from, &spender, amount);
        spend_balance(&env, &from, amount);
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Events};

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
        client.set_allowed(&alice, &true);
        client.set_allowed(&bob, &true);
        
        client.mint(&alice, &1000);
        assert_eq!(client.balance(&alice), 1000);

        client.transfer(&alice, &bob, &400);
        assert_eq!(client.balance(&alice), 600);
        assert_eq!(client.balance(&bob), 400);

        client.burn(&alice, &100);
        assert_eq!(client.balance(&alice), 500);
    }

    #[test]
    #[should_panic(expected = "address is not allowlisted")]
    fn test_not_allowed_mint() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let alice = Address::generate(&env);

        let id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &id);

        client.initialize(&admin, &7u32, &String::from_str(&env, "MyToken"), &String::from_str(&env, "MTK"));
        // Alice is not allowed, this should panic
        client.mint(&alice, &1000);
    }

    #[test]
    #[should_panic(expected = "address is frozen")]
    fn test_frozen_transfer() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);

        let id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &id);

        client.initialize(&admin, &7u32, &String::from_str(&env, "MyToken"), &String::from_str(&env, "MTK"));
        client.set_allowed(&alice, &true);
        client.set_allowed(&bob, &true);
        client.mint(&alice, &1000);
        
        client.set_frozen(&alice, &true);
        
        // Alice is frozen, cannot transfer
        client.transfer(&alice, &bob, &100);
    }
    
    #[test]
    fn test_clawback() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let alice = Address::generate(&env);

        let id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &id);

        client.initialize(&admin, &7u32, &String::from_str(&env, "MyToken"), &String::from_str(&env, "MTK"));
        client.set_allowed(&alice, &true);
        client.mint(&alice, &1000);
        
        client.clawback(&alice, &200);
        assert_eq!(client.balance(&alice), 800);
    }

    #[test]
    fn test_forced_transfer() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let alice = Address::generate(&env);
        let bob = Address::generate(&env);

        let id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &id);

        client.initialize(&admin, &7u32, &String::from_str(&env, "MyToken"), &String::from_str(&env, "MTK"));
        client.set_allowed(&alice, &true);
        client.set_allowed(&bob, &true);
        client.mint(&alice, &1000);
        
        client.forced_transfer(&alice, &bob, &300);
        assert_eq!(client.balance(&alice), 700);
        assert_eq!(client.balance(&bob), 300);
    }

    #[test]
    fn test_events() {
        let env = Env::default();
        env.mock_all_auths();

        let admin = Address::generate(&env);
        let alice = Address::generate(&env);

        let id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &id);

        client.initialize(&admin, &7u32, &String::from_str(&env, "MyToken"), &String::from_str(&env, "MTK"));
        client.set_allowed(&alice, &true);
        
        let events = env.events().all();
        // Since event publishing is mocked/checked here:
        assert_eq!(events.len(), 1);
        
        client.set_frozen(&alice, &true);
        let events = env.events().all();
        assert_eq!(events.len(), 2);
    }
}
