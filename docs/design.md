# Design

polyfw is an installed host CLI that operates on independently configured firmware projects.
[Configuration](configuration.md) defines the input contract and [Usage](usage.md) describes
observable command behavior. This document explains the boundaries that keep those operations
consistent as the implementation changes.

The accepted release-linked configuration compatibility decision is recorded in
[ADR 0001](adr/0001-release-linked-configuration-compatibility.md).

## Ownership and boundaries

| Owner | Responsibility |
| --- | --- |
| Consuming project | Board registry, Cargo workspaces, Rust pins, targets, features, profiles and firmware sources |
| polyfw | Root discovery, registry validation, board/profile selection, check planning, artifact paths and subprocess orchestration |
| Cargo and rustup | Package resolution, compilation, tests, formatting and compiler selection within each workspace |
| probe-rs | Probe access, flashing, reset and RTT sessions |
| VS Code extension | Debug sessions using polyfw's generated launch configuration |

An explicit registry keeps board-to-package and board-to-chip relationships independent of
repository naming conventions. Per-workspace Cargo configuration allows BSPs with different
CPUs or targets to coexist. polyfw does not copy consumer sources, own a shared compiler pin,
or infer firmware from a directory layout.

## Load only the context a command needs

The entry point parses CLI arguments before project discovery. Help, version and schema work
without a project; schema is generated directly from the Rust configuration model. Probe listing
only launches probe-rs from the caller's directory.

Project discovery selects an explicit root or the nearest ancestor containing `polyfw.toml`, then
canonicalizes it. A selected invalid project is an error. Installation location, Git boundaries
and the tool's own source directory do not participate in discovery.

Loading has two stages:

1. **Configuration:** deserialize with unknown-field rejection, validate IDs, workspace paths,
   board-to-BSP references and dependency patterns.
2. **Cargo resolution:** load metadata for every BSP, require registered firmware packages to
   belong to their declared workspace, validate library/binary targets, then resolve target/CPU.

Most project commands require both stages, even when they operate on one board. Selection clearing
needs only configuration and does not read the old state. Reset also uses configuration alone,
except when `--select` must generate a launch file and therefore resolve an ELF path.

Cargo validation counts every declared binary, independently of required features or `default-run`.
An omitted `bin` is inferred into `resolved_bin` for artifact lookup; it remains omitted from build
arguments. Different packages sharing a BSP cannot produce the same binary name because their
ELF paths would collide.

## Configuration compatibility (planned)

This is an accepted design awaiting implementation. The current configuration model does not
yet accept these declarations. Terms are defined in the [glossary](../GLOSSARY.md).

Require two top-level declarations using full polyfw release versions, including any
prerelease suffix. For example, a hypothetical configuration could declare:

```toml
config-version = "0.3.0"
min-polyfw-version = "0.2.0"
```

`config-version` identifies the release whose configuration contract the file follows.
`min-polyfw-version` identifies the oldest CLI expected to interpret and execute that file
correctly. Neither changes merely because another CLI is installed or run.

Each CLI takes its own version from `Cargo.toml` and declares the oldest configuration contract
it accepts. Compare versions by SemVer precedence and require all three conditions:

```text
min-polyfw-version <= config-version
CLI.oldest-supported-config-version <= config-version
min-polyfw-version <= CLI.version
```

A configuration contract newer than the CLI is allowed when its minimum CLI declaration
permits that CLI and the configuration passes the remaining validation. These declarations
express a compatibility promise; they do not replace structural or semantic validation.

