# Usage

Start with [Project configuration](configuration.md) if the project has no `polyfw.toml`.
The commands below use its example board, `controller`. Replace it with a board listed by
your project's `polyfw info`.

## Command overview

Run `polyfw help COMMAND` for the installed version's arguments and examples.

| Command | Purpose | Aliases |
| --- | --- | --- |
| `schema` | Print or write the project configuration's JSON Schema | |
| `info` | Inspect boards, saved selection and existing firmware images | `i` |
| `select` | Save or clear a board and optional profile | `s` |
| `build` | Build firmware | `b` |
| `clean` | Remove firmware artifacts | |
| `fmt` | Format selected source files in place | |
| `lint` | Run MCU and host Clippy, denying warnings | |
| `test` | Run host tests and doctests | `t` |
| `verify` | Check formatting, run Clippy, then run host tests | `v` |
| `probes` | List connected probes | |
| `run` | Flash and verify an existing image, then monitor | |
| `flash` | Flash and verify an existing image, reset and exit | `f`, `download`, `d` |
| `attach` | Monitor an existing image without flashing | |
| `reset` | Reset a board without an image | |

polyfw does not forward arbitrary Cargo arguments. Define targets, features and profiles in the
consuming project, or invoke Cargo directly when an operation needs different flags.

## Choose a project

By default, polyfw searches the working directory and its ancestors for the nearest
`polyfw.toml`. To operate elsewhere:

```sh
polyfw --project-root /path/to/controller info
```

`--project-root` is global and can appear before or after the command. Relative values start
at the caller's working directory; the last explicit value wins. An invalid explicit root or
the nearest invalid configuration fails instead of falling back to another project.

The selected root owns state, editor output, firmware caches and hardware-command working
directories. It need not be a Git repository. Help, version, `schema` and `probes` ignore project
discovery and `--project-root`. Help, version and schema need no external tools; `probes` needs
only probe-rs and runs from the caller's directory.

## Save a board and profile

```sh
polyfw select controller --release
polyfw build
polyfw flash
```

`select` saves the board and any explicit profile in `.polyfw-state/selected`. With editor
generation enabled, it also writes the debug configuration. Use `polyfw select` to choose in a
searchable picker, or `polyfw select --clear` to remove the state and managed launch file.

| Invocation | Board choice | Saved state |
| --- | --- | --- |
| `polyfw build` | Saved board, or a picker if no board is saved | Unchanged |
| `polyfw build controller` | Named board for this invocation | Unchanged |
| `polyfw build --select controller` | Named board | Saved before building |
| `polyfw build --select` | Always open the picker | Saved before building |
| `polyfw build --all-boards` | Every registered board | Unchanged |

The same board rules apply to `clean`, `fmt`, `lint`, `test` and `verify`. Hardware commands
accept a board and `--select`, and operate on one board. `--all-boards` conflicts with a board
argument and `--select`. Pickers require a terminal; scripts should use a board ID or
`--all-boards`.

`build`, `run`, `flash` and `attach` inherit the saved profile, including with a one-time board
override. Use `--dev`, `-r`/`--release` or `--profile NAME` to override it. These forms are mutually
exclusive. Saving a selection retains only an explicitly supplied profile:
`polyfw select controller` clears any previously saved profile. With no explicit or saved profile, polyfw
omits the Cargo profile flag and Cargo's build default applies. An explicit `--dev` is saved.
`--all-boards` uses only the profile passed on that invocation. `clean`, `fmt`, `lint`, `test`,
`verify` and `reset` take no profile option.

A stale or malformed saved selection is an error. A one-time board override still reads that
state; replace it with `polyfw select controller` or remove it with `polyfw select --clear`.
Clearing works without Cargo and without parsing the old state. A successful save remains even
if the following build or hardware action fails.

## Inspect boards and images

```sh
polyfw info
polyfw info --board controller
polyfw info --board controller --json > controller.json
```

The overview lists all boards, their referenced BSPs, the saved selection, and dev/release ELF
presence. A different saved profile adds another artifact column. Details show one board's
package, binary, workspace, target, CPU, chip, absolute ELF paths and check dependencies.
`polyfw info --board` opens the picker for this query without saving the choice.

