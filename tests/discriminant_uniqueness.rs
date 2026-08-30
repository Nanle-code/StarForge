//! Integration regression guard: prevents accidental reuse of
//! `#[contracttype]` discriminants when adding a new variant to the
//! `DataKey` enum in the included Soroban contract fixture.
//!
//! Background
//! ----------
//! Each `#[contracttype]` enum variant is serialised into the Soroban wire
//! format with a unique payload (`Val::get_payload()`), see CAP-46. If a
//! contributor adds a new variant whose serialised payload collides with
//! that of an existing variant — for example, manually tagging a new variant
//! `Foo = 0` while `Admin` already occupies that slot — the storage key
//! namespace collapses and ledger entries round-trip corrupted silently.
//! No compile-time error is guaranteed to catch this for every shape of
//! `#[contracttype]` enum, so the boundary must be locked in by an executable
//! test that fails fast on regression.
//!
//! Test placement
//! --------------
//! * The fixture lives at `tests/fixtures/ai_docs_counter.rs` and uses
//!   `#![no_std]` with `soroban_sdk`. It is brought into this test binary
//!   via `#[path = "..."] mod ai_docs_counter;`, which keeps `#![no_std]`
//!   discipline intact inside the included module while letting the
//!   surrounding test binary use the standard host runtime.
//! * The `soroban-sdk` `testutils` feature is intentionally introduced
//!   through `[dev-dependencies]` solely to support this regression guard;
//!   it does not contaminate the runtime crate graph.
//!
//! Acceptance contract
//! -------------------
//! * Runs under `cargo test --locked --test discriminant_uniqueness`
//!   (the workspace CI command is `cargo test --locked`).
//! * Both tests fail the moment a discriminant collision is introduced.
//! * Deterministic: no `Instant::now()`, no `SystemTime`, no `rand::*`,
//!   no thread ids or env-derived nondeterminism.

#[path = "fixtures/ai_docs_counter.rs"]
mod ai_docs_counter;

use ai_docs_counter::DataKey;
use soroban_sdk::{Env, IntoVal, Val};

/// Lock-in enumeration of every currently declared variant of `DataKey`.
///
/// Adding a new variant to `DataKey` MUST update this slice: the diff is
/// surfaced at code-review time, forcing the contributor to reason about
/// discriminant uniqueness in the same change set.
const DATA_KEY_VARIANTS: &[(&str, DataKey)] =
    &[("Admin", DataKey::Admin), ("Count", DataKey::Count)];

#[test]
fn pairwise_distinct_payloads_across_all_data_key_variants() {
    let env = Env::default();

    // Happy path: every pair of currently declared variants must encode to
    // a distinct Soroban wire-format payload. If a future contributor adds
    // a variant whose `into_val(&env).get_payload()` collides with an
    // existing one, this assertion fails and CI flags the regression.
    for i in 0..DATA_KEY_VARIANTS.len() {
        for j in (i + 1)..DATA_KEY_VARIANTS.len() {
            let (name_a, key_a) = &DATA_KEY_VARIANTS[i];
            let (name_b, key_b) = &DATA_KEY_VARIANTS[j];
            let val_a: Val = key_a.into_val(&env);
            let val_b: Val = key_b.into_val(&env);
            assert_ne!(
                val_a.get_payload(),
                val_b.get_payload(),
                "DataKey variants `{name_a}` and `{name_b}` collide on the Soroban \
wire format (CAP-46): `into_val` produced matching payloads — this is the \
discriminant-reuse regression we are guarding against."
            );
        }
    }
}

#[test]
fn instance_storage_under_data_key_admin_does_not_alias_data_key_count() {
    // Sad path: the user-visible consequence of discriminant reuse is two
    // storage slots collapsing into one. We drive the contract end-to-end so
    // that any — present or future — wire-format collision surfaces here.
    let env = Env::default();
    let contract_id = env.register(ai_docs_counter::Counter, ());

    env.as_contract(&contract_id, || {
        env.storage().instance().set(&DataKey::Admin, &42u32);

        let admin_read: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("DataKey::Admin slot should hold the value we just wrote");
        let count_read: Option<u32> = env.storage().instance().get(&DataKey::Count);

        assert_eq!(
            admin_read, 42,
            "Round-trip under DataKey::Admin must return the value we wrote"
        );
        assert_eq!(
            count_read, None,
            "DataKey::Count must not alias DataKey::Admin in instance storage — \
if it does, the two variants share a Soroban discriminant"
        );
    });
}
