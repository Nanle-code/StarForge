// The Ledger APDU constants and codec helpers below (build_apdu,
// frame_apdu_for_hid, parse_hd_path, encode_hd_path, extract_*_bytes) are
// exercised by the `hardware-wallet` feature's transport implementation and
// by this module's own unit tests, but not by a default build with the
// feature off and no tests compiled — that's the one configuration where
// they're genuinely unused, not a sign anything here is actually dead.
#![cfg_attr(not(any(test, feature = "hardware-wallet")), allow(dead_code))]

use anyhow::{Context, Result};
use clap::ValueEnum;

/// Stellar SLIP-0010 / BIP-44 HD derivation path.
/// Default: m/44'/148'/0' (account index 0).
pub const STELLAR_HD_PATH: &str = "m/44'/148'/0'";

const LEDGER_VENDOR_ID: u16 = 0x2c97;
const HID_PACKET_SIZE: usize = 64;
const HID_CHANNEL: u16 = 0x0101;
const HID_TAG_APDU: u8 = 0x05;
const SW_OK: [u8; 2] = [0x90, 0x00];

const CLA_STELLAR: u8 = 0xE0;
const INS_GET_PUBLIC_KEY: u8 = 0x02;
const INS_SIGN_TX: u8 = 0x04;

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum HardwareWalletKind {
    Ledger,
    Trezor,
}

impl std::fmt::Display for HardwareWalletKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HardwareWalletKind::Ledger => write!(f, "Ledger"),
            HardwareWalletKind::Trezor => write!(f, "Trezor"),
        }
    }
}

/// Basic information returned by a connected hardware wallet.
#[derive(Debug, Clone)]
pub struct HardwareWalletInfo {
    pub kind: HardwareWalletKind,
    pub device_count: usize,
    pub stellar_address: Option<String>,
    pub hd_path: String,
}

#[cfg(not(feature = "hardware-wallet"))]
pub fn connect(kind: HardwareWalletKind) -> Result<HardwareWalletInfo> {
    anyhow::bail!(
        "Hardware wallet support is disabled in this build.\n\
         Rebuild with `cargo build --features hardware-wallet` to enable {} detection.",
        kind
    )
}

#[cfg(not(feature = "hardware-wallet"))]
pub fn sign(_kind: HardwareWalletKind, _message: &[u8]) -> Result<Vec<u8>> {
    anyhow::bail!("Hardware wallet support is disabled in this build.")
}

#[cfg(not(feature = "hardware-wallet"))]
pub fn get_stellar_address(_kind: HardwareWalletKind, _hd_path: &str) -> Result<String> {
    anyhow::bail!("Hardware wallet support is disabled in this build.")
}

#[cfg(not(feature = "hardware-wallet"))]
pub fn device_status(_kind: HardwareWalletKind) -> Result<String> {
    anyhow::bail!("Hardware wallet support is disabled in this build.")
}

#[cfg(not(feature = "hardware-wallet"))]
pub fn connect_with_timeout(
    kind: HardwareWalletKind,
    _timeout: std::time::Duration,
) -> Result<HardwareWalletInfo> {
    connect(kind)
}

#[cfg(not(feature = "hardware-wallet"))]
pub fn sign_transaction(
    kind: HardwareWalletKind,
    _hd_path: &str,
    _transaction: &[u8],
    _network_passphrase: &str,
) -> Result<Vec<u8>> {
    anyhow::bail!(
        "Hardware wallet support is disabled in this build.\n\
         Rebuild with `cargo build --features hardware-wallet` to sign with {}.",
        kind
    )
}