`info` does not build, connect to hardware, change selection or write editor files. A stale board
in otherwise valid saved state is reported without blocking the query. Malformed state still
fails. An ELF marked `present` may be older than the source or differ from the running image.

The JSON view contains:

| Field | Meaning |
| --- | --- |
| `repository_root` | Absolute operating project root |
| `saved_selection` | Saved `board` and optional `profile`; omitted if no state exists |
| `bsps` | Map of BSPs referenced by the reported boards, with `workspace`, `target`, `cpu`, `chip` and `registered_boards` |
| `boards` | Map of boards, limited to the requested board when using `--board` |
| `boards[ID].description`, `.bsp`, `.package`, `.bin` | Registry values and the resolved binary name |
| `boards[ID].artifacts` | Ordered `{profile, path, exists}` entries for dev, release and any different saved profile |
| `boards[ID].local_dependencies` | Details only: dependency `{package, manifest_path, target}` entries, excluding the firmware itself |

Workspace and artifact paths are absolute. Dependency manifest paths are project-relative,
including `../` for external packages, or absolute when they cannot share a filesystem prefix.
Dependencies are sorted by package name and path; their `target` is `embedded` or `host`.
`cpu` is `null` when no CPU is explicit. Missing saved selection, saved profile and overview
dependency details are omitted; `false` and empty arrays are retained.

JSON goes to stdout; interactive prompts go to stderr. Name the board in scripts so terminal
availability and picker cancellation cannot change the result.

## Build, check and clean

```sh
polyfw build controller --release
polyfw fmt controller
polyfw lint controller
polyfw test controller
polyfw verify --all-boards
polyfw clean controller
```

Builds run from the BSP's Cargo workspace with `--locked`. An explicit registry `bin` adds
`--bin`; otherwise Cargo builds the package's default targets. Every board is built separately,
including with `--all-boards`.

Checks cover the firmware package and its configured, reachable local dependencies:

- `fmt` runs `cargo fmt` and **rewrites source files**.
- `lint` checks runtime libraries and binaries for the MCU, then selected libraries and tests
  for the host. Both stages deny warnings.
- `test` runs the selected packages' default host test targets and library doctests.
- `verify` checks formatting without rewriting files, then runs all lint stages, then all test
  stages. It stops at the first failure. It does not build a release image.

