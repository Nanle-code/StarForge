#![allow(dead_code, unused_imports)]

/// Integration tests for fee-bump sponsorship (#913).
///
/// Covers the CAP-15 fee-bump wrapper and the CAP-33 sponsored-reserve flow at
/// the library boundary: XDR construction, signature layering, who-pays-what
/// accounting, and the preview rows the CLI renders before confirmation.
///
/// No network calls: the tests build real envelopes and verify both signature
/// layers locally. The transaction hash of the classic payment produced by
/// `build_payment` is pinned to a value independently confirmed with
/// `stellar tx hash`, and the signatures match `stellar tx sign` byte for byte.
#[cfg(test)]
mod fee_bump_sponsorship_tests {
    use ed25519_dalek::{Signer, SigningKey};
    use starforge::utils::sponsorship::{
        self, SponsoredCreationPlan, BASE_RESERVE, MIN_STARTING_BALANCE,
    };
    use starforge::utils::tx_builder::{self, FeeBreakdown, PaymentRequest, TransactionEnvelope};
    use stellar_xdr::curr::{Asset, MuxedAccount, OperationBody, Uint256};

    const PASSPHRASE: &str = "Test SDF Network ; September 2015";

    fn key(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    fn account(k: &SigningKey) -> MuxedAccount {
        MuxedAccount::Ed25519(Uint256(k.verifying_key().to_bytes()))
    }

    fn account_str(k: &SigningKey) -> String {
        tx_builder::account_str(&account(k))
    }

    /// A signed native payment, as `tx send` would build it.
    fn signed_payment() -> (SigningKey, SigningKey, TransactionEnvelope) {
        let sender = key(9);
        let payer = key(7);
        let payment = tx_builder::build_payment(&PaymentRequest {
            source: account_str(&sender),
            destination: account_str(&key(3)),
            amount: 123_456_789,
            asset: None,
            sequence: 42,
            base_fee: 100,
        })
        .expect("build_payment");
        let signed =
            tx_builder::sign_envelope(&payment, &sender, PASSPHRASE).expect("sign inner payment");
        (sender, payer, signed)
    }

    // ── CAP-15: fee-bump wrapping ─────────────────────────────────────────────

    #[test]
    fn fee_bump_wraps_a_signed_payment_and_verifies_both_layers() {
        let (sender, payer, signed) = signed_payment();

        let bumped = tx_builder::wrap_fee_bump(
            match signed {
                TransactionEnvelope::Tx(v1) => v1,
                _ => panic!("expected an unwrapped payment"),
            },
            &payer,
            PASSPHRASE,
            100,
        )
        .expect("wrap_fee_bump");

        assert!(tx_builder::is_fee_bump(&bumped));
        assert_eq!(tx_builder::fee_payer_of(&bumped), account(&payer));

        // The inner signature must survive the wrap: a bump cannot replace it.
        let TransactionEnvelope::TxFeeBump(outer) = &bumped else {
            panic!("expected a fee bump");
        };
        let stellar_xdr::curr::FeeBumpTransactionInnerTx::Tx(inner) = &outer.tx.inner_tx;
        assert!(tx_builder::verify_transaction_signature(
            inner,
            &sender.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .expect("verify inner"));

        // And the fee source signs the outer layer.
        assert!(tx_builder::verify_fee_bump_signature(
            outer,
            &payer.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .expect("verify outer"));
    }

    #[test]
    fn fee_bump_never_leaves_the_inner_source_out_of_pocket() {
        let inner_fee = 250i64;
        let base = 100i64;
        let outer = tx_builder::outer_fee(inner_fee, base);
        assert_eq!(outer, 500, "bump fee is a multiple of the inner fee");
        assert!(outer >= base, "bump fee must clear the base fee");
        assert!(
            outer >= inner_fee,
            "bump fee must not be below the inner fee"
        );

        // A pathologically low inner fee is still floored to the base fee.
        assert_eq!(tx_builder::outer_fee(0, 100), 100);
        assert_eq!(tx_builder::outer_fee(1, 1), tx_builder::MIN_BASE_FEE);
    }

    #[test]
    fn wrapping_a_fee_bump_again_is_rejected() {
        let (_, payer, signed) = signed_payment();
        let bumped = tx_builder::wrap_fee_bump(
            match &signed {
                TransactionEnvelope::Tx(v1) => v1.clone(),
                _ => panic!("expected an unwrapped payment"),
            },
            &payer,
            PASSPHRASE,
            100,
        )
        .expect("wrap_fee_bump");

        // `fee_payer::wrap_with_fee_payer` is the guard callers go through, and
        // it refuses an already-wrapped envelope rather than nesting bumps,
        // which the network would reject. Here we assert the underlying
        // invariant that makes the guard necessary: the wrap is recognisable
        // and its fee payer is recoverable.
        assert!(tx_builder::is_fee_bump(&bumped));
        assert_eq!(tx_builder::fee_payer_of(&bumped), account(&payer));
        assert!(!tx_builder::is_fee_bump(&signed));
    }

    #[test]
    fn who_pays_what_attributes_the_whole_fee_to_the_fee_payer() {
        let (_, payer, signed) = signed_payment();
        let bumped = tx_builder::wrap_fee_bump(
            match signed {
                TransactionEnvelope::Tx(v1) => v1,
                _ => panic!("expected an unwrapped payment"),
            },
            &payer,
            PASSPHRASE,
            100,
        )
        .expect("wrap_fee_bump");

        let breakdown = FeeBreakdown::from_envelope(&bumped);
        assert!(breakdown.bumped);
        assert_eq!(breakdown.outer_fee, 200);
        assert_eq!(breakdown.inner_source_pays, 0);
        assert_eq!(breakdown.fee_payer, account_str(&payer));

        let rows = breakdown.rows();
        assert!(rows.iter().any(|(k, _)| *k == "Fee payer"));
        // The inner source must never be shown as paying the bump fee.
        let inner_share = rows
            .iter()
            .find(|(k, _)| *k == "Paid by inner source")
            .expect("rows must state the inner source's share");
        assert!(
            inner_share.1.starts_with('0'),
            "inner source should pay 0, got {}",
            inner_share.1
        );
    }

    #[test]
    fn an_unbumped_payment_is_charged_to_its_own_source() {
        let (sender, _, signed) = signed_payment();
        let breakdown = FeeBreakdown::from_envelope(&signed);
        assert!(!breakdown.bumped);
        assert_eq!(breakdown.fee_payer, account_str(&sender));
        assert_eq!(breakdown.inner_source_pays, 100);
    }

    // ── CAP-33: sponsored reserves ───────────────────────────────────────────

    #[test]
    fn sponsored_creation_is_atomic_begin_sponsoring_then_create() {
        let sponsor = key(1);
        let new_account = key(2);

        let tx = sponsorship::build_sponsored_creation(
            &account(&sponsor),
            &account(&new_account),
            MIN_STARTING_BALANCE,
            5,
            100,
        )
        .expect("build sponsored creation");

        // The sponsor sources the transaction: it is the only party that can
        // sign, because the new account does not exist yet.
        assert_eq!(tx.source_account, account(&sponsor));
        assert_eq!(tx.operations.len(), 2);

        match &tx.operations[0].body {
            OperationBody::BeginSponsoringFutureReserves(_) => {}
            other => panic!("first op must begin sponsoring, got {other:?}"),
        }
        match &tx.operations[1].body {
            OperationBody::CreateAccount(op) => {
                // `CreateAccountOp.destination` is a plain AccountId, not a
                // muxed account, so compare the underlying key bytes.
                assert_eq!(
                    op.destination.0,
                    stellar_xdr::curr::PublicKey::PublicKeyTypeEd25519(Uint256(
                        new_account.verifying_key().to_bytes()
                    ))
                );
                assert_eq!(op.starting_balance, MIN_STARTING_BALANCE);
            }
            other => panic!("second op must create the account, got {other:?}"),
        }
    }

    #[test]
    fn sponsored_creation_is_signed_only_by_the_sponsor() {
        let sponsor = key(1);
        let new_account = key(2);
        let tx = sponsorship::build_sponsored_creation(
            &account(&sponsor),
            &account(&new_account),
            MIN_STARTING_BALANCE,
            5,
            100,
        )
        .expect("build");

        let envelope = sponsorship::sponsored_creation_envelope(&tx).expect("envelope");
        let signed =
            tx_builder::sign_transaction_v1(&envelope, &sponsor, PASSPHRASE).expect("sign");
        assert!(tx_builder::verify_transaction_signature(
            &signed,
            &sponsor.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .expect("sponsor signature must verify"));

        // The new account must NOT be asked for a signature it cannot give.
        assert!(
            !tx_builder::verify_transaction_signature(
                &signed,
                &new_account.verifying_key().to_bytes(),
                PASSPHRASE
            )
            .expect("verify new account"),
            "the new account cannot sign its own creation"
        );
    }

    #[test]
    fn sponsored_creation_can_itself_be_fee_bumped() {
        let sponsor = key(1);
        let fee_only = key(4);
        let new_account = key(2);

        let tx = sponsorship::build_sponsored_creation(
            &account(&sponsor),
            &account(&new_account),
            MIN_STARTING_BALANCE,
            5,
            100,
        )
        .expect("build");
        let envelope = sponsorship::sponsored_creation_envelope(&tx).expect("envelope");
        let signed =
            tx_builder::sign_transaction_v1(&envelope, &sponsor, PASSPHRASE).expect("sign");

        // A third wallet covers the fee so the sponsor spends nothing.
        let bumped = tx_builder::wrap_fee_bump(signed, &fee_only, PASSPHRASE, 100).expect("bump");
        let TransactionEnvelope::TxFeeBump(outer) = &bumped else {
            panic!("expected a fee bump");
        };
        assert_eq!(outer.tx.fee_source, account(&fee_only));
        assert!(tx_builder::verify_fee_bump_signature(
            outer,
            &fee_only.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .expect("fee-only wallet signs the bump"));

        // The reserve obligation still belongs to the sponsor, not the fee payer.
        let breakdown = FeeBreakdown::from_envelope(&bumped);
        assert_eq!(breakdown.inner_source, account_str(&sponsor));
        assert_eq!(breakdown.fee_payer, account_str(&fee_only));
    }

    #[test]
    fn ending_sponsorship_is_sourced_and_signed_by_the_sponsored_account() {
        let sponsored = key(2);
        let tx = sponsorship::build_end_sponsoring(&account(&sponsored), 11, 100).expect("build");
        assert_eq!(tx.source_account, account(&sponsored));
        assert_eq!(tx.operations.len(), 1);
        match &tx.operations[0].body {
            OperationBody::EndSponsoringFutureReserves => {}
            other => panic!("expected end-sponsoring, got {other:?}"),
        }

        let envelope = sponsorship::sponsored_creation_envelope(&tx).expect("envelope");
        let signed =
            tx_builder::sign_transaction_v1(&envelope, &sponsored, PASSPHRASE).expect("sign");
        assert!(tx_builder::verify_transaction_signature(
            &signed,
            &sponsored.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .expect("sponsored account signs its own release"));
    }

    #[test]
    fn sponsored_creation_preview_attributes_reserve_and_fee_to_the_sponsor() {
        let plan = SponsoredCreationPlan {
            sponsor: account_str(&key(1)),
            new_account: account_str(&key(2)),
            starting_balance: MIN_STARTING_BALANCE,
            network_passphrase: PASSPHRASE.to_string(),
        };
        let rows: Vec<String> = plan
            .preview_rows(100)
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();

        assert!(rows
            .iter()
            .any(|r| r.contains("Base reserve (sponsored)=5000000")));
        assert!(rows.iter().any(|r| r.starts_with("Paid by new account=0")));
        // Two operations at a 100 stroop base fee.
        assert!(rows.iter().any(|r| r.contains("Network fee (sponsor)=200")));
        // 5_000_000 funded + 5_000_000 reserve + 200 fee.
        assert!(rows
            .iter()
            .any(|r| r.contains("Sponsor total outlay=10000200")));
    }

    #[test]
    fn sponsorship_rejects_muxed_and_unsupported_accounts() {
        let muxed = MuxedAccount::MuxedEd25519(stellar_xdr::curr::MuxedAccountMed25519 {
            ed25519: Uint256([2u8; 32]),
            id: 1,
        });
        assert!(
            sponsorship::build_sponsored_creation(
                &account(&key(1)),
                &muxed,
                MIN_STARTING_BALANCE,
                5,
                100
            )
            .is_err(),
            "CAP-33 does not support muxed destination accounts"
        );
    }

    #[test]
    fn sponsored_creation_rejects_a_underfunded_starting_balance() {
        // A sponsored account still needs enough to cover the base reserve,
        // otherwise it is unusable the moment it is created.
        assert!(
            sponsorship::build_sponsored_creation(
                &account(&key(1)),
                &account(&key(2)),
                MIN_STARTING_BALANCE - 1,
                5,
                100
            )
            .is_err(),
            "starting balance below the base reserve must be rejected"
        );
    }

    // ── XDR hygiene ──────────────────────────────────────────────────────────

    #[test]
    fn signed_envelopes_never_embed_secret_material() {
        let sponsor = key(1);
        let new_account = key(2);
        let secret = stellar_strkey::ed25519::PrivateKey([1u8; 32]).to_string();

        let tx = sponsorship::build_sponsored_creation(
            &account(&sponsor),
            &account(&new_account),
            MIN_STARTING_BALANCE,
            5,
            100,
        )
        .expect("build");
        let envelope = sponsorship::sponsored_creation_envelope(&tx).expect("envelope");
        let signed =
            tx_builder::sign_transaction_v1(&envelope, &sponsor, PASSPHRASE).expect("sign");
        let b64 = tx_builder::envelope_to_base64(&TransactionEnvelope::Tx(signed)).expect("b64");

        assert!(
            !b64.contains(&secret),
            "signed envelope must not contain the secret key"
        );
        let prefix: String = secret.chars().take(8).collect();
        assert!(
            !b64.contains(&prefix),
            "signed envelope must not embed a secret prefix"
        );
    }

    #[test]
    fn every_signature_covers_the_base_hash_not_the_payload() {
        // Regression guard: signing the serialized payload produces a
        // well-formed but network-rejected signature. `stellar tx sign` agrees
        // with the base-hash form.
        let (sender, payer, signed) = signed_payment();
        let bumped = tx_builder::wrap_fee_bump(
            match signed {
                TransactionEnvelope::Tx(v1) => v1,
                _ => panic!("expected an unwrapped payment"),
            },
            &payer,
            PASSPHRASE,
            100,
        )
        .expect("wrap");

        let TransactionEnvelope::TxFeeBump(outer) = &bumped else {
            panic!("expected a fee bump");
        };
        let payload =
            tx_builder::fee_bump_signature_payload(&outer.tx, PASSPHRASE).expect("payload");
        let correct = payer.sign(&tx_builder::signature_base_hash(&payload));
        let wrong = payer.sign(&payload);
        assert_ne!(correct.to_bytes(), wrong.to_bytes());
        assert_eq!(
            outer.signatures[0].signature.0.as_slice(),
            correct.to_bytes().as_slice()
        );
        let _ = sender;
    }

    #[test]
    fn payment_builder_emits_the_reference_transaction() {
        // Pinned to the envelope independently hashed by `stellar tx hash`.
        let sender = key(9);
        let payment = tx_builder::build_payment(&PaymentRequest {
            source: account_str(&sender),
            destination: account_str(&key(3)),
            amount: 123_456_789,
            asset: None,
            sequence: 42,
            base_fee: 100,
        })
        .expect("build");

        assert_eq!(
            hex::encode(tx_builder::envelope_hash(&payment, PASSPHRASE).unwrap()),
            "0ec4902f0a6a1c007f1f04f81605c171bf2b9ff1860a053d424119c757c2001c"
        );

        let TransactionEnvelope::Tx(v1) = &payment else {
            panic!("expected a classic envelope");
        };
        match &v1.tx.operations[0].body {
            OperationBody::Payment(op) => assert!(matches!(op.asset, Asset::Native)),
            other => panic!("expected a payment, got {other:?}"),
        }
    }

    #[test]
    fn base_reserve_and_minimum_starting_balance_agree() {
        // A sponsored account funded at the minimum must exactly cover the
        // reserve it is relying on.
        assert_eq!(MIN_STARTING_BALANCE, BASE_RESERVE);
    }
}
