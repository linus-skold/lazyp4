# Releasing

## Make a release

1. On GitHub, open **Actions → Prepare release → Run workflow**.
2. Type the version without the `v`, for example `0.2.0`.
3. When the run is complete, open **Releases**. Examine the draft, then
   publish it.

`.github/workflows/prepare-release.yml` does these steps:

1. It sets `version` in `[workspace.package]` in `Cargo.toml` and updates
   `Cargo.lock` to agree.
2. It commits the change to `main` as `chore(release): v0.2.0`. If the version
   is already correct, it makes no commit.
3. It tags the commit `v0.2.0` and pushes the commit and the tag together. If
   `main` changed during the run, the push fails and neither lands.
4. It runs `.github/workflows/release.yml` for the tag.

The install scripts download from `/releases/latest`, which does not include
drafts. A release is not available to the scripts until you publish it.

## Why the version changes before the tag

The binary gets its version from `Cargo.toml` when it is built. If you push the
tag first and a workflow then changes the version, the tag points to a commit
with the old version. The release then contains a binary that reports the
wrong version, and the tag does not show the code that was released.

## Tag by hand

You can also change the version, commit it, and push a `v*` tag yourself.
`release.yml` starts on the tag. Its first job stops the release if the tag and
`Cargo.toml` do not agree.

A tag that a workflow pushes with `GITHUB_TOKEN` does not start other
workflows. For this reason, `prepare-release.yml` calls `release.yml` directly.

## What a release contains

| File | Contents |
| --- | --- |
| `lazyp4-windows-x86_64.zip` | `lazyp4.exe` and `README.md` |
| `lazyp4-linux-x86_64.tar.gz` | `lazyp4` and `README.md` |
| `lazyp4-macos-arm64.tar.gz` | `lazyp4` and `README.md` |
| `lazyp4_<version>_amd64.deb` | Installs `/usr/bin/lazyp4`. nfpm makes it from `packaging/nfpm.yaml` |
| `lazyp4-<version>-1.x86_64.rpm` | The same, for RPM |
| `SHA256SUMS` | The install scripts check their download against this file |

Do not rename the archives. The install scripts download them by name.

## Limits

- The Linux build runs on `ubuntu-22.04`, so it needs glibc 2.35 or later. If
  you move it to a different runner, change the `depends` in
  `packaging/nfpm.yaml` to agree.
- If you enable the `Default` ruleset on `main`, its pull request rule stops
  the workflow from pushing the version commit.
- Each release distributes the P4API, which is linked statically into the
  binary. Its licence is not clear about redistribution. See
  [issue #10](https://github.com/linus-skold/lazyp4/issues/10).
