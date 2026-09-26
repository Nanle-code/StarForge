# Install Script Hardening: Checksums, Version Pinning, Rollback

This document covers `install.sh`'s integrity verification, version pinning, and
rollback support (#812) for security-conscious curl-to-bash installs.

---

## 1. Checksum verification

`install.sh` already verifies the downloaded release archive's SHA-256 checksum
against the release's `SHA256SUMS.txt` before extracting anything, and aborts on
a mismatch. This section shows how to verify it **yourself**, independently of
the script, before trusting it to run at all.

### Manual verification flow

```bash
# 1. Download the release archive and its checksums file for your platform/arch.
TAG="v1.4.0"
OS="darwin"        # or "linux"
ARCH="aarch64"      # or "x86_64"
curl -sLO "https://github.com/Nanle-code/StarForge/releases/download/$TAG/starforge-$OS-$ARCH.tar.gz"
curl -sLO "https://github.com/Nanle-code/StarForge/releases/download/$TAG/SHA256SUMS.txt"

# 2. Verify the archive's checksum matches the published one.
grep "starforge-$OS-$ARCH.tar.gz" SHA256SUMS.txt | shasum -a 256 -c -
# or on Linux: grep "starforge-$OS-$ARCH.tar.gz" SHA256SUMS.txt | sha256sum -c -

# 3. Only extract/install after step 2 prints "OK".
tar -xzf "starforge-$OS-$ARCH.tar.gz"
```

`install.sh` performs exactly this flow automatically (using whichever of
`sha256sum`/`shasum` is available) and exits non-zero if the checksum doesn't
match, before the archive is ever extracted.

---

## 2. Version pinning

By default `install.sh` installs the latest GitHub release. Pass a release tag
as the first argument to install an exact, pinned version instead:

```bash
./install.sh v1.4.0
```

This is the same download-and-verify flow as installing "latest" — the pinned
tag's archive and `SHA256SUMS.txt` are fetched and checksum-verified exactly the
same way. Pinning only changes *which* release tag is requested; it does not
weaken verification.

Use this for reproducible environments (CI runners, Docker image builds, fleet
provisioning) where an unannounced new release should never silently change
what gets installed on the next run.

If the pinned tag doesn't exist, the download step fails with a non-zero exit
and nothing is installed or overwritten.

---

## 3. Rollback

Before overwriting an existing `starforge` binary, the installer copies it to
`$INSTALL_DIR/starforge.bak`. If a newly installed version misbehaves, you have
two rollback options:

### Option A — restore the immediate previous binary

```bash
mv "$INSTALL_DIR/starforge.bak" "$INSTALL_DIR/starforge"
starforge --version   # confirm it's back to the prior version
```

This is fast and needs no network access, but only one backup is kept — each
install overwrites the previous `.bak`.

### Option B — reinstall a specific known-good version

```bash
./install.sh v1.3.0
starforge --version   # confirm it matches v1.3.0
```

Use this when you need to go back further than one version, or want the
rollback itself to go through the same checksum verification as a normal
install.

### Manually tested rollback walkthrough

This was exercised by hand as follows, confirming the acceptance criteria
("rollback instructions tested manually"):

1. `./install.sh v1.3.0` — installs v1.3.0, no prior binary exists yet so no
   `.bak` is created.
2. `./install.sh v1.4.0` — installs v1.4.0; `starforge.bak` now contains the
   v1.3.0 binary that was just replaced.
3. Simulate a bad upgrade: confirm `starforge --version` reports v1.4.0.
4. Roll back: `mv "$INSTALL_DIR/starforge.bak" "$INSTALL_DIR/starforge"`.
5. Confirm: `starforge --version` reports v1.3.0 again, with no re-download.

---

## 4. Automated tests

`tests/installer/test_install.sh` exercises this script against stubbed
network calls (no real downloads, no root required). Run it with:

```bash
bash tests/installer/test_install.sh
```

New tests added alongside this doc (`test_pinned_version_skips_latest_lookup`,
`test_backup_created_on_upgrade`, `test_rollback_restores_previous_binary`)
cover: a pinned tag is installed without any "fetch latest" network call, an
upgrade backs up the previous binary to `starforge.bak`, and restoring that
backup (the rollback flow above) recovers the previous version's behavior.
