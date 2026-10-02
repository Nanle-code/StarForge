## Description

Brief description of what this PR does and why.

Closes #(issue number)

## Type of Change

- [ ] Bug fix (non-breaking change which fixes an issue)
- [ ] New feature (non-breaking change which adds functionality)
- [ ] Breaking change (fix or feature that would cause existing functionality to change)
- [ ] Documentation update

## Changes Made

- Change 1
- Change 2
- Change 3

## Testing

### How has this been tested?

Describe the tests you ran and how to reproduce them.

- [ ] Unit tests added/updated
- [ ] Integration tests added/updated
- [ ] Manual testing performed

### Test Coverage

Describe what scenarios have been tested:
- Happy path: 
- Edge cases: 
- Error handling: 

## Code Quality Checklist

- [ ] My code follows the style guidelines of this project (`cargo fmt`)
- [ ] I have performed a self-review of my own code
- [ ] I have commented my code, particularly in hard-to-understand areas
- [ ] I have made corresponding changes to the documentation
- [ ] My changes generate no new warnings (`cargo clippy -- -D warnings`)
- [ ] I have added tests that prove my fix is effective or that my feature works
- [ ] New and existing unit tests pass locally with my changes
- [ ] The CI checks pass (format, clippy, tests)

## Breaking Changes

- [ ] This PR introduces breaking changes

If checked, describe the breaking changes and migration path:

### GraphQL Breaking-Change Checklist

<!-- Complete this section for ANY PR that touches src/graphql/ — leave unchecked items as-is if not applicable -->

- [ ] **No non-deprecated field is removed** — if a field is removed, confirm
      it carried `#[graphql(deprecation)]` for ≥ 90 days (see `GRAPHQL_SCHEMA_VERSIONING.md`).
- [ ] **Deprecation registered** — new deprecations are recorded in
      `src/graphql/schema_deprecations.rs` with correct `deprecated_since`
      and `earliest_removal` dates (≥ deprecated_since + 90 days).
- [ ] **Replacement documented** — the `#[graphql(deprecation = "...")]` reason string
      names the replacement field/type.
- [ ] **Schema version bumped** — `SCHEMA_VERSION` in `src/graphql/schema.rs`
      updated if a field/type is removed or a type signature changed.
- [ ] **CHANGELOG updated** — entry added under `### Deprecated` or `### Removed` as appropriate.
- [ ] **CI lint passes** — `bash scripts/check-graphql-deprecations.sh` exits 0.
- [ ] **Consumer impact assessed** — first-party client libraries
      (`client.go`, `client.py`, `client.rs`, `client.ts`) checked or updated.

## Documentation

- [ ] README.md updated
- [ ] DEVELOPER_GUIDE.md updated (if applicable)
- [ ] API_REFERENCE.md updated (if applicable)
- [ ] No documentation changes needed

## Screenshots (if applicable)

Add screenshots or GIFs for UI changes.

## Additional Context

Add any other context about the PR here.

---

**Note**: Make sure all tests pass locally before submitting:
```bash
cargo test
cargo fmt --all
cargo clippy -- -D warnings
```