Checks operate on whole packages, so other binaries can be included. MCU lint is grouped per
BSP; host work is grouped and deduplicated per owning workspace. Grouped commands use workspace
defaults and unify package features. The scope is not an exact reproduction of one image's
compilation graph. See [configuration](configuration.md#select-local-dependencies-for-checks)
to control dependency selection.

All BSP firmware artifacts use `target/firmware/<bsp>` under the operating project. For the
example release image:

```text
target/firmware/controller-model/thumbv7em-none-eabihf/release/controller-firmware
```

Absent, dev and test profiles use `debug`; release and bench use `release`; custom profiles use
their own names. Define custom profiles in the consuming Cargo workspace.

`clean controller` cleans that package in its BSP cache, affecting its other binaries and boards
sharing the package. `clean --all-boards` removes each registered BSP's full firmware cache.
The command lists the boards it affects. Other build directories remain independent.

## Flash and monitor

Install [probe-rs](https://probe.rs/docs/getting-started/installation/) on `PATH`, configure host
probe access, then connect the board:

```sh
polyfw probes
polyfw build controller --release
polyfw flash controller --release
polyfw attach controller --release
# Or flash and start monitoring in one command:
polyfw run controller --release
```

`run`, `flash` and `attach` use an existing ELF and **never build**. `attach` needs the ELF that
matches the running firmware. `reset` requires no ELF. The registry supplies the chip.

The four board hardware commands accept transient `--probe VID:PID[:SERIAL]`, `--speed KHZ` and
`--connect-under-reset` options. Omitted probe/speed options are delegated to probe-rs. For example:

```sh
polyfw run controller --release --probe VID:PID:SERIAL --speed 4000
```

`run` delegates to `probe-rs run --verify`; `flash` uses `probe-rs download --verify --reset`.
`run` and `attach` default to RTT `no-block-skip`, which drops whole messages when the buffer
is full. Choose `--rtt-channel-mode no-block-trim` or `block-if-full` if needed. RTT/defmt output
requires matching support in the firmware. `--no-catch-reset` and `--no-catch-hardfault` disable
probe-rs capture for those events.

Ctrl-C ends the probe-rs session; firmware may continue running. Attaching may briefly halt the
core, and connecting under reset drives the reset line. Board hardware commands run from the
operating project root, so project-relative probe-rs settings resolve there.

The integration targets the probe-rs 0.32.0 CLI contract. Automated tests check command
construction and subprocess behavior; they do not establish physical flashing or RTT behavior.
Hardware sessions for this standalone tool remain unverified.

## Debug with VS Code

Install `probe-rs.probe-rs-debugger` and a matching probe-rs CLI on the editor host. Generated
settings follow the extension's
[0.32.0 schema](https://github.com/probe-rs/vscode/blob/v0.32.0/package.json).
Open the operating project root as the VS Code folder, then:

```sh
polyfw select controller --release
polyfw build
```

The saved selection generates `Launch controller (release)` and `Attach controller (release)`
in `.vscode/launch.json`. A selection without an explicit profile omits the name suffix and uses
the debug ELF.

- **Launch** flashes and verifies the existing image, resets and halts before startup. Set a
  breakpoint and continue to reach the application.
- **Attach** uses the existing image without flashing or resetting; connecting may pause the core.

Both use SWD, core 0 and RTT channel 0 with `Defmt`/`NoBlockSkip`. Neither builds the image.
One-time CLI overrides leave the launch file unchanged; every selection save replaces the
entire file, and clearing selection deletes it. CLI probe flags are not saved into debugger
settings. Multiple connected probes need an explicit DAP probe selector; use a manually managed
configuration when that is required.

Set [`config.vscode.enabled = false`](configuration.md#choose-who-manages-vs-code-configuration)
to retain a manually maintained launch file. Physical VS Code sessions and RTT delivery remain
unverified; binary platform smoke tests do not verify these sessions.

## Use polyfw in scripts and CI

Install a pinned published polyfw version and the consuming project's toolchains, targets,
rustfmt and Clippy. From the project root, use explicit board scope:

```sh
polyfw verify --all-boards
polyfw build --all-boards --release
```

Use `polyfw info --board controller --json` when a script needs resolved paths. Build and check
output includes numbered stages, status and elapsed time; redirected output omits colors and
Unicode marks. `--color always` and `--color never` override automatic color selection;
`NO_COLOR=1` disables automatic colors. Cargo may need network access to resolve dependencies.

## Troubleshoot

| Symptom | Action |
| --- | --- |
| No `polyfw.toml` found | Run from the consuming project or pass `--project-root DIR` |
| A command fails on an unrelated BSP | Fix every registered BSP's workspace and Cargo configuration; most project commands validate the whole registry |
| No explicit target or multiple targets | Set one `build.target` or `CARGO_BUILD_TARGET`; inspect environment overrides |
| Package has no library target | Add a host-testable library to the firmware package |
| Multiple binary targets | Set the board's `bin` to a declared binary; `default-run` does not select it for polyfw |
| Saved board removed or state malformed | Run `polyfw select controller` to replace it, or `polyfw select --clear` |
| Unexpected release/dev image | Inspect saved profile with `info`; use an explicit profile or save a new selection |
| ELF missing | Run the reported build command for that board and profile |
| Host lint/test tries to compile hardware code | Separate host-testable library code and configure target-specific binary/test compilation |
| A local dependency is missing from checks | Inspect `info --board controller` and canonical directory-glob matching |
| `--locked` fails | Update and commit the owning workspace's lockfile with Cargo |
| Probe missing or ambiguous | Check `polyfw probes`, host permissions and the explicit `--probe` selector |
| VS Code changes disappear | Disable launch generation before maintaining that file yourself |
