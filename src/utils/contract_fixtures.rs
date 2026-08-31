use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Lifecycle phase for a fixture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FixturePhase {
    Created,
    Setup,
    Active,
    TearingDown,
    Destroyed,
}

/// A named value held by a fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureValue {
    pub key: String,
    pub value: serde_json::Value,
    pub fixture_type: String,
}

/// An account created as part of a fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestAccount {
    pub id: String,
    pub address: String,
    pub secret_key: Option<String>,
    pub balance: u64,
    pub role: AccountRole,
}

/// Role that a test account plays in a fixture.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountRole {
    Admin,
    User,
    Minter,
    Unauthorized,
    Funder,
}

impl std::fmt::Display for AccountRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountRole::Admin => write!(f, "admin"),
            AccountRole::User => write!(f, "user"),
            AccountRole::Minter => write!(f, "minter"),
            AccountRole::Unauthorized => write!(f, "unauthorized"),
            AccountRole::Funder => write!(f, "funder"),
        }
    }
}

/// Pre-seeded storage entries for a fixture.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageSeed {
    pub key: String,
    pub value: serde_json::Value,
    pub durability: StorageDurability,
}

/// Mirrors Soroban storage durability classes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageDurability {
    Persistent,
    Temporary,
    Instance,
}

/// A named event that the fixture expects a contract to emit.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedEvent {
    pub name: String,
    pub topics: Vec<serde_json::Value>,
    pub data: Option<serde_json::Value>,
    pub required: bool,
}

/// Full context captured after fixture setup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FixtureContext {
    pub name: String,
    pub phase: FixturePhase,
    pub accounts: HashMap<String, TestAccount>,
    pub storage: HashMap<String, StorageSeed>,
    pub values: HashMap<String, FixtureValue>,
    pub wasm_path: Option<PathBuf>,
    pub contract_id: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl FixtureContext {
    pub fn account(&self, role: &str) -> Option<&TestAccount> {
        self.accounts.get(role)
    }

    pub fn value(&self, key: &str) -> Option<&serde_json::Value> {
        self.values.get(key).map(|v| &v.value)
    }

    pub fn storage_entry(&self, key: &str) -> Option<&StorageSeed> {
        self.storage.get(key)
    }

    pub fn contract_id(&self) -> Option<&str> {
        self.contract_id.as_deref()
    }
}

/// Teardown hook signature.
type TeardownFn = Box<dyn Fn(&FixtureContext) -> Result<()> + Send + Sync>;

/// Builder for constructing a [`ContractFixture`].
pub struct FixtureBuilder {
    name: String,
    accounts: Vec<TestAccount>,
    storage_seeds: Vec<StorageSeed>,
    values: Vec<FixtureValue>,
    expected_events: Vec<ExpectedEvent>,
    wasm_path: Option<PathBuf>,
    contract_id: Option<String>,
    metadata: HashMap<String, String>,
    teardowns: Vec<TeardownFn>,
}

impl FixtureBuilder {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            accounts: Vec::new(),
            storage_seeds: Vec::new(),
            values: Vec::new(),
            expected_events: Vec::new(),
            wasm_path: None,
            contract_id: None,
            metadata: HashMap::new(),
            teardowns: Vec::new(),
        }
    }

    /// Add a test account to the fixture.
    pub fn with_account(mut self, account: TestAccount) -> Self {
        self.accounts.push(account);
        self
    }

    /// Add a pre-seeded storage entry.
    pub fn with_storage(mut self, seed: StorageSeed) -> Self {
        self.storage_seeds.push(seed);
        self
    }

    /// Store an arbitrary named value accessible during tests.
    pub fn with_value(mut self, key: impl Into<String>, value: serde_json::Value) -> Self {
        self.values.push(FixtureValue {
            key: key.into(),
            value,
            fixture_type: "generic".into(),
        });
        self
    }

    /// Declare an event the contract is expected to emit.
    pub fn expect_event(mut self, event: ExpectedEvent) -> Self {
        self.expected_events.push(event);
        self
    }

    /// Set the WASM file for the contract under test.
    pub fn with_wasm(mut self, path: impl AsRef<Path>) -> Self {
        self.wasm_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Set a pre-deployed contract ID.
    pub fn with_contract_id(mut self, id: impl Into<String>) -> Self {
        self.contract_id = Some(id.into());
        self
    }

    /// Attach arbitrary metadata.
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Register a teardown hook.
    pub fn on_teardown<F>(mut self, f: F) -> Self
    where
        F: Fn(&FixtureContext) -> Result<()> + Send + Sync + 'static,
    {
        self.teardowns.push(Box::new(f));
        self
    }

    pub fn build(self) -> ContractFixture {
        ContractFixture {
            name: self.name,
            accounts: self.accounts,
            storage_seeds: self.storage_seeds,
            values: self.values,
            expected_events: self.expected_events,
            wasm_path: self.wasm_path,
            contract_id: self.contract_id,
            metadata: self.metadata,
            teardowns: Arc::new(Mutex::new(self.teardowns)),
            context: None,
        }
    }
}