Within configuration loading, first parse valid TOML and read the version declarations without
deserializing the rest into the strict configuration model. Check the declarations before
rejecting unknown fields, resolving Cargo, writing selection/editor state or starting hardware
actions. After the gate succeeds, perform the existing strict parsing and validation. Preserve
the [command loading boundaries](#load-only-the-context-a-command-needs), including the lighter
selection-clearing and reset paths.

Missing declarations, invalid versions, inconsistent declarations and incompatible versions
are hard errors. Report the file, relevant declared versions, current CLI and applicable
compatibility floor, with guidance to change CLI or manually migrate the configuration. Do not
infer a version for existing files or suggest that changing a version string alone migrates
their contents. There is no automatic migration or bypass.

Maintain the compatibility floors according to the change, independently of release numbering:

| Change | Policy |
| --- | --- |
| No configuration contract change | Keep both compatibility floors and existing configuration declarations unchanged |
| Add a field or capability | Configurations using it adopt an appropriate contract version and minimum CLI; retain the CLI's existing configuration floor when old configurations still work |
| Remove fields or change meanings/defaults so old configurations no longer retain their meaning | Raise the CLI's configuration floor; also raise the recommended minimum CLI when older CLIs cannot correctly execute the new contract |

Each release provides conservative authoring guidance for its minimum required CLI. Do not
build a per-field version registry or automatically infer a file's minimum CLI. Retain old
support only while the current implementation handles it with its original meaning; raise the
floor when support would require old-field aliases, old-default branches or separate parsers.
The first release implementing the declarations establishes the initial compatibility
baseline; CLIs predating the declarations cannot read the new fields.

Pin the editor's remote `#:schema` URL to the release identified by `config-version`, even when
a newer CLI operates on that configuration. The comment remains editor-only: runtime checks
neither interpret it nor fetch a schema. `polyfw schema` continues to emit its own executable's
model without loading a project; for a local schema, use the executable corresponding to the
declared configuration contract. Keep the existing per-release Schema `$id` convention.

Verification must cover acceptance at compatibility boundaries, rejection below them, newer
CLIs reading retained older contracts, and older CLIs accepting or rejecting newer contracts
according to their minimum CLI declarations. Check required/invalid declarations, diagnostic
priority over unknown fields, actual configuration behavior and the existing command loading
boundaries. Representative fixtures must substantiate retained compatibility; there is no
promise to support every historical release indefinitely.

When implementing this design, update the public [configuration guide](configuration.md),
model-generated Schema and [release preparation](releasing.md#prepare-a-version) together.

## Workspace and compiler isolation

All Cargo processes use `runner::workspace_cargo` or `workspace_command`: they run from their
owning workspace and remove inherited `RUSTUP_TOOLCHAIN`. This includes metadata, workspace
discovery, configuration probes and test-fixture setup. The installed CLI's compiler must not
become an implicit compiler override for a consumer.

Configuration resolution uses `rustup which cargo` from the consuming workspace and gives
`cargo-config2` that absolute path. Its adjacent rustc handles configuration probes, including
when a bare `RUSTC=rustc` would otherwise invoke a rustup proxy. Effective target and CPU settings
include Cargo's file hierarchy and environment overrides. The resolver reads compiler settings;
it does not establish what an arbitrary compiler wrapper does with them.

Firmware Cargo commands additionally set `CARGO_TARGET_DIR` to the operating project's
`target/firmware/<bsp>`. Host checks run from each owning workspace; when that workspace is also a
registered BSP workspace, they reuse its isolated cache. Other workspaces retain their ordinary
Cargo target-directory behavior. Building polyfw itself remains independent of these caches.

## Check planning

`scope` resolves the selected firmware's local check graph from full Cargo metadata. It avoids a
filesystem scan and uses canonical manifest paths as package identity, so same-named packages in
different workspaces remain distinct.

1. Start from selected firmware package IDs. Traverse normal, build and dev edges across all
   platforms and the workspace's resolved features.
2. Exclude registry and Git sources. Select path packages whose canonical directories match
   `config.local-dependencies`; keep traversing through excluded packages. Always select the
   firmware itself.
3. Seed host traversal in each reached package's own workspace to include its local test helpers
   and their dev-dependencies.
4. Normal dependencies preserve the current platform. Build/dev edges, proc-macros and their
   dependencies switch to the host. A package reached in both roles receives both checks.

This produces a review/check scope, not the exact compilation graph of one image. Workspace
feature unification can make optional dependencies reachable; the planner does not enable every
feature. Unrelated root packages remain outside the scope.

Commands turn that graph into grouped stages:

| Stage | Grouping and Cargo selection |
| --- | --- |
| Formatting | Once per owning workspace; select packages by name |
| MCU Clippy | Once per BSP; select runtime package IDs with `--lib --bins` |
| Host Clippy | Once per owning workspace; select package IDs with `--lib --tests --target host-tuple` |
| Host tests | Once per owning workspace; default package test targets and doctests with `--target host-tuple` |

Grouped checks use each workspace's defaults and unify features across selected packages.
They do not repeat each package with a separate feature set. Runtime libraries therefore need to
support both the MCU and host checks. `verify` shares one scope across formatting, lint and test
stages and stops at the first failure. Only checks and detailed `info` resolve this graph.

## Artifact identity

A firmware image is identified by its BSP, package and resolved binary. Board IDs may share that
image. The BSP determines the workspace and cache; its effective target, chosen profile directory
and binary determine the ELF path.

`runner::firmware_elf_path` is the shared path function for information output, hardware commands
and editor generation. Profile-directory mapping lives in `profile`; build arguments and lookup
must keep the same mapping. No caller should reconstruct the path independently.

Builds operate once per board. Package cleaning uses the same isolated BSP cache and can affect
multiple registered boards sharing a package; all-board cleaning runs once per BSP.

## Selection and editor writes

Saved selection is project-local TOML containing a board and optional explicit profile. Absence,
malformed state and a stale board are distinct cases. Profile inheritance and one-time overrides
are resolved before orchestration; saving with no explicit profile clears the previous profile.

A save may precede a build or hardware action. Its success is retained if the later action fails.
With editor generation enabled, the save first replaces the launch file, then writes selection
state. If the state write fails, it restores the previous launch bytes or removes the newly created
file. Each replacement uses a temporary file, rename and cleanup guard. This is rollback between
files, not a filesystem transaction covering both files.

Clearing removes the managed launch file before the state, including a launch file left without
state. Disabled generation leaves editor files untouched during both save and clear. polyfw owns
the whole managed launch file; its entries have no build task and share the ELF resolver.

## Hardware subprocesses

Hardware actions validate their selection and required existing ELF before starting probe-rs.
They do not build or determine source/image freshness. Arguments are explicit and the child's
terminal input/output are inherited.

polyfw installs a Ctrl-C handler that keeps the parent waiting while probe-rs receives the original
terminal signal and cleans up. Process-launch errors retain their OS error; only a missing
executable adds the installation hint. Automated tests use subprocess fixtures, so physical
flashing, RTT and debugger behavior need separate hardware validation.