/// Translate hardware wallet failures into actionable recovery guidance.
pub fn map_signing_error(err: anyhow::Error, kind: HardwareWalletKind) -> anyhow::Error {
    let message = err.to_string().to_lowercase();
    let remediation = if message.contains("timeout") || message.contains("timed out") {
        "Ensure the device is unlocked, the Stellar app is open, and approve the prompt on-screen. Retry when ready."
    } else if message.contains("not found")
        || message.contains("no ledger")
        || message.contains("no trezor")
    {
        "Connect the device via USB, unlock it, open the Stellar app, then retry."
    } else if message.contains("reject") || message.contains("denied") || message.contains("cancel")
    {
        "The request was rejected on the device. Review the transaction details and approve to continue."
    } else if message.contains("does not support")
        || message.contains("not supported")
        || message.contains("unsupported")
    {
        "This request envelope isn't supported. If the device reported it, update the Stellar app (or open the correct app) and retry; if it's a client-side limitation, no retry will help — check the error text for guidance."
    } else if message.contains("status") || message.contains("apdu") {
        "Close other wallet apps, reopen the Stellar app on the device, and retry the operation."
    } else {
        "Verify connectivity, unlock the device, open the Stellar app, and retry. Run `starforge diagnostics --wallet ledger|trezor` for a live probe."
    };

    anyhow::anyhow!(
        "{} signing failed: {}\nRecovery: {}",
        kind,
        err,
        remediation
    )
}

#[cfg(feature = "hardware-wallet")]
pub fn connect(kind: HardwareWalletKind) -> Result<HardwareWalletInfo> {
    match kind {
        HardwareWalletKind::Ledger => {
            let transport = LedgerTransport::connect()?;
            let stellar_address = transport.get_public_key(STELLAR_HD_PATH).ok();

            Ok(HardwareWalletInfo {
                kind,
                device_count: transport.device_count,
                stellar_address,
                hd_path: STELLAR_HD_PATH.to_string(),
            })
        }
        HardwareWalletKind::Trezor => TrezorTransport::connect_info(STELLAR_HD_PATH),
    }
}

#[cfg(feature = "hardware-wallet")]
pub fn get_stellar_address(kind: HardwareWalletKind, hd_path: &str) -> Result<String> {
    match kind {
        HardwareWalletKind::Ledger => LedgerTransport::connect()?.get_public_key(hd_path),
        HardwareWalletKind::Trezor => TrezorTransport::get_public_key(hd_path),
    }
}

#[cfg(feature = "hardware-wallet")]
pub fn device_status(kind: HardwareWalletKind) -> Result<String> {
    match kind {
        HardwareWalletKind::Ledger => {
            let transport = LedgerTransport::connect()?;
            Ok(format!(
                "{}: {} HID device(s) visible, Stellar app reachable",
                kind, transport.device_count
            ))
        }
        HardwareWalletKind::Trezor => TrezorTransport::status(),
    }
}

#[cfg(feature = "hardware-wallet")]
pub fn connect_with_timeout(
    kind: HardwareWalletKind,
    timeout: std::time::Duration,
) -> Result<HardwareWalletInfo> {
    match kind {
        HardwareWalletKind::Ledger => {
            let transport = LedgerTransport::connect_with_timeout(timeout)?;
            let stellar_address = transport.get_public_key(STELLAR_HD_PATH).ok();
            Ok(HardwareWalletInfo {
                kind,
                device_count: transport.device_count,
                stellar_address,
                hd_path: STELLAR_HD_PATH.to_string(),
            })
        }
        HardwareWalletKind::Trezor => TrezorTransport::connect_info(STELLAR_HD_PATH),
    }
}

#[cfg(feature = "hardware-wallet")]
pub fn sign(kind: HardwareWalletKind, message: &[u8]) -> Result<Vec<u8>> {
    sign_transaction(kind, STELLAR_HD_PATH, message, "")
}

#[cfg(feature = "hardware-wallet")]
pub fn sign_transaction(
    kind: HardwareWalletKind,
    hd_path: &str,
    transaction: &[u8],
    network_passphrase: &str,
) -> Result<Vec<u8>> {
    match kind {
        HardwareWalletKind::Ledger => {
            LedgerTransport::connect()?.sign_message(hd_path, transaction)
        }
        HardwareWalletKind::Trezor => {
            TrezorTransport::sign_transaction(hd_path, transaction, network_passphrase)
        }
    }
}

