//! Round-trip coverage for the transaction XDR toolbox (#916).
//!
//! XDR arrives at a developer from wallets, Stellar Lab, multisig coordinators
//! or a failed CI run, and `tx decode | tx encode` must be a no-op on the
//! bytes: if decoding a payment and re-encoding it silently changed anything,
//! the signature over the re-encoded blob would no longer match the
//! transaction the signer meant to approve. These fixtures assert byte-exact
//! round-trips for every operation type the envelope format defines, plus
//! real ed25519 signing and hash stability.

use starforge::utils::tx_xdr;
use stellar_strkey::ed25519 as strkey;
use stellar_xdr::curr::{
    AccountId, AllowTrustOp, AlphaNum4, Asset, AssetCode, AssetCode4,
    BeginSponsoringFutureReservesOp, BumpSequenceOp, ChangeTrustAsset, ChangeTrustOp,
    ClaimClaimableBalanceOp, ClaimPredicate, ClaimableBalanceId, Claimant, ClaimantV0,
    ClawbackClaimableBalanceOp, ClawbackOp, CreateAccountOp, CreateClaimableBalanceOp,
    CreatePassiveSellOfferOp, DataValue, ExtendFootprintTtlOp, ExtensionPoint, FeeBumpTransaction,
    FeeBumpTransactionEnvelope, FeeBumpTransactionExt, FeeBumpTransactionInnerTx, Hash,
    HostFunction, InvokeHostFunctionOp, LedgerKey, LedgerKeyAccount, LiquidityPoolDepositOp,
    LiquidityPoolWithdrawOp, ManageBuyOfferOp, ManageDataOp, ManageSellOfferOp, Memo, MuxedAccount,
    Operation, OperationBody, OperationType, PathPaymentStrictReceiveOp, PathPaymentStrictSendOp,
    PaymentOp, PoolId, Price, PublicKey, RestoreFootprintOp, RevokeSponsorshipOp, SequenceNumber,
    SetOptionsOp, SetTrustLineFlagsOp, Signer, SignerKey, String32, String64, TransactionEnvelope,
    TransactionV0, TransactionV0Envelope, TransactionV0Ext, Uint256, VecM,
};

const TESTNET_PASSPHRASE: &str = "Test SDF Network ; September 2015";
const MAINNET_PASSPHRASE: &str = "Public Global Stellar Network ; September 2015";
/// Seed for the wallet the CLI pipe test imports; never a real funded account.
const SIGNER_SEED: [u8; 32] = [7u8; 32];

fn account_id(seed: u8) -> AccountId {
    AccountId(PublicKey::PublicKeyTypeEd25519(Uint256([seed; 32])))
}

fn muxed(seed: u8) -> MuxedAccount {
    MuxedAccount::Ed25519(Uint256([seed; 32]))
}

fn usd() -> Asset {
    Asset::CreditAlphanum4(AlphaNum4 {
        asset_code: AssetCode4(*b"USD\0"),
        issuer: account_id(9),
    })
}

fn price(n: i32, d: i32) -> Price {
    Price { n, d }
}

fn string32(value: &str) -> String32 {
    String32(value.to_string().try_into().unwrap())
}

fn string64(value: &str) -> String64 {
    String64(value.to_string().try_into().unwrap())
}

fn operation(body: OperationBody) -> Operation {
    Operation {
        source_account: Some(muxed(1)),
        body,
    }
}

fn envelope_for(body: OperationBody) -> TransactionEnvelope {
    tx_xdr::unsigned_envelope(&muxed(1), 1_700_000_000, vec![operation(body)]).unwrap()
}