/// A complete test fixture for a Soroban contract.
pub struct ContractFixture {
    pub name: String,
    accounts: Vec<TestAccount>,
    storage_seeds: Vec<StorageSeed>,
    values: Vec<FixtureValue>,
    expected_events: Vec<ExpectedEvent>,
    pub wasm_path: Option<PathBuf>,
    pub contract_id: Option<String>,
    pub metadata: HashMap<String, String>,
    teardowns: Arc<Mutex<Vec<TeardownFn>>>,
    context: Option<FixtureContext>,
}

impl ContractFixture {
    /// Set up the fixture and return the active [`FixtureContext`].
    pub fn setup(&mut self) -> Result<&FixtureContext> {
        let mut accounts_map = HashMap::new();
        for account in &self.accounts {
            accounts_map.insert(account.role.to_string(), account.clone());
            accounts_map.insert(account.id.clone(), account.clone());
        }

        let mut storage_map = HashMap::new();
        for seed in &self.storage_seeds {
            storage_map.insert(seed.key.clone(), seed.clone());
        }

        let mut values_map = HashMap::new();
        for value in &self.values {
            values_map.insert(value.key.clone(), value.clone());
        }

        let ctx = FixtureContext {
            name: self.name.clone(),
            phase: FixturePhase::Active,
            accounts: accounts_map,
            storage: storage_map,
            values: values_map,
            wasm_path: self.wasm_path.clone(),
            contract_id: self.contract_id.clone(),
            metadata: self.metadata.clone(),
        };

        self.context = Some(ctx);
        Ok(self.context.as_ref().unwrap())
    }

    /// Tear down the fixture, running all registered hooks.
    pub fn teardown(&mut self) -> Result<()> {
        if let Some(ref ctx) = self.context {
            let hooks = self.teardowns.lock().unwrap();
            for hook in hooks.iter() {
                hook(ctx)?;
            }
        }
        if let Some(ref mut ctx) = self.context {
            ctx.phase = FixturePhase::Destroyed;
        }
        Ok(())
    }

    pub fn context(&self) -> Option<&FixtureContext> {
        self.context.as_ref()
    }

    pub fn expected_events(&self) -> &[ExpectedEvent] {
        &self.expected_events
    }
}

// ── Pre-built fixture factories ────────────────────────────────────────────

/// Builds a fixture for a simple counter contract.
pub fn counter_fixture() -> ContractFixture {
    FixtureBuilder::new("counter")
        .with_account(TestAccount {
            id: "owner".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000001".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000001".into()),
            balance: 10_000_000_000,
            role: AccountRole::Admin,
        })
        .with_account(TestAccount {
            id: "caller".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000002".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000002".into()),
            balance: 1_000_000_000,
            role: AccountRole::User,
        })
        .with_storage(StorageSeed {
            key: "count".into(),
            value: serde_json::json!(0u64),
            durability: StorageDurability::Instance,
        })
        .with_value("initial_count", serde_json::json!(0u64))
        .with_value("max_count", serde_json::json!(u64::MAX))
        .with_metadata("contract_type", "counter")
        .with_metadata("version", "1.0.0")
        .build()
}

