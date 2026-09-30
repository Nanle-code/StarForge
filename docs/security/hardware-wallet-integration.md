# Hardware Wallet Integration

## Overview
StarForge supports hardware wallet signing for deploy and contract invoke operations through Ledger and Trezor devices. Hardware wallets provide enhanced security by keeping private keys isolated on dedicated hardware that never exposes secrets to the host system.

## Supported Devices

### Ledger
- **Status**: Fully supported for transaction signing
- **Requirements**: Ledger device with Stellar app installed
- **Connection**: USB HID transport
- **HD Path**: `m/44'/148'/0'` (configurable)

### Trezor
- **Status**: Address derivation supported; transaction signing planned
- **Requirements**: Trezor One or Trezor Model T
- **Connection**: WebUSB transport
- **Limitation**: Raw XDR envelope signing not yet implemented

## Security Properties

### Secrets Never Leave the Device
- Private keys are generated and stored exclusively on the hardware device
- Signing operations execute on the device itself
- StarForge only receives the final signature, never the private key

### User Confirmation Required
Every transaction requires physical confirmation on the device:
1. Device displays transaction details
2. User reviews and approves/rejects on-screen
3. Device signs only after explicit user approval

### No Persistence
Hardware wallet operations are stateless:
- No private keys or secrets are saved to disk
- Each operation requires fresh device communication
- Watch-only wallets store only the public key

## Usage

### Connect and Verify Device

Check device connectivity and retrieve the Stellar address:

```bash
# Connect to Ledger with default timeout (30s)
starforge wallet connect ledger

# Connect with custom timeout
starforge wallet connect trezor --timeout 60s

# Get address without full connect flow
starforge wallet hw-address ledger

# Check device status
starforge wallet hw-status ledger
```

Output example:
```
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
  Hardware Wallet â€" Connect
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

Step 1/3: Initializing HID subsystem for Ledger…
Step 2/3: Reading Stellar address at m/44'/148'/0'…
Step 3/3: Device ready

Public Key: GABCDEF...XYZ
HD Path: m/44'/148'/0'
Device Count: 1
```

### Import Watch-Only Wallet

Create a watch-only wallet from a hardware device (no private key stored locally):

```bash
# Import from Ledger with default path
starforge wallet import ledger-main --hardware ledger

# Import with custom HD path
starforge wallet import ledger-account-5 --hardware ledger --hd-path "m/44'/148'/5'"

# Import from Trezor
starforge wallet import trezor-main --hardware trezor
```

This creates a wallet entry with the public key but no local secret key. All signing operations will require the hardware device.

### Deploy with Hardware Wallet

```bash
# Deploy using hardware wallet (experimental - hidden flag)
starforge deploy --wasm contract.wasm --hardware ledger

# With custom HD path
starforge deploy --wasm contract.wasm --hardware ledger --hd-path "m/44'/148'/1'"
```

**Note**: Hardware wallet flags for deploy/invoke are currently hidden pending full integration. Use wallet-level signing in the meantime.

### Sign Messages

Sign arbitrary messages with hardware wallet:

```bash
starforge wallet sign --message "Hello Stellar" --hardware ledger
```

### Multi-Signature with Hardware Wallets

Combine hardware wallet signing with multi-sig workflows:

```bash
# Create multisig account with hardware wallet as signer
starforge wallet multisig create shared-account --threshold 2 --signers ledger-main,alice

# Sign multisig transaction with hardware wallet
starforge wallet multisig sign shared-account --transaction tx.json --hardware ledger
```

## Device Detection

StarForge automatically detects connected hardware wallets:

```bash
starforge diagnostics
```

This checks:
- Device visibility on USB bus
- Correct app running (Stellar app for Ledger)
- Communication channel readiness

## Error Handling and Recovery

### Device Not Found
**Error**: `No Ledger device detected. Connect it, unlock it, and open the Stellar app.`

**Recovery**:
1. Connect device via USB
2. Enter PIN to unlock device
3. Navigate to and open the Stellar app
4. Retry the command

### User Rejected Request
**Error**: `Ledger rejected the request on-device (status 6985): the user denied the prompt`

**Recovery**:
- Review transaction details on device screen
- Approve the transaction if correct
- Reject if details don't match expectations

### Timeout
**Error**: `Timed out waiting for Ledger response after 15000 ms`

**Recovery**:
1. Ensure device is unlocked
2. Verify Stellar app is open and idle
3. Approve the on-device prompt
4. Increase timeout if needed: `--timeout 60s`

### Unsupported Operation
**Error**: `Ledger does not support this request envelope (status 6d00): the Stellar app may be outdated`

**Recovery**:
1. Update the Stellar app via Ledger Live
2. Verify the correct app is open
3. If using Trezor for transaction signing, switch to Ledger (not yet supported)

### APDU Status Errors
**Error**: `Ledger returned APDU status 6fxx`

**Recovery**:
- Close other wallet applications (Ledger Live, etc.)
- Reopen the Stellar app on the device
- Retry the operation

## HD Derivation Paths

### Default Path
`m/44'/148'/0'` - Standard Stellar BIP-44 path, account index 0

### Custom Paths
Specify alternate accounts or advanced setups:

```bash
# Account index 1
--hd-path "m/44'/148'/1'"

# Account index 5
--hd-path "m/44'/148'/5'"
```

