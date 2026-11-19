---
status: accepted
---

# Separate firmware, platform and Cargo workspace identities

The terminology redesign makes Firmware the selectable software entry and Platform its
reusable target hardware configuration. Each firmware identifies a Cargo package by its
manifest and references exactly one platform. Cargo supplies package names and owning
workspaces; polyfw has no separately named workspace registry. A platform can describe a
board model or an MCU type and can be shared across workspaces. Board refers to physical
hardware.

The previous Board and BSP concepts mixed software selection, hardware identity and
workspace ownership: two programs for one physical board required two Board entries, while
several BSP entries could point to the same Cargo workspace. Separating these identities
lets firmware and platform names express their actual roles without duplicating Cargo's
workspace model.

Platforms require a probe-rs chip and may override the compilation target and CPU. Omitted
values retain Cargo's effective configuration. Use native Cargo configuration and argument
precedence: an explicit `--target` overrides target defaults, while target-scoped CPU flags
follow Cargo's compiler-flag rules, including environment overrides. Preserve the remaining
effective compiler flags, including linker arguments; do not combine lower-precedence flag
sources that Cargo would normally suppress. Target flags apply to MCU crates; host checks,
build scripts and proc macros retain their host compilation settings.

Prefer ordinary Cargo configuration as the integration boundary with rust-analyzer and
other Rust tooling. This lets those tools read shared Cargo inputs without introducing
custom editor check commands, and accepts Cargo's native precedence instead of forcing
platform CPU settings above environment flags.

Each firmware must ultimately resolve to one explicit compilation target. If the platform
does not specify one, Cargo must provide exactly one; missing or multiple defaults are
errors. An explicit platform target overrides those defaults. An explicit `host-tuple`
remains valid. Different platforms can therefore select different targets within one Cargo
workspace while sharing that workspace's package resolution and toolchain.

Register separate firmware IDs when one package manifest is used with different platforms.
Each ID keeps one platform binding; selecting a firmware determines its platform as well.
There is no temporary platform override on an individual command.

This accepts an intentional configuration and interface migration without old Board or BSP
aliases. Apply the existing [configuration compatibility policy](0001-release-linked-configuration-compatibility.md)
to the changed contract. See the [glossary](../../GLOSSARY.md) for the domain terms and
[Cargo configuration rules](https://doc.rust-lang.org/cargo/reference/config.html) for the
underlying target and compiler-flag precedence.
