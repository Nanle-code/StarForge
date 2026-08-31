# Configuration schema migrations

StarForge persists a `version` field in `~/.starforge/config.toml`. The value
is a schema version, not the CLI version. Every incompatible storage change
must add one migration step from the immediately preceding version.

## Authoring a migration

1. Increment `CURRENT_CONFIG_VERSION` in `src/utils/config.rs`.
2. Add `migrate_vN_to_vN+1` beside the existing migration helpers.
3. Register the step in `MIGRATION_STEPS` in ascending order.
4. Preserve secrets and unknown optional fields unless the migration explicitly
   owns them.
5. Add a fixture covering the old shape and tests for both the migrated values
   and the resulting version.

Migrations run in order, so a configuration at version `N` can safely move
through each supported `N → N+1` step. Before writing, StarForge creates a
versioned backup such as `config.backup.vN.<timestamp>.toml`.

## Future versions

If a config declares a version newer than the binary supports, loading fails
with an upgrade instruction. StarForge must never reinterpret a future schema
as the current one: doing so could silently change wallet, network, or plugin
trust settings.

Plugin authors should keep plugin configuration migrations independent from the
core config migration and include a schema version in each plugin's persisted
document. A plugin should reject unknown future versions with the same clear
upgrade guidance rather than dropping fields.

## Review checklist

- The migration is deterministic and idempotent after its version is applied.
- Existing wallet/key material is not logged or rewritten unnecessarily.
- A backup and rollback path remain available.
- Current, old, malformed, and future-version fixtures are tested.
- `README.md` and release notes describe user-visible migration behavior.

