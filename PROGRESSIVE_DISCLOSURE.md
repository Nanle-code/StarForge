# Progressive Disclosure for Advanced CLI Flags

StarForge uses progressive disclosure to reduce help noise by hiding advanced/power-user flags by default. This keeps the main help output focused and approachable for new users while still providing access to powerful features for experienced users.

## How It Works

Advanced flags are marked with `#[arg(hide = true)]` in the clap command definitions. These flags are still fully functional but don't appear in the default `--help` output.

## Accessing Hidden Flags

### Method 1: Using --help-all

Run any command with the `--help-all` flag to see information about progressive disclosure:

```bash
starforge --help-all
```

This will display a message explaining how to access hidden flags and list some common hidden flags.

### Method 2: Environment Variable

Set the `STARFORGE_SHOW_ALL_HELP=1` environment variable to show all flags in help output:

```bash
STARFORGE_SHOW_ALL_HELP=1 starforge deploy --help
```

## Examples of Hidden Flags

Some common hidden flags include:

- `--allow-network-passphrase-mismatch`: Allow signing with mismatched passphrase (unsafe)
- `--hardware <ledger|trezor>`: Use hardware wallet for signing
- `--compliance`: Run AI-driven compliance checks
- `--mem`, `--iterations`, `--parallelism`: Advanced encryption parameters
- `--reveal`: Show secret keys in plaintext (security-sensitive)
- `--soroban_rpc_url`, `--friendbot_url`, `--passphrase`: Advanced network configuration

## JSON Help Output

JSON help output (`--json`) always includes all flags, including hidden ones, to ensure machine-readability and automation compatibility.

## Rationale

Progressive disclosure helps:

1. **Reduce cognitive load** for new users by focusing on the most common flags
2. **Prevent accidental misuse** of dangerous or advanced features
3. **Maintain discoverability** for power users who need advanced features
4. **Keep help output scannable** and focused on primary workflows

## Contributing

When adding new flags to StarForge commands:

- Mark advanced or power-user flags with `#[arg(hide = true)]`
- Keep common, frequently-used flags visible by default
- Document hidden flags in this file so users can discover them
- Consider security implications when deciding whether to hide a flag
