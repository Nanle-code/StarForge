/// Mock hardware wallet interface for CI testing without physical devices.
///
/// This module provides a mockable interface that simulates hardware wallet
/// operations for automated testing environments where physical devices are
/// not available.

use anyhow::Result;
use std::sync::{Arc, Mutex};

/// Mock hardware wallet behavior configuration
#[derive(Debug, Clone, Default)]
pub struct MockConfig {
    /// Simulate device connection failure
    pub simulate_disconnect: bool,
    /// Simulate user rejection on device
    pub simulate_rejection: bool,
    /// Simulate unsupported operation
    pub simulate_unsupported: bool,
    /// Pre-configured mock signature to return
    pub mock_signature: Option<Vec<u8>>,
    /// Pre-configured mock address to return
    pub mock_address: Option<String>,
}

lazy_static::lazy_static! {
    static ref MOCK_STATE: Arc<Mutex<MockConfig>> = Arc::new(Mutex::new(MockConfig::default()));
}

/// Set the mock configuration for testing
pub fn set_mock_config(config: MockConfig) {
    let mut state = MOCK_STATE.lock().unwrap();
    *state = config;
}

/// Reset mock configuration to defaults
pub fn reset_mock_config() {
    set_mock_config(MockConfig::default());
}

/// Get current mock configuration
pub fn get_mock_config() -> MockConfig {
    MOCK_STATE.lock().unwrap().clone()
}

/// Mock hardware wallet signing operation
pub fn mock_sign_transaction(
    _hd_path: &str,
    _transaction: &[u8],
    _network_passphrase: &str,
) -> Result<Vec<u8>> {
    let config = get_mock_config();

    if config.simulate_disconnect {
        anyhow::bail!("Mock: No hardware device detected");
    }

    if config.simulate_rejection {
        anyhow::bail!("Mock: User rejected the request on device");
    }

    if config.simulate_unsupported {
        anyhow::bail!("Mock: Operation not supported by device");
    }

    if let Some(sig) = config.mock_signature {
        return Ok(sig);
    }

    // Default: return a valid-looking 64-byte signature
    Ok(vec![0xAB; 64])
}

/// Mock hardware wallet address retrieval
pub fn mock_get_address(_hd_path: &str) -> Result<String> {
    let config = get_mock_config();

    if config.simulate_disconnect {
        anyhow::bail!("Mock: No hardware device detected");
    }

    if let Some(addr) = config.mock_address {
        return Ok(addr);
    }

    // Return a valid-looking Stellar address
    Ok("GAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAWHF".to_string())
}

/// Mock hardware wallet connection check
pub fn mock_check_connection() -> Result<()> {
    let config = get_mock_config();

    if config.simulate_disconnect {
        anyhow::bail!("Mock: No hardware device detected");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mock_default_success() {
        reset_mock_config();
        let result = mock_sign_transaction("m/44'/148'/0'", b"test", "testnet");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 64);
    }

    #[test]
    fn test_mock_disconnect() {
        set_mock_config(MockConfig {
            simulate_disconnect: true,
            ..Default::default()
        });
        let result = mock_sign_transaction("m/44'/148'/0'", b"test", "testnet");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("device"));
        reset_mock_config();
    }

    #[test]
    fn test_mock_rejection() {
        set_mock_config(MockConfig {
            simulate_rejection: true,
            ..Default::default()
        });
        let result = mock_sign_transaction("m/44'/148'/0'", b"test", "testnet");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("rejected"));
        reset_mock_config();
    }

    #[test]
    fn test_mock_custom_signature() {
        let custom_sig = vec![0xFF; 64];
        set_mock_config(MockConfig {
            mock_signature: Some(custom_sig.clone()),
            ..Default::default()
        });
        let result = mock_sign_transaction("m/44'/148'/0'", b"test", "testnet");
        assert_eq!(result.unwrap(), custom_sig);
        reset_mock_config();
    }

    #[test]
    fn test_mock_address_retrieval() {
        reset_mock_config();
        let result = mock_get_address("m/44'/148'/0'");
        assert!(result.is_ok());
        assert!(result.unwrap().starts_with('G'));
    }

    #[test]
    fn test_mock_connection_check() {
        reset_mock_config();
        assert!(mock_check_connection().is_ok());

        set_mock_config(MockConfig {
            simulate_disconnect: true,
            ..Default::default()
        });
        assert!(mock_check_connection().is_err());
        reset_mock_config();
    }
}
