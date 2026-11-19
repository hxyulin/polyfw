# polyfw design

The contract for the standalone CLI and optional project-local just wrappers.
The internals may change while these commands and scopes stay the same. Usage is in the
[README](../README.md).

## Goals

- Every physical board is an explicit, discoverable command target.
- Board-model support lives in its BSP workspace, not copied into each firmware directory.
- One small vocabulary selects, builds, checks, flashes and monitors a board.
- Single-board work stays fast; an explicit all-boards check serves CI and review.
- Commands fail early when the registry, Cargo packages and firmware binaries disagree.

Size reports, automatic competition-policy checks and checks of unrelated root packages are out of
scope.

## Registry

`polyfw.toml` is the source of board command targets. Its `bsps` table holds each board
model's consuming workspace path and probe chip. The `boards` table holds each
board's description, BSP, Cargo package and optional `bin`. A board's image is identified by
`(BSP, package, bin)`; a BSP ID need not name a package, and its workspace may be virtual.
`[config]` holds tool options. Its `local-dependencies` string array selects dependency package
directories with globs. It defaults to `["./**"]`; an explicit empty array includes only selected
firmware. `[config.vscode]` holds editor generation options and defaults to `enabled = true`.
Missing tables preserve these defaults, and unknown fields are rejected at every level.

Two alternatives were rejected:

- Inferring boards from robot and board-model directory names hides package and chip relationships
  in code, and a new layout would silently change command behavior.
- One Cargo configuration at the repository root cannot serve two BSPs with different CPUs, and the
  independent host package must not compile for an embedded target.

Project commands validate configuration structure, relative workspace paths and BSP references.
Commands needing Cargo then validate manifests and package membership and resolve target/CPU.
Every firmware must have a library target, so host checks reach its logic. An explicit
`bin` must name a binary target. Without one, the package must have exactly one binary target,
counted regardless of required features and ignoring `default-run`. Validation stores the
inferred name separately from `bin`, so inference never adds `--bin` to a build. Binaries in
different packages of one BSP must have distinct names, because the ELF path has no package
component.

`polyfw schema` prints JSON Schema from the Rust model without loading a project or Cargo metadata.
`--output FILE` writes it to the requested path relative to the caller's current directory.
The source checkout's committed `polyfw.schema.json` is checked against the generator by a test.

The project root is the explicit global `--project-root <DIR>`, or the nearest ancestor of the
working directory containing `polyfw.toml`. Finding neither is an error. Explicit relative roots
start at the caller's working
directory; every selected root is canonicalized. Invalid explicit roots and discovered registries
fail without trying another project. Registry paths, state, generated files and firmware caches
all use the selected root. Root discovery does not depend on Cargo or Git workspaces or where
polyfw was installed. Help, version, schema and probe listing do not load or discover a project.

Clearing selection reads configuration but does not inspect Cargo or old state. Reset also skips
Cargo unless it saves a selection with VS Code generation enabled, which needs a resolved ELF path.
Other project commands keep full Cargo validation. Dependency errors are reported at the action
that requires the tool, preserving an already saved selection if that action fails.

## Cargo configuration

The consuming workspace owns each image's MCU flags and profiles. A BSP ID names a board model;
its registry workspace path names the consuming workspace, which can be virtual. Firmware uses
its own root workspace; BSP bring-up uses separate DM-MC02 and RM-C workspaces to isolate MCU
features. Cargo runs from that workspace for builds and MCU lint.

Target and CPU are derived from that workspace's effective Cargo configuration with
`cargo-config2`, including ancestor files, Cargo home and environment overrides. The registry has
no compiler-setting fields. A board needs one explicit build target so the ELF directory is
unambiguous; `host-tuple` uses the resolved host triple, and a custom target uses its file stem.
The last explicit CPU option is reported, or `null` when the compiler's default applies.
Response-file rustflags are rejected rather than guessing their CPU settings. This reads compiler
configuration; it does not check that a custom compiler wrapper preserves its arguments.

Configuration probes select an absolute Cargo path with `rustup which cargo` from the consuming
workspace, clearing inherited `RUSTUP_TOOLCHAIN`. The resolver uses that toolchain's adjacent
rustc, including when `RUSTC=rustc` names the proxy, instead of inheriting the tool's compiler.

The installed binary is the primary interface. Project-local just wrappers pass their own
absolute `--project-root` and forward arguments. The host-only polyfw crate has its own lockfile,
Rust pin and formatting checks; no consumer keeps a source copy or symlink.

Every Cargo child clears inherited `RUSTUP_TOOLCHAIN` and runs from its operating workspace,
including metadata and workspace discovery. This lets the tool and each consumer use their own
pins. An outer `cargo +VERSION` or environment override does not override these children; use
Cargo directly in the workspace when testing another compiler.

## Check scope

`fmt`, `lint`, `test` and `verify` cover the firmware package and its local dependencies. polyfw
finds them from full [Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html),
with no directory scan:

- Traversal starts at the firmware package IDs and follows normal, build and dev edges, over all
  platforms and each workspace's resolved features.
- Registry and Git sources are dropped before any filesystem check. A path package qualifies if
  its canonical manifest's parent matches a `local-dependencies` directory glob. Relative patterns
  start at the registry directory; absolute paths and leading `..` may include external packages.
  The default `["./**"]` stays within the project. Symlinks match their physical location.
  Exact directory patterns exclude descendants; `*` matches one level and `**` includes zero or
  more levels. Traversal continues through excluded packages to find matching packages beyond them.
- Selected firmware is always checked, even when no dependency patterns match it. Invalid globs
  fail during registry validation; valid unmatched patterns select nothing.
- Each reached package seeds host traversal in its own workspace, which adds its dev-dependencies.
  Packages found only this way get host checks.
- Normal edges keep the platform. Build and dev edges, proc-macros and their dependencies switch to
  the host. A package reached both ways gets both.
- Canonical manifest paths identify packages, so same-named packages in different workspaces stay
  distinct.

The scope does not enable all features or reproduce one firmware's exact compilation graph. Feature
unification across a workspace can make an optional dependency reachable. Unrelated root packages,
including `polyfw`, keep their ordinary Cargo workflows.

Only the check commands and `info --board BOARD` resolve this scope. `select`, `clean`, `build` and
the `info` overview need only registry validation.

## Check commands

Checks are grouped to keep `--all-boards` fast:

- MCU lint: one command per BSP, `--lib --bins`, warnings denied.
- Host lint: one command per owning workspace, `--lib --tests --target host-tuple`, with that
  workspace's defaults and dev-dependencies.
- Tests: one command per owning workspace with `--target host-tuple` and no target-selection flags,
  so unit tests and doctests both run.
- Formatting: `-p NAME` per owning workspace. Lint and tests select full package IDs instead, to
  avoid ambiguous names.

A grouped command unifies features across its packages, and no package is checked again with its
own feature set. Every runtime library must therefore build for both the MCU and the host.

`verify` validates the registry and shares one scope across its stages. It runs all format checks,
then all lints, then all tests, and stops at the first failure.

`clean` passes the BSP workspace as Cargo's directory and `target/firmware/<bsp>` as
`CARGO_TARGET_DIR`, so it touches no other cache. There is no per-binary clean.

## Selection and profiles

The saved board and optional profile are TOML in `.polyfw-state/selected`. A profile is saved only
when given explicitly. An unspecified profile is never passed to Cargo, so Cargo's own default
applies, while an explicit dev is kept and passed. Display names omit an absent profile. Invalid
saved state is an error, never treated as absent. Profiles are defined and validated by Cargo in the
BSP workspace, not in the registry.

Single-board commands inherit the saved profile unless a flag overrides it for that run.
All-board commands use only explicit profiles.

## Target layout

Each consuming project's `target/firmware/<bsp>` holds the BSP workspace's shared firmware cache.
Other workspaces use their ordinary target directory for host checks. Building polyfw uses this
repository's own target directory and is separate from consuming firmware artifacts.

The ELF path is `target/firmware/<bsp>/<target>/<profile-dir>/<bin>`, generated by one pure
function that `build` and the ELF commands share. Unspecified, dev and test profiles map to
`debug`, release and bench to `release`, and custom profiles to their own names.

## Probe commands

`run`, `flash`, `attach` and `reset` call the installed `probe-rs` CLI with the terminal's input and
output; the contract is checked against 0.32.0. They never build, and the caller is responsible for
source freshness. `run` uses `probe-rs run --verify`, `flash` uses
`probe-rs download --verify --reset`, and `reset` needs no ELF.

After selection and ELF checks, polyfw installs a Ctrl-C handler and starts probe-rs. Both processes
share the terminal's process group or console, so probe-rs receives the original Ctrl-C and cleans
up while polyfw waits. Launch failures keep the OS error, and only `NotFound` adds the
installation link.

## VS Code configuration

With `config.vscode.enabled = true`, saving a selection generates `.vscode/launch.json` from the
same resolved board and profile. The
registry supplies the chip and binary; the shared ELF function supplies the path. There is no
adapter wrapper, custom extension, variable provider or `preLaunchTask`, so the file is plain
probe-rs configuration.

polyfw owns the ignored file as a whole and replaces it on every successful save, including a
`--select` whose command later fails. Each file is written by rename. A failed state write restores
the previous launch file or removes the new one. Clearing removes the launch file before the state,
and also removes a launch file left without state.

With `enabled = false`, saving and clearing only operate on `.polyfw-state/selected`, without
reading, generating or deleting editor files. Disabling generation leaves any existing launch
file in place. This applies to `select` and every command's `--select` option.

RTT channel 0 is set explicitly to `Defmt` and `NoBlockSkip`. Setting the channel disables DAP's
format detection, so the format is written with the mode. The first supported scope is local Linux,
one session and one probe; DAP needs an explicit probe selector when several are connected.