fn parse_hd_path(path: &str) -> Result<Vec<u32>> {
    let cleaned = path.trim();
    let segments = cleaned
        .strip_prefix("m/")
        .or_else(|| cleaned.strip_prefix("M/"))
        .unwrap_or(cleaned);

    if segments.is_empty() {
        anyhow::bail!("HD path cannot be empty");
    }

    let mut values = Vec::new();
    for segment in segments.split('/') {
        if segment.is_empty() {
            anyhow::bail!("Invalid HD path '{}'", path);
        }
        let hardened = segment.ends_with('\'');
        let digits = if hardened {
            &segment[..segment.len() - 1]
        } else {
            segment
        };
        let index: u32 = digits
            .parse()
            .with_context(|| format!("Invalid HD path segment '{}'", segment))?;
        if index >= 0x8000_0000 {
            anyhow::bail!("HD path segment '{}' is out of range", segment);
        }
        values.push(if hardened { index | 0x8000_0000 } else { index });
    }

    Ok(values)
}

fn encode_hd_path(path: &str) -> Result<Vec<u8>> {
    let indices = parse_hd_path(path)?;
    let mut out = Vec::with_capacity(1 + indices.len() * 4);
    out.push(indices.len() as u8);
    for index in indices {
        out.extend_from_slice(&index.to_be_bytes());
    }
    Ok(out)
}

