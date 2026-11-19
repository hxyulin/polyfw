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
executable, this README, the MIT license and the detailed guides in `docs/`. Check the archive
against the accompanying `SHA256SUMS`, extract it and put the executable on `PATH`.
See [Releasing](docs/releasing.md#assets-and-build-targets)
for the build and test environments.

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

Run `polyfw init` in your existing firmware project, then
[edit the generated configuration](docs/configuration.md#initialize-a-configuration)
to match its workspace, chip and firmware packages.
The `firmware-demos` example registers `led-demo` and `buzzer-demo` as two firmware targets for
the same physical board, sharing the `demo-board` BSP. From that project or any subdirectory:

```sh
polyfw info
polyfw select led-demo --release
polyfw verify --all-boards
polyfw build --all-boards --release
polyfw flash
polyfw select buzzer-demo --release
polyfw flash
```

`verify --all-boards` checks formatting, runs MCU and host Clippy, then runs host tests for both
packages. `build --all-boards --release` creates both release images. Selection chooses which
image to flash. `run`, `flash` and `attach` use an existing image: build again after changing
source. Use `polyfw run` to flash and monitor, or `polyfw attach` to monitor firmware already
running on the board.

Pass a project explicitly when running elsewhere:

```sh
polyfw --project-root /path/to/firmware-demos info
polyfw help build
polyfw verify --all-boards
```

## Documentation

| Task | Guide |
| --- | --- |
| Register workspaces and boards; configure checks and editor generation | [Project configuration](docs/configuration.md) |
| Select, build, test, flash, debug and troubleshoot | [Usage](docs/usage.md) |
| Understand validation, dependency scopes and state management | [Design](docs/design.md) |
| Change polyfw, run checks and maintain generated documentation | [Development](docs/development.md) |
| Prepare and publish stable or prerelease versions | [Releasing](docs/releasing.md) |

The crate and binary archives include these guides for offline reading alongside this README.
Relative links open the bundled documentation for that version. The CLI's `--help` and `schema`
output also describe the installed version.

Licensed under the [MIT license](LICENSE).
