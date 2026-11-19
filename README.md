# polyfw

`polyfw` builds, checks, flashes and monitors embedded Rust firmware. Register your existing
Cargo workspaces and boards in `polyfw.toml`, then use the same commands for each board.

A **board** names a programmable target. Its **BSP** identifies the board model, consuming
Cargo workspace and probe chip. Cargo keeps ownership of compiler settings, dependencies
and profiles; polyfw manages board selection, firmware artifact paths and optional VS Code
debug configurations.

## Install

For a published stable version, download a prebuilt binary with
[cargo-binstall](https://github.com/cargo-bins/cargo-binstall):

```sh
cargo binstall polyfw
```

Or build from crates.io with Rust 1.100 or newer:

```sh
cargo install polyfw --locked
```

[GitHub Releases](https://github.com/inaku-Gyan/polyfw/releases) provide archives for
Linux GNU and macOS on x86_64/aarch64, and Windows x86_64 MSVC. Each archive contains the
executable, this README and the MIT license. Check the archive against the accompanying
`SHA256SUMS`, extract it and put the executable on `PATH`.

To request a published prerelease, specify its version:

```sh
cargo binstall polyfw --version 0.2.0-rc.1
# Alternatively:
cargo install polyfw --locked --version 0.2.0-rc.1
```

For an unpublished version or a development checkout, run this from the polyfw repository:

```sh
cargo install --path . --locked
polyfw --version
```

Firmware operations need Cargo and rustup, plus the toolchain, targets, rustfmt and Clippy
required by the consuming project. Flashing and monitoring also need
[probe-rs](https://probe.rs/docs/getting-started/installation/) on `PATH`.

## Use a project

First [configure your existing firmware project](https://github.com/inaku-Gyan/polyfw/blob/main/docs/configuration.md).
The guide uses a board named `controller`; substitute an ID from your own registry.
From that project or any subdirectory:

```sh
polyfw info
polyfw select controller --release
polyfw verify
polyfw build
polyfw flash
```

`verify` checks formatting, runs MCU and host Clippy, then runs host tests. `build` creates
the selected firmware image. `run`, `flash` and `attach` use an existing image: build again after
changing source. Use `polyfw run` to flash and monitor, or `polyfw attach` to monitor firmware
already running on the board.

Pass a project explicitly when running elsewhere:

```sh
polyfw --project-root /path/to/firmware info
polyfw help build
polyfw verify --all-boards
```

## Documentation

| Task | Guide |
| --- | --- |
| Register workspaces and boards; configure checks and editor generation | [Project configuration](https://github.com/inaku-Gyan/polyfw/blob/main/docs/configuration.md) |
| Select, build, test, flash, debug and troubleshoot | [Usage](https://github.com/inaku-Gyan/polyfw/blob/main/docs/usage.md) |
| Understand validation, dependency scopes and state management | [Design](https://github.com/inaku-Gyan/polyfw/blob/main/docs/design.md) |
| Change polyfw, run checks and maintain generated documentation | [Development](https://github.com/inaku-Gyan/polyfw/blob/main/docs/development.md) |
| Prepare and publish stable or prerelease versions | [Releasing](https://github.com/inaku-Gyan/polyfw/blob/main/docs/releasing.md) |

These links open the documentation on `main`, including when this README is read from an
installed crate or binary archive. For a specific release, browse the same files at its Git tag.
The CLI's `--help` and `schema` output describe the installed version.

Licensed under the [MIT license](https://github.com/inaku-Gyan/polyfw/blob/main/LICENSE).
