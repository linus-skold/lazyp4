# lazyp4

[![CI](https://github.com/linus-skold/lazyp4/actions/workflows/ci.yml/badge.svg)](https://github.com/linus-skold/lazyp4/actions/workflows/ci.yml)

A terminal UI for Perforce (Helix Core), in the style of lazygit. Browse
changelists and their files in panels, read diffs in the shell, and sync,
arrange, shelve, resolve and submit a task without dropping to the command line.

![lazyp4: Status, Files, Changelists and History panels down the left, with a
diff filling the right and the keys for the focused panel along the
bottom](example-lazyp4.png)

## Install

You do not need Rust. On Linux or macOS (Apple Silicon):

```sh
curl -fsSL https://raw.githubusercontent.com/linus-skold/lazyp4/main/scripts/install.sh | sh
```

On Windows:

```powershell
irm https://raw.githubusercontent.com/linus-skold/lazyp4/main/scripts/install.ps1 | iex
```

The script downloads the latest release, checks it against `SHA256SUMS`, and
puts the binary in `~/.local/bin` or `%LOCALAPPDATA%\Programs\lazyp4`. On
Windows it also adds that directory to your user `PATH`. To install a specific
release, set `LAZYP4_VERSION` (for example `v0.2.0`). To use a different
directory, set `LAZYP4_INSTALL_DIR`.

On Debian, Ubuntu, Fedora or RHEL, you can install the `.deb` or `.rpm` from
[Releases](https://github.com/linus-skold/lazyp4/releases) instead:

```sh
sudo apt install ./lazyp4_0.2.0_amd64.deb
sudo dnf install ./lazyp4-0.2.0-1.x86_64.rpm
```

The Linux build needs glibc 2.35 or later. The P4API, OpenSSL and — on
Windows — the C runtime are all linked statically, so it is one file: no DLLs,
no redistributable, nothing else to install.

Or build it yourself, from the repository root:

```powershell
cargo install --path crates/lazyp4
```

Either way, run it with `lazyp4`.

## Connecting

lazyp4 uses the ambient `P4PORT`, `P4USER` and `P4CLIENT`, so run it from a
workspace directory with a `p4config.txt`, the same way you would run `p4`.

It talks to the server through the native API rather than through `p4`, but it
cannot yet accept an SSL fingerprint or fetch a ticket. On a machine that has
never reached your server, use the `p4` CLI once:

```powershell
p4 set P4CONFIG=p4config.txt
p4 trust -y      # only for an ssl: port
p4 login
```

After that lazyp4 stands on its own. Closing that gap is
[issue #1](https://github.com/linus-skold/lazyp4/issues/1).

## Documentation

| | |
| --- | --- |
| [Usage](docs/usage.md) | The panels, and how to move files, submit, shelve, resolve and blame |
| [Keys](docs/keys.md) | Every key, by panel. `?` shows the same thing in the app |
| [Configuration](docs/configuration.md) | Colours, keybindings and the diff tab width |
| [Building](docs/building.md) | Building from source, and what the build script fetches |
| [Releasing](docs/releasing.md) | How to make a release, and what it contains |
| [Design](docs/design.md) | The crates, the worker thread, and where Perforce and git part company |

## Contributing

What is left to build is in
[issues](https://github.com/linus-skold/lazyp4/issues), labelled by area:
`ui` for the terminal interface, `p4-api` for the Perforce layer, and `build`
for packaging and CI.
