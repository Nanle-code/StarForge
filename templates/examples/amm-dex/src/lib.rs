#![no_std]
//! Automated Market Maker (AMM) / DEX contract for Soroban.
//!
//! Implements a constant-product AMM (x * y = k) that allows:
//! - Liquidity providers to deposit token pairs and receive LP shares
//! - Traders to swap tokens at algorithmically determined prices
//! - Liquidity providers to withdraw their share of the pool
//!
//! Security features:
//! - Minimum liquidity lock to prevent division-by-zero attacks
//! - Slippage protection via minimum output amount checks
//! - Overflow-checked integer arithmetic throughout

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

/// Minimum liquidity permanently locked on first deposit to prevent
/// price manipulation attacks (analogous to Uniswap's MINIMUM_LIQUIDITY).
const MINIMUM_LIQUIDITY: i128 = 1000;

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    /// Address of token A in the pair.
    TokenA,
    /// Address of token B in the pair.
    TokenB,
    /// Total LP shares outstanding.
    TotalShares,
    /// Reserve of token A held by this pool.
    ReserveA,
    /// Reserve of token B held by this pool.
    ReserveB,
    /// LP share balance for a liquidity provider.
    LpBalance(Address),
    /// Whether the pool has been initialised.
    Initialized,
}

#[contract]
pub struct {{PROJECT_NAME_PASCAL}};

#[contractimpl]
impl {{PROJECT_NAME_PASCAL}} {
    // <starforge:add:methods>
    // ── Initialisation ───────────────────────────────────────────────────────

