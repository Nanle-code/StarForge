# GraphQL Schema Versioning and Deprecation Policy

> Related: [GRAPHQL_GUIDE.md](./GRAPHQL_GUIDE.md) · Issue #321 · Issue #781

This document is the authoritative policy for how StarForge's GraphQL API schema
evolves over time. All contributors making schema changes **must** read and follow
it before opening a PR.

---

## Table of Contents

- [Why a Schema Policy](#why-a-schema-policy)
- [Schema Versioning](#schema-versioning)
- [Deprecation SLA](#deprecation-sla)
- [Deprecation Process Step-by-Step](#deprecation-process-step-by-step)
- [Worked Example: Deprecating and Removing a Field](#worked-example-deprecating-and-removing-a-field)
- [Breaking-Change Checklist for PRs](#breaking-change-checklist-for-prs)
- [CI Lint Enforcement](#ci-lint-enforcement)
- [Approvals Required for Breaking Changes](#approvals-required-for-breaking-changes)
- [Interaction with API Versioning](#interaction-with-api-versioning)
- [FAQ](#faq)

---

## Why a Schema Policy

The StarForge GraphQL API is consumed by:
- The StarForge CLI client libraries (`client.go`, `client.py`, `client.rs`, `client.ts`)
- Third-party integrations and tooling built on the public API

Silent removal of a field breaks callers immediately and without warning. A
published deprecation policy sets clear, enforceable expectations so consumers
can plan upgrades before fields disappear.

---

## Schema Versioning

### How schema versions are declared

StarForge uses a **single versioned schema** served on `/graphql`. There is no
URL-level versioning (`/v1/graphql`, `/v2/graphql`). The schema version is
communicated through:

1. **A `schemaVersion` query field** exposed on the root `Query` type:

   ```graphql
   type Query {
     schemaVersion: String!   # e.g. "2026-10"
     # ... other fields
   }
   ```

   The value is a `YYYY-MM` calendar string, updated whenever a breaking change
   (field removal or type change) ships. Non-breaking additions do not require a
   version bump.

2. **A `X-Schema-Version` response header** set by the GraphQL server so
   clients that do not introspect can still detect the active version.

3. **This document and CHANGELOG entries** — every version bump must have a
   corresponding entry in `CHANGELOG.md` under the `## [Unreleased]` heading,
   moved to a dated release when the version ships.

### When to increment the schema version

| Change type | Version bump required? |
|---|---|
| Add a new field/type/argument (non-breaking) | No |
| Mark a field `@deprecated` | No |
| Change a field's type (even compatible widening) | **Yes** |
| Remove a field that has completed its deprecation window | **Yes** |
| Rename a field | **Yes** (treat as remove + add) |
| Add a non-null argument to an existing field | **Yes** |

---

## Deprecation SLA

A field **must** carry the `@deprecated` directive for a minimum of **90 days**
(approximately one calendar quarter) before it may be removed from the schema.

| Clock starts | When the PR that adds `@deprecated` merges to `master` |
| Earliest removal | 90 days after the deprecation merged |
| Extension required | If any known first-party consumer has not yet migrated |
| Emergency removal | Only for active security vulnerabilities; requires maintainer sign-off and an immediate major version bump |

The 90-day window is enforced by the deprecation record maintained in
[`src/graphql/schema_deprecations.rs`](src/graphql/schema_deprecations.rs).
Each deprecated field has a `deprecated_since` date; the CI lint script
(`scripts/check-graphql-deprecations.sh`) will block removal PRs where the
window has not elapsed.

---

## Deprecation Process Step-by-Step

### 1. Add the `@deprecated` directive (code-first: `#[graphql(deprecation)]`)

Because StarForge uses `async-graphql` (code-first), deprecation is declared
on the resolver with the `deprecation` attribute:

```rust
#[Object]
impl Query {
    /// Get all wallets
    ///
    /// # Deprecated
    /// Use `walletsPaginated` instead, which supports cursor-based pagination.
    #[graphql(deprecation = "Use `walletsPaginated` instead.")]
    async fn wallets(&self) -> Vec<Wallet> {
        // ...
    }

    /// Get wallets with cursor-based pagination (replacement for `wallets`)
    async fn wallets_paginated(
        &self,
        first: Option<i32>,
        after: Option<String>,
    ) -> WalletConnection {
        // ...
    }
}
```

### 2. Register the deprecation in `schema_deprecations.rs`

Add an entry to the static `DEPRECATED_FIELDS` table in
`src/graphql/schema_deprecations.rs`:

```rust
DeprecatedField {
    type_name: "Query",
    field_name: "wallets",
    deprecated_since: "2026-10-01",   // Date this PR merges
    earliest_removal: "2026-12-31",   // deprecated_since + 90 days
    replacement: Some("walletsPaginated"),
    reason: "Use walletsPaginated for cursor-based pagination.",
},
```

### 3. Open the PR

The PR description must include the breaking-change checklist below
(copy it from the section below or from `.github/pull_request_template.md`).

### 4. Notify consumers

Comment on the issue or discussion thread, and update `CHANGELOG.md`:

```markdown
### Deprecated

- `Query.wallets` — deprecated in favour of `Query.walletsPaginated`
  (cursor-based pagination). Will be removed on or after 2026-12-31.
```

### 5. Remove the field (after the SLA window)

After 90 days, open a follow-up PR to:
1. Delete the resolver method and `DeprecatedField` entry.
2. Bump the `schemaVersion` value in `src/graphql/schema.rs`.
3. Add a `### Removed` entry to `CHANGELOG.md`.
4. Re-run CI (the lint script verifies the window elapsed).

---

## Worked Example: Deprecating and Removing a Field

This concrete example walks through the full lifecycle of `Query.wallets` being
replaced by `Query.walletsPaginated`.

### Phase 1 — Deprecation PR (Day 0, e.g. 2026-10-01)

**`src/graphql/resolvers.rs`** — add `deprecation` attribute and replacement:

```rust
/// List all wallets (deprecated — use `walletsPaginated`)
#[graphql(deprecation = "Use `walletsPaginated(first, after)` instead.")]
async fn wallets(&self) -> Vec<Wallet> {
    vec![/* ... */]
}

/// List wallets with cursor-based pagination.
async fn wallets_paginated(
    &self,
    first: Option<i32>,
    after: Option<String>,
) -> Vec<Wallet> {
    // new implementation
    vec![/* ... */]
}
```

**`src/graphql/schema_deprecations.rs`** — register the deprecation:

```rust
DeprecatedField {
    type_name: "Query",
    field_name: "wallets",
    deprecated_since: "2026-10-01",
    earliest_removal: "2026-12-31",
    replacement: Some("walletsPaginated"),
    reason: "Use walletsPaginated for cursor-based pagination.",
},
```

**`CHANGELOG.md`** entry:

```markdown
### Deprecated
- `Query.wallets` — use `walletsPaginated` (cursor pagination). Removal on/after 2026-12-31.
```

### Phase 2 — Consumer Migration (Days 1–90)

Consumers update their queries:

```graphql
# Before
query { wallets { id name balance } }

# After
query { walletsPaginated(first: 20) { id name balance } }
```

### Phase 3 — Removal PR (Day 91+, e.g. 2027-01-15)

**`src/graphql/resolvers.rs`** — remove the deprecated method entirely.

**`src/graphql/schema_deprecations.rs`** — remove the `DeprecatedField` entry.

**`src/graphql/schema.rs`** — bump `SCHEMA_VERSION`:

```rust
pub const SCHEMA_VERSION: &str = "2027-01";  // was "2026-10"
```

**`CHANGELOG.md`**:

```markdown
### Removed
- `Query.wallets` — removed after 90-day deprecation window. Use `walletsPaginated`.
```

**CI result**: the schema-lint job passes because the `earliest_removal` date
(`2026-12-31`) is in the past relative to the removal PR date.

---

## Breaking-Change Checklist for PRs

Copy this checklist into any PR that modifies the GraphQL schema.
It is also embedded in `.github/pull_request_template.md`.

```markdown
### GraphQL Breaking-Change Checklist

<!-- Complete this section for ANY PR that touches src/graphql/ -->

- [ ] **No non-deprecated field is removed** — if a field is removed, confirm
      it carried `@deprecated` / `#[graphql(deprecation)]` for ≥ 90 days.
- [ ] **Deprecation registered** — new deprecations are recorded in
      `src/graphql/schema_deprecations.rs` with correct `deprecated_since`
      and `earliest_removal` dates.
- [ ] **Replacement documented** — the `#[graphql(deprecation)]` reason string
      names the replacement field/type.
- [ ] **Schema version bumped** — `SCHEMA_VERSION` in `src/graphql/schema.rs`
      updated if a field/type is removed or a type signature changed.
- [ ] **CHANGELOG updated** — entry added under `### Deprecated` or
      `### Removed` as appropriate.
- [ ] **CI lint passes** — `scripts/check-graphql-deprecations.sh` exits 0.
- [ ] **Consumer impact assessed** — first-party client libraries
      (`client.go`, `client.py`, `client.rs`, `client.ts`) checked or updated.
```

---

## CI Lint Enforcement

A required CI job (`GraphQL Schema Lint`) runs on every push and PR that touches
`src/graphql/**` or `GRAPHQL_*.md`.

The job runs `scripts/check-graphql-deprecations.sh`, which:

1. Parses `src/graphql/schema_deprecations.rs` for all `DeprecatedField` entries.
2. Scans `src/graphql/resolvers.rs` and `src/graphql/types.rs` for resolver
   methods and type fields that were removed (i.e. present in the deprecation
   registry but absent from the source).
3. For each such removal, checks that `earliest_removal ≤ today`.
4. **Fails with exit code 1** if any removed field is still within its 90-day
   window, printing which field was removed prematurely and when removal becomes
   valid.
5. Scans for resolver methods/fields that are absent from **both** the source
   and the deprecation registry — flagging them as undocumented removals.

See [`scripts/check-graphql-deprecations.sh`](scripts/check-graphql-deprecations.sh)
for the full implementation.

### Running the lint locally

```bash
bash scripts/check-graphql-deprecations.sh
```

Expected output when everything is clean:

```
[graphql-lint] Checking schema deprecation windows...
[graphql-lint] Registered deprecated fields: 0
[graphql-lint] No premature removals detected. ✓
[graphql-lint] No undocumented field removals detected. ✓
```

Expected output when a field is removed prematurely:

```
[graphql-lint] ERROR: Field Query.wallets was removed but earliest_removal
               (2026-12-31) has not elapsed (today: 2026-10-15).
               Keep the field with @deprecated until 2026-12-31 or later.
exit 1
```

---

## Approvals Required for Breaking Changes

| Change | Required approvals |
|---|---|
| Adding `@deprecated` to a field | 1 maintainer |
| Removing a deprecated field (after SLA) | 1 maintainer |
| Emergency removal (security) | 2 maintainers + issue linked |
| Type signature change | 2 maintainers |
| Renaming a field | 2 maintainers (treated as remove + add) |

"Maintainer" means a contributor with `write` access to
`Nanle-code/StarForge`. Approvals are enforced via GitHub branch protection
on `master`.

---

## Interaction with API Versioning

StarForge currently has no URL-based API versioning. The schema version
(`YYYY-MM` string) is the sole versioning mechanism for the GraphQL surface.

If URL versioning is introduced in future (e.g. `/v2/graphql`), this policy
applies independently to each URL version: fields deprecated in `/v1/graphql`
start their 90-day clock at that endpoint and are not automatically deprecated
in `/v2/graphql`.

The REST API (if ever added) is governed separately; schema versioning here
applies only to the GraphQL surface.

---

## FAQ

**Q: What if a deprecated field has zero known consumers?**  
A: The 90-day window still applies. Zero known consumers does not mean zero
actual consumers — the API is public. Skip the window only for security
emergencies (with 2-maintainer sign-off).

**Q: Can I rename a field without a deprecation window?**  
A: No. A rename is a remove + add. Deprecate the old name and add the new name
in the same PR; wait 90 days, then remove the old name.

**Q: What about subscription fields?**  
A: The same policy applies. Deprecate with `#[graphql(deprecation)]` on the
subscription field, register in `schema_deprecations.rs`, wait 90 days.

**Q: The replacement field doesn't exist yet. Can I still deprecate?**  
A: Add the replacement in the same PR as the deprecation. Do not deprecate a
field before its replacement is available — that would cause immediate breakage.

---

*Policy introduced: 2026-10-01 · Issues #321, #781*  
*Owner: StarForge maintainers*