/// One fixture per operation type the current envelope format supports.
fn operation_fixtures() -> Vec<(&'static str, OperationBody)> {
    vec![
        (
            "create_account",
            OperationBody::CreateAccount(CreateAccountOp {
                destination: account_id(2),
                starting_balance: 100_000_000,
            }),
        ),
        (
            "payment",
            OperationBody::Payment(PaymentOp {
                destination: muxed(3),
                asset: usd(),
                amount: 25_000_000,
            }),
        ),
        (
            "path_payment_strict_receive",
            OperationBody::PathPaymentStrictReceive(PathPaymentStrictReceiveOp {
                send_asset: usd(),
                send_max: 10_000_000,
                destination: muxed(3),
                dest_asset: Asset::Native,
                dest_amount: 9_500_000,
                path: VecM::default(),
            }),
        ),
        (
            "manage_sell_offer",
            OperationBody::ManageSellOffer(ManageSellOfferOp {
                selling: usd(),
                buying: Asset::Native,
                amount: 10_000_000,
                price: price(1, 2),
                offer_id: 0,
            }),
        ),
        (
            "create_passive_sell_offer",
            OperationBody::CreatePassiveSellOffer(CreatePassiveSellOfferOp {
                selling: usd(),
                buying: Asset::Native,
                amount: 10_000_000,
                price: price(1, 2),
            }),
        ),
        (
            "set_options",
            OperationBody::SetOptions(SetOptionsOp {
                inflation_dest: Some(account_id(4)),
                clear_flags: Some(0),
                set_flags: Some(1),
                master_weight: Some(1000),
                low_threshold: Some(1),
                med_threshold: Some(2),
                high_threshold: Some(3),
                home_domain: Some(string32("starforge.example")),
                signer: Some(Signer {
                    key: SignerKey::Ed25519(Uint256([5u8; 32])),
                    weight: 500,
                }),
            }),
        ),
        (
            "change_trust",
            OperationBody::ChangeTrust(ChangeTrustOp {
                line: ChangeTrustAsset::CreditAlphanum4(AlphaNum4 {
                    asset_code: AssetCode4(*b"USD\0"),
                    issuer: account_id(9),
                }),
                limit: 1_000_000_000,
            }),
        ),
        (
            "allow_trust",
            OperationBody::AllowTrust(AllowTrustOp {
                trustor: account_id(6),
                asset: AssetCode::CreditAlphanum4(AssetCode4(*b"USD\0")),
                authorize: 1,
            }),
        ),
        ("account_merge", OperationBody::AccountMerge(muxed(7))),
        ("inflation", OperationBody::Inflation),
        (
            "manage_data",
            OperationBody::ManageData(ManageDataOp {
                data_name: string64("starforge-toolbox"),
                data_value: Some(DataValue(vec![1u8, 2, 3, 4].try_into().unwrap())),
            }),
        ),
        (
            "bump_sequence",
            OperationBody::BumpSequence(BumpSequenceOp {
                bump_to: SequenceNumber(1_700_000_001),
            }),
        ),
        (
            "manage_buy_offer",
            OperationBody::ManageBuyOffer(ManageBuyOfferOp {
                selling: usd(),
                buying: Asset::Native,
                buy_amount: 5_000_000,
                price: price(3, 4),
                offer_id: 12,
            }),
        ),
        (
            "path_payment_strict_send",
            OperationBody::PathPaymentStrictSend(PathPaymentStrictSendOp {
                send_asset: usd(),
                send_amount: 10_000_000,
                destination: muxed(3),
                dest_asset: Asset::Native,
                dest_min: 9_000_000,
                path: VecM::default(),
            }),
        ),
        (
            "create_claimable_balance",
            OperationBody::CreateClaimableBalance(CreateClaimableBalanceOp {
                asset: usd(),
                amount: 1_000_000,
                claimants: vec![Claimant::ClaimantTypeV0(ClaimantV0 {
                    destination: account_id(8),
                    predicate: ClaimPredicate::Unconditional,
                })]
                .try_into()
                .unwrap(),
            }),
        ),
        (
            "claim_claimable_balance",
            OperationBody::ClaimClaimableBalance(ClaimClaimableBalanceOp {
                balance_id: ClaimableBalanceId::ClaimableBalanceIdTypeV0(Hash([11u8; 32])),
            }),
        ),
        (
            "begin_sponsoring_future_reserves",
            OperationBody::BeginSponsoringFutureReserves(BeginSponsoringFutureReservesOp {
                sponsored_id: account_id(12),
            }),
        ),
        (
            "end_sponsoring_future_reserves",
            OperationBody::EndSponsoringFutureReserves,
        ),
        (
            "revoke_sponsorship",
            OperationBody::RevokeSponsorship(RevokeSponsorshipOp::LedgerEntry(LedgerKey::Account(
                LedgerKeyAccount {
                    account_id: account_id(13),
                },
            ))),
        ),
        (
            "clawback",
            OperationBody::Clawback(ClawbackOp {
                asset: usd(),
                from: muxed(14),
                amount: 2_000_000,
            }),
        ),
        (
            "clawback_claimable_balance",
            OperationBody::ClawbackClaimableBalance(ClawbackClaimableBalanceOp {
                balance_id: ClaimableBalanceId::ClaimableBalanceIdTypeV0(Hash([15u8; 32])),
            }),
        ),
        (
            "set_trust_line_flags",
            OperationBody::SetTrustLineFlags(SetTrustLineFlagsOp {
                trustor: account_id(16),
                asset: usd(),
                clear_flags: 0,
                set_flags: 1,
            }),
        ),
        (
            "liquidity_pool_deposit",
            OperationBody::LiquidityPoolDeposit(LiquidityPoolDepositOp {
                liquidity_pool_id: PoolId(Hash([17u8; 32])),
                max_amount_a: 10_000_000,
                max_amount_b: 20_000_000,
                min_price: price(1, 3),
                max_price: price(3, 1),
            }),
        ),
        (
            "liquidity_pool_withdraw",
            OperationBody::LiquidityPoolWithdraw(LiquidityPoolWithdrawOp {
                liquidity_pool_id: PoolId(Hash([17u8; 32])),
                amount: 1_000_000,
                min_amount_a: 100,
                min_amount_b: 200,
            }),
        ),
        (
            "invoke_host_function",
            OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: HostFunction::UploadContractWasm(
                    vec![0u8, 97, 115, 109].try_into().unwrap(),
                ),
                auth: VecM::default(),
            }),
        ),
        (
            "extend_footprint_ttl",
            OperationBody::ExtendFootprintTtl(ExtendFootprintTtlOp {
                ext: ExtensionPoint::V0,
                extend_to: 40_960,
            }),
        ),
        (
            "restore_footprint",
            OperationBody::RestoreFootprint(RestoreFootprintOp {
                ext: ExtensionPoint::V0,
            }),
        ),
    ]
}

