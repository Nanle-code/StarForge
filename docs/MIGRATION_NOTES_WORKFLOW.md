# Migration Notes Generator — Maintainer Workflow

> Command reference: `starforge migrate-notes --help`  
> Issue: #780

This document describes when, how, and where to use the AI-assisted migration
notes generator. Read this before a release that contains breaking CLI or config
changes.

---

## Table of Contents

- [Overview](#overview)
- [When to Run the Generator](#when-to-run-the-generator)
- [Step-by-Step Workflow](#step-by-step-workflow)
- [How to Review and Edit the Output](#how-to-review-and-edit-the-output)
- [Where to Publish the Final Notes](#where-to-publish-the-final-notes)
- [Snapshot Format Reference](#snapshot-format-reference)
- [Fixture Breaking-Change Example](#fixture-breaking-change-example)

---

## Overview

The generator diffs two CLI snapshots — a "before" and "after" JSON file — and
produces a markdown document listing:

- Renamed commands and flags
- Removed commands and flags
- Config key renames, removals, and type changes

**Every generated document begins with a mandatory human-review banner** that
cannot be suppressed. A maintainer **must** read, edit, and approve the output
before it is published anywhere.

---

## When to Run the Generator

Run the generator as part of the release-prep workflow, specifically:

1. **Before cutting a release tag** that contains breaking CLI or config changes.
2. **During release-prep PR creation** — attach the draft notes to the PR for
   reviewer reference.
3. **After a merge that introduces a breaking change** — captured in a draft
   CHANGELOG entry for the upcoming release.

Do NOT publish AI-generated notes verbatim. The output is a first draft that
identifies structural changes; context, motivation, and links must be added by
hand.

---

## Step-by-Step Workflow

### Step 1 — Capture a "before" snapshot on the last stable tag

```bash
# Check out the last release tag
git checkout v0.1.0

# Build and capture the current CLI surface
cargo build --release
starforge migrate-notes snapshot --version v0.1.0 --output /tmp/before-v0.1.0.json
```

### Step 2 — Return to your release branch and capture "after"

```bash
git checkout feat/your-release-branch

cargo build --release
starforge migrate-notes snapshot --version v0.2.0 --output /tmp/after-v0.2.0.json
```

### Step 3 — Generate the migration notes

```bash
starforge migrate-notes generate \
  --before /tmp/before-v0.1.0.json \
  --after  /tmp/after-v0.2.0.json \
  --summary \
  --output  docs/migrations/v0.1.0-to-v0.2.0.md
```

The `--summary` flag prints a count of each change category to the console.
The generated file starts with the human-review banner.

### Step 4 — Review and edit the output

See [How to Review and Edit the Output](#how-to-review-and-edit-the-output).

### Step 5 — Publish

See [Where to Publish the Final Notes](#where-to-publish-the-final-notes).

---

## How to Review and Edit the Output

Open the generated markdown file and:

1. **Remove or confirm each rename** — the rename heuristic matches by
   description text. Occasionally it will mismatch an unrelated pair; verify
   each rename is intentional.

2. **Add migration commands** — for each renamed command or flag, add an
   explicit example showing the old invocation and the new one:

   ```markdown
   **Before:**
   ```bash
   starforge wallet create --wallet-name my-wallet
   ```
   **After:**
   ```bash
   starforge wallet create --name my-wallet
   ```
   ```

3. **Add context for removals** — if a command was removed without replacement,
   explain why and what the user should do instead.

4. **Verify config key changes** — open the relevant config source file and
   confirm the key names, types, and defaults are accurate.

5. **Remove the review banner** only after you have completed your review and
   are satisfied the notes are accurate. (Or keep it if publishing as a draft.)

6. **Spell-check and format** — run the notes through a markdown linter if
   your project uses one.

---

## Where to Publish the Final Notes

StarForge uses the following release communication channels:

| Channel | When to use | How |
|---------|-------------|-----|
| `CHANGELOG.md` | Every release | Add a `## [v0.2.0]` section with `### Breaking Changes` and `### Migration` subsections. |
| `docs/migrations/` | Releases with breaking changes | Commit the reviewed notes file here. |
| GitHub Release body | Every tagged release | Copy the CHANGELOG section into the GitHub Release created by `scripts/release.sh`. |
| PR description | During release PR | Link to the migration notes file committed to `docs/migrations/`. |

### CHANGELOG format example

```markdown
## [v0.2.0] - 2027-01-15

### Breaking Changes

- `starforge wallet create` flag `--wallet-name` renamed to `--name`.
- Config key `network.default_network` renamed to `network.default`.

### Migration

See [docs/migrations/v0.1.0-to-v0.2.0.md](docs/migrations/v0.1.0-to-v0.2.0.md)
for a full list of changes and migration steps.
```

---

## Snapshot Format Reference

A snapshot is a JSON file with this structure:

```json
{
  "version": "v0.1.0",
  "commands": [
    {
      "command": "wallet create",
      "description": "Create a new test wallet",
      "flags": [
        {
          "name": "wallet-name",
          "short": "n",
          "required": true,
          "default": null,
          "description": "Wallet name"
        }
      ]
    }
  ],
  "config_keys": [
    {
      "key": "network.default_network",
      "value_type": "string",
      "default": "testnet",
      "description": "Default network for all commands"
    }
  ]
}
```

Snapshots are captured with `starforge migrate-notes snapshot` and stored in
`tests/fixtures/migrate_notes/` for fixture tests.

---

## Fixture Breaking-Change Example

The directory `tests/fixtures/migrate_notes/` contains a canonical before/after
pair used in `tests/cli_migration_notes.rs`:

| File | Description |
|------|-------------|
| `before.json` | CLI surface of a hypothetical v0.1.0 with old flag names |
| `after.json`  | CLI surface of v0.2.0 with renamed flag and removed command |

To regenerate the expected output from the fixture:

```bash
starforge migrate-notes generate \
  --before tests/fixtures/migrate_notes/before.json \
  --after  tests/fixtures/migrate_notes/after.json \
  --output /tmp/fixture-notes.md

cat /tmp/fixture-notes.md
```

To run the fixture test directly:

```bash
cargo test --test cli_migration_notes
```

---

*Introduced: 2026-10-01 · Issue #780*