fn build_apdu(cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> Vec<u8> {
    let mut apdu = Vec::with_capacity(5 + data.len());
    apdu.push(cla);
    apdu.push(ins);
    apdu.push(p1);
    apdu.push(p2);
    apdu.push(data.len() as u8);
    apdu.extend_from_slice(data);
    apdu
}

fn frame_apdu_for_hid(apdu: &[u8]) -> Vec<[u8; HID_PACKET_SIZE]> {
    let mut framed = Vec::new();
    let mut remaining = apdu;
    let mut sequence: u16 = 0;

    while sequence == 0 || !remaining.is_empty() {
        let mut packet = [0u8; HID_PACKET_SIZE];
        packet[0..2].copy_from_slice(&HID_CHANNEL.to_be_bytes());
        packet[2] = HID_TAG_APDU;
        packet[3..5].copy_from_slice(&sequence.to_be_bytes());

        let header_len = if sequence == 0 {
            packet[5..7].copy_from_slice(&(apdu.len() as u16).to_be_bytes());
            7
        } else {
            5
        };

        let chunk_len = remaining.len().min(HID_PACKET_SIZE - header_len);
        packet[header_len..header_len + chunk_len].copy_from_slice(&remaining[..chunk_len]);
        remaining = &remaining[chunk_len..];
        framed.push(packet);
        sequence += 1;
    }

    framed
}

#[cfg(feature = "hardware-wallet")]
struct LedgerTransport {
    device: hidapi::HidDevice,
    device_count: usize,
    read_timeout_ms: i32,
}

#[cfg(feature = "hardware-wallet")]
impl LedgerTransport {
    fn connect() -> Result<Self> {
        Self::connect_with_timeout(std::time::Duration::from_secs(15))
    }

    fn connect_with_timeout(timeout: std::time::Duration) -> Result<Self> {
        let api = hidapi::HidApi::new().context("Failed to initialize HID API")?;
        let devices = api
            .device_list()
            .filter(|info| info.vendor_id() == LEDGER_VENDOR_ID)
            .collect::<Vec<_>>();

        if devices.is_empty() {
            anyhow::bail!(
                "No Ledger device detected. Connect it, unlock it, and open the Stellar app."
            );
        }

        let device = devices[0]
            .open_device(&api)
            .context("Failed to open Ledger HID device")?;

        Ok(Self {
            device,
            device_count: devices.len(),
            read_timeout_ms: timeout.as_millis().clamp(500, 60_000) as i32,
        })
    }

    fn exchange(&self, apdu: &[u8]) -> Result<Vec<u8>> {
        for packet in frame_apdu_for_hid(apdu) {
            self.device
                .write(&packet)
                .context("Failed to write APDU packet to Ledger")?;
        }

        let mut response = Vec::new();
        let mut expected_len: Option<usize> = None;
        let mut sequence: u16 = 0;

        loop {
            let mut packet = [0u8; HID_PACKET_SIZE];
            let read = self
                .device
                .read_timeout(&mut packet, self.read_timeout_ms)
                .with_context(|| {
                    format!(
                        "Timed out waiting for Ledger response after {} ms",
                        self.read_timeout_ms
                    )
                })?;

            if read < 5 {
                anyhow::bail!("Received short HID response from Ledger");
            }
            if packet[0..2] != HID_CHANNEL.to_be_bytes() || packet[2] != HID_TAG_APDU {
                anyhow::bail!("Received invalid Ledger HID framing");
            }

            let packet_sequence = u16::from_be_bytes([packet[3], packet[4]]);
            if packet_sequence != sequence {
                anyhow::bail!("Ledger response sequence mismatch");
            }

            let start = if sequence == 0 {
                let total_len = u16::from_be_bytes([packet[5], packet[6]]) as usize;
                expected_len = Some(total_len);
                7
            } else {
                5
            };

            response.extend_from_slice(&packet[start..read]);

            if let Some(total) = expected_len {
                if response.len() >= total {
                    response.truncate(total);
                    break;
                }
            }

            sequence += 1;
        }

        check_apdu_status(&response)
    }

    fn get_public_key(&self, hd_path: &str) -> Result<String> {
        let path_bytes = encode_hd_path(hd_path)?;
        let apdu = build_apdu(CLA_STELLAR, INS_GET_PUBLIC_KEY, 0x01, 0x00, &path_bytes);
        let response = self.exchange(&apdu)?;
        let public_key_bytes = extract_public_key_bytes(&response)?;
        Ok(stellar_strkey::ed25519::PublicKey(public_key_bytes).to_string())
    }

    fn sign_message(&self, hd_path: &str, message: &[u8]) -> Result<Vec<u8>> {
        let path_bytes = encode_hd_path(hd_path)?;
        let total_chunks = message.chunks(255).count().max(1);
        let mut signature = None;

        for (index, chunk) in message.chunks(255).enumerate() {
            let mut payload = Vec::new();
            if index == 0 {
                payload.extend_from_slice(&path_bytes);
            }
            payload.extend_from_slice(chunk);

            let p1 = if index == 0 { 0x00 } else { 0x80 };
            let p2 = if index + 1 == total_chunks {
                0x00
            } else {
                0x80
            };
            let apdu = build_apdu(CLA_STELLAR, INS_SIGN_TX, p1, p2, &payload);
            let response = self.exchange(&apdu)?;

            if index + 1 == total_chunks {
                signature = Some(extract_signature_bytes(&response)?);
            }
        }

        signature.ok_or_else(|| anyhow::anyhow!("Ledger did not return a signature"))
    }
}

#[cfg(feature = "hardware-wallet")]
struct TrezorTransport;

#[cfg(feature = "hardware-wallet")]
impl TrezorTransport {
    fn connect_info(hd_path: &str) -> Result<HardwareWalletInfo> {
        let device_count = trezor_client::find_devices(false).len();
        if device_count == 0 {
            anyhow::bail!("No Trezor device detected. Connect and unlock your Trezor.");
        }

        let stellar_address = Self::get_public_key(hd_path).ok();
        Ok(HardwareWalletInfo {
            kind: HardwareWalletKind::Trezor,
            device_count,
            stellar_address,
            hd_path: hd_path.to_string(),
        })
    }

    fn status() -> Result<String> {
        let mut trezor = Self::connect()?;
        trezor
            .init_device(None)
            .context("Failed to initialize Trezor session")?;
        let features = trezor
            .features()
            .ok_or_else(|| anyhow::anyhow!("Trezor did not return feature information"))?;
        Ok(format!(
            "Trezor: model {}, firmware {}.{}.{}, label '{}'",
            features.model(),
            features.major_version(),
            features.minor_version(),
            features.patch_version(),
            features.label()
        ))
    }

    fn get_public_key(hd_path: &str) -> Result<String> {
        let mut trezor = Self::connect()?;
        trezor
            .init_device(None)
            .context("Failed to initialize Trezor session")?;

        let mut request = trezor_client::protos::StellarGetAddress::new();
        request.address_n = parse_hd_path(hd_path)?;
        request.set_show_display(false);
        request.set_chunkify(false);

        let response = trezor.call(
            request,
            Box::new(|_, message: trezor_client::protos::StellarAddress| {
                Ok(message.address().to_string())
            }),
        )?;
        let address = trezor_client::client::handle_interaction(response)
            .context("Trezor did not return a Stellar address")?;
        crate::utils::config::validate_public_key(&address)?;
        Ok(address)
    }

    fn sign_transaction(
        hd_path: &str,
        transaction: &[u8],
        network_passphrase: &str,
    ) -> Result<Vec<u8>> {
        // Trezor's Stellar protocol has no "raw envelope" field. Signing requires
        // decomposing the transaction into a `StellarSignTx` header followed by one
        // protobuf message per operation (`StellarPaymentOp`, `StellarCreateAccountOp`,
        // …), which starforge does not build yet. Refuse clearly rather than sending a
        // request the device is guaranteed to reject.
        //
        // Validate the envelope's own addressing first, then fail on the real
        // limitation — without touching the device. There is no point opening
        // a session just to reject the request afterwards, and doing it this
        // way keeps the "unsupported envelope" path exercisable in CI, where
        // no physical Trezor is ever attached.
        parse_hd_path(hd_path)?;
        let _ = (transaction, network_passphrase);
        anyhow::bail!(
            "Trezor transaction signing is not supported yet.\n\
             The device requires per-operation messages rather than a raw XDR envelope.\n\
             Use a Ledger device, or sign with a software wallet: starforge wallet sign <name>."
        )
    }

    fn connect() -> Result<trezor_client::Trezor> {
        let mut devices = trezor_client::find_devices(false);
        match devices.len() {
            0 => anyhow::bail!("No Trezor device detected. Connect and unlock your Trezor."),
            1 => devices
                .remove(0)
                .connect()
                .context("Failed to connect to Trezor over WebUSB"),
            count => anyhow::bail!(
                "Found {} Trezor devices. Disconnect extras and retry.",
                count
            ),
        }
    }
}

/// Coarse outcome of a Ledger APDU status word, used to turn raw device
/// responses into actionable errors for approval, rejection, and unsupported
/// (out-of-date app / wrong envelope) cases.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ApduOutcome {
    Approved,
    Rejected,
    Unsupported,
    Other,
}