#[test]
fn every_operation_type_round_trips_byte_for_byte() {
    let fixtures = operation_fixtures();
    let covered: Vec<OperationType> = fixtures
        .iter()
        .map(|(_, body)| body.discriminant())
        .collect();
    assert_eq!(
        covered,
        OperationBody::VARIANTS.to_vec(),
        "the fixture list must cover every operation type exactly once, in XDR order"
    );

    for (name, body) in fixtures {
        let envelope = envelope_for(body);
        let xdr = tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Base64).unwrap();

        let json = tx_xdr::to_json(&envelope, true).unwrap();
        let reencoded = tx_xdr::write_envelope(
            &tx_xdr::encode_json(&json).unwrap(),
            tx_xdr::WireFormat::Base64,
        )
        .unwrap();
        assert_eq!(xdr, reencoded, "JSON round-trip changed {name}");

        // Compact JSON round-trips too, which is what the pipe emits.
        let compact = tx_xdr::to_json(&envelope, false).unwrap();
        let from_compact = tx_xdr::encode_json(&compact).unwrap();
        assert_eq!(
            envelope, from_compact,
            "compact JSON round-trip changed {name}"
        );

        let decoded = tx_xdr::decode(&xdr, Some(TESTNET_PASSPHRASE)).unwrap();
        assert_eq!(decoded.kind, "transaction");
        assert_eq!(
            decoded.summary.operations,
            vec![name.to_string()],
            "decoded summary reported the wrong operation for {name}"
        );
        assert_eq!(
            tx_xdr::to_json(&tx_xdr::parse_envelope(&xdr).unwrap(), true).unwrap(),
            json
        );
    }
}