/// Builds a fixture for a token (fungible) contract.
pub fn token_fixture() -> ContractFixture {
    FixtureBuilder::new("token")
        .with_account(TestAccount {
            id: "admin".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000010".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000010".into()),
            balance: 100_000_000_000,
            role: AccountRole::Admin,
        })
        .with_account(TestAccount {
            id: "minter".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000011".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000011".into()),
            balance: 10_000_000_000,
            role: AccountRole::Minter,
        })
        .with_account(TestAccount {
            id: "user_a".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000012".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000012".into()),
            balance: 1_000_000_000,
            role: AccountRole::User,
        })
        .with_account(TestAccount {
            id: "unauthorized".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000099".into(),
            secret_key: None,
            balance: 0,
            role: AccountRole::Unauthorized,
        })
        .with_storage(StorageSeed {
            key: "total_supply".into(),
            value: serde_json::json!(0u64),
            durability: StorageDurability::Persistent,
        })
        .with_storage(StorageSeed {
            key: "decimals".into(),
            value: serde_json::json!(7u32),
            durability: StorageDurability::Instance,
        })
        .with_value("mint_amount", serde_json::json!(1_000_000_000u64))
        .with_value("transfer_amount", serde_json::json!(100_000_000u64))
        .with_value("token_name", serde_json::json!("TestToken"))
        .with_value("token_symbol", serde_json::json!("TST"))
        .expect_event(ExpectedEvent {
            name: "mint".into(),
            topics: vec![serde_json::json!("mint")],
            data: None,
            required: false,
        })
        .expect_event(ExpectedEvent {
            name: "transfer".into(),
            topics: vec![serde_json::json!("transfer")],
            data: None,
            required: false,
        })
        .with_metadata("contract_type", "token")
        .with_metadata("standard", "SEP-41")
        .build()
}

/// Builds a fixture for a multisig contract.
pub fn multisig_fixture(required_signatures: u32) -> ContractFixture {
    let mut builder = FixtureBuilder::new("multisig")
        .with_value(
            "required_signatures",
            serde_json::json!(required_signatures),
        )
        .with_storage(StorageSeed {
            key: "signers".into(),
            value: serde_json::json!([]),
            durability: StorageDurability::Persistent,
        })
        .with_storage(StorageSeed {
            key: "threshold".into(),
            value: serde_json::json!(required_signatures),
            durability: StorageDurability::Instance,
        })
        .with_metadata("contract_type", "multisig");

    for i in 0..required_signatures + 1 {
        builder = builder.with_account(TestAccount {
            id: format!("signer_{}", i),
            address: format!(
                "GBXYZTEST000000000000000000000000000000000000000000000000{:02}",
                20 + i
            ),
            secret_key: Some(format!(
                "STEST00000000000000000000000000000000000000000000000000{:02}",
                20 + i
            )),
            balance: 5_000_000_000,
            role: if i == 0 {
                AccountRole::Admin
            } else {
                AccountRole::User
            },
        });
    }

    builder.build()
}

/// Builds a fixture for a DEX/liquidity-pool contract.
pub fn liquidity_pool_fixture() -> ContractFixture {
    FixtureBuilder::new("liquidity_pool")
        .with_account(TestAccount {
            id: "lp_provider".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000030".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000030".into()),
            balance: 50_000_000_000,
            role: AccountRole::Admin,
        })
        .with_account(TestAccount {
            id: "trader".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000031".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000031".into()),
            balance: 10_000_000_000,
            role: AccountRole::User,
        })
        .with_storage(StorageSeed {
            key: "reserve_a".into(),
            value: serde_json::json!(0u64),
            durability: StorageDurability::Persistent,
        })
        .with_storage(StorageSeed {
            key: "reserve_b".into(),
            value: serde_json::json!(0u64),
            durability: StorageDurability::Persistent,
        })
        .with_storage(StorageSeed {
            key: "fee_bps".into(),
            value: serde_json::json!(30u32),
            durability: StorageDurability::Instance,
        })
        .with_value("deposit_a", serde_json::json!(1_000_000_000u64))
        .with_value("deposit_b", serde_json::json!(1_000_000_000u64))
        .with_value("fee_bps", serde_json::json!(30u32))
        .with_metadata("contract_type", "liquidity_pool")
        .build()
}

