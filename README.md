# lazyp4

[![CI](https://github.com/linus-skold/lazyp4/actions/workflows/ci.yml/badge.svg)](https://github.com/linus-skold/lazyp4/actions/workflows/ci.yml)

A terminal UI for Perforce (Helix Core), in the style of lazygit. Browse
changelists and their files in panels, read diffs in the shell, and sync,
arrange, shelve, resolve and submit a task without dropping to the command line.

```text
┌ 1  Status ───────────────┐┌ 0  Diff  Foo.cpp#3 ─────────────────────┐
│ user    linsko           ││ @@ -38,7 +38,8 @@                       │
│ client  linus-desktop    ││ 38 38  Intermediate/                    │
│ stream  //darksim/main   ││ 41    -let x = compute(alpha, beta);    │
├ 2  Files of 395 ─────────┤│    41 +let x = compute(alpha, gamma);   │
│   ▾ Source/ 2            ││    42 +Binaries/                        │
│ M    Door.cpp            ││                                         │
│ A    Tool.cpp            ││                                         │
├ 3  Changelists ──────────┤│                                         │
│ Local 2 │ Shelved 1 │ …  ││                                         │
│ ▸    395 # Do not submit ││                                         │
│ ▸    308 Interaction     ││                                         │
├ 4  History (1) ──────────┤│                                         │
│ ✓    396 # Updated .p4…  ││                                         │
└──────────────────────────┘└─────────────────────────────────────────┘
 space  move   d  revert   H  history   a  blame   r  refresh   ? help
```

## Install

Take the archive for your platform from
[Releases](https://github.com/linus-skold/lazyp4/releases) and put the binary on
your `PATH`. The P4API, OpenSSL and — on Windows — the C runtime are all linked
statically, so it is one file: no DLLs, no redistributable, nothing else to
install.

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
| [Design](docs/design.md) | The crates, the worker thread, and where Perforce and git part company |

## Contributing

What is left to build is in
[issues](https://github.com/linus-skold/lazyp4/issues), labelled by area:
`ui` for the terminal interface, `p4-api` for the Perforce layer, and `build`
for packaging and CI.