#[test]
fn hex_and_base64_describe_the_same_envelope() {
    let envelope = envelope_for(OperationBody::Inflation);
    let base64 = tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Base64).unwrap();
    let hex = tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Hex).unwrap();

    assert_eq!(tx_xdr::parse_envelope(&hex).unwrap(), envelope);
    assert_eq!(
        tx_xdr::signature_hash(&tx_xdr::parse_envelope(&hex).unwrap(), TESTNET_PASSPHRASE).unwrap(),
        tx_xdr::signature_hash(&envelope, TESTNET_PASSPHRASE).unwrap()
    );
    assert!(!base64.contains('\n'));
}

#[test]
fn multi_operation_envelope_keeps_operation_order() {
    let envelope = tx_xdr::unsigned_envelope(
        &muxed(1),
        42,
        vec![
            operation(OperationBody::Inflation),
            operation(OperationBody::AccountMerge(muxed(2))),
            operation(OperationBody::BumpSequence(BumpSequenceOp {
                bump_to: SequenceNumber(99),
            })),
        ],
    )
    .unwrap();

    let xdr = tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Base64).unwrap();
    let decoded = tx_xdr::decode(&xdr, Some(TESTNET_PASSPHRASE)).unwrap();
    assert_eq!(
        decoded.summary.operations,
        vec!["inflation", "account_merge", "bump_sequence"]
    );
    assert_eq!(decoded.summary.seq_num, "42");
    assert_eq!(decoded.summary.fee_stroops, 100);
}

#[test]
fn signature_hash_is_stable_and_network_specific() {
    let envelope = envelope_for(OperationBody::Inflation);
    let testnet = tx_xdr::signature_hash(&envelope, TESTNET_PASSPHRASE).unwrap();
    let mainnet = tx_xdr::signature_hash(&envelope, MAINNET_PASSPHRASE).unwrap();

    assert_eq!(
        testnet,
        tx_xdr::signature_hash(&envelope, TESTNET_PASSPHRASE).unwrap()
    );
    assert_ne!(
        testnet, mainnet,
        "the network passphrase must change the hash"
    );

    let decoded = tx_xdr::decode(
        &tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Base64).unwrap(),
        Some(TESTNET_PASSPHRASE),
    )
    .unwrap();
    assert_eq!(decoded.hash, Some(hex::encode(testnet)));
}

#[test]
fn signing_adds_a_verifiable_signature_and_leaves_the_payload_intact() {
    let secret = strkey::PrivateKey(SIGNER_SEED).to_string();
    let signer = tx_xdr::public_key_for_secret(&secret).unwrap();
    assert_eq!(
        signer,
        strkey::PublicKey(ed25519_pubkey(SIGNER_SEED)).to_string()
    );

    let unsigned = envelope_for(OperationBody::Inflation);
    let signed = tx_xdr::sign_envelope(&unsigned, &secret, TESTNET_PASSPHRASE).unwrap();

    let unsigned_json = tx_xdr::decode(
        &tx_xdr::write_envelope(&unsigned, tx_xdr::WireFormat::Base64).unwrap(),
        Some(TESTNET_PASSPHRASE),
    )
    .unwrap();
    let signed_json = tx_xdr::decode(
        &tx_xdr::write_envelope(&signed, tx_xdr::WireFormat::Base64).unwrap(),
        Some(TESTNET_PASSPHRASE),
    )
    .unwrap();
    // Signing only appends a signature decorator; the signed bytes are still
    // the transaction the signer looked at.
    assert_eq!(signed_json.hash, unsigned_json.hash);
    assert!(unsigned_json.summary.signature_hints.is_empty());
    assert_eq!(
        signed_json.summary.signature_hints,
        vec![hex::encode(&ed25519_pubkey(SIGNER_SEED)[28..])]
    );

    assert!(tx_xdr::verify_signature(&signed, TESTNET_PASSPHRASE, &signer).unwrap());
    assert!(
        !tx_xdr::verify_signature(&signed, MAINNET_PASSPHRASE, &signer).unwrap(),
        "a signature must not verify under the other network's hash"
    );
    assert!(
        !tx_xdr::verify_signature(&unsigned, TESTNET_PASSPHRASE, &signer).unwrap(),
        "an unsigned envelope has nothing to verify"
    );
    assert!(
        !tx_xdr::verify_signature(
            &signed,
            TESTNET_PASSPHRASE,
            &strkey::PublicKey([9u8; 32]).to_string()
        )
        .unwrap(),
        "an unrelated key must not verify the signature"
    );
}