/// Builds a fixture for an authorization-focused contract.
///
/// This fixture models a contract that requires different authorization
/// levels for different operations — useful for testing `require_auth`
/// patterns, role-based access control, and unauthorized-call rejection.
///
/// Accounts:
/// - `admin` — full admin access (can grant/revoke roles)
/// - `operator` — limited mutation access (can transfer, mint)
/// - `viewer` — read-only access
/// - `unauthorized` — no access (should be rejected)
pub fn auth_fixture() -> ContractFixture {
    FixtureBuilder::new("auth")
        .with_account(TestAccount {
            id: "admin".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000040".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000040".into()),
            balance: 100_000_000_000,
            role: AccountRole::Admin,
        })
        .with_account(TestAccount {
            id: "operator".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000041".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000041".into()),
            balance: 10_000_000_000,
            role: AccountRole::User,
        })
        .with_account(TestAccount {
            id: "viewer".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000042".into(),
            secret_key: Some("STEST0000000000000000000000000000000000000000000000000042".into()),
            balance: 1_000_000_000,
            role: AccountRole::User,
        })
        .with_account(TestAccount {
            id: "unauthorized".into(),
            address: "GBXYZTEST00000000000000000000000000000000000000000000000099".into(),
            secret_key: None,
            balance: 0,
            role: AccountRole::Unauthorized,
        })
        .with_storage(StorageSeed {
            key: "admin".into(),
            value: serde_json::json!("GBXYZTEST00000000000000000000000000000000000000000000000040"),
            durability: StorageDurability::Instance,
        })
        .with_storage(StorageSeed {
            key: "operator_role".into(),
            value: serde_json::json!("GBXYZTEST00000000000000000000000000000000000000000000000041"),
            durability: StorageDurability::Persistent,
        })
        .with_value(
            "authorized_actions",
            serde_json::json!(vec!["read", "transfer", "mint"]),
        )
        .with_value(
            "admin_actions",
            serde_json::json!(vec![
                "read",
                "transfer",
                "mint",
                "grant_role",
                "revoke_role"
            ]),
        )
        .with_metadata("contract_type", "auth")
        .with_metadata("version", "1.0.0")
        .build()
}

/// Builds a deterministic fixture variant with a fixed seed.
///
/// Deterministic fixtures use a counter for account addresses so that
/// snapshot comparisons are stable across runs. The `seed` parameter
/// offsets the address suffixes (e.g. seed=100 produces addresses ending
/// in `0040` for admin, `0041` for operator, etc.).
///
/// This is useful for:
/// - Snapshot testing (compare serialized fixture context across runs)
/// - Property-based testing (generate fixtures from a seed)
/// - Regression testing (reproduce exact fixture state)
pub fn deterministic_fixture(seed: u32) -> ContractFixture {
    let base = seed * 100;
    FixtureBuilder::new(format!("deterministic_{}", seed))
        .with_account(TestAccount {
            id: "admin".into(),
            address: format!(
                "GBXYZTEST0000000000000000000000000000000000000000000000{:04}",
                base
            ),
            secret_key: Some(format!(
                "STEST00000000000000000000000000000000000000000000000000{:04}",
                base
            )),
            balance: 10_000_000_000,
            role: AccountRole::Admin,
        })
        .with_account(TestAccount {
            id: "user".into(),
            address: format!(
                "GBXYZTEST0000000000000000000000000000000000000000000000{:04}",
                base + 1
            ),
            secret_key: Some(format!(
                "STEST00000000000000000000000000000000000000000000000000{:04}",
                base + 1
            )),
            balance: 1_000_000_000,
            role: AccountRole::User,
        })
        .with_storage(StorageSeed {
            key: "count".into(),
            value: serde_json::json!(0u64),
            durability: StorageDurability::Instance,
        })
        .with_value("seed", serde_json::json!(seed))
        .with_metadata("contract_type", "deterministic")
        .with_metadata("seed", seed.to_string())
        .build()
}

/// A registry that manages multiple fixtures by name.
#[derive(Default)]
pub struct FixtureRegistry {
    fixtures: HashMap<String, ContractFixture>,
}

impl FixtureRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, fixture: ContractFixture) {
        self.fixtures.insert(fixture.name.clone(), fixture);
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut ContractFixture> {
        self.fixtures.get_mut(name)
    }

    pub fn setup_all(&mut self) -> Result<()> {
        for (name, fixture) in self.fixtures.iter_mut() {
            fixture
                .setup()
                .with_context(|| format!("Failed to set up fixture '{}'", name))?;
        }
        Ok(())
    }