fn classify_status_word(status: [u8; 2]) -> ApduOutcome {
    match status {
        SW_OK => ApduOutcome::Approved,
        // 0x6985 = conditions of use not satisfied, 0x6982 = security status not
        // satisfied: both are raised when the user declines the on-device prompt.
        [0x69, 0x85] | [0x69, 0x82] => ApduOutcome::Rejected,
        // 0x6D00 = INS not supported, 0x6E00 = CLA not supported, 0x6A81 = function
        // not supported: the device understood the envelope but can't service it,
        // typically because the Stellar app is outdated or the wrong app is open.
        [0x6d, 0x00] | [0x6e, 0x00] | [0x6a, 0x81] => ApduOutcome::Unsupported,
        _ => ApduOutcome::Other,
    }
}

/// Validate a reassembled Ledger APDU response, stripping and interpreting the
/// trailing status word. Returns the response payload on approval, or a
/// descriptive error for rejection, unsupported envelopes, and other failures.
fn check_apdu_status(response: &[u8]) -> Result<Vec<u8>> {
    if response.len() < 2 {
        anyhow::bail!("Ledger response did not include a status word");
    }
    let status = [response[response.len() - 2], response[response.len() - 1]];
    let payload = response[..response.len() - 2].to_vec();

    match classify_status_word(status) {
        ApduOutcome::Approved => Ok(payload),
        ApduOutcome::Rejected => anyhow::bail!(
            "Ledger rejected the request on-device (status {:02x}{:02x}): the user denied the prompt",
            status[0],
            status[1]
        ),
        ApduOutcome::Unsupported => anyhow::bail!(
            "Ledger does not support this request envelope (status {:02x}{:02x}): the Stellar app may be outdated or the wrong app is open",
            status[0],
            status[1]
        ),
        ApduOutcome::Other => anyhow::bail!(
            "Ledger returned APDU status {:02x}{:02x}",
            status[0],
            status[1]
        ),
    }
}

fn extract_public_key_bytes(response: &[u8]) -> Result<[u8; 32]> {
    if response.len() >= 32 {
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&response[..32]);
        return Ok(bytes);
    }
    anyhow::bail!("Ledger public-key response was too short")
}

