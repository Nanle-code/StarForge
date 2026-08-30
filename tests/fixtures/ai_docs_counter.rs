//! A simple counter contract for testing AI documentation generation.

// #![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol};

const COUNTER: Symbol = symbol_short!("COUNTER");

#[contracttype]
pub enum DataKey {
    /// Administrator address key
    Admin,
    Count,
}

/// Emitted when the counter changes.
pub struct CounterEvent {
    pub value: u32,
}

#[contract]
pub struct Counter;

#[contractimpl]
impl Counter {
    /// Increment the counter and return the new value.
    ///
    /// # Examples
    ///
    /// ```
    /// let value = client.increment();
    /// ```
    pub fn increment(env: Env) -> u32 {
        let mut count: u32 = env.storage().instance().get(&COUNTER).unwrap_or(0);
        count += 1;
        env.storage().instance().set(&COUNTER, &count);
        count
    }

    pub fn get_count(env: Env) -> u32 {
        env.storage().instance().get(&COUNTER).unwrap_or(0)
    }

    pub fn reset(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&COUNTER, &0u32);
    }

    pub fn set_admin(env: Env, admin: Address) {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
    }
}
