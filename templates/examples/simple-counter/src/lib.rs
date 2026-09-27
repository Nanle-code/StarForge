#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Env, Symbol};

const COUNTER: Symbol = symbol_short!("COUNTER");

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    // <starforge:add:methods>
    /// Increment the counter and return the new value
    pub fn increment(env: Env) -> u32 {
        // <starforge:pause-check>
        let mut count: u32 = env.storage().instance().get(&COUNTER).unwrap_or(0);
        count += 1;
        env.storage().instance().set(&COUNTER, &count);
        count
    }

    /// Get the current counter value
    pub fn get_count(env: Env) -> u32 {
        env.storage().instance().get(&COUNTER).unwrap_or(0)
    }

    /// Reset the counter to zero
    pub fn reset(env: Env) {
        // <starforge:pause-check>
        env.storage().instance().set(&COUNTER, &0u32);
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_increment() {
        let env = Env::default();
        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        assert_eq!(client.get_count(), 0);
        assert_eq!(client.increment(), 1);
        assert_eq!(client.increment(), 2);
        assert_eq!(client.get_count(), 2);
    }

    #[test]
    fn test_reset() {
        let env = Env::default();
        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);

        client.increment();
        client.increment();
        assert_eq!(client.get_count(), 2);
        
        client.reset();
        assert_eq!(client.get_count(), 0);
    }
}