**Caution**: Non-standard paths may not be compatible with other Stellar wallets. Stick to `m/44'/148'/<n>'` format.

## CI/CD Integration

### Mock Hardware Wallet

For automated testing without physical devices, use the mock feature:

```toml
# Cargo.toml feature flag
[features]
mock-hardware-wallet = []
```

Activate in CI:

```bash
cargo build --features mock-hardware-wallet
cargo test --features mock-hardware-wallet
```

### Mock Configuration

Tests can configure mock behavior:

```rust
use starforge::utils::hardware_wallet_mock::{set_mock_config, MockConfig};

// Simulate device disconnection
set_mock_config(MockConfig {
    simulate_disconnect: true,
    ..Default::default()
});

// Simulate user rejection
set_mock_config(MockConfig {
    simulate_rejection: true,
    ..Default::default()
});

// Provide custom signature
set_mock_config(MockConfig {
    mock_signature: Some(vec![0xFF; 64]),
    ..Default::default()
});
```

### Fallback Signing

In CI without mock enabled, hardware wallet commands gracefully fail with clear guidance:

```
Hardware wallet support is disabled in this build.
Rebuild with `cargo build --features hardware-wallet` to enable Ledger detection.
```

Use software wallet signing as fallback:

```bash
# Instead of: starforge deploy --hardware ledger
starforge wallet create deployer --encrypt
starforge deploy --wallet deployer
```

## Advanced Topics

### Multiple Devices

If multiple devices of the same type are connected:

**Ledger**: Uses the first detected device
**Trezor**: Fails with error requesting you disconnect extras

**Workaround**: Connect only one device at a time for signing operations.

### Device Firmware Updates

Before updating device firmware:
1. Verify current wallet balances
2. Confirm backup recovery phrase is safely stored
3. Update firmware following manufacturer instructions
4. Reinstall Stellar app after firmware update
5. Verify address matches before transacting

### Transport Protocols

**Ledger**: HID over USB (cross-platform, no drivers required on most systems)
**Trezor**: WebUSB (requires browser-based transport for Node.js)

## Comparison: Hardware vs Software Wallets

| Feature | Hardware Wallet | Encrypted Software Wallet | Plaintext Software Wallet |
|---------|----------------|---------------------------|--------------------------|
| Private key location | Isolated hardware | Disk (encrypted at rest) | Disk (plaintext) |
| Key exposure risk | None | Decrypted in memory during signing | Always in memory |
| User confirmation | Physical device button | Passphrase entry | None |
| Backup method | Recovery phrase | Encrypted file + passphrase | File copy |
| Best for | Production, high-value | Development, staging | Local testing only |
| Cost | ~$60-150 device | Free | Free |

## Best Practices

### Production Deployments
- âœ… Use hardware wallets for mainnet deployer accounts
- âœ… Combine with multi-sig for critical operations
- âœ… Keep firmware updated
- âœ… Store recovery phrase offline (not in password manager)

### Development Workflows
- âœ… Use encrypted software wallets for testnet
- âœ… Reserve hardware wallets for mainnet-bound accounts
- âœ… Test transaction flows on testnet before mainnet signing

### Multi-Signature
- âœ… Distribute signers across hardware and software wallets
- âœ… Require hardware approval for high thresholds (mainnet, high-value)
- âœ… Use software wallets for low-risk signers (monitoring, read-only)

## Troubleshooting

### Linux Permissions

If device is detected but not accessible:

```bash
# Create udev rule for Ledger
echo 'SUBSYSTEMS=="usb", ATTRS{idVendor}=="2c97", MODE="0660", GROUP="plugdev"' | \
  sudo tee /etc/udev/rules.d/20-ledger.rules

# Reload udev rules
sudo udevadm control --reload-rules
sudo udevadm trigger

# Add user to plugdev group
sudo usermod -aG plugdev $USER

# Log out and back in for group change to take effect
```

### macOS Security Prompts

First connection may prompt for input monitoring permission. Grant access in:
**System Preferences → Security & Privacy → Privacy → Input Monitoring**

### Windows Driver Issues

If device not recognized:
1. Install Ledger Live or Trezor Suite (includes drivers)
2. Verify device appears in Device Manager
3. Restart StarForge after driver installation

## Roadmap

### Implemented (v0.x)
- [x] Ledger connection and address derivation
- [x] Ledger transaction signing
- [x] Trezor connection and address derivation
- [x] Device detection and error mapping
- [x] Mock interface for CI testing
- [x] Hardware wallet watch-only import

### Planned (v1.x)
- [ ] Trezor transaction signing (requires XDR decomposition)
- [ ] Blind signing warnings for complex operations
- [ ] Device app version detection and upgrade prompts
- [ ] Multiple account management UI
- [ ] Hardware wallet integration in deploy/invoke (unhide flags)

## References

- [Ledger Stellar App](https://github.com/LedgerHQ/app-stellar)
- [Trezor Stellar Support](https://wiki.trezor.io/Stellar_(XLM))
- [BIP-44: Multi-Account Hierarchy](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki)
- [SLIP-0010: Universal HD Derivation](https://github.com/satoshilabs/slips/blob/master/slip-0010.md)
- [Stellar SEP-0005: Key Derivation](https://github.com/stellar/stellar-protocol/blob/master/ecosystem/sep-0005.md)
