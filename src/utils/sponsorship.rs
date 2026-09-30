//! CAP-33 sponsored reserves: let a sponsor pay a new account's base reserve.
//!
//! Sponsoring account creation is a **two-phase, single-envelope** operation.
//! Both operations are submitted in one transaction so the ledger applies them
//! atomically — if either fails, neither takes effect:
//!
//! 1. `BeginSponsoringFutureReserves { sponsoredId: newAccount }`
//! 2. `CreateAccount { destination: newAccount, startingBalance }`
//!
//! A third, separate transaction is used later to stop the sponsorship:
//! `EndSponsoringFutureReserves`, whose source is the *sponsored* account.
//!
//! ## Why only the sponsor signs
//!
//! A brand-new account has no sequence number and no ledger entry, so it
//! **cannot** produce a valid signature. The creation transaction is therefore
//! authorised solely by the sponsor, and its operation source is the sponsor.
//! Ending the sponsorship later is a different transaction, signed by the
//! sponsored account once it exists and has a sequence number.
//!
//! The minimum starting balance is one base reserve (currently 0.5 XLM); the
//! sponsor covers it via the sponsorship, which is the entire point of the flow.

use anyhow::{bail, Context, Result};
use stellar_xdr::curr::{
    AccountId, BeginSponsoringFutureReservesOp, CreateAccountOp, DecoratedSignature, Memo,
    MuxedAccount, Operation, OperationBody, Preconditions, PublicKey, SequenceNumber, Transaction,
    TransactionExt, TransactionV1Envelope, Uint256, VecM,
};

use crate::utils::tx_builder;

/// The protocol minimum starting balance for a new account: one base reserve.
pub const MIN_STARTING_BALANCE: i64 = 5_000_000;

/// One base reserve expressed in stroops (0.5 XLM).
pub const BASE_RESERVE: i64 = 5_000_000;

/// Operations in the sponsored-creation transaction: begin-sponsoring plus create.
const CREATION_OP_COUNT: u32 = 2;

/// Operations in the end-sponsorship transaction.
const END_OP_COUNT: u32 = 1;

/// Total fee for `ops` operations, never below the protocol base fee.
fn fee_for_ops(base_fee: i64, ops: u32) -> u32 {
    let base = base_fee.max(tx_builder::MIN_BASE_FEE);
    u32::try_from(
        base.saturating_mul(i64::from(ops))
            .max(tx_builder::MIN_BASE_FEE),
    )
    .unwrap_or(u32::MAX)
}

/// A parsed, validated request to create a sponsored account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SponsoredCreationPlan {
    /// The sponsor: pays the reserve and signs.
    pub sponsor: String,
    /// The account being created: receives the sponsored reserve.
    pub new_account: String,
    /// Starting balance in stroops, funded by the sponsor.
    pub starting_balance: i64,
    /// Network passphrase used for signing.
    pub network_passphrase: String,
}

impl SponsoredCreationPlan {
    /// Validate the request and resolve both accounts.
    pub fn validated(&self) -> Result<(MuxedAccount, MuxedAccount)> {
        if self.sponsor.trim() == self.new_account.trim() {
            bail!("sponsor and new account must be different accounts");
        }
        if self.starting_balance < MIN_STARTING_BALANCE {
            bail!(
                "starting balance must be at least one base reserve ({} stroops / {:.1} XLM)",
                MIN_STARTING_BALANCE,
                MIN_STARTING_BALANCE as f64 / 10_000_000.0
            );
        }
        let sponsor = tx_builder::parse_muxed_account(&self.sponsor)?;
        let new_account = tx_builder::parse_muxed_account(&self.new_account)?;
        Ok((sponsor, new_account))
    }

    /// The transaction fee for the creation, given the network base fee.
    pub fn fee(&self, base_fee: i64) -> i64 {
        base_fee.max(tx_builder::MIN_BASE_FEE) * i64::from(CREATION_OP_COUNT)
    }

    /// Total the sponsor is debited: the starting balance it funds, the base
    /// reserve it locks, and the network fee.
    pub fn sponsor_outlay(&self, base_fee: i64) -> i64 {
        self.starting_balance + BASE_RESERVE + self.fee(base_fee)
    }

