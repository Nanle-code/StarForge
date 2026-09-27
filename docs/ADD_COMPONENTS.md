# Adding components to an existing contract

`starforge add` bolts a reusable feature onto a contract you already have — the
same thing `shadcn add` does for web components, but for Soroban contracts.

```bash
starforge new my-token --template token      # scaffold
cd my-token
starforge add ownership --dry-run            # preview the patch
starforge add ownership                      # apply it
```

## Why not just paste the code?

Templates only help at project creation, and most features land later. Adding
access control or pausability by hand is error-prone: it is easy to forget a
`require_auth`, to guard only some of the mutating methods, or to leave the
contract uncompilable. `starforge add` makes the edit repeatable and refuses to
write anything it cannot prove is correct.

## Components

| Component | Adds |
| --- | --- |
| `access-control` | `initialize_access_control`, `set_authorized`, `is_authorized`, `require_authorized` |
| `pausability` | `initialize_pause_admin`, `pause`, `unpause`, `is_paused`, `assert_not_paused` |
| `upgradeability` | `initialize_upgrade_admin`, `upgrade` |
| `ownership` | `initialize_owner`, `owner`, `transfer_ownership` |

They compose: you can add all four to the same contract, in any order.

Each component stores its own state under its own storage key, so they do not
collide with each other. A contract that already implements similar methods is
detected by name and refused — see [Refusals](#refusals).

## Usage

```
starforge add <COMPONENT> [--path <DIR>] [--dry-run]
```

- `--path` — project directory, defaults to the current directory. The contract
  is `<path>/src/lib.rs`.
- `--dry-run` — print a unified diff and write nothing.

## How the edit is made safely

Every built-in template ships with two markers in `src/lib.rs`:

```rust
#[contractimpl]
impl MyToken {
    // <starforge:add:methods>      <-- components splice their methods in here

    pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
        // <starforge:pause-check>  <-- rewritten into a guard by `add pausability`
        from.require_auth();
        // ...
    }
}
```

`starforge add` then:

1. Parses `src/lib.rs` with `syn` and refuses if it is not valid Rust.
2. Refuses if the component is already present.
3. Requires **exactly one** `// <starforge:add:methods>` anchor.
4. Walks the AST and refuses if any method the component would add already
   exists.
5. Inserts the methods after the anchor, tagged with
   `// <starforge:component:<name>` so the addition is idempotent.
6. For `pausability`, rewrites every `// <starforge:pause-check>` anchor into
   `Self::assert_not_paused(env.clone());`.
7. Re-parses the result with `syn` and **refuses to write** if the patch is not
   valid Rust.

Step 7 is the important one: the command will never leave you with a
`src/lib.rs` it cannot parse.

## Refusals

`add` fails, without writing, when:

| Message | Cause |
| --- | --- |
| `Component '<name>' is already present` | It has already been added |
| `Expected exactly one StarForge add anchor...` | No anchor, or more than one |
| `Cannot add '<name>': method '<m>' already exists` | Name collision in your contract |
| `src/lib.rs is not valid Rust; refusing to patch it` | Input does not parse |
| `Generated component patch is not valid Rust; refusing to write` | Internal safety net |

## Which templates are supported

All built-in templates carry the anchors, so `add` works on:

- rendered inline by `starforge new`: `hello-world`, `token`, `voting`, `nft`
- packaged under `templates/examples/`: `amm-dex`, `dao-governance`, `escrow`,
  `multisig-vault`, `nft`, `rwa-token`, `sep41-token`, `simple-counter`,
  `staking`, `token-allowlist`

For a contract you wrote yourself, drop the two markers into
`src/lib.rs` (or start from a template) and `add` will work.

## Adding a new component

Components live in the `Component` enum in `src/commands/add.rs`. To add one:

1. Add a variant to `Component` and give it a kebab-case `key()`.
2. Return its Rust methods from `methods()`, as a raw string indented to sit
   inside the `#[contractimpl]` block.
3. List the method names it introduces in `method_names()` — this is what the
   conflict check runs against.
4. Add it to the matrix in the `tests` module at the bottom of the file.

```rust
Self::TimeLock => {
    r#"    pub fn lock_until(env: Env, until: u64) {
        env.storage().instance().set(&soroban_sdk::symbol_short!("Lock"), &until);
    }"#
}
```

The tests iterate every built-in template and every component, and assert the
patched source still parses. Run them with:

```bash
cargo test --lib commands::add
```

## Testing

`src/commands/add.rs` holds the unit tests, which cover every component against
every built-in template (inline and packaged), the refusals, the dry-run
behaviour, and that the injected statements are correctly indented.

`tests/add_components.rs` drives the real binary: scaffold, dry-run, apply,
refusals, and all four components on one contract.

> **Note:** the generated contracts are verified against the `soroban-sdk`
> version each template pins. `add` does not change a project's dependencies.

## See also

- [`docs/COMMAND_REFERENCE.md`](COMMAND_REFERENCE.md) — every command
- [`docs/TEMPLATE_CONTRIBUTING.md`](TEMPLATE_CONTRIBUTING.md) — authoring templates
- [`templates/README.md`](../templates/README.md) — template catalogue