    /// Initialise the pool with the two token contract addresses.
    ///
    /// Can only be called once; reverts if already initialised.
    pub fn initialize(env: Env, token_a: Address, token_b: Address) {
        // <starforge:pause-check>
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("pool already initialized");
        }
        if token_a == token_b {
            panic!("tokens must be distinct");
        }
        env.storage().instance().set(&DataKey::TokenA, &token_a);
        env.storage().instance().set(&DataKey::TokenB, &token_b);
        env.storage().instance().set(&DataKey::ReserveA, &0i128);
        env.storage().instance().set(&DataKey::ReserveB, &0i128);
        env.storage().instance().set(&DataKey::TotalShares, &0i128);
        env.storage().instance().set(&DataKey::Initialized, &true);
    }

    // ── Liquidity management ─────────────────────────────────────────────────

    /// Add liquidity to the pool. The caller deposits `amount_a` of token A
    /// and `amount_b` of token B and receives LP shares proportional to their
    /// contribution.
    ///
    /// On the first deposit the shares are `sqrt(amount_a * amount_b)` minus
    /// `MINIMUM_LIQUIDITY` which is permanently locked.
    ///
    /// # Returns
    /// The number of LP shares minted.
    pub fn add_liquidity(
        env: Env,
        provider: Address,
        amount_a: i128,
        amount_b: i128,
    ) -> i128 {
        // <starforge:pause-check>
        provider.require_auth();

        if amount_a <= 0 || amount_b <= 0 {
            panic!("amounts must be positive");
        }

        let reserve_a = Self::reserve_a(env.clone());
        let reserve_b = Self::reserve_b(env.clone());
        let total_shares = Self::total_shares(env.clone());

        let shares = if total_shares == 0 {
            // First deposit: geometric mean minus minimum liquidity
            let product = (amount_a as u128)
                .checked_mul(amount_b as u128)
                .expect("overflow computing initial liquidity");
            let sqrt = integer_sqrt(product) as i128;
            if sqrt <= MINIMUM_LIQUIDITY {
                panic!("initial liquidity too small");
            }
            sqrt - MINIMUM_LIQUIDITY
        } else {
            // Subsequent deposits: proportional to smaller ratio
            let shares_a = amount_a
                .checked_mul(total_shares)
                .expect("overflow")
                .checked_div(reserve_a)
                .expect("zero reserve");
            let shares_b = amount_b
                .checked_mul(total_shares)
                .expect("overflow")
                .checked_div(reserve_b)
                .expect("zero reserve");
            shares_a.min(shares_b)
        };

        if shares <= 0 {
            panic!("insufficient liquidity minted");
        }

        // Update reserves
        env.storage()
            .instance()
            .set(&DataKey::ReserveA, &(reserve_a + amount_a));
        env.storage()
            .instance()
            .set(&DataKey::ReserveB, &(reserve_b + amount_b));

        // Mint LP shares to provider
        let existing = Self::lp_balance(env.clone(), provider.clone());
        env.storage()
            .persistent()
            .set(&DataKey::LpBalance(provider), &(existing + shares));

        // Update total supply (first deposit also locks MINIMUM_LIQUIDITY)
        let new_total = if total_shares == 0 {
            shares + MINIMUM_LIQUIDITY
        } else {
            total_shares + shares
        };
        env.storage()
            .instance()
            .set(&DataKey::TotalShares, &new_total);

        shares
    }

    /// Remove liquidity from the pool. Burns `shares` LP tokens and returns
    /// the proportional amounts of token A and token B to the provider.
    ///
    /// # Returns
    /// `(amount_a, amount_b)` returned to the provider.
    pub fn remove_liquidity(env: Env, provider: Address, shares: i128) -> (i128, i128) {
        // <starforge:pause-check>
        provider.require_auth();

        if shares <= 0 {
            panic!("shares must be positive");
        }

        let lp_bal = Self::lp_balance(env.clone(), provider.clone());
        if lp_bal < shares {
            panic!("insufficient LP balance");
        }

        let total_shares = Self::total_shares(env.clone());
        let reserve_a = Self::reserve_a(env.clone());
        let reserve_b = Self::reserve_b(env.clone());

        let amount_a = shares
            .checked_mul(reserve_a)
            .expect("overflow")
            .checked_div(total_shares)
            .expect("zero total shares");
        let amount_b = shares
            .checked_mul(reserve_b)
            .expect("overflow")
            .checked_div(total_shares)
            .expect("zero total shares");

        if amount_a <= 0 || amount_b <= 0 {
            panic!("insufficient liquidity burned");
        }

        // Burn LP shares
        env.storage()
            .persistent()
            .set(&DataKey::LpBalance(provider), &(lp_bal - shares));
        env.storage()
            .instance()
            .set(&DataKey::TotalShares, &(total_shares - shares));

        // Update reserves
        env.storage()
            .instance()
            .set(&DataKey::ReserveA, &(reserve_a - amount_a));
        env.storage()
            .instance()
            .set(&DataKey::ReserveB, &(reserve_b - amount_b));

        (amount_a, amount_b)
    }

    // ── Swapping ─────────────────────────────────────────────────────────────

    /// Swap an exact `amount_in` of token A for at least `min_amount_out` of
    /// token B. A 0.3 % fee (represented as 997/1000) is applied.
    ///
    /// # Returns
    /// The actual amount of token B received.
    pub fn swap_a_for_b(
        env: Env,
        trader: Address,
        amount_in: i128,
        min_amount_out: i128,
    ) -> i128 {
        // <starforge:pause-check>
        trader.require_auth();
        let reserve_a = Self::reserve_a(env.clone());
        let reserve_b = Self::reserve_b(env.clone());
        let out = Self::get_amount_out(amount_in, reserve_a, reserve_b);
        if out < min_amount_out {
            panic!("slippage: output below minimum");
        }
        env.storage()
            .instance()
            .set(&DataKey::ReserveA, &(reserve_a + amount_in));
        env.storage()
            .instance()
            .set(&DataKey::ReserveB, &(reserve_b - out));
        out
    }

    /// Swap an exact `amount_in` of token B for at least `min_amount_out` of
    /// token A.
    ///
    /// # Returns
    /// The actual amount of token A received.
    pub fn swap_b_for_a(
        env: Env,
        trader: Address,
        amount_in: i128,
        min_amount_out: i128,
    ) -> i128 {
        // <starforge:pause-check>
        trader.require_auth();
        let reserve_a = Self::reserve_a(env.clone());
        let reserve_b = Self::reserve_b(env.clone());
        let out = Self::get_amount_out(amount_in, reserve_b, reserve_a);
        if out < min_amount_out {
            panic!("slippage: output below minimum");
        }
        env.storage()
            .instance()
            .set(&DataKey::ReserveB, &(reserve_b + amount_in));
        env.storage()
            .instance()
            .set(&DataKey::ReserveA, &(reserve_a - out));
        out
    }

    // ── Read helpers ─────────────────────────────────────────────────────────

    /// Return the current reserve of token A.
    pub fn reserve_a(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::ReserveA)
            .unwrap_or(0)
    }

    /// Return the current reserve of token B.
    pub fn reserve_b(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::ReserveB)
            .unwrap_or(0)
    }

    /// Return the total outstanding LP shares.
    pub fn total_shares(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::TotalShares)
            .unwrap_or(0)
    }

    /// Return the LP share balance of `provider`.
    pub fn lp_balance(env: Env, provider: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::LpBalance(provider))
            .unwrap_or(0)
    }

    /// Calculate the output for swapping `amount_in` given reserves, with a
    /// 0.3 % fee (997 / 1000 factor on the input).
    ///
    /// Formula: `out = (amount_in * 997 * reserve_out) / (reserve_in * 1000 + amount_in * 997)`
    pub fn get_amount_out(amount_in: i128, reserve_in: i128, reserve_out: i128) -> i128 {
        if reserve_in <= 0 || reserve_out <= 0 {
            panic!("insufficient liquidity");
        }
        if amount_in <= 0 {
            panic!("amount must be positive");
        }
        let amount_in_with_fee = amount_in.checked_mul(997).expect("overflow");
        let numerator = amount_in_with_fee
            .checked_mul(reserve_out)
            .expect("overflow");
        let denominator = reserve_in
            .checked_mul(1000)
            .expect("overflow")
            .checked_add(amount_in_with_fee)
            .expect("overflow");
        numerator.checked_div(denominator).expect("division by zero")
    }

    /// Return the spot price of token A in terms of token B (scaled ×10^6).
    pub fn price_a_in_b(env: Env) -> i128 {
        let reserve_a = Self::reserve_a(env.clone());
        let reserve_b = Self::reserve_b(env.clone());
        if reserve_a <= 0 {
            panic!("no liquidity");
        }
        reserve_b.checked_mul(1_000_000).expect("overflow") / reserve_a
    }
}

