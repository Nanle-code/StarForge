# Binding Snapshot Tests

Golden/snapshot files for the binding generators. Each file contains the
expected output of generating bindings from the `complex_metadata()` fixture
contract, which exercises:

- Functions with multiple parameter types (Address, u128, Option, Vec)
- Structs with nested optional fields
- Enums with data variants
- Event definitions

## Files

| File | Language | Generator |
|------|----------|-----------|
| `bindings_rust.rs` | Rust | `generate_rust()` |
| `bindings_typescript.ts` | TypeScript | `generate_typescript()` |
| `bindings_python.py` | Python | `generate_python()` |
| `bindings_go.go` | Go | `generate_go()` |

## How snapshot tests work

1. Each test in `tests/bindings_snapshots.rs` calls `generate_from_metadata()`
   with the `complex_metadata()` fixture.
2. The output is **normalized** (line endings stripped, trailing whitespace
   removed, single trailing newline).
3. The normalized output is compared against the golden file.
4. If they don't match, the test fails with a diff-like message.

## Updating snapshots

When you intentionally change the binding generator output (e.g. adding new
type mappings, improving code style, fixing a bug), update the golden files:

```bash
# Generate new snapshots
UPDATE_SNAPSHOTS=1 cargo test --test bindings_snapshots

# Review what changed
git diff tests/fixtures/snapshots/

# Stage and commit the updated snapshots with your generator changes
git add tests/fixtures/snapshots/
git commit -m "Update binding snapshots for <reason>"
```

**Important:** Always review the snapshot diff before committing. The snapshots
are the contract between the generator and downstream consumers. Unexpected
changes may indicate a regression.

## What CI checks

- **Snapshot match**: Each golden file matches the generator output exactly
  (after normalization).
- **No empty snapshots**: All golden files contain non-empty output.
- **Normalization**: No `\r\n` line endings, exactly one trailing newline.
- **File existence**: All 4 golden files must exist.

If a generator refactor changes the output, CI will fail until the snapshots
are updated with `UPDATE_SNAPSHOTS=1`.

## Adding new fixture contracts

To test additional contract shapes:

1. Create a new `ContractMetadata` builder function in the test file or in
   `src/utils/bindings.rs` (as a `pub` function).
2. Add a new snapshot test that calls `generate_from_metadata()`.
3. Add a corresponding golden file in this directory.
4. Run `UPDATE_SNAPSHOTS=1 cargo test --test bindings_snapshots` to populate it.
