---
status: accepted
---

# Separate firmware, platform and Cargo workspace identities

The terminology redesign makes Firmware the selectable software entry and Platform its
reusable target hardware configuration. Each firmware declares a package name and optional
binary, references exactly one platform and resolves a Cargo working directory. Cargo supplies
workspace membership and package manifest paths from that execution context; polyfw has no
separately named workspace registry. A platform can describe a board model or an MCU type
and can be shared across workspaces. Board refers to physical hardware.

Support firmware packages that are members of a workspace but physically outside its
directory tree. Cargo's workspace membership and canonical package identity determine
ownership; physical containment beneath the workspace root is not a membership test.

The previous Board and BSP concepts mixed software selection, hardware identity and
workspace ownership: two programs for one physical board required two Board entries, while
several BSP entries could point to the same Cargo workspace. Separating these identities
lets firmware and platform names express their actual roles without duplicating Cargo's
workspace model.

Platforms require a compilation target, CPU and probe-rs chip. Use native Cargo
configuration and argument precedence: an explicit `--target` overrides target defaults,
while target-scoped CPU flags follow Cargo's compiler-flag rules, including environment
overrides. Preserve the remaining effective compiler flags, including linker arguments;
do not combine lower-precedence flag sources that Cargo would normally suppress. Target
flags apply to MCU crates; host checks, build scripts and proc macros retain their host
compilation settings.

Both Platform and Firmware may declare `working-dir`, `build-dir` and `target-dir`. Resolve
each field independently: a Firmware value overrides its Platform value. `working-dir` is
the Cargo working directory and must be supplied at one of these levels. Accept absolute
paths and relative paths containing `..`, including directories outside the polyfw project.
Resolve relative directory values against the directory containing polyfw.toml, including
inherited values; they are not relative to `working-dir` or another directory field.

Run firmware metadata, configuration probes, builds and MCU checks from the resolved
working directory. Discovering a different workspace root does not change that directory.
Cargo and rustup determine configuration ancestry and toolchain selection there; workspace
membership still determines the lockfile and manifest profiles. This intentionally permits
different execution contexts within one workspace and replaces the old rule of always
running from its root.

The effective `build-dir` overrides Cargo's directory for intermediate artifacts, and
`target-dir` overrides its directory for final outputs. When either is omitted at both
levels, retain Cargo's native selection for it, including configuration, environment and
workspace defaults. Sharing a build directory with distinct target directories can reuse
compatible intermediates while retaining separate firmware images; Cargo decides what is
reusable. A common Platform is not by itself a complete cache identity: flags, toolchains,
dependency graphs and execution environments can still differ.

Host lint and tests for packages in the firmware's workspace retain its resolved working,
build and target directories. They explicitly select `host-tuple` and do not inject the
Platform CPU. Local dependencies belonging to another workspace are checked from that
workspace's root with its native directory settings. Distinct firmware execution contexts
must not be merged merely because Cargo reports the same owning workspace.

Retain Cargo's native handling of same-named binaries rather than adding a bin-name
uniqueness error. Final outputs that share a path follow Cargo's replacement behavior;
projects can choose distinct target directories when their images must coexist. Do not
introduce a separate per-firmware ELF-copy mechanism for this purpose.

Dedicated rust-analyzer integration is outside this change. Do not generate its settings,
custom editor commands, analysis modes or a new Cargo configuration layout for editor
integration. Existing debugger launch generation is separate. Host tests remain explicit
host-target CLI operations. Native Cargo precedence remains the compiler configuration
contract, including environment overrides of file-based CPU flags.

Each platform declares exactly one compilation target rather than inferring one from Cargo
defaults. An explicit `host-tuple` remains valid. Different platforms can select different
targets within one Cargo workspace; the declared working directory selects its compilation
environment.

Register separate firmware IDs when one package and binary are used with different platforms.
Each ID keeps one platform binding; selecting a firmware determines its platform as well.
There is no temporary platform override on an individual command.

This accepts an intentional configuration and interface migration without old Board or BSP
aliases. Apply the existing [configuration compatibility policy](0001-release-linked-configuration-compatibility.md)
to the changed contract. See the [glossary](../../GLOSSARY.md) for the domain terms and
[Cargo configuration rules](https://doc.rust-lang.org/cargo/reference/config.html) for the
underlying target and compiler-flag precedence.
