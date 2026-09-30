# StarForge documentation

StarForge is a developer productivity CLI for Stellar and Soroban: scaffold
contracts from templates, manage encrypted wallets, and plan, deploy and track
contracts with safety checks. It sits on top of
[stellar-cli](https://developers.stellar.org/docs/tools/cli) rather than
replacing it.

## Start here

- [Installation](INSTALL.md): the install script, checksums, platforms,
  building from source.
- [Using StarForge](USAGE.md): a task-oriented tour of the everyday commands.
- [Migrating from stellar-cli](MIGRATING_FROM_STELLAR_CLI.md): command
  mapping, identity import, and what stellar-cli still does better.
- [Command reference](COMMAND_REFERENCE.md) and
  [cheat sheet](COMMAND_CHEATSHEET.md).
- [Unified `--dry-run` semantics](DRY_RUN_SEMANTICS.md): the plan-first
  guarantee shared by every state-changing command.

Every page here is also plain Markdown in the
[`docs/` directory](https://github.com/Nanle-code/StarForge/tree/master/docs).
Shell examples marked runnable are executed in CI on every change, so they
match the current CLI. See
[Documentation snippets](https://github.com/Nanle-code/StarForge/blob/master/CONTRIBUTING.md#documentation-snippets).