    pub fn teardown_all(&mut self) -> Result<()> {
        let mut errors: Vec<String> = Vec::new();
        for (name, fixture) in self.fixtures.iter_mut() {
            if let Err(e) = fixture.teardown() {
                errors.push(format!("'{}': {}", name, e));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            anyhow::bail!("Teardown errors: {}", errors.join("; "))
        }
    }
}

/// Serialize a [`FixtureContext`] to a JSON file for later inspection.
pub fn save_fixture_snapshot(ctx: &FixtureContext, path: &Path) -> Result<()> {
    let json = serde_json::to_string_pretty(ctx)?;
    std::fs::write(path, json)
        .with_context(|| format!("Failed to write fixture snapshot to {}", path.display()))
}

/// Load a previously saved fixture snapshot.
pub fn load_fixture_snapshot(path: &Path) -> Result<FixtureContext> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("Failed to read fixture snapshot from {}", path.display()))?;
    serde_json::from_str(&content).context("Failed to deserialize fixture snapshot")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counter_fixture_setup_teardown() {
        let mut f = counter_fixture();
        let ctx = f.setup().unwrap();
        assert_eq!(ctx.name, "counter");
        assert_eq!(ctx.phase, FixturePhase::Active);
        assert!(ctx.account("admin").is_some());
        assert!(ctx.storage_entry("count").is_some());
        assert_eq!(ctx.value("initial_count"), Some(&serde_json::json!(0u64)));
        f.teardown().unwrap();
    }

    #[test]
    fn token_fixture_has_required_accounts() {
        let mut f = token_fixture();
        let ctx = f.setup().unwrap();
        assert!(ctx.account("admin").is_some());
        assert!(ctx.account("minter").is_some());
        assert!(ctx.account("unauthorized").is_some());
    }

    #[test]
    fn multisig_fixture_creates_signers() {
        let mut f = multisig_fixture(3);
        let ctx = f.setup().unwrap();
        assert!(ctx.account("signer_0").is_some());
        assert!(ctx.account("signer_1").is_some());
        assert!(ctx.account("signer_2").is_some());
        assert!(ctx.value("required_signatures").is_some());
    }

    #[test]
    fn registry_setup_and_teardown_all() {
        let mut registry = FixtureRegistry::new();
        registry.register(counter_fixture());
        registry.register(token_fixture());
        registry.setup_all().unwrap();
        registry.teardown_all().unwrap();
    }

    #[test]
    fn snapshot_round_trip() {
        let mut f = counter_fixture();
        let ctx = f.setup().unwrap().clone();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("snapshot.json");
        save_fixture_snapshot(&ctx, &path).unwrap();
        let loaded = load_fixture_snapshot(&path).unwrap();
        assert_eq!(loaded.name, ctx.name);
    }

    #[test]
    fn auth_fixture_has_all_roles() {
        let mut f = auth_fixture();
        let ctx = f.setup().unwrap();
        assert!(ctx.account("admin").is_some());
        assert!(ctx.account("operator").is_some());
        assert!(ctx.account("viewer").is_some());
        assert!(ctx.account("unauthorized").is_some());
        assert_eq!(
            ctx.metadata.get("contract_type").map(|s| s.as_str()),
            Some("auth")
        );
    }

    #[test]
    fn auth_fixture_storage_seeds() {
        let mut f = auth_fixture();
        let ctx = f.setup().unwrap();
        assert!(ctx.storage_entry("admin").is_some());
        assert!(ctx.storage_entry("operator_role").is_some());
        assert!(
            ctx.value("authorized_actions").is_some(),
            "auth fixture must have authorized_actions value"
        );
        assert!(
            ctx.value("admin_actions").is_some(),
            "auth fixture must have admin_actions value"
        );
    }

    #[test]
    fn deterministic_fixture_is_deterministic() {
        let mut f1 = deterministic_fixture(42);
        let ctx1 = f1.setup().unwrap().clone();
        let mut f2 = deterministic_fixture(42);
        let ctx2 = f2.setup().unwrap().clone();
        assert_eq!(ctx1.name, ctx2.name);
        assert_eq!(
            ctx1.account("admin").unwrap().address,
            ctx2.account("admin").unwrap().address
        );
        assert_eq!(
            ctx1.account("user").unwrap().address,
            ctx2.account("user").unwrap().address
        );
    }

    #[test]
    fn deterministic_fixture_different_seeds() {
        let mut f1 = deterministic_fixture(1);
        let ctx1 = f1.setup().unwrap();
        let mut f2 = deterministic_fixture(2);
        let ctx2 = f2.setup().unwrap();
        assert_ne!(
            ctx1.account("admin").unwrap().address,
            ctx2.account("admin").unwrap().address
        );
    }

    #[test]
    fn fixture_factory_all_types_in_registry() {
        let mut registry = FixtureRegistry::new();
        registry.register(counter_fixture());
        registry.register(token_fixture());
        registry.register(auth_fixture());
        registry.register(deterministic_fixture(1));
        registry.setup_all().unwrap();
        registry.teardown_all().unwrap();
    }
}
