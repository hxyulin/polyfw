# Cargo execution context

Investigated 2026-11-20. The current design uses an explicit Cargo working directory.
Dedicated rust-analyzer support is outside scope. Firmware declares a package, optional
binary and one Platform. Its working directory comes from Firmware or Platform configuration.
Platform target, CPU and chip are required.

## Implementation before 0.2

[Design](../design.md#workspace-and-compiler-isolation) and [runner](../../src/runner.rs)
previously required owning-workspace cwd and removal of inherited `RUSTUP_TOOLCHAIN`.
Firmware commands additionally injected a project-local BSP cache. [Commands](../../src/commands.rs)
grouped formatting and host checks by owner, while MCU Clippy uses the firmware context.
[Scope](../../src/scope.rs) discovers an unknown owner using Cargo from the package directory,
then loads metadata at the owner root. Replacing that invariant was a design change, not
something Cargo itself requires.

## Current design direction

The user chose an explicit `working-dir` field instead of a package manifest. Both Firmware
and Platform may supply `working-dir`, `build-dir` and `target-dir`; resolve each field
independently, with Firmware overriding Platform. An effective `working-dir` is required.
Run Cargo from this directory so its native configuration, environment and rustup ancestry apply.
Load metadata there and select the named package from workspace members. The resulting
workspace root provides membership, lockfile and profile ownership; discovering it does not
change the execution directory. Metadata/configuration/build contexts must remain aligned.

The effective `target-dir` overrides Cargo's final-output directory, and `build-dir`
overrides its intermediate directory. Omission at both levels leaves the corresponding Cargo
configuration, environment and defaults untouched. Absolute paths and relative paths containing
`..` are supported, including project-external directories. All relative directory inputs start
at the directory containing polyfw.toml, including inherited values; `working-dir` does not
rebase the other fields.
Use metadata's effective `target_directory` when locating Cargo output rather than assuming
a project-local cache. A supplied `target-dir` does not itself override a separately
configured `build.build-dir`; the optional `build-dir` input controls that setting.

Required Platform target/CPU values do not guarantee complete cache equivalence. Compiler
versions, rustflags, features, dependencies and build-script inputs remain relevant. Retain
Cargo's own fingerprint/rebuild decisions. See the [cache/artifact investigation](cargo-cache-and-artifacts.md)
for same-name binary behavior and native sharing with separate final-output directories.
The design retains Cargo's collision behavior without an additional bin-name error or ELF
copy mechanism. Projects that need simultaneous distinct images can choose separate
target directories while sharing compatible intermediates.

Reporting will show Platform CPU as its declared value instead of attempting to reconstruct
the final rustflags. Firmware reports still expose Cargo's discovered workspace and effective
directory/artifact paths. Native Cargo resolves configuration includes, string/array flag
settings, target conditions and environment overrides; polyfw will not retain a parallel
rustflags resolver merely to report an effective CPU. The later requirement for optional
consistency checking may require configuration resolution solely for that comparison.

`check-platform` is optional on both Platform and Firmware, with Firmware overriding Platform
even when false; the final default is true. It compares Cargo's effective target-side target/CPU
after polyfw's injected settings with the declared Platform. A confirmed mismatch or an
inability to verify the effective values reliably is an error; false skips configuration
analysis and checking silently. Perform it before selected builds and target-side Clippy stages,
including when Cargo will reuse cached outputs. Do not apply it to host tests, build scripts,
proc macros, formatting, info, cleaning or hardware commands. This is a configuration-layer
check, not inspection of external compiler/wrapper internals or proof about an existing ELF.

Formatting, host lint and tests of packages in the firmware's workspace use the same resolved
working and output directories. Host lint/tests explicitly select `host-tuple` and do not inject
Platform CPU flags. Checks of local dependencies in other workspaces run from those workspace
roots with their native directory settings. Grouping must preserve distinct execution contexts
rather than merging every registration that shares a workspace, even when formatting the same
source more than once under different toolchains.

Single-firmware cleaning keeps Cargo's package-clean behavior in its resolved context, with
explicit firmware and host targets. All-firmware cleaning delegates full Cargo cleaning for
each distinct directory context. There is no longer an isolated cache boundary protecting
unregistered packages or manual Cargo outputs that share those directories. This breaking
contract will be introduced as polyfw 0.2.0, with both compatibility baselines raised accordingly.

The following directory-inheritance fragment belongs to the 0.2 configuration contract.
Both firmware entries inherit the working and build directories, and override the final-output
directory independently:

```toml
[platforms.demo-board]
target = "thumbv7em-none-eabihf"
cpu = "cortex-m7"
chip = "STM32H723VGTx"
working-dir = "."
build-dir = "target/build/demo-board"
target-dir = "target/demo-board"

[firmwares.led-demo]
description = "LED demo firmware"
platform = "demo-board"
package = "led-demo"
target-dir = "target/firmware/led-demo"

[firmwares.buzzer-demo]
description = "Buzzer demo firmware"
platform = "demo-board"
package = "buzzer-demo"
target-dir = "target/firmware/buzzer-demo"
```

## Facts that constrain the choice

Cargo discovers `.cargo/config.toml` from invocation cwd through every ancestor, including
above the polyfw project, plus `CARGO_HOME`; project/Git/workspace boundaries do not stop it.
Workspace-root invocation ignores member-local configs. Legacy `.cargo/config` wins when
both filenames exist. Deeper scalars win; arrays concatenate with higher-priority entries
later. Environment overrides files; `--config` overrides environment. Includes load recursively,
left to right, before the containing file; include paths start at that file.
[Cargo configuration](https://doc.rust-lang.org/cargo/reference/config.html#hierarchical-structure)

`--manifest-path` chooses a manifest; `-p` chooses packages. Neither changes cwd or config
discovery. A process working directory change does. Thus calling from the caller/project root
with an absolute manifest still reads that root's ancestry, even for an external workspace.
[Cargo build options](https://doc.rust-lang.org/cargo/commands/cargo-build.html#options)

Rustup independently walks cwd ancestors for toolchain files and directory overrides;
`RUSTUP_TOOLCHAIN` outranks both. Clearing it preserves polyfw's existing compiler-isolation
policy but does not neutralize directory overrides or `RUSTC`/wrappers.
[Rustup overrides](https://rust-lang.github.io/rustup/overrides.html)

Config path-valued settings normally resolve relative to the directory above their containing
directory; environment and inline `--config` paths use cwd. Raw rustflags are compiler arguments,
not automatically rewritten paths. With an explicit compilation target, flags apply to target
compilation rather than host build scripts/proc-macros. CPU overrides must preserve native
flag-source precedence, including environment replacement.
[Cargo path and flag rules](https://doc.rust-lang.org/cargo/reference/config.html#config-relative-paths)

Cargo's currently published stable source runs rustc/rustdoc from the workspace root for local
sources beneath it, otherwise from the package root. This is an implementation observation,
not a promise that member-cwd makes `-Clink-arg=-Tfoo` member-relative.
[Cargo path_args](https://doc.rust-lang.org/stable/nightly-rustc/src/cargo/util/workspace.rs.html#128-152)
Build-script execution separately starts in its own package root.
[Build-script inputs](https://doc.rust-lang.org/cargo/reference/build-scripts.html#inputs-to-the-build-script)

A dependency's own `.cargo` settings do not activate merely because Cargo compiles that
dependency: discovery belongs to the invocation. Independently checking it in its owner/package
context may therefore use different flags. This follows from the hierarchy above.

Formatting has separate rules: `cargo fmt -p NAME` selects named workspace packages via metadata;
rustfmt searches `rustfmt.toml`/`.rustfmt.toml` from source-file directories upward.
Owner-root formatting can therefore still honor package formatting configuration.
[cargo-fmt selection](https://github.com/rust-lang/rustfmt/blob/main/src/cargo-fmt/main.rs#L437-L460),
[rustfmt configuration](https://github.com/rust-lang/rustfmt#configuring-rustfmt)

Metadata's graph includes all target platforms by default, and feature activation depends on
the current package. Changing cwd can change that context. Metadata is not an exact compilation
unit graph. Config resolution, cfg probes and firmware metadata should share execution inputs;
metadata caches keyed only by workspace would conceal distinct package contexts.
[Metadata contract](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html#output-format)

The pinned `cargo-config2` 0.1.45 loader has no include processing. The 0.2 checker therefore
rejects includes and conditional target settings as unverifiable when checking is enabled;
when disabled, Cargo handles them directly. Reports expose the CPU declaration without
claiming resolver parity. Fixing discovery context alone would not establish that parity.
[Pinned loader](https://github.com/taiki-e/cargo-config2/blob/v0.1.45/src/de.rs#L131-L184)

## Members outside the workspace directory

Workspace membership and configuration ancestry are independent. Cargo documents
`package.workspace` for a member outside its workspace's directory tree. A temporary
Cargo 1.100 fixture verified this through polyfw's existing Cargo runner: a workspace at
`workspace/` listed `../packages/led-demo`, and that package declared
`workspace = "../../workspace"`. Ordinary and detailed `info` succeeded. Removing the
package's explicit workspace pointer produced the diagnostic that the member was not
hierarchically below the workspace root. This probe established membership handling,
not config or compiler selection.
[External member ownership](https://doc.rust-lang.org/cargo/reference/workspaces.html#the-members-and-exclude-fields)

For such a member, package-cwd invocation still uses the workspace lockfile and manifest
profiles. Its config and rustup searches follow the package's physical ancestors; they do
not jump to the declared workspace. Thus `workspace/.cargo/config.toml` and
`workspace/rust-toolchain.toml` are not inherited merely through membership. Shared settings
need a shared ancestor or an explicit supported reference. Owner-root invocation chooses
the workspace's configuration ancestry instead. An external path dependency is a separate
case: it need not be a member and may have a different owning workspace.
[Cargo configuration ancestry](https://doc.rust-lang.org/cargo/reference/config.html#hierarchical-structure),
[Rustup ancestry](https://rust-lang.github.io/rustup/overrides.html)

## Earlier execution alternatives

| Policy | Benefit | Cost or unresolved branch |
| --- | --- | --- |
| A. Owning-workspace cwd everywhere | Preserves current grouping and compiler/config convention | Member configs and pins remain ignored; manifest-based registration still needs owner discovery |
| B. Firmware package cwd for build/MCU lint and matching probes/metadata | Matches manual Cargo invocation inside that firmware; supports member settings | Host checks must separately choose owner grouping or package contexts; firmware contexts cannot merge solely by workspace |
| C. Explicit configurable cwd | Supports unusual layouts | Adds another identity, validation and grouping dimension; can disconnect manifest, configuration and compiler selection |

The previous owner-root recommendation was superseded by the explicit working-directory
design above. It would preserve check grouping and workspace configuration ancestry, while
member-cwd would honor member-local settings. The explicit directory makes that choice a
project input instead of a tool-imposed inference.

## Earlier registration alternatives for an owner-root policy

These alternatives were superseded by working-directory plus package/bin selection.
External-member support remains required; no locator should reject a package because its
manifest is outside the workspace tree.

| Locator | Implementation | Tradeoff |
| --- | --- | --- |
| Package manifest | Discover the owner, then reload metadata/configuration and execute from that root | Retains the previously discussed input, but needs a discovery/bootstrap rule before the owner's compiler is available |
| Workspace directory plus package name | Select the workspace environment first, load metadata there, then resolve a workspace member by name | Repeats the directory in registrations and package renames require configuration updates, but closely follows the existing implementation |

Cargo metadata already exposes `workspace_root`, member IDs and package manifest paths when
queried with a package manifest. A separate workspace path and package name need not be
maintained in polyfw configuration. Lightweight owner discovery can instead use
`cargo locate-project --workspace --manifest-path <package-manifest>`; this honors Cargo's
ancestor search and explicit `package.workspace`, including external members.
[Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html),
[locate-project](https://doc.rust-lang.org/cargo/commands/cargo-locate-project.html)

The owner-root alternative would separate discovery from resolution:

1. Canonicalize the registered package manifest and ask Cargo for its owning workspace.
2. Canonicalize the returned root, then select Cargo and load metadata/configuration there.
3. Match the canonical package manifest to a package in `workspace_members`; derive its
   package name, ID and binary from metadata, without a containment test.
4. Use the resolved root for subsequent build/check processes, configuration and target/CPU
   probes. Cache owner mappings by canonical manifest and metadata by workspace context.

The current [scope owner lookup](../../src/scope.rs) already performs this two-stage operation
for local dependencies and can supply a shared discovery helper. Discovery needs an available
Cargo before the owner is known; this is an internal bootstrap concern, not a reason to
require duplicated workspace registration. The package's Cargo manifest remains responsible
for a valid `package.workspace` declaration when Cargo needs one.

Caller cwd sacrifices invocation-location independence. Fixed project-root cwd is deterministic
but gives unrelated external packages the project's Cargo configuration and toolchain context.

For every option, preserve explicit package/bin selection, `--locked`, profile semantics,
absolute cache placement and the shared ELF resolver. Keep host lint/tests explicitly
`--target host-tuple`. Workspace ownership still determines the lockfile and manifest profiles.
[Workspace contract](https://doc.rust-lang.org/cargo/reference/workspaces.html)

Owner-root policy has a bootstrap question: `cargo locate-project --workspace` launched from
the package selects its toolchain before discovering the owner, so an unavailable package pin
can prevent discovery. Cargo documents owner lookup through ancestors or `package.workspace`;
choosing a bootstrap compiler or reproducing that lookup is additional policy.
[locate-project](https://doc.rust-lang.org/cargo/commands/cargo-locate-project.html)