/// Integer square root using the Babylonian (Newton's) method.
fn integer_sqrt(n: u128) -> u128 {
    if n == 0 {
        return 0;
    }
    let mut x = n;
    let mut y = (x + 1) / 2;
    while y < x {
        x = y;
        y = (x + n / x) / 2;
    }
    x
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn setup(env: &Env) -> (Address, Address, Address) {
        let contract_id = env.register_contract(None, {{PROJECT_NAME_PASCAL}});
        let client = {{PROJECT_NAME_PASCAL}}Client::new(env, &contract_id);
        let token_a = Address::generate(env);
        let token_b = Address::generate(env);
        client.initialize(&token_a, &token_b);
        (contract_id, token_a, token_b)
    }

    #[test]
    fn test_add_remove_liquidity() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _, _) = setup(&env);
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);
        let lp = Address::generate(&env);

        let shares = client.add_liquidity(&lp, &100_000i128, &400_000i128);
        assert!(shares > 0);
        assert_eq!(client.reserve_a(), 100_000);
        assert_eq!(client.reserve_b(), 400_000);

        let (a, b) = client.remove_liquidity(&lp, &shares);
        assert!(a > 0);
        assert!(b > 0);
    }

    #[test]
    fn test_swap_a_for_b() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _, _) = setup(&env);
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);
        let lp = Address::generate(&env);
        let trader = Address::generate(&env);

        client.add_liquidity(&lp, &1_000_000i128, &1_000_000i128);
        let out = client.swap_a_for_b(&trader, &1_000i128, &900i128);
        assert!(out >= 900);
        assert!(out < 1_000); // fee applied
    }

    #[test]
    fn test_get_amount_out_with_fee() {
        // 1000 in against 100_000/100_000 pool → ~996 out (0.3% fee)
        let out = {{PROJECT_NAME_PASCAL}}::get_amount_out(1_000, 100_000, 100_000);
        assert!(out > 990 && out < 1_000);
    }

    #[test]
    #[should_panic(expected = "slippage")]
    fn test_slippage_protection() {
        let env = Env::default();
        env.mock_all_auths();
        let (contract_id, _, _) = setup(&env);
        let client = {{PROJECT_NAME_PASCAL}}Client::new(&env, &contract_id);
        let lp = Address::generate(&env);
        let trader = Address::generate(&env);

        client.add_liquidity(&lp, &1_000_000i128, &1_000_000i128);
        // Demand more than the pool can deliver
        client.swap_a_for_b(&trader, &100i128, &100_000i128);
    }
}