fn extract_signature_bytes(response: &[u8]) -> Result<Vec<u8>> {
    if response.len() >= 64 {
        return Ok(response[..64].to_vec());
    }
    anyhow::bail!("Ledger signature response was too short")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hd_path_constant_is_valid() {
        assert_eq!(STELLAR_HD_PATH, "m/44'/148'/0'");
    }

    #[test]
    fn kind_display() {
        assert_eq!(HardwareWalletKind::Ledger.to_string(), "Ledger");
        assert_eq!(HardwareWalletKind::Trezor.to_string(), "Trezor");
    }

    #[test]
    fn parses_hd_path_segments() {
        let parsed = parse_hd_path("m/44'/148'/0'").unwrap();
        assert_eq!(parsed, vec![0x8000_002c, 0x8000_0094, 0x8000_0000]);
    }

    #[test]
    fn encodes_hd_path_prefix_and_bytes() {
        let encoded = encode_hd_path("m/44'/148'/0'").unwrap();
        assert_eq!(encoded[0], 3);
        assert_eq!(&encoded[1..5], &0x8000_002c_u32.to_be_bytes());
    }

    #[test]
    fn builds_apdu_header() {
        let apdu = build_apdu(0xE0, 0x02, 0x01, 0x00, &[1, 2, 3]);
        assert_eq!(apdu, vec![0xE0, 0x02, 0x01, 0x00, 3, 1, 2, 3]);
    }

    #[test]
    fn frames_large_apdu_into_multiple_hid_packets() {
        let apdu = vec![0xAB; 120];
        let packets = frame_apdu_for_hid(&apdu);
        assert!(packets.len() >= 2);
        assert_eq!(packets[0][0..2], HID_CHANNEL.to_be_bytes());
        assert_eq!(packets[0][2], HID_TAG_APDU);
    }

    #[test]
    fn extracts_public_key_from_recorded_vector() {
        let response = [7u8; 32];
        let key = extract_public_key_bytes(&response).unwrap();
        assert_eq!(key, [7u8; 32]);
    }

    #[test]
    fn extracts_signature_from_recorded_vector() {
        let response = vec![9u8; 64];
        let signature = extract_signature_bytes(&response).unwrap();
        assert_eq!(signature.len(), 64);
        assert!(signature.iter().all(|byte| *byte == 9));
    }

    #[test]
    fn map_signing_error_includes_recovery_guidance() {
        let err = map_signing_error(
            anyhow::anyhow!("Timed out waiting for Ledger response"),
            HardwareWalletKind::Ledger,
        );
        let message = err.to_string().to_lowercase();
        assert!(message.contains("recovery") || message.contains("retry"));
        assert!(message.contains("timeout") || message.contains("ledger"));
    }

    // -- APDU status-word interpretation: approval / rejection / unsupported --

    #[test]
    fn apdu_status_approved_returns_payload() {
        let response = vec![1, 2, 3, 0x90, 0x00];
        let payload = check_apdu_status(&response).unwrap();
        assert_eq!(payload, vec![1, 2, 3]);
    }

    #[test]
    fn apdu_status_conditions_not_satisfied_is_a_rejection() {
        let err = check_apdu_status(&[0x69, 0x85]).unwrap_err();
        let message = err.to_string().to_lowercase();
        assert!(message.contains("reject"));
    }

    #[test]
    fn apdu_status_security_not_satisfied_is_a_rejection() {
        let err = check_apdu_status(&[0x69, 0x82]).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("reject"));
    }

    #[test]
    fn apdu_status_ins_not_supported_is_an_unsupported_envelope() {
        let err = check_apdu_status(&[0x6d, 0x00]).unwrap_err();
        let message = err.to_string().to_lowercase();
        assert!(message.contains("does not support"));
    }

    #[test]
    fn apdu_status_cla_not_supported_is_an_unsupported_envelope() {
        let err = check_apdu_status(&[0x6e, 0x00]).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("does not support"));
    }

    #[test]
    fn apdu_status_unrecognized_code_falls_back_to_generic_error() {
        let err = check_apdu_status(&[0x6f, 0xff]).unwrap_err();
        assert!(err.to_string().contains("6fff"));
    }

    #[test]
    fn apdu_status_missing_status_word_is_a_failure() {
        let err = check_apdu_status(&[0x90]).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("status word"));
    }

    // -- HD path boundary / failure cases (malformed request envelopes) --

    #[test]
    fn parse_hd_path_rejects_empty_segment() {
        assert!(parse_hd_path("m//148'").is_err());
    }

    #[test]
    fn parse_hd_path_rejects_out_of_range_index() {
        assert!(parse_hd_path("m/2147483648").is_err());
    }

    #[test]
    fn parse_hd_path_rejects_empty_path() {
        assert!(parse_hd_path("m/").is_err());
    }

    // -- map_signing_error guidance for rejection / unsupported / disconnect --

    #[test]
    fn map_signing_error_reports_rejection_guidance() {
        let err = map_signing_error(
            anyhow::anyhow!(
                "Ledger rejected the request on-device (status 6985): the user denied the prompt"
            ),
            HardwareWalletKind::Ledger,
        );
        let message = err.to_string().to_lowercase();
        assert!(message.contains("approve"));
    }

    #[test]
    fn map_signing_error_reports_unsupported_envelope_guidance() {
        let err = map_signing_error(
            anyhow::anyhow!(
                "Ledger does not support this request envelope (status 6d00): the Stellar app may be outdated"
            ),
            HardwareWalletKind::Ledger,
        );
        let message = err.to_string().to_lowercase();
        assert!(message.contains("update") && message.contains("app"));
    }

    #[test]
    fn map_signing_error_reports_disconnect_guidance() {
        let err = map_signing_error(
            anyhow::anyhow!(
                "No Ledger device detected. Connect it, unlock it, and open the Stellar app."
            ),
            HardwareWalletKind::Ledger,
        );
        let message = err.to_string().to_lowercase();
        assert!(message.contains("connect"));
    }

    // -- Disconnect path: exercises the real optional backend against an absent
    // device. These run whenever the crate is compiled with `--features
    // hardware-wallet` (as CI now does) and are deterministic on machines/runners
    // without a physical Ledger or Trezor attached — unlike the `#[ignore]`d test
    // below, they require no hardware and no opt-in flag.

    #[cfg(feature = "hardware-wallet")]
    #[test]
    fn ledger_connect_without_device_reports_disconnect() {
        let err = connect(HardwareWalletKind::Ledger).unwrap_err();
        let message = err.to_string().to_lowercase();
        assert!(message.contains("ledger") || message.contains("device"));
    }

    #[cfg(feature = "hardware-wallet")]
    #[test]
    fn trezor_connect_without_device_reports_disconnect() {
        let err = connect(HardwareWalletKind::Trezor).unwrap_err();
        let message = err.to_string().to_lowercase();
        assert!(message.contains("trezor") || message.contains("device"));
    }

    #[cfg(feature = "hardware-wallet")]
    #[test]
    fn ledger_device_status_without_device_is_graceful() {
        let err = device_status(HardwareWalletKind::Ledger).unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    #[cfg(feature = "hardware-wallet")]
    #[test]
    fn trezor_device_status_without_device_is_graceful() {
        let err = device_status(HardwareWalletKind::Trezor).unwrap_err();
        assert!(!err.to_string().is_empty());
    }

    // Trezor's Stellar protocol needs the transaction decomposed into
    // structured fields rather than accepting a raw XDR envelope, which this
    // build does not yet do — it must reject the request up front instead of
    // hanging or mis-signing.
    #[cfg(feature = "hardware-wallet")]
    #[test]
    fn trezor_sign_transaction_reports_unsupported_envelope() {
        let err = sign_transaction(
            HardwareWalletKind::Trezor,
            STELLAR_HD_PATH,
            b"fake-xdr-envelope",
            "Test SDF Network ; September 2015",
        )
        .unwrap_err();
        let message = err.to_string().to_lowercase();
        assert!(message.contains("not supported"));
    }

    #[cfg(feature = "hardware-wallet")]
    #[test]
    fn trezor_sign_transaction_rejects_malformed_hd_path_before_reporting_unsupported() {
        let err =
            sign_transaction(HardwareWalletKind::Trezor, "not-a-path", b"data", "").unwrap_err();
        assert!(!err.to_string().to_lowercase().contains("not supported"));
    }

    #[cfg(feature = "hardware-wallet")]
    #[test]
    #[ignore = "requires a connected Ledger with the Stellar app open"]
    fn ledger_integration_requires_device() {
        let _ = connect(HardwareWalletKind::Ledger);
    }
}
