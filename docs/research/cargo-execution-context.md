# Cargo execution context

Investigated 2026-11-20. Findings and alternatives; no cwd policy is accepted here.
Dedicated rust-analyzer support is outside scope. The proposed model identifies a Firmware
by its manifest and one Platform; Platform target/CPU are optional and chip is required.

## Current implementation

[Design](../design.md#workspace-and-compiler-isolation) and [runner](../../src/runner.rs)
currently require owning-workspace cwd and removal of inherited `RUSTUP_TOOLCHAIN`.
Firmware commands additionally inject a project-local BSP cache. [Commands](../../src/commands.rs)
group formatting and host checks by owner, while MCU Clippy uses the firmware context.
[Scope](../../src/scope.rs) discovers an unknown owner using Cargo from the package directory,
then loads metadata at the owner root. Revisiting this invariant is a design change, not
something Cargo itself requires.

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

The current resolver pins `cargo-config2` 0.1.45, whose loader has no include processing.
Native include support and polyfw's reported target/CPU can therefore disagree regardless
of cwd. Fixing discovery context alone does not establish resolver parity.
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

## Product alternatives

| Policy | Benefit | Cost or unresolved branch |
| --- | --- | --- |
| A. Owning-workspace cwd everywhere | Preserves current grouping and compiler/config convention | Member configs and pins remain ignored; manifest-based registration still needs owner discovery |
| B. Firmware package cwd for build/MCU lint and matching probes/metadata | Matches manual Cargo invocation inside that firmware; supports member settings | Host checks must separately choose owner grouping or package contexts; firmware contexts cannot merge solely by workspace |
| C. Explicit configurable cwd | Supports unusual layouts | Adds another identity, validation and grouping dimension; can disconnect manifest, configuration and compiler selection |

**Revised recommendation, pending a user decision:** retain owning-workspace cwd for
Cargo operations. External members then use the same workspace configuration ancestry
and compiler pin as internal members, while Platform overrides express firmware target/CPU
differences. This preserves existing check grouping and avoids adding an invocation-directory
setting or a directory-dependent fallback. Member-cwd remains the alternative when honoring
member-local configuration is more important than a shared workspace environment.

## Registration alternatives for an owner-root policy

External-member support is required. Retain the previously agreed package-manifest input;
explicit workspace/package registration was investigated but is not required. Neither
locator should reject a package because its manifest is outside the workspace tree.

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

For the owner-root execution policy under discussion, separate discovery from resolution:

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
