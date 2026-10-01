# Plugin authoring cookbook

This cookbook takes you from an empty crate to a signed plugin that other
people can install with trust metadata. Every recipe uses the plugin
interfaces that exist in this repository today, and the two example plugins
it refers to are built and tested in CI:

| Example | What it shows |
|---|---|
| [`examples/plugins/hello-plugin`](https://github.com/Nanle-code/StarForge/tree/master/examples/plugins/hello-plugin) | The smallest loadable plugin: `Plugin` trait, `export_plugin!`, manifest, unit tests. |
| [`examples/plugins/wasm-inspect-plugin`](https://github.com/Nanle-code/StarForge/tree/master/examples/plugins/wasm-inspect-plugin) | A useful plugin: argument handling, error reporting, a declared `fs:read` capability, integration tests, and capability checks against the SDK. |

Related reference pages:

- [Plugin capabilities](capabilities.md)
- [Sandboxed WebAssembly plugins](wasm.md)
- [Plugin trust model and lifecycle](https://github.com/Nanle-code/StarForge/blob/master/docs/PLUGIN_TRUST.md)
  (trust levels, supported-version policy, signature statuses)

## Which plugin interface to target

StarForge has more than one extension surface. Pick the one that matches what
you want the CLI to do with your code.

| Interface | Where it lives | Loaded by |
|---|---|---|
| **Native plugin**: `starforge::plugins::Plugin` + `starforge::export_plugin!` | [`src/plugins/interface.rs`](https://github.com/Nanle-code/StarForge/blob/master/src/plugins/interface.rs) | `starforge plugin install` / `load` and `starforge <plugin-name>` |
| **AI plugin**: `starforge::plugins::AIPlugin` + `starforge::export_ai_plugin!` | [`src/plugins/ai.rs`](https://github.com/Nanle-code/StarForge/blob/master/src/plugins/ai.rs), example in [`examples/ai_plugin_example`](https://github.com/Nanle-code/StarForge/tree/master/examples/ai_plugin_example) | The same loader, which enforces `AICapability` against the trust level |
| **Sandboxed WASM**: a module exporting `run() -> i32`, contract in [`wit/starforge-plugin.wit`](https://github.com/Nanle-code/StarForge/blob/master/wit/starforge-plugin.wit) | [`src/plugins/wasm.rs`](https://github.com/Nanle-code/StarForge/blob/master/src/plugins/wasm.rs) | The library API `PluginManager::load_wasm_plugin`; there is no `starforge plugin` subcommand for WASM plugins yet |

This cookbook covers **native plugins**, the only kind the `starforge plugin`
commands install and run today.

> **About `crates/starforge-plugin-sdk`.** The SDK crate defines the shared
> capability names (`Capability`, with `manifest_name()` returning `fs:read`,
> `fs:write` and `network`), the sandbox budget type `PluginPolicy`, and
> `PluginMeta::is_compatible_with`. Its own `export_plugin!` macro exports a
> `_starforge_plugin_create` symbol that the current loader does **not** look
> up: the loader only resolves `PLUGIN_DECLARATION` (from
> `starforge::export_plugin!`) and `AI_PLUGIN_DECLARATION` (from
> `starforge::export_ai_plugin!`). Use `starforge::export_plugin!` for anything
> you want `starforge plugin install` to load, and use the SDK for capability
> names, as the `wasm-inspect` example does in its tests.

## Before you start

Native plugins are Rust `cdylib` libraries that StarForge opens with
`libloading`. Two things follow from that:

1. **The CLI must be built with native plugin loading enabled.** It is off by
   default because a native plugin runs inside the CLI process. Without the
   `unsafe-native-plugins` feature every load fails with a
   `permission_denied` diagnostic. From a StarForge checkout:

   ```bash norun
   cargo install --path . --locked --features unsafe-native-plugins
   ```

2. **Build the plugin with the same `rustc` and the same StarForge source as
   the CLI that loads it.** See [ABI compatibility](#abi-compatibility).

## Recipe 1: hello world

### Layout

```text
hello-plugin/
├── Cargo.toml
├── starforge-plugin.toml   # manifest, shipped next to the library
└── src/
    └── lib.rs
```

### `Cargo.toml`

```toml
[package]
name = "starforge-hello-plugin"
version = "0.1.0"
edition = "2021"

[lib]
# `starforge plugin install hello` (without --path) looks for libstarforge_hello.so
name = "starforge_hello"
# cdylib is what StarForge loads; rlib lets tests link the crate.
crate-type = ["cdylib", "rlib"]

[dependencies]
# The StarForge checkout whose CLI will load this plugin.
starforge = { path = "../../.." }
```

StarForge is consumed as a path dependency on the checkout you build the CLI
from. The examples use `../../..`, the repository root.

### `src/lib.rs`

```rust,ignore
use starforge::plugins::{Plugin, PluginRegistrar};

#[derive(Debug, Default)]
pub struct HelloPlugin;

impl Plugin for HelloPlugin {
    fn name(&self) -> &'static str {
        "hello" // the command users type: `starforge hello`
    }

    fn version(&self) -> &'static str {
        env!("CARGO_PKG_VERSION")
    }

    fn description(&self) -> &'static str {
        "Prints a greeting (plugin cookbook example)"
    }

    fn execute(&self, args: &[String]) -> Result<(), String> {
        let who = args.first().map(String::as_str).unwrap_or("world");
        println!("Hello, {who}!");
        Ok(())
    }
}

pub fn register(registrar: &mut dyn PluginRegistrar) {
    registrar.register_plugin(Box::new(HelloPlugin));
}

starforge::export_plugin!(register);
```

What the pieces do:

- `Plugin` is the trait the host calls. `name`, `version`, `description` and
  `execute` are required. `commands`, `on_load`, `on_unload` and `on_hook`
  have default implementations.
- `execute` receives every argument after the plugin name
  (`starforge hello a b` passes `["a", "b"]`). Return `Err(message)` for user
  errors. Panics are caught by the host and reported, but an `Err` gives the
  user a clean message.
- `starforge::export_plugin!(register)` emits the `PLUGIN_DECLARATION` static
  that the loader looks up. It records the `rustc` version and StarForge core
  version the plugin was compiled with, which the loader checks before calling
  `register`.

The host routes `starforge <name> ...` by the plugin's `name()`. Keep the
default `commands()` (one command named after the plugin) and handle
subcommands yourself by inspecting `args`.

> `on_hook` and `LifecycleHook` are part of the trait so plugins compiled now
> stay source compatible, but the CLI does not dispatch lifecycle hooks yet.
> Do not rely on them being called.

### `starforge-plugin.toml`

```toml
name = "hello"               # must equal the install name and Plugin::name()
version = "0.1.0"            # the plugin's own version
starforge_version = "0.1.0"  # StarForge version the plugin is built for
description = "Prints a greeting (plugin cookbook example)"
```

`starforge plugin install` refuses to register a library without this file.
It is looked up next to the library, or one directory above it. The fields
are defined by `PluginManifest` in
[`src/plugins/manifest.rs`](https://github.com/Nanle-code/StarForge/blob/master/src/plugins/manifest.rs).

### Build, install and run

From the repository root, using the example crate:

```bash norun
cp Cargo.lock examples/plugins/Cargo.lock   # same dependency versions as the CLI
cargo build --release --manifest-path examples/plugins/Cargo.toml -p starforge-hello-plugin

mkdir -p dist/hello
# libstarforge_hello.dylib on macOS, starforge_hello.dll on Windows
cp examples/plugins/target/release/libstarforge_hello.so dist/hello/
cp examples/plugins/hello-plugin/starforge-plugin.toml dist/hello/

starforge plugin install hello --path "$PWD/dist/hello/libstarforge_hello.so"
starforge hello Stellar
```

`starforge hello Stellar` prints `Hello, Stellar!`. A `--path` install without
`--source` is classified as trust level `local`. The registry stores `--path`
exactly as given, so pass an absolute path; a relative one only resolves from
the directory you installed from.

To see what is registered and to remove it again:

```bash norun
starforge plugin list
starforge plugin commands hello
starforge plugin uninstall hello
```

On a fresh machine the registry is empty:

```bash run
starforge plugin list
```

## Recipe 2: capabilities

### What a capability is

A capability names host access a plugin needs: reading files, writing files,
or talking to the network. Declaring it tells the user, reviewers and the
host what the plugin will do. The names are defined once in
`starforge_plugin_sdk::Capability` and shared by the manifest and the WASM
host:

| `Capability` variant | Manifest name |
|---|---|
| `FileSystemRead` | `fs:read` |
| `FileSystemWrite` | `fs:write` |
| `Network` | `network` |

Declare them in `starforge-plugin.toml`:

```toml
required_capabilities = ["fs:read"]
```

`PluginManifest::has_capability` also accepts a few aliases (`fs` or
`filesystem` for both file capabilities, `net` or `networkaccess` for
`network`) and `*` for everything. Prefer the exact names above: an alias or
wildcard grants more than the plugin needs.

### How capabilities are enforced

Enforcement depends on the plugin kind. Be precise about this when you explain
your plugin to users:

- **Native plugins** (this cookbook) run in the CLI process with the CLI
  user's operating-system permissions. `required_capabilities` is parsed and
  validated, and `PluginManifest::enforce_filesystem_access` /
  `enforce_network_access` reject undeclared access for code that consults
  them, but the loader does not sandbox native code. The declaration is a
  contract you must keep and that users review before trusting you, not a
  sandbox.
- **AI plugins** declare `AICapability` values from `capabilities()`. When the
  plugin was installed from a source with trust level `unknown`, the loader
  refuses to load it if it requests `NetworkAccess`, `FileSystemAccess` or
  `ExecuteCode`, with a `permission_denied` diagnostic.
- **Sandboxed WASM plugins** get no ambient imports at all. A module that
  imports WASI filesystem or networking functions fails to instantiate. See
  [Sandboxed WebAssembly plugins](wasm.md).

Security implications for authors:

- Declare the smallest set you use. `wasm-inspect` only reads the file named
  on the command line, so it declares `fs:read` and not `fs:write` or
  `network`.
- Never read wallet files, `~/.starforge/config.toml` or other secrets the
  user did not pass to you. A native plugin technically can; doing so is
  exactly what makes users stop trusting plugins.
- Adding a capability in a new release is a change users must review. Call it
  out in your release notes.

### Worked example: `wasm-inspect`

[`examples/plugins/wasm-inspect-plugin`](https://github.com/Nanle-code/StarForge/tree/master/examples/plugins/wasm-inspect-plugin)
summarises a compiled contract:

```bash norun
starforge wasm-inspect target/wasm32-unknown-unknown/release/my_contract.wasm
```

It reports the module size, WebAssembly version, every section, and whether
the module carries Soroban's `contractspecv0` section. Its manifest declares
exactly the access the code performs:

```toml
name = "wasm-inspect"
version = "0.1.0"
starforge_version = "0.1.0"
starforge_version_min = "0.1.0"
description = "Summarises the sections of a compiled Soroban contract WASM"
required_capabilities = ["fs:read"]
```

A test keeps the declaration honest: every declared name must be an SDK
capability, and write and network access must stay undeclared.

```rust,ignore
#[test]
fn manifest_declares_least_privilege_capabilities() {
    let manifest = manifest(); // parsed with load_manifest_for_library

    let known = [
        Capability::FileSystemRead,
        Capability::FileSystemWrite,
        Capability::Network,
    ]
    .map(Capability::manifest_name);
    for declared in &manifest.required_capabilities {
        assert!(known.contains(&declared.as_str()), "unknown {declared}");
    }

    assert!(manifest.enforce_filesystem_access(false).is_ok());
    assert!(manifest.enforce_filesystem_access(true).is_err());
    assert!(manifest.enforce_network_access().is_err());
}
```

If a later change makes the plugin write a report file, this test fails until
`fs:write` is declared, so the capability change shows up in review.

## Recipe 3: testing

Plugins are ordinary Rust crates, so `cargo test` is the test runner. The
examples use three layers.

**Unit tests of plain functions.** Keep logic out of `execute` so it can be
tested without I/O. `wasm-inspect` parses a byte slice in `inspect(&[u8])`;
`hello` builds its message in `HelloPlugin::greeting`.

**Registration tests.** Implement `PluginRegistrar` in the test to capture
what `register` hands to the host, exactly as the loader's registrar does:

```rust,ignore
#[derive(Default)]
struct TestRegistrar(Vec<Box<dyn Plugin>>);

impl PluginRegistrar for TestRegistrar {
    fn register_plugin(&mut self, plugin: Box<dyn Plugin>) {
        self.0.push(plugin);
    }
}

#[test]
fn register_exposes_one_command_named_after_the_plugin() {
    let mut registrar = TestRegistrar::default();
    register(&mut registrar);

    assert_eq!(registrar.0.len(), 1);
    assert_eq!(registrar.0[0].commands()[0].name, "hello");
}
```

**Manifest tests.** Parse your manifest with the host's own function and
validate it against the StarForge core you compile against. This catches a
renamed plugin, a forgotten version bump, or a `starforge_version` the CLI
would reject:

```rust,ignore
use starforge::plugins::manifest::load_manifest_for_library;

#[test]
fn manifest_matches_plugin_and_running_core() {
    // The manifest is found beside the (hypothetical) library path.
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("libstarforge_hello.so");
    let manifest = load_manifest_for_library(&lib).unwrap().unwrap();

    assert_eq!(manifest.name, HelloPlugin.name());
    assert_eq!(manifest.version, HelloPlugin.version());
    manifest.validate().unwrap();
}
```

`wasm-inspect` also has integration tests in `tests/plugin.rs`. They link the
crate as an `rlib` (hence `crate-type = ["cdylib", "rlib"]`), call `execute`
on real files, and check that bad input returns `Err` rather than panicking.

Run the example tests the same way CI does:

```bash norun
cp Cargo.lock examples/plugins/Cargo.lock   # pin to the CLI's dependency versions
cargo test --manifest-path examples/plugins/Cargo.toml --workspace
```

### Validate the plugin against a real CLI

Unit tests do not exercise the loader. Before you publish, install the built
library into a throwaway home directory so your real registry is untouched:

```bash norun
export HOME="$(mktemp -d)"
starforge plugin install hello --path "$PWD/dist/hello/libstarforge_hello.so"
starforge plugin verify hello --deep                # manifest, trust, signature, compatibility
starforge plugin audit hello --runtime-check        # also loads the library
starforge hello Stellar
```

`plugin audit` (and `plugin verify --deep`, which runs the same checks) prints
one line per check and exits non-zero when a check fails, so it is suitable for
your own CI. Plain `plugin verify` only prints its findings and exits 0 even
when a signature is invalid, so do not gate on it.

## Recipe 4: signing

Signing lets users check that the library they downloaded is the one you
built. It is implemented in
[`src/plugins/verifier.rs`](https://github.com/Nanle-code/StarForge/blob/master/src/plugins/verifier.rs).

**What is signed.** The SHA-256 digest of the exact library file users will
install (`libstarforge_<name>.so`, `.dylib` or `.dll`). The signature is an
Ed25519 signature over those 32 digest bytes. The verifier also accepts a
signature over the whole file, but sign the digest. Each platform build is a
different file and needs its own signature, and any rebuild invalidates the
signature.

**Key material.** An Ed25519 key pair. The public key goes in the manifest as
`publisher_key`, either as a Stellar `G...` address or as 64 hex characters.
The signature goes in `signature`, as 128 hex characters or base64.

**Tooling.** StarForge verifies signatures but has no `plugin sign` command,
and `starforge wallet sign` signs a UTF-8 message rather than raw digest bytes,
so its output does not verify. Any Ed25519 implementation works. With OpenSSL
3.0 or later:

```bash norun
# One-time: create the publisher key. Keep it outside the repository.
mkdir -p ~/.config/starforge-publisher && chmod 700 ~/.config/starforge-publisher
openssl genpkey -algorithm ed25519 -out ~/.config/starforge-publisher/ed25519.pem
chmod 600 ~/.config/starforge-publisher/ed25519.pem

# publisher_key: the raw 32-byte public key, hex encoded
openssl pkey -in ~/.config/starforge-publisher/ed25519.pem -pubout -outform DER \
  | tail -c 32 | od -An -v -tx1 | tr -d ' \n'; echo

# signature: Ed25519 over the SHA-256 digest of the release library
openssl dgst -sha256 -binary dist/hello/libstarforge_hello.so > hello.sha256
openssl pkeyutl -sign -rawin -inkey ~/.config/starforge-publisher/ed25519.pem -in hello.sha256 \
  | od -An -v -tx1 | tr -d ' \n'; echo
```

Put the two hex strings in the manifest that ships next to that library
(`dist/hello/starforge-plugin.toml`), together with the publisher name:

```toml
publisher = "Example Plugins"
publisher_key = "<64 hex characters from the first command>"
signature = "<128 hex characters from the second command>"
```

How it is checked: on `plugin install`, `plugin verify` and `plugin audit`, StarForge hashes
the library on disk, decodes the key and signature, and verifies. The result
is one of the statuses below. `plugin install` refuses `invalid_signature`,
`malformed_key`, `malformed_signature` and `untrusted_publisher` unless the
user passes `--force`.

| Status | Meaning |
|---|---|
| `verified` | Signature matches the library, and the key is trusted (or no allowlist is configured) |
| `unsigned` | No `signature` in the manifest |
| `invalid_signature` | Library changed after signing, or wrong key |
| `untrusted_publisher` | Valid signature, but the key is not in `plugin_trust.trusted_publishers` |
| `malformed_key` / `malformed_signature` | The strings cannot be decoded |
| `failed` | The library could not be read |

What to protect:

- **Never commit the private key** (`*.pem`), and never paste it into CI logs,
  issues or the manifest. Only `publisher_key` and `signature` are public.
  If you sign in CI, store the key as an encrypted CI secret, write it to a
  temporary file for the signing step only, and delete it afterwards.
- Add `*.pem` to your plugin repository's `.gitignore`.
- If the key leaks, generate a new one, re-sign every supported release, and
  tell users to replace the old key in `trusted_publishers`. There is no
  revocation list, so removing the old key from users' allowlists is the only
  way to distrust it.
- Commit the unsigned manifest to source control. Add `signature` only to the
  copy in your release artifacts, because the value is tied to one binary.

## Recipe 5: trust metadata

Trust metadata is what a user's StarForge uses to decide whether to install,
warn about, or refuse your plugin. It comes from two places.

**From the author, in `starforge-plugin.toml`:**

| Field | Purpose |
|---|---|
| `name`, `version`, `description` | Identity shown by `plugin list` and `plugin install` |
| `starforge_version`, `starforge_version_min`, `starforge_version_max` | Compatibility, checked before the library is opened |
| `required_capabilities` | Declared access (Recipe 2) |
| `publisher` | Human-readable publisher name |
| `publisher_key`, `signature` | Publisher authentication (Recipe 4) |

**From the user, at install time and in their StarForge configuration:**

- `--source <url>` on `plugin install` sets the plugin's **trust level**.
  Without `--source` the plugin is `local`. A source matching a trusted prefix
  is `trusted`. The defaults are `https://github.com/Nanle-code/starforge-*`,
  `https://github.com/StarForge-Labs/*` and
  `https://crates.io/crates/starforge-plugin-*`, and users can add entries
  under `plugin_trust.trusted_sources`. Anything else is `unknown` and needs
  `--force`.
- `plugin_trust.trusted_publishers` lists publisher keys the user trusts.
  When it is non-empty, a valid signature from any other key is
  `untrusted_publisher`.
- `plugin_trust.require_signatures = true` makes `plugin install` refuse
  unsigned plugins.

StarForge keeps its configuration in `~/.starforge/starforge.db`, and
`plugin_trust` has no `config set` key, so the table is changed by exporting
the configuration to TOML, editing it, and importing it back:

```bash norun
starforge config show > /dev/null      # creates the configuration on first use
starforge config db export --out ~/.starforge/config.toml
# edit the [plugin_trust] table in ~/.starforge/config.toml, then:
starforge config db migrate            # imports config.toml into starforge.db
```

```toml
[plugin_trust]
trusted_sources = [
    "https://github.com/Nanle-code/starforge-*",
    "https://github.com/StarForge-Labs/*",
    "https://crates.io/crates/starforge-plugin-*",
    "https://plugins.example.com/",
]
trusted_publishers = ["<your publisher_key>"]
require_signatures = true
```

Keep the default entries in `trusted_sources` when you add your own: the list
you write replaces the defaults.

The registry (`~/.starforge/plugins/registry.json`) records the source,
publisher, publisher key and verification status of each installed plugin.
`plugin list`, `plugin verify` and `plugin audit` report them.

> **Known limitation.** `plugin install` classifies `--source` against the
> configured `trusted_sources`, but `plugin verify`, `plugin audit` and the
> warning printed when a plugin runs only check the three built-in prefixes.
> A plugin installed from a source the user added to `trusted_sources`
> therefore installs as `trusted` and is later reported as `trust=unknown`
> (a warning, not a failure, in `plugin audit`).

Before you publish, check:

- `name` in the manifest equals `Plugin::name()` and the name users will
  install with.
- `version` equals the crate version you built.
- `starforge_version` and any min/max bounds match the CLI version you built
  against.
- `required_capabilities` lists everything the code does, and nothing else.
- The `signature` was produced from the exact file in the release, and
  `plugin verify <name>` on a clean install reports `verification=verified`.

## Recipe 6: publishing

There is no central plugin upload command. `starforge plugin search` reads a
marketplace index, but you do not upload to it from the CLI. Publishing means
putting the library and its manifest where users can download them, then
giving users an install command with a `--source` they can classify.

The end-to-end flow:

1. **Build** the release library with the same toolchain and StarForge source
   as the CLI release you target:

   ```bash norun
   rustc --version        # must match the toolchain that built the CLI
   starforge --version    # the StarForge version you target
   cargo build --release --manifest-path examples/plugins/Cargo.toml -p starforge-hello-plugin
   ```

2. **Test** it: `cargo test`, then the throwaway-home install from Recipe 3.
3. **Package** one directory per platform containing the library and
   `starforge-plugin.toml`.
4. **Sign** each platform's library and fill in `publisher`, `publisher_key`
   and `signature` in that directory's manifest (Recipe 4).
5. **Verify** the packaged directory exactly as a user would, in a throwaway
   home directory whose `[plugin_trust]` table (Recipe 5) trusts the URL you
   will publish under (here `https://plugins.example.com/`) and your
   `publisher_key`, with `require_signatures = true`:

   ```bash norun
   export HOME="$(mktemp -d)"
   starforge config show > /dev/null
   starforge config db export --out ~/.starforge/config.toml
   # edit [plugin_trust] as in Recipe 5, then:
   starforge config db migrate

   starforge plugin install hello --path "$PWD/dist/hello/libstarforge_hello.so" \
     --source https://plugins.example.com/hello/
   starforge plugin audit hello --runtime-check
   ```

   `plugin install` should report `Trust trusted` and `Verification verified`,
   and `plugin audit` should pass the `signature` and `runtime` checks.

6. **Publish** the directories as release assets under that URL, and publish
   your `publisher_key` somewhere users can check it independently of the
   download (for example your README).
7. **Document the install command** for users. They download the directory for
   their platform, then run:

   ```bash norun
   starforge plugin install hello \
     --path ./hello/libstarforge_hello.so \
     --source https://plugins.example.com/hello/
   ```

   and, if they want signature enforcement, add your `publisher_key` to
   `plugin_trust.trusted_publishers`.

How users' StarForge classifies your `--source`:

- Plugins published under one of the default trusted prefixes (the project's
  own `https://github.com/Nanle-code/starforge-*` and
  `https://github.com/StarForge-Labs/*` repositories, or crates named
  `starforge-plugin-*` on crates.io) install as `trusted` without extra steps.
- Any other URL is `unknown`: `plugin install` refuses it unless the user adds
  your prefix to `plugin_trust.trusted_sources` or passes `--force`. Tell users
  which prefix to trust, rather than telling them to use `--force`.

For updates, `starforge plugin update` re-registers plugins whose source is a
`https://crates.io/crates/...` URL by running `cargo install`. For other
sources, users replace the library at the registered path and run
`plugin update` or reinstall.

## ABI compatibility

Native plugins share Rust types (`Box<dyn Plugin>`) with the CLI across a
dynamic-library boundary. Rust does not have a stable ABI, so StarForge only
loads plugins that match it closely. The rules are enforced in
[`src/plugins/loader.rs`](https://github.com/Nanle-code/StarForge/blob/master/src/plugins/loader.rs)
and [`src/plugins/manifest.rs`](https://github.com/Nanle-code/StarForge/blob/master/src/plugins/manifest.rs)
(`SupportedVersionPolicy`), and written up under
[Supported-Version Policy](https://github.com/Nanle-code/StarForge/blob/master/docs/PLUGIN_TRUST.md#supported-version-policy-and-compatibility-requirements).

**What you target.** `starforge::export_plugin!` records two values at compile
time: the output of `rustc --version` (set by StarForge's `build.rs`) and the
StarForge crate version you compiled against. Your manifest's
`starforge_version` states the same StarForge version for tools that read the
manifest without opening the library.

**What the host checks, in order:**

1. Before opening the library: the manifest's `starforge_version` has the
   running CLI's **major** version, and the running version is within
   `starforge_version_min` / `starforge_version_max` when those are set.
   Failure: `manifest_incompatible`.
2. The `rustc` version string baked into the plugin is **identical** to the
   CLI's. Failure: `abi_mismatch`.
3. The StarForge core version baked into the plugin has the same **major**
   version as the CLI. Failure: `unsupported_core_version`.

**What breaks compatibility:**

- A different Rust compiler, even a patch release.
- A different StarForge major version (`0.x` plugins do not load in `1.x`).
- A running CLI outside your declared min/max range.
- A CLI built without `unsafe-native-plugins` loads no native plugins.

**What is not promised.** The policy treats versions with the same major as
compatible, but the interface is a Rust trait object, and the repository does
not document a stable layout for it across releases. Build and test against
the exact StarForge version your users run, and rebuild when they upgrade.
Portable, toolchain-independent plugins are the goal of the WASM contract in
[`wit/starforge-plugin.wit`](https://github.com/Nanle-code/StarForge/blob/master/wit/starforge-plugin.wit)
(see [Sandboxed WebAssembly plugins](wasm.md)).

**Where to check:** `starforge --version` and `rustc --version` on the target
machine, the `starforge_version*` fields in your manifest, and the diagnostic
category printed by `plugin install`, `plugin audit --runtime-check` or
`plugin load` when a plugin is rejected.
