# polyfw

`polyfw` builds, checks, flashes and monitors registered embedded Rust firmware. One installed
binary operates on projects described by `polyfw.toml`. Each project owns its board selection,
generated debug configuration and firmware artifacts. See the [tool design](docs/design.md) for
the command contract and implementation.

## Install and start

This first version is built from source. From a checkout of this repository:

```sh
cargo install --path . --locked
polyfw --version
```

Building polyfw requires Rust 1.100 or newer. This repository pins its development compiler and
installs rustfmt and Clippy. Operating on firmware requires Cargo and rustup, with the compiler,
embedded target, rustfmt and Clippy needed by that project's commands. Hardware commands use an
independently installed [probe-rs](https://probe.rs/docs/getting-started/installation/).
Help, version and schema output require none of these external tools; `probes` needs only probe-rs.
The first version is tested on Linux. crates.io and GitHub Releases distribution is planned.

Every Cargo child selects its compiler from the workspace it operates on. polyfw clears inherited
`RUSTUP_TOOLCHAIN`, including an outer `cargo +VERSION` or environment override, so the tool's
compiler does not override a firmware pin. To try another compiler, run Cargo directly in that
workspace.

- A **board** is a physical programmable target, such as `sentry-chassis` or `bringup-dm-mc02`.
- A **BSP** identifies a board model and its hardware settings. Its registry workspace identifies
  the consuming firmware workspace; robot firmware and bring-up can use different workspaces.
- **Local dependencies** are reachable path packages selected by the registry's directory globs.
  Each registry limits these to its project's owned directories.

Create `polyfw.toml` in the operating project root. Every registered firmware must have a
host-testable library and at least one binary. This example describes a consuming workspace at
the project root, with a package named `controller-firmware`:

```toml
#:schema ./polyfw.schema.json

[config]
local-dependencies = ["./**"]

[config.vscode]
enabled = true

[bsps.controller]
workspace = "."
chip = "STM32H723VGTx"

[boards.controller]
description = "Controller firmware"
bsp = "controller"
package = "controller-firmware"
```

`[config]` and its nested tables hold tool options; `bsps` and `boards` describe firmware.
Omitting `[config]` keeps both defaults shown above. Unknown fields are errors.
Generate a local editor schema with `polyfw schema --output polyfw.schema.json`, or read
[polyfw.schema.json](polyfw.schema.json). Without `--output`, `schema` prints JSON to stdout.
Neither form loads a project or accesses tool source files.

```sh
polyfw info
polyfw select controller --release
polyfw build
polyfw flash
polyfw help build
```

Commands below use example board IDs; replace them with IDs from your project's `polyfw info`.

## Project root

Selection, generated VS Code files, firmware artifacts and probe working directories use the
operating project root. Run polyfw from the project or any subdirectory, or pass `--project-root`:

```sh
polyfw --project-root /absolute/project info
```

The global option can appear before or after the command; a later explicit value overrides an
earlier entry default. Relative paths start at the executable's working directory. Without an
explicit value, the tool searches that directory and its ancestors for the nearest `polyfw.toml`.
Finding none is an error. Invalid explicit roots and invalid discovered configurations fail without
selecting a different project. The reported `repository_root` means this operating project root,
which need not be a Git repository. Help, version, `schema` and `probes` ignore `--project-root`.

Projects may retain existing just recipes as thin wrappers:

```just
[private]
_polyfw *args:
    polyfw --project-root "{{justfile_directory()}}" {{args}}

build *args:
    just _polyfw build {{args}}
```

## Cargo configuration

The registry records workspace paths, probe chips and firmware packages. Each consuming
workspace's Cargo configuration supplies its target and CPU; do not put `target` or `cpu` in
`polyfw.toml`. The workspace must select one explicit `build.target`, or a single
`CARGO_BUILD_TARGET`. Multiple targets and an implicit host default are rejected because one board
command must identify one firmware path. `host-tuple` resolves to the host's actual target name.

`info` and ELF lookup use the effective configuration, including ancestor files, Cargo home and
environment overrides. CPU information uses the last explicit `target-cpu` option; without one,
the table shows `compiler default` and JSON uses `null`. Rustflags response files are rejected
because their CPU options cannot be determined from the resolved configuration.

## Local dependency directories

The optional `config.local-dependencies` string array selects the path packages
included in `fmt`, `lint`, `test`, `verify` and detailed `info` output. Omission uses `["./**"]`.
For example:

```toml
# Include this project's packages and reachable packages in a sibling directory.
[config]
local-dependencies = ["./**", "../common/**"]
```

Relative patterns start at the directory holding `polyfw.toml`; absolute patterns also
work. Patterns form a union and match a package's canonical physical directory, the parent of the
`Cargo.toml` containing its `[package]` metadata. A directory does not include its subdirectories
unless the pattern matches them:

| Pattern | Package directories included |
|---|---|
| `"shared"` | Only `shared` itself, if `shared/Cargo.toml` defines a package |
| `"shared/*"` | Immediate package directories such as `shared/control` |
| `"shared/**"` | `shared` itself and package directories at any depth beneath it |
| `"../common/**"` | A sibling directory and its descendants |

`*` and `?` stay within a directory component; `**` matches zero or more complete components.
Character classes such as `[ab]` and `[!ab]` are supported. Matching is case-sensitive, includes
hidden directories, and normalizes `.` and trailing separators. `..` must precede the first
wildcard. Existing literal directory prefixes are canonicalized. A symlink out of the project is
excluded by the default and may be included with an explicit pattern for that directory.

An empty array includes no dependency packages; selected firmware is always checked. Valid
unmatched patterns include nothing, and malformed patterns are errors before Cargo runs. polyfw
filters Cargo metadata without scanning directories and continues traversal through excluded
packages. Registry and Git dependencies are always excluded. These patterns select packages for
polyfw's checks; Cargo still builds dependencies needed by those packages.

## Selecting a board and profile

```sh
polyfw select sentry-chassis        # save the board, profile unspecified
polyfw select sentry-chassis -r     # save the board and the release profile
polyfw select                       # choose and save from a searchable picker
polyfw select --clear               # remove the saved state and launch.json
polyfw build                        # the saved board and profile
polyfw build --dev                  # dev once; the saved profile stays
polyfw build --select sentry-chassis  # save the board, then build it
polyfw build --all-boards
```

`clean`, `build`, `fmt`, `lint`, `test`, `verify`, `run`, `flash`, `attach` and `reset` use the
saved board when no board is named. With no saved board, a terminal prompts with the picker, and
that choice applies only to the current command. `-s`/`--select` saves the named board, or the
picked one when no name is given. `clean` through `verify` also accept `-a`/`--all-boards`, which
conflicts with a board name and `-s`. The probe commands operate on one board.

`select` and `--select` save a profile only when `--dev`, `-r`/`--release` or `--profile NAME` is
given. The dev and release flags are shorthand for `--profile dev` and `--profile release`, and the
three forms are mutually exclusive. `build`, `run`, `flash` and `attach` inherit the saved
profile, even with a one-time board override; a profile flag overrides it for one invocation.
`--all-boards` ignores the saved profile. An unspecified profile is not passed to Cargo, which then
defaults to dev; an explicit `--dev` is saved and passed. Custom profiles are defined in the BSP
workspace's Cargo configuration. `fmt`, `lint`, `test`, `clean` and `reset` take no profile.

The selection lives in `.polyfw-state/selected`; add `.polyfw-state/`, `target/` and
`.vscode/launch.json` to the project's `.gitignore`. Malformed saved state is an error;
`select --clear` removes it without inspecting Cargo workspaces. Each save also regenerates
VS Code configurations unless disabled as described below. A saved selection is kept if the
following build or hardware action fails.

Build, check and clean output numbers each Cargo stage with its status and elapsed time. Redirected
output drops colors and Unicode marks; `--color always` or `--color never` overrides this. Set
`NO_COLOR=1` to disable colors.

## Board information

`polyfw info [--board [BOARD]] [--json]` is a read-only query. It never builds, connects to a
probe, saves a selection or rewrites VS Code configuration.

A saved board that is no longer registered does not prevent `info` from listing current boards.
The report retains the stale board and profile in `saved_selection` and leaves the saved file
unchanged. Run `polyfw select BOARD` to replace it or `polyfw select --clear` to remove it.
Commands that use the saved board still reject it until it is replaced or cleared.

Without `--board`, it lists all BSPs and boards, highlights the saved selection, and shows whether
each board's dev and release ELF is `present` or `unbuilt`. A different saved profile adds a column.
With `--board BOARD`, it shows the board's package, binary, BSP workspace, target, CPU, chip,
absolute ELF paths and local dependencies. That dependency list is the scope of `fmt`, `lint`,
`test` and `verify`. Its `embedded` packages get MCU and host lint; `host` packages get host lint.
`--board` without a name opens the picker for this query only. ELF presence does not show that the
file is fresh or matches the image on the board.

`--json` writes one object to stdout. Prompts go to stderr, so
`polyfw info --board --json > board.json` still works interactively. Scripts should name the
board, because cancelling or a missing terminal exits nonzero with no report. Fields whose value
would be `null` are omitted; `false` and empty arrays are kept.

| Field | Meaning |
|---|---|
| `repository_root` | Absolute operating project path |
| `saved_selection` | Omitted when absent; otherwise `board` and an optional `profile`. An unspecified profile is omitted, explicit dev stays `"dev"` |
| `bsps` | Map by BSP ID, with absolute `workspace`, resolved `target`, `cpu` (or `null` for the compiler default), `chip` and `registered_boards` |
| `boards` | Map by board ID; `--board` limits it and `bsps` to the requested board and its BSP |
| `boards[ID].description`, `.bsp`, `.package`, `.bin` | Registered configuration and resolved binary name |
| `boards[ID].artifacts` | Ordered `{profile, path, exists}` for dev, release and any different saved profile; absolute paths |
| `boards[ID].local_dependencies` | Details only. `{package, manifest_path, target}` sorted by package and path; `manifest_path` is project-relative (including `../` for external packages), or absolute across filesystem prefixes. `target` is `"embedded"` or `"host"`. The firmware itself is excluded |

## Building, checking and cleaning

```sh
polyfw build --release
polyfw fmt
polyfw lint
polyfw test
polyfw verify --all-boards
polyfw clean
```

Builds and MCU lint run Cargo from the board's BSP workspace, whose `.cargo/config.toml` sets the
target, CPU and linker scripts. Every build uses `--locked`. `build` passes `--bin` only when the
registry names a `bin`; otherwise Cargo builds the package's default library and binary targets.

ELF commands need one binary per board. A board without `bin` must belong to a package with exactly
one binary target, and that target's name is used; several binaries need an explicit `bin`, and the
error lists them. Within one BSP, binaries in different packages must have different names, because
the ELF path has no package component. Several boards may share a package and binary.

`fmt`, `lint` and `test` cover the firmware package and its local dependencies, each from its owning
workspace. `lint` denies warnings and checks libraries and binaries for the MCU, then libraries and
tests on the host. `test` runs each package's default host test targets and library doctests.
`verify` runs every format check, then every lint, then every test, and stops at the first failure.
`--all-boards` runs each package once per owning workspace and each MCU lint once per BSP. These
commands work on whole packages, so other binaries in a package are included.

`clean` runs `cargo clean -p <package>` for one board, which also removes that package's other
binaries, and a full `cargo clean` once per BSP with `--all-boards`. It names every registered board
it affects. It only touches `target/firmware/<bsp>`.

## Flashing and logs

Install [probe-rs](https://probe.rs/docs/getting-started/installation/) on your `PATH`. The commands
call its CLI, checked against version 0.32.0, and display each command before running it.

| Command | Behavior |
|---|---|
| `probes` | List connected probes; needs no board or ELF |
| `run [BOARD]` | Flash the existing ELF with readback verification, start, then monitor RTT/defmt |
| `flash [BOARD]` | Flash the existing ELF with readback verification, reset the target and exit. Aliases `f`, `download`, `d` |
| `attach [BOARD]` | Monitor using the existing ELF, without building or flashing |
| `reset [BOARD]` | Reset the target without building or flashing |

```sh
polyfw build sentry-chassis --release
polyfw flash sentry-chassis --release
polyfw attach sentry-chassis --release
polyfw run sentry-chassis --probe VID:PID:SERIAL --speed 4000
```

`run`, `flash` and `attach` never build: build first. The ELF path is
`target/firmware/<bsp>/<target>/<profile-dir>/<bin>`. An unspecified profile and dev or test use
`debug`, release and bench use `release`, and a custom profile uses its own name. A missing file
reports its path and the build command to run. `attach` needs the ELF that is already running on the
target; a newer build does not match an older image. RTT delivery requires RTT support in firmware.

The four board commands accept `--probe <VID:PID[:SERIAL]>`, `--speed <KHZ>` and
`--connect-under-reset`; the chip comes from the registry. Without them, probe-rs picks a unique
probe, prompts when there are several and uses its configured or default speed. Neither is saved.
`probes` runs from the caller's directory, without a project. `reset` needs only project
configuration and probe-rs; saving a selection also needs Cargo when VS Code generation is enabled.
Other probe commands run from the operating project root; see probe-rs
[configuration](https://probe.rs/docs/tools/probe-rs/#configuration) for presets.

`run` and `attach` default to `--rtt-channel-mode no-block-skip`, which drops whole messages when
the RTT buffer is full. `--no-catch-reset` and `--no-catch-hardfault` turn off probe-rs's capture.
Ctrl-C ends the probe-rs session; the firmware may keep running. Attaching can briefly halt the
core, and `--connect-under-reset` drives the reset line.

The original command implementation was checked on RM-C with a CMSIS-DAP on 2026-10-18, including
probe and speed selection and Ctrl-C. This extracted version has not been tested on hardware.
RTT log delivery remains unverified.

## VS Code debugging

Install the recommended `probe-rs.probe-rs-debugger` extension and the probe-rs CLI on the editor
host's `PATH`, at matching versions. The configuration follows the
[extension's 0.32.0 schema](https://github.com/probe-rs/vscode/blob/v0.32.0/package.json).

```sh
polyfw select sentry-chassis -r
polyfw build
```

Every save of a selection writes two entries to the ignored `.vscode/launch.json`, here
`Launch sentry-chassis (release)` and `Attach sentry-chassis (release)`. Without a saved profile the
names have no suffix and use the `debug` ELF.

Disable all launch file management in the project configuration with:

```toml
[config.vscode]
enabled = false
```

Omitting this table or `enabled` defaults to true. When false, saving and clearing selections
only changes `.polyfw-state/selected`: no launch file is read, created, overwritten or deleted.
Changing the option from true to false leaves an existing file in place. When enabled,
polyfw owns the whole file; saving overwrites it and clearing deletes it.

- **Launch** flashes the existing ELF, verifies it, resets and halts before startup. Set a
  breakpoint and continue to reach `main`.
- **Attach** uses the existing ELF without flashing or resetting. The ELF must match the running
  firmware, and connecting can pause the core.

Neither entry builds; a missing ELF is a debugger error. One-time overrides do not rewrite the file,
and a new save overwrites manual edits. Both entries use SWD, core 0 and RTT channel 0 as defmt in
`NoBlockSkip` mode. With several probes connected, DAP needs an explicit probe selector; CLI probe
flags are not copied into the file.

Host tests cover the generated fields, profiles and clearing. Debugging sessions and RTT through
VS Code have not been checked on hardware.

## Host test fixtures

Fixtures that run checks copy this repository's `rust-toolchain.toml` into their temporary
project root. A dependency workspace that moves outside that root carries its own copy. Setup
commands use the workspace Cargo launcher too. A regression runs the affected tests beneath an
unusable ancestor toolchain so the machine's default Rust installation cannot hide a missing pin.
The other-binary lint test requires its deliberate compiler diagnostic, rather than any failure.

Scope tests canonicalize the temporary directory before writing dependency manifests. On macOS,
`/var` aliases `/private/var`; mixing both paths can make Cargo report a lockfile package collision
when a dependency is reached through both relative and absolute paths. The tests need no `TMPDIR`
override.

Generated configuration writes use a temporary file and rename it into place. A drop guard removes
the temporary file on failure or unwind and is dismissed after a successful rename.

## Development

```sh
cargo run --locked -- --project-root /absolute/project info
just ci
```

`just ci` checks formatting, Clippy with warnings denied, host tests, a release build and dependency
policy. After changing the configuration types, run `just schema` and commit the regenerated
`polyfw.schema.json`. Tests cover project/toolchain isolation, Cargo scopes, command arguments,
selection rollback, disabled editor generation and project-free commands. Probe CLI tests use
fixtures; they do not establish hardware behavior.

See [migration](docs/migration.md) for adapting an existing xtask project.

Licensed under the [MIT license](LICENSE).
