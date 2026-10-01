//! SAC contract-id vectors (#909) — compared against `stellar contract id asset`.

use starforge::utils::soroban_native::{asset_contract_id, parse_classic_asset};

#[test]
fn native_xlm_matches_stellar_cli_testnet() {
    let asset = parse_classic_asset("XLM").unwrap();
    let id = asset_contract_id(&asset, "testnet").unwrap();
    assert_eq!(
        id, "CDLZFC3SYJYDZT7K67VZ75HPJVIEUVNIXF47ZG2FB2RMQQVU2HHGCYSC",
        "must match `stellar contract id asset --asset native --network testnet`"
    );
}

#[test]
fn native_xlm_matches_stellar_cli_mainnet() {
    let asset = parse_classic_asset("native").unwrap();
    let id = asset_contract_id(&asset, "mainnet").unwrap();
    assert_eq!(
        id, "CAS3J7GYLGXMF6TDJBBYYSE3HQ6BBSMLNUQ34T6TZMYMW2EVH34XOWMA",
        "must match `stellar contract id asset --asset native --network mainnet`"
    );
}

#[test]
fn issued_asset_matches_stellar_cli_testnet() {
    let issuer = "GBRPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFSHONUCEOASW7QC7OX2H";
    let asset = parse_classic_asset(&format!("USDC:{issuer}")).unwrap();
    let id = asset_contract_id(&asset, "testnet").unwrap();
    assert_eq!(
        id,
        "CDGEB6EHEYGZ2OJ37R4NIQJYA2RCBDDLDXOOVC7VXWM6YFPW7AD2DHBM"
    );
}

#[test]
fn long_asset_code_accepted() {
    let issuer = "GBRPYHIL2CI3FNQ4BXLFMNDLFJUNPU2HY3ZMFSHONUCEOASW7QC7OX2H";
    let asset = parse_classic_asset(&format!("BANANASFT:{issuer}")).unwrap();
    let id = asset_contract_id(&asset, "testnet").unwrap();
    assert!(id.starts_with('C'));
    assert_eq!(id.len(), 56);
}
