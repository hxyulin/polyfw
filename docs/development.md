# Development

This guide is for changes to polyfw itself. To integrate an existing firmware project, use
[Project configuration](configuration.md); to understand implementation constraints, read
[Design](design.md).

## Set up a checkout

Use rustup with the repository's `rust-toolchain.toml`, plus `just`. The pin includes rustfmt and
Clippy. Install the embedded target used by Cargo-running test fixtures, even though polyfw is a
host application:

```sh
rustup target add thumbv7em-none-eabihf
just check
```

`just check` checks Rust formatting, runs Clippy for all targets with warnings denied, runs the
Rust tests, and builds a release executable.

Use `just --list` for repository recipes. `just fix` formats source and applies Clippy fixes;
review those changes before committing. The Rust minimum in `Cargo.toml` is the supported compiler
floor; `rust-toolchain.toml` selects the development compiler. Assess both when introducing Rust
features or updating dependencies.

## Run against a consuming project

`just run` runs the development CLI with locked dependencies. Pass polyfw arguments directly
after the recipe name; their boundaries are preserved, including quoted paths with spaces.
With no arguments, it prints CLI help.

The polyfw repository has no firmware registry of its own. Point the development binary at an
existing consumer:

```sh
just run --project-root /path/to/firmware-demos info
just run --project-root /path/to/firmware-demos verify --all-boards
```

Or install this checkout with `cargo install --path . --locked`. Cargo children launched by polyfw
use the consuming workspaces' pins rather than inheriting the compiler used to run polyfw.

## Find the relevant code

| Files | Responsibility |
| --- | --- |
| `src/main.rs`, `src/cli.rs` | Project-free dispatch, loading boundaries and CLI arguments/help |
| `src/model.rs`, `src/registry.rs` | Configuration/schema model, root discovery, validation and saved state |
| `src/config_version.rs` | Version preflight, SemVer precedence and release-maintained compatibility baselines |
| `src/init.rs` | Configuration template and exclusive creation without project loading |
| `src/cargo_config.rs` | Effective Cargo target/CPU and toolchain resolution |
| `src/local_dependencies.rs`, `src/scope.rs` | Canonical directory matching and host/MCU package scopes |
| `src/commands.rs` | Board/profile resolution and ordered Cargo stages |
| `src/runner.rs`, `src/profile.rs` | Process construction, cache isolation and shared artifact paths |
| `src/probe.rs`, `src/vscode.rs` | Hardware subprocesses and generated debugger settings |
| `src/info.rs`, `src/ui.rs` | Read-only reports and terminal output |
| `tests/project_root.rs`, `tests/standalone.rs` | Executable-level project isolation and external-tool boundaries |
| `.github/workflows/release.yml` | Tag validation, native binary archives and publication |

Unit tests live beside their implementation. CLI parser tests check argument combinations;
subprocess fixtures check commands without accessing hardware.

## Maintain generated and public documentation

Each description has one source:

| Surface | Source to edit | Follow-up |
| --- | --- | --- |
| CLI help | Documentation comments and help strings in `src/cli.rs` | Inspect `just run help COMMAND` |
| Configuration Schema | Model comments and schema annotations in `src/model.rs` | Run `just schema` and commit `polyfw.schema.json` |
| Installation and navigation | `README.md` | Keep relative guide links usable in source, crate and binary archives |
| Configuration rules | `docs/configuration.md` | Match model defaults, Cargo requirements and validation |
| User operations and JSON output | `docs/usage.md` | Match CLI parsing, command behavior and serialization |
| Architecture and invariants | `docs/design.md` | Update alongside changes to ownership, algorithms or boundaries |
| Contributor workflow | This document | Match recipes and automation |
| Publication procedure | `docs/releasing.md` | Match the tagged workflow |
| Agent instructions | `AGENTS.md` | Keep repository-specific rules and conditional document pointers concise |

The schema equality test rejects output that differs from the Rust generator. Editing the JSON
alone is not sufficient. Its `$id` uses the compile-time `CARGO_PKG_VERSION` and the corresponding
`v<VERSION>` source tag; `$schema` remains the Draft 7 dialect URI. Regenerate it after package
version changes as well as model changes. Follow the
[version preparation steps](releasing.md#prepare-a-version) to keep the committed schema and
configuration guide's pinned link aligned. The release workflow checks that link, and every
release binary must emit the committed schema.

The schema describes configuration structure; Cargo-dependent constraints still need runtime
validation and prose explanation. Use a locally generated schema for an untagged development
build, as described in [Configuration](configuration.md#register-the-firmware-targets).

Use English throughout, with the `firmware-demos` project for user-facing examples: `led-demo`
and `buzzer-demo` share the `demo-board` BSP. Put complete explanations in their owning guide
and link from other pages. Use relative links between bundled guides and README so they work
offline with the matching release. The [publication contract](releasing.md#prepare-a-version)
defines which documentation is included. Use explicit online links for references to excluded
repository files.

## Keep test fixtures isolated

Cargo-running fixtures must carry a copy of the repository Rust pin in their temporary project
root. A dependency workspace outside that root needs its own pin. Set up their Cargo commands
through the workspace launcher as well. The ancestor-toolchain regression checks that an unusable
outer pin cannot be hidden by a developer's default installation.

Canonicalize temporary roots before writing relative or absolute path dependencies. On macOS,
`/var` and `/private/var` can name the same directory; mixing them can produce lockfile package
collisions. Fixtures do not need a `TMPDIR` override. Tests expecting a compiler failure must check
the intended diagnostic so an unrelated setup failure cannot count as success.

Ordinary fixture builds may emit unused-dependency warnings. CI explicitly allows those build
warnings; the repository's Clippy command still uses `-D warnings`. Hardware command and debugger
fixtures establish generated arguments/settings, not physical device behavior.

## Validate changes

- For Rust, CLI help or model documentation changes, run `just check`. Regenerate the schema when
  its model descriptions, fields or package version change, and inspect the affected command help.
- For workflow or packaging changes, run `just check` and the package/dry-run checks in
  [Releasing](releasing.md#prepare-a-version).
- For Markdown-only changes, verify links, heading anchors, configuration snippets and command
  claims against their sources. Run affected examples where practical.
- Before committing, run `git diff --check` and review the complete diff, including generated files.

## Understand repository automation

[CI (online)](https://github.com/inaku-Gyan/polyfw/blob/main/.github/workflows/ci.yml) runs the
Rust checks on pushes to `main` and non-draft pull requests. Markdown-only changes are skipped.
Marking a PR ready starts checks; returning it to draft cancels its current run. New commits
cancel older runs for the same ref. The release workflow also calls CI before packaging and
publishing.

[Dependabot (online)](https://github.com/inaku-Gyan/polyfw/blob/main/.github/dependabot.yml)
checks Cargo and GitHub Actions every Monday at 09:00 Asia/Shanghai. Minor/patch updates are
grouped per ecosystem; major updates are separate.

For publishing credentials, stable/prerelease tags, binary targets and retry rules, follow
[Releasing](releasing.md).
