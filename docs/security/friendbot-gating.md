# Friendbot Faucet Security Gating

## 1. Overview
StarForge enforces strict security gating around Friendbot faucet requests. Friendbot is prevented from being used against mainnet or custom networks configured with the Public Global Stellar Network passphrase.

## 2. Gating Rules
1. **Mainnet Gating**: Any attempt to run `starforge wallet fund` against `mainnet` is halted with a security diagnostic.
2. **Passphrase Verification**: Custom networks with public/production passphrases (`Public Global Stellar Network ; September 2015`) cannot execute Friendbot requests.
3. **Explicit Faucet URL**: Custom test networks must supply a valid `friendbot_url` to prevent unintended fallback routing.
