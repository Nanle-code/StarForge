//! Integration coverage for the `--signer browser` localhost handoff.
//!
//! Nothing here needs a browser or a real wallet. The tests drive the public
//! handoff API with a mocked [`WalletSigner`] and act as a mock browser wallet
//! over a raw TCP socket, so the full request/response/validation path runs in
//! CI without Freighter.

use base64::{engine::general_purpose, Engine as _};
use starforge::utils::browser_signer::{
    sign_with, BrowserSignRequest, HandoffError, HandoffServer, MockWalletSigner, NonceGuard,
    WalletSigner,
};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::time::Duration;

fn unsigned_envelope() -> BrowserSignRequest {
    BrowserSignRequest::new(
        general_purpose::STANDARD.encode(b"unsigned-envelope-bytes"),
        "testnet",
    )
}

/// A stand-in for a Stellar Wallets Kit wallet: it returns a canned signed
/// envelope without any browser involvement.
struct FakeBrowserWallet {
    signed_xdr: String,
}

impl WalletSigner for FakeBrowserWallet {
    fn sign(&self, _request: &BrowserSignRequest) -> Result<String, HandoffError> {
        Ok(self.signed_xdr.clone())
    }
}

/// A stand-in for a user who closes the wallet without approving.
struct DecliningBrowserWallet;

impl WalletSigner for DecliningBrowserWallet {
    fn sign(&self, _request: &BrowserSignRequest) -> Result<String, HandoffError> {
        Err(HandoffError::WalletRejected(
            "user declined in the extension".to_string(),
        ))
    }
}

#[test]
fn mocked_wallet_returns_a_validated_signed_xdr() {
    let request = unsigned_envelope();
    let signed = general_purpose::STANDARD.encode(b"signed-envelope-bytes");
    let wallet = FakeBrowserWallet {
        signed_xdr: signed.clone(),
    };

    assert_eq!(sign_with(&wallet, &request).unwrap(), signed);
    assert_eq!(
        sign_with(&MockWalletSigner::returning(signed.clone()), &request).unwrap(),
        signed
    );
}

#[test]
fn mocked_wallet_failures_are_propagated() {
    let request = unsigned_envelope();

    assert!(matches!(
        sign_with(&DecliningBrowserWallet, &request),
        Err(HandoffError::WalletRejected(_))
    ));
    assert!(matches!(
        sign_with(&MockWalletSigner::timing_out(), &request),
        Err(HandoffError::Timeout(_))
    ));
    // A wallet that returns non-base64 garbage must not be accepted.
    assert!(matches!(
        sign_with(&MockWalletSigner::returning("not base64!!"), &request),
        Err(HandoffError::InvalidSignature(_))
    ));
}

#[test]
fn handoff_page_and_signature_round_trip_over_tcp() {
    let mut server = HandoffServer::bind(unsigned_envelope(), Duration::from_secs(5))
        .expect("bind handoff server");
    let port = server.port();
    let nonce = server.nonce().to_string();
    let signed = general_purpose::STANDARD.encode(b"signed-envelope-bytes");

    let handle = std::thread::spawn(move || server.wait());

    // The mock wallet fetches the transaction the page would render.
    let mut get = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(
        get,
        "GET /api/transaction?nonce={nonce} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut get_response = String::new();
    get.read_to_string(&mut get_response).unwrap();
    assert!(get_response.contains("200 OK"), "{get_response}");
    assert!(get_response.contains("transactionXdr"), "{get_response}");
    assert!(get_response.contains("networkPassphrase"), "{get_response}");

    // Then it posts the signed envelope back, like the page does.
    let payload = serde_json::json!({ "nonce": &nonce, "signedXdr": &signed }).to_string();
    let mut post = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(
        post,
        "POST /api/sign-result?nonce={nonce} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    )
    .unwrap();
    let mut post_response = String::new();
    post.read_to_string(&mut post_response).unwrap();
    assert!(post_response.contains("200 OK"), "{post_response}");

    let returned = handle
        .join()
        .unwrap()
        .expect("handoff returns the signature");
    assert_eq!(returned, signed);
}

#[test]
fn off_loopback_host_header_cannot_read_the_page() {
    let mut server = HandoffServer::bind(unsigned_envelope(), Duration::from_secs(5))
        .expect("bind handoff server");
    let port = server.port();
    let nonce = server.nonce().to_string();
    let signed = general_purpose::STANDARD.encode(b"signed-envelope-bytes");

    let handle = std::thread::spawn(move || server.wait());

    // A rebound hostname cannot fetch the page even with the right nonce.
    let mut rebound = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(
        rebound,
        "GET /?nonce={nonce} HTTP/1.1\r\nHost: attacker.example\r\nConnection: close\r\n\r\n"
    )
    .unwrap();
    let mut rebound_response = String::new();
    rebound.read_to_string(&mut rebound_response).unwrap();
    assert!(rebound_response.contains("403"), "{rebound_response}");

    // The legitimate handoff still completes afterwards.
    let payload = serde_json::json!({ "nonce": &nonce, "signedXdr": &signed }).to_string();
    let mut post = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(
        post,
        "POST /api/sign-result?nonce={nonce} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    )
    .unwrap();
    let mut post_response = String::new();
    post.read_to_string(&mut post_response).unwrap();
    assert!(post_response.contains("200 OK"), "{post_response}");

    assert_eq!(handle.join().unwrap().unwrap(), signed);
}

#[test]
fn handoff_times_out_without_a_wallet() {
    let mut server =
        HandoffServer::bind(unsigned_envelope(), Duration::from_millis(150)).expect("bind");
    let err = server.wait().unwrap_err();
    assert!(matches!(err, HandoffError::Timeout(_)));
}

#[test]
fn nonce_guard_rejects_reuse() {
    let mut guard = NonceGuard::from_nonce("abc123");
    assert!(!guard.is_consumed());
    guard.verify_and_consume("abc123").unwrap();
    assert!(guard.is_consumed());
    assert!(matches!(
        guard.verify_and_consume("abc123"),
        Err(HandoffError::NonceAlreadyUsed)
    ));
}
