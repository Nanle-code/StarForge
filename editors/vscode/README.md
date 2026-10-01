# StarForge for VS Code

Build, deploy, and invoke Soroban smart contracts without leaving your editor.
The extension wraps the [StarForge CLI](https://github.com/Nanle-code/StarForge)
JSON API to give you tree views, one-click commands, integrated diagnostics, and
a VS Code task provider — all on top of the same stable contract the CLI exposes
via `--json`.

---

## Demo

> **GIF coming soon** — a recording of the full build → deploy → invoke workflow
> will be added here before the 1.0 release.
>
> To record your own: install the extension, open a Soroban project, then run
> `StarForge: Build Contract` → `StarForge: Deploy Contract` →
> `StarForge: Invoke Contract Function` and capture the session with
> [Recordit](https://recordit.co) or [LICEcap](https://www.cockos.com/licecap/).

---

## Features

### Activity-bar panel

Three tree views live in the **StarForge** side-panel (click the ⚡ icon):

| View | Data source |
|---|---|
| **Wallets** | `starforge wallet list --json` |
| **Networks** | `starforge network list --json` |
| **Deployed Contracts** | `starforge deployments list --json` |

Each node is expandable to reveal full details (public key, RPC URL, WASM hash,
deploy timestamp, fee, etc.). A refresh button sits in every view's title bar.

### Commands

Open the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`) and type **StarForge**:

| Command | Description |
|---|---|
| `StarForge: Build Contract` | Runs `starforge contract build` as a VS Code Task; errors and warnings appear in the **Problems** panel. |
| `StarForge: Test Contract` | Runs `starforge contract test` as a VS Code Task with the same problem matcher. |
| `StarForge: Deploy Contract` | Interactive wizard — pick a `.wasm` file, network, and wallet; choose dry-run or live deploy. |
| `StarForge: Invoke Contract Function` | Pick a contract from history, enter a function name and arguments, see the result inline. |
| `StarForge: Inspect Contract` | Fetches on-chain metadata and opens the JSON in a side panel. |
| `StarForge: Show Info` | Prints CLI version, active network, config path, and wallet/network counts. |
| `StarForge: Refresh Wallets / Networks / Contracts` | Force-refresh any tree view. |

Right-click a deployed contract in the tree to **Copy Contract ID** or **Invoke**
directly from context.

### Task provider

Declare StarForge tasks in `.vscode/tasks.json` for CI-equivalent local runs:

```jsonc
{
  "version": "2.0.0",
  "tasks": [
    {
      "type": "starforge",
      "task": "build",
      "group": { "kind": "build", "isDefault": true },
      "problemMatcher": ["$starforge-build", "$starforge-warning"]
    },
    {
      "type": "starforge",
      "task": "test",
      "group": { "kind": "test", "isDefault": true }
    }
  ]
}
```

### Problem matchers

The `$starforge-build` and `$starforge-warning` matchers parse Rust/Cargo
`error[E…]: …` and `warning[…]: …` diagnostics and wire them to the correct
source file and line in the **Problems** panel. They are attached automatically
to every StarForge task.

---

## Requirements

- **VS Code 1.90** or later
- **StarForge CLI** installed and on `PATH`
  (or configured via `starforge.binaryPath`)

### Install the CLI

```bash
# macOS / Linux (curl installer)
curl -fsSL https://raw.githubusercontent.com/Nanle-code/StarForge/master/install.sh | bash

# Cargo
cargo install starforge
```

Verify:

```bash
starforge --version
```

---

## Extension settings

| Setting | Default | Description |
|---|---|---|
| `starforge.binaryPath` | `"starforge"` | Absolute or `PATH`-relative path to the `starforge` binary. |
| `starforge.defaultNetwork` | `"testnet"` | Network pre-selected in deploy / invoke wizards. |
| `starforge.showJsonOutput` | `false` | Echo raw JSON responses in the StarForge output channel. |
| `starforge.autoRefresh` | `true` | Refresh tree views when `starforge-project.toml` or `.wasm` files change. |

---

## Quick start

1. **Install the extension** from the VS Code Marketplace or Open VSX.
2. **Open a Soroban project** — any workspace with a `Cargo.toml` or
   `starforge-project.toml` at the root.
3. **Click the ⚡ StarForge icon** in the activity bar to see your wallets and
   networks.
4. Press `Ctrl+Shift+B` (`Cmd+Shift+B`) to trigger the default **build** task,
   or run `StarForge: Build Contract` from the Command Palette.
5. Run `StarForge: Deploy Contract`, select the compiled `.wasm`, pick a
   network, and choose **Preview (dry-run)** first to validate before going
   live.
6. Right-click the newly deployed contract in the **Deployed Contracts** tree
   and choose **Invoke** to call a function.

---

## Troubleshooting

**"StarForge binary not found"**  
The CLI is not on `PATH`. Either add it (`export PATH="$HOME/.cargo/bin:$PATH"`)
or set `starforge.binaryPath` in your VS Code settings to the absolute path.

**Tree views show "Error: …"**  
The CLI returned a non-zero exit or the JSON envelope contained `ok: false`.
Click the StarForge status-bar item (bottom-left) or run
`StarForge: Show Info` to check the active configuration.

**Diagnostics not appearing in the Problems panel**  
Make sure the task was started via the StarForge task provider (not a raw
`cargo build` shell task). The `$starforge-build` problem matcher is only
attached to tasks of type `"starforge"`.

---

## Contributing

See the [root CONTRIBUTING.md](../../CONTRIBUTING.md) for the full guide.

### Local development

```bash
cd editors/vscode
npm install
npm run build          # esbuild bundle
npm run compile        # TypeScript type-check only
npm run lint           # ESLint
```

Press **F5** in VS Code (with the `editors/vscode` folder open) to launch the
**Extension Development Host** with the extension loaded from source.

### Running the tests

```bash
npm test               # builds + runs @vscode/test-electron suite
```

Tests run headlessly in CI via `xvfb-run`.

---

## Release

VSIX packages are built and published automatically by the
[`.github/workflows/vscode-extension.yml`](../../.github/workflows/vscode-extension.yml)
workflow on every `v*` tag push:

- **VS Code Marketplace** — published with `vsce publish`
- **Open VSX** — published with `ovsx publish`

Both publishers require a `VSCE_PAT` / `OVSX_PAT` secret in the repository
settings.

---

## License

MIT — see [LICENSE](../../LICENSE).
