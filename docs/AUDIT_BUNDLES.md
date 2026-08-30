# Audit Bundles

`utils::audit_bundle::AuditBundle` provides a versioned JSON envelope for
review evidence: tool versions, artifact checksums, deployment summaries, and
configuration hashes. Values containing private-key material or secret markers
are replaced with `[REDACTED]` before serialization.

Callers may provide a signing key to add an HMAC-SHA256 signature. The
signature is calculated over the redacted unsigned JSON structure, so the
reviewer can verify exactly what was exported.