fn ed25519_pubkey(seed: [u8; 32]) -> [u8; 32] {
    use ed25519_dalek::SigningKey;
    *SigningKey::from_bytes(&seed).verifying_key().as_bytes()
}

#[test]
fn public_key_for_secret_round_trips_through_the_strkey_form() {
    let secret = strkey::PrivateKey(SIGNER_SEED).to_string();
    assert_eq!(
        tx_xdr::public_key_for_secret(&secret).unwrap(),
        strkey::PublicKey(ed25519_pubkey(SIGNER_SEED)).to_string()
    );
    assert!(tx_xdr::public_key_for_secret("Snot-a-key").is_err());
}

#[test]
fn fee_bump_envelopes_round_trip_and_sign() {
    let inner = match envelope_for(OperationBody::Inflation) {
        TransactionEnvelope::Tx(v1) => v1,
        _ => panic!("fixture builder must produce a v1 envelope"),
    };
    let envelope = TransactionEnvelope::TxFeeBump(FeeBumpTransactionEnvelope {
        tx: FeeBumpTransaction {
            fee_source: muxed(20),
            fee: 200,
            inner_tx: FeeBumpTransactionInnerTx::Tx(inner),
            ext: FeeBumpTransactionExt::V0,
        },
        signatures: VecM::default(),
    });

    let xdr = tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Base64).unwrap();
    let json = tx_xdr::to_json(&envelope, true).unwrap();
    assert_eq!(
        xdr,
        tx_xdr::write_envelope(
            &tx_xdr::encode_json(&json).unwrap(),
            tx_xdr::WireFormat::Base64
        )
        .unwrap()
    );

    let decoded = tx_xdr::decode(&xdr, Some(TESTNET_PASSPHRASE)).unwrap();
    assert_eq!(decoded.kind, "fee bump");
    assert_eq!(decoded.summary.operations, vec!["inflation"]);

    let secret = strkey::PrivateKey(SIGNER_SEED).to_string();
    let signed = tx_xdr::sign_envelope(&envelope, &secret, TESTNET_PASSPHRASE).unwrap();
    assert!(tx_xdr::verify_signature(
        &signed,
        TESTNET_PASSPHRASE,
        &tx_xdr::public_key_for_secret(&secret).unwrap()
    )
    .unwrap());
}

#[test]
fn v0_envelopes_decode_but_cannot_be_hashed_or_signed() {
    let envelope = TransactionEnvelope::TxV0(TransactionV0Envelope {
        tx: TransactionV0 {
            source_account_ed25519: Uint256([1u8; 32]),
            fee: 100,
            seq_num: SequenceNumber(7),
            time_bounds: None,
            memo: Memo::None,
            operations: vec![operation(OperationBody::Inflation)]
                .try_into()
                .unwrap(),
            ext: TransactionV0Ext::V0,
        },
        signatures: VecM::default(),
    });

    let xdr = tx_xdr::write_envelope(&envelope, tx_xdr::WireFormat::Base64).unwrap();
    let decoded = tx_xdr::decode(&xdr, None).unwrap();
    assert_eq!(decoded.kind, "transaction (v0)");
    assert_eq!(decoded.summary.operations, vec!["inflation"]);
    assert_eq!(decoded.hash, None);

    let error = tx_xdr::signature_hash(&envelope, TESTNET_PASSPHRASE).unwrap_err();
    assert!(error.to_string().contains("v0"), "got: {error}");
}

#[test]
fn malformed_input_is_rejected_locally() {
    for garbage in ["", "   ", "not-xdr!!", "####"] {
        assert!(
            tx_xdr::parse_envelope(garbage).is_err(),
            "'{garbage}' should not parse"
        );
    }
    // Valid base64 that is not an envelope.
    let bytes = vec![0u8; 8];
    let encoded = base64(&bytes);
    assert!(tx_xdr::parse_envelope(&encoded).is_err());
    assert!(tx_xdr::encode_json("{\"not\":\"an envelope\"}").is_err());
}

fn base64(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose, Engine as _};
    general_purpose::STANDARD.encode(bytes)
}
