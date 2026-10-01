# Offline multisig cosigning

StarForge proposals can be exchanged with an air-gapped cosigner as JSON files
or as text encoded into a QR transport by an external transfer tool. The
exported proposal contains no private key material and removes any signatures
already present, so each signer produces a separate partial-signature payload.

```bash
starforge multisig export proposal.json --output unsigned-payload.json
starforge multisig import unsigned-payload.json --output airgapped-proposal.json \
  --network testnet --hash <payload-hash>
starforge multisig sign airgapped-proposal.json --wallet offline-signer
starforge multisig export airgapped-proposal.json --output partial-signature.json
```

The payload hash binds the proposal ID, network, and transaction XDR. Import
and signing reject a payload whose stored digest no longer matches those
fields. When a network or expected hash is supplied to `import`, it must also
match exactly; this prevents a payload copied across networks or replaced
between QR/file transfers from being signed accidentally.

Treat QR codes and removable media as untrusted transport. Display the
network, payload hash, transaction details, and signer set on the air-gapped
device before authorizing. Transfer only the unsigned or partial-signature
JSON, never a wallet secret or seed phrase, and verify the returned hash again
before submitting the completed proposal.