    /// Preview rows describing who pays what.
    pub fn preview_rows(&self, base_fee: i64) -> Vec<(&'static str, String)> {
        vec![
            ("Sponsor", self.sponsor.clone()),
            ("New account", self.new_account.clone()),
            (
                "Starting balance to new account",
                tx_builder::stroops_xlm(self.starting_balance),
            ),
            (
                "Base reserve (sponsored)",
                tx_builder::stroops_xlm(BASE_RESERVE),
            ),
            ("Paid by new account", tx_builder::stroops_xlm(0)),
            (
                "Network fee (sponsor)",
                tx_builder::stroops_xlm(self.fee(base_fee)),
            ),
            (
                "Sponsor total outlay",
                tx_builder::stroops_xlm(self.sponsor_outlay(base_fee)),
            ),
        ]
    }
}

/// Build the begin-sponsoring + create-account transaction.
///
/// `sponsor_sequence` must be the sponsor's current sequence number, fetched
/// fresh at submission time — a stale value is the most common `txBAD_SEQ`.
pub fn build_sponsored_creation(
    sponsor: &MuxedAccount,
    new_account: &MuxedAccount,
    starting_balance: i64,
    sponsor_sequence: i64,
    base_fee: i64,
) -> Result<Transaction> {
    if starting_balance < MIN_STARTING_BALANCE {
        bail!(
            "starting balance must be at least one base reserve ({} stroops)",
            MIN_STARTING_BALANCE
        );
    }

    let account_id = |account: &MuxedAccount| -> Result<AccountId> {
        match account {
            MuxedAccount::Ed25519(Uint256(bytes)) => {
                Ok(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(*bytes))))
            }
            // CAP-33 sponsorship data must name a plain ed25519 account.
            MuxedAccount::MuxedEd25519(_) => {
                bail!("sponsorship requires a plain ed25519 account (G...), not a muxed address")
            }
        }
    };

    let operations = vec![
        Operation {
            source_account: None,
            body: OperationBody::BeginSponsoringFutureReserves(BeginSponsoringFutureReservesOp {
                sponsored_id: account_id(new_account)?,
            }),
        },
        Operation {
            source_account: None,
            body: OperationBody::CreateAccount(CreateAccountOp {
                destination: account_id(new_account)?,
                starting_balance,
            }),
        },
    ];

    Ok(Transaction {
        source_account: sponsor.clone(),
        fee: fee_for_ops(base_fee, CREATION_OP_COUNT),
        seq_num: SequenceNumber(sponsor_sequence),
        cond: Preconditions::None,
        memo: Memo::None,
        operations: VecM::<Operation, 100>::try_from(operations)
            .context("sponsored creation operations")?,
        ext: TransactionExt::V0,
    })
}

/// Build the empty envelope wrapping a sponsored-creation transaction.
pub fn sponsored_creation_envelope(tx: &Transaction) -> Result<TransactionV1Envelope> {
    Ok(TransactionV1Envelope {
        tx: tx.clone(),
        signatures: VecM::<DecoratedSignature, 20>::try_from(Vec::new())
            .context("empty signature list")?,
    })
}

/// Build the `EndSponsoringFutureReserves` transaction.
///
/// The source is the *sponsored* account, so the sponsored account must sign
/// this one. It is submitted separately, later, once the account exists.
pub fn build_end_sponsoring(
    sponsored_account: &MuxedAccount,
    sequence: i64,
    base_fee: i64,
) -> Result<Transaction> {
    Ok(Transaction {
        source_account: sponsored_account.clone(),
        fee: fee_for_ops(base_fee, END_OP_COUNT),
        seq_num: SequenceNumber(sequence),
        cond: Preconditions::None,
        memo: Memo::None,
        operations: VecM::<Operation, 100>::try_from(vec![Operation {
            source_account: None,
            body: OperationBody::EndSponsoringFutureReserves,
        }])
        .context("end-sponsoring operation")?,
        ext: TransactionExt::V0,
    })
}

/// A parsed, validated request to end a sponsorship.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndSponsoringPlan {
    /// The sponsored account, which signs and pays.
    pub sponsored_account: String,
    pub network_passphrase: String,
}

impl EndSponsoringPlan {
    /// Validate and resolve the sponsored account.
    pub fn validated(&self) -> Result<MuxedAccount> {
        tx_builder::parse_muxed_account(&self.sponsored_account)
    }

    /// Preview rows describing who pays what.
    /// Preview rows describing who pays what.
    pub fn preview_rows(&self, base_fee: i64) -> Vec<(&'static str, String)> {
        vec![
            ("Sponsored account", self.sponsored_account.clone()),
            (
                "Reserve released to sponsor",
                tx_builder::stroops_xlm(BASE_RESERVE),
            ),
            ("Reserve paid by new account", tx_builder::stroops_xlm(0)),
            (
                "Network fee",
                tx_builder::stroops_xlm(
                    base_fee.max(tx_builder::MIN_BASE_FEE) * i64::from(END_OP_COUNT),
                ),
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use stellar_xdr::curr::{AccountId, PublicKey};

    const PASSPHRASE: &str = "Test SDF Network ; September 2015";

    fn account(seed: u8) -> MuxedAccount {
        let key = SigningKey::from_bytes(&[seed; 32]);
        MuxedAccount::Ed25519(Uint256(key.verifying_key().to_bytes()))
    }

    fn account_id(account: &MuxedAccount) -> AccountId {
        match account {
            MuxedAccount::Ed25519(Uint256(bytes)) => {
                AccountId(PublicKey::PublicKeyTypeEd25519(Uint256(*bytes)))
            }
            MuxedAccount::MuxedEd25519(_) => unreachable!(),
        }
    }

    fn plan() -> SponsoredCreationPlan {
        SponsoredCreationPlan {
            sponsor: tx_builder::account_str(&account(1)),
            new_account: tx_builder::account_str(&account(2)),
            starting_balance: BASE_RESERVE,
            network_passphrase: PASSPHRASE.to_string(),
        }
    }

    #[test]
    fn creation_contains_begin_sponsoring_then_create_account() {
        let sponsor = account(1);
        let new_account = account(2);
        let tx = build_sponsored_creation(&sponsor, &new_account, BASE_RESERVE, 5, 100).unwrap();

        let operations = tx.operations.to_vec();
        assert_eq!(operations.len(), 2);
        assert!(matches!(
            operations[0].body,
            OperationBody::BeginSponsoringFutureReserves(_)
        ));
        assert!(matches!(
            operations[1].body,
            OperationBody::CreateAccount(_)
        ));
    }

    #[test]
    fn creation_is_sourced_from_the_sponsor() {
        let sponsor = account(1);
        let new_account = account(2);
        let tx = build_sponsored_creation(&sponsor, &new_account, BASE_RESERVE, 5, 100).unwrap();
        assert_eq!(tx.source_account, sponsor);
    }

    #[test]
    fn sponsored_id_and_destination_are_the_new_account() {
        let sponsor = account(1);
        let new_account = account(2);
        let tx = build_sponsored_creation(&sponsor, &new_account, BASE_RESERVE, 5, 100).unwrap();
        let operations = tx.operations.to_vec();
        let expected = account_id(&new_account);

        let OperationBody::BeginSponsoringFutureReserves(begin) = &operations[0].body else {
            panic!("expected begin-sponsoring");
        };
        assert_eq!(begin.sponsored_id, expected);

        let OperationBody::CreateAccount(create) = &operations[1].body else {
            panic!("expected create-account");
        };
        assert_eq!(create.destination, expected);
        assert_eq!(create.starting_balance, BASE_RESERVE);
    }

    #[test]
    fn fee_scales_with_operation_count() {
        let sponsor = account(1);
        let new_account = account(2);
        let tx = build_sponsored_creation(&sponsor, &new_account, BASE_RESERVE, 5, 100).unwrap();
        assert_eq!(tx.fee, 200);
    }

    #[test]
    fn sequence_number_comes_from_the_sponsor() {
        let sponsor = account(1);
        let new_account = account(2);
        let tx = build_sponsored_creation(&sponsor, &new_account, BASE_RESERVE, 42, 100).unwrap();
        assert_eq!(tx.seq_num, SequenceNumber(42));
    }

    #[test]
    fn sponsor_signature_is_the_only_one_required_and_it_verifies() {
        let sponsor_key = SigningKey::from_bytes(&[1u8; 32]);
        let sponsor = MuxedAccount::Ed25519(Uint256(sponsor_key.verifying_key().to_bytes()));
        let new_account = account(2);

        let tx = build_sponsored_creation(&sponsor, &new_account, BASE_RESERVE, 5, 100).unwrap();
        let envelope = sponsored_creation_envelope(&tx).unwrap();
        let signed = tx_builder::sign_transaction_v1(&envelope, &sponsor_key, PASSPHRASE).unwrap();

        assert_eq!(signed.signatures.len(), 1);
        assert!(tx_builder::verify_transaction_signature(
            &signed,
            &sponsor_key.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .unwrap());
    }

    #[test]
    fn creation_can_itself_be_fee_bumped_by_another_sponsor() {
        let sponsor_key = SigningKey::from_bytes(&[1u8; 32]);
        let sponsor = MuxedAccount::Ed25519(Uint256(sponsor_key.verifying_key().to_bytes()));
        let new_account = account(2);
        let payer_key = SigningKey::from_bytes(&[3u8; 32]);

        let tx = build_sponsored_creation(&sponsor, &new_account, BASE_RESERVE, 5, 100).unwrap();
        let envelope = sponsored_creation_envelope(&tx).unwrap();
        let signed = tx_builder::sign_transaction_v1(&envelope, &sponsor_key, PASSPHRASE).unwrap();
        let bumped = tx_builder::wrap_fee_bump(signed, &payer_key, PASSPHRASE, 100).unwrap();

        assert!(tx_builder::is_fee_bump(&bumped));
        // Bump fee is max(2 * 200, 100) = 400.
        assert_eq!(tx_builder::effective_fee(&bumped), 400);
    }

    #[test]
    fn rejects_starting_balance_below_one_base_reserve() {
        let sponsor = account(1);
        let new_account = account(2);
        assert!(build_sponsored_creation(&sponsor, &new_account, 1, 5, 100).is_err());
    }

    #[test]
    fn plan_rejects_sponsor_equal_to_new_account() {
        let mut bad = plan();
        bad.new_account = bad.sponsor.clone();
        assert!(bad.validated().is_err());
    }

    #[test]
    fn plan_rejects_undersized_starting_balance() {
        let mut bad = plan();
        bad.starting_balance = 10;
        assert!(bad.validated().is_err());
    }

    #[test]
    fn end_sponsoring_is_sourced_from_the_sponsored_account() {
        let sponsored = account(2);
        let tx = build_end_sponsoring(&sponsored, 11, 100).unwrap();
        assert_eq!(tx.source_account, sponsored);
        assert_eq!(tx.operations.len(), 1);
        assert!(matches!(
            tx.operations.to_vec()[0].body,
            OperationBody::EndSponsoringFutureReserves
        ));
    }

    #[test]
    fn end_sponsoring_is_signed_by_the_sponsored_account() {
        let key = SigningKey::from_bytes(&[2u8; 32]);
        let sponsored = MuxedAccount::Ed25519(Uint256(key.verifying_key().to_bytes()));
        let tx = build_end_sponsoring(&sponsored, 11, 100).unwrap();
        let envelope = sponsored_creation_envelope(&tx).unwrap();
        let signed = tx_builder::sign_transaction_v1(&envelope, &key, PASSPHRASE).unwrap();
        assert!(tx_builder::verify_transaction_signature(
            &signed,
            &key.verifying_key().to_bytes(),
            PASSPHRASE
        )
        .unwrap());
    }

    #[test]
    fn preview_shows_sponsor_paying_the_reserve() {
        let rows = plan().preview_rows(100);
        let labelled: Vec<String> = rows
            .iter()
            .map(|(key, value)| format!("{key}={value}"))
            .collect();
        assert!(labelled.iter().any(|row| row.starts_with("Sponsor=G")));
        assert!(labelled
            .iter()
            .any(|row| row.contains("Base reserve (sponsored)=5000000")));
        assert!(labelled
            .iter()
            .any(|row| row.starts_with("Paid by new account=0")));
        assert!(
            labelled
                .iter()
                .any(|row| row.contains("Network fee (sponsor)=200")),
            "two operations means twice the base fee, got {labelled:?}"
        );
        // 5_000_000 starting + 5_000_000 reserve + 200 fee.
        assert!(
            labelled
                .iter()
                .any(|row| row.contains("Sponsor total outlay=10000200")),
            "sponsor outlay must total the funded amount, reserve and fee: {labelled:?}"
        );
    }

    #[test]
    fn rejects_muxed_accounts_for_sponsorship() {
        let sponsor = account(1);
        let muxed = MuxedAccount::MuxedEd25519(stellar_xdr::curr::MuxedAccountMed25519 {
            ed25519: Uint256([2u8; 32]),
            id: 1,
        });
        assert!(build_sponsored_creation(&sponsor, &muxed, BASE_RESERVE, 5, 100).is_err());
    }
}
