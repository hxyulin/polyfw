# Project configuration

This guide connects an existing embedded Rust project to polyfw. The example project,
`firmware-demos`, has two firmware packages: `led-demo` and `buzzer-demo`. Each has a matching
board ID, and both run on the same physical board through the shared `demo-board` BSP.
Replace the chip, target, CPU and package names with those of your project.

For everyday commands, see [Usage](usage.md). For package traversal and validation internals,
see [Design](design.md).

## Prepare the Cargo project

polyfw expects working firmware sources, runtime setup and linker scripts. Keep those in
the consuming project. Each firmware package must:

- Be a member of the Cargo workspace registered for its BSP.
- Declare a library that can be checked on both the MCU and the development host.
- Declare at least one binary. A package with multiple binaries needs an explicit `bin`
  in its board registration.

A workspace containing both firmware packages can have this layout:

```text
firmware-demos/
├── .cargo/config.toml
├── .gitignore
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── polyfw.toml
└── firmwares/
    ├── led-demo/
    │   ├── Cargo.toml
    │   └── src/
    │       ├── lib.rs
    │       └── main.rs
    └── buzzer-demo/
        ├── Cargo.toml
        └── src/
            ├── lib.rs
            └── main.rs
```

The root `Cargo.toml` defines a virtual workspace:

```toml
[workspace]
members = ["firmwares/led-demo", "firmwares/buzzer-demo"]
resolver = "3"
```

Relevant parts of `firmwares/led-demo/Cargo.toml`:

```toml
[package]
name = "led-demo"
version = "0.1.0"
edition = "2024"

[lib]
path = "src/lib.rs"

[[bin]]
name = "led-demo"
path = "src/main.rs"
test = false
bench = false
```

The second package, `firmwares/buzzer-demo/Cargo.toml`, declares its own library and binary:

```toml
[package]
name = "buzzer-demo"
version = "0.1.0"
edition = "2024"

[lib]
path = "src/lib.rs"

[[bin]]
name = "buzzer-demo"
path = "src/main.rs"
test = false
bench = false
```

Keep host-testable logic in each package's library. For a hardware-only binary, `test = false` keeps
Cargo's default host test run from creating a binary test harness. The host Clippy stage
still checks enabled test targets; use conditional compilation where host and MCU code differ.
Run Cargo directly to confirm each package's host tests work before registering it.

From the consuming workspace, install its required components and target:

```sh
rustup component add rustfmt clippy
rustup target add thumbv7em-none-eabihf
```

Use the consuming project's Rust pin. polyfw's own development compiler is independent of it.
If a workspace has no lockfile, run `cargo generate-lockfile` there and commit the result.
Keep a lockfile for every workspace used by the checks; build, lint and test use `--locked`.

## Configure the firmware target

Set the target in the consuming workspace's `.cargo/config.toml`. For the example MCU:

```toml
[build]
target = "thumbv7em-none-eabihf"

[target.thumbv7em-none-eabihf]
rustflags = ["-C", "target-cpu=cortex-m7"]
```

Merge these entries into your existing Cargo configuration and retain the linker settings
required by the firmware. CPU selection alone does not provide a runtime or a memory layout.

Each BSP workspace must resolve to exactly one explicit `build.target` or
`CARGO_BUILD_TARGET`. An implicit host default or a list of multiple targets is rejected.
Target, CPU and profiles belong to Cargo configuration, not `polyfw.toml`.

polyfw reads the effective configuration, including ancestor files, Cargo home and environment
overrides. `host-tuple` resolves to the actual host target. A custom target uses its target-file
stem for the artifact directory. The last explicit `target-cpu` option is reported; without one,
the UI shows `compiler default` and JSON reports `cpu: null`. Rustflags response files such as
`@flags.txt` are rejected because polyfw cannot resolve their CPU settings.

Every Cargo subprocess runs from its owning workspace and clears inherited `RUSTUP_TOOLCHAIN`.
This lets firmware and dependencies use their own Rust pins. To test a compiler override, run
Cargo directly in that workspace; an outer `cargo +VERSION` does not override polyfw's children.

## Register the firmware targets

Create `polyfw.toml` at the `firmware-demos` root, which owns selection state, generated editor
files and firmware artifacts. Register both firmware targets against the same BSP:

```toml
#:schema https://raw.githubusercontent.com/inaku-Gyan/polyfw/v0.1.0/polyfw.schema.json

[config]
local-dependencies = ["./**"]

[config.vscode]
enabled = true

[bsps.demo-board]
workspace = "."
chip = "STM32H723VGTx"

[boards.led-demo]
description = "LED demo firmware"
bsp = "demo-board"
package = "led-demo"

[boards.buzzer-demo]
description = "Buzzer demo firmware"
bsp = "demo-board"
package = "buzzer-demo"
```

The board IDs select the two firmware packages for the same hardware. Each package has one
binary, so both registrations omit `bin`. The shared BSP points to the root workspace and chip.

Both `config` tables are optional; omitting them preserves the defaults shown here.
Unknown fields are errors at every level. Board and BSP IDs start with a lowercase letter and
contain lowercase letters, digits and single hyphens, for example `demo-board`.
At least one BSP and one board are required.

| Field | Required | Meaning |
| --- | --- | --- |
| `bsps.ID.workspace` | Yes | Cargo workspace directory relative to the project root; `.` is valid, absolute paths and `..` are rejected |
| `bsps.ID.chip` | Yes | probe-rs chip name used by hardware commands and VS Code |
| `boards.ID.description` | Yes | Label shown in board lists and the selector |
| `boards.ID.bsp` | Yes | ID of a BSP in the same registry |
| `boards.ID.package` | Yes | Firmware package name in that BSP's workspace |
| `boards.ID.bin` | No | Binary target name; omit only if the package declares exactly one binary |
| `config.local-dependencies` | No | Directory globs selecting reachable path dependencies for checks; default `["./**"]` |
| `config.vscode.enabled` | No | Manage the entire `.vscode/launch.json` on selection save/clear; default `true` |

For an explicit binary, add `bin = "led-demo"` to `[boards.led-demo]`. Without `bin`, polyfw
infers the sole binary for ELF lookup but leaves Cargo's default build targets intact.
It counts all declared binaries, including feature-gated targets, independently of `default-run`.
Different packages in one BSP must use distinct binary names because their artifact paths share
one BSP cache.

The `#:schema` URL pins the full polyfw version. Run `polyfw --version` and replace the tag
in the example with that version, including patch and prerelease suffixes: for example,
`0.2.0-rc.1` uses `v0.2.0-rc.1`. Use the same version as the CLI that operates on the project.
Keep the fixed tag instead of `main` or `latest` so later releases cannot change its editor rules.

For offline use or a development build, generate a local schema from the installed executable:

```sh
polyfw schema --output polyfw.schema.json
```

Then replace the remote directive in `polyfw.toml` with:

```toml
#:schema ./polyfw.schema.json
```

Regenerate the local file when changing polyfw versions. The command works without a project,
Rust installation or network access. Its `$id` is the same versioned URL as the released file;
`$schema` identifies the JSON Schema Draft 7 dialect, independently of the software version.

Source builds use the version declared in `Cargo.toml`, without a Git or development suffix.
A future tag's URL is unavailable until the tag is pushed. While developing changes under an
unchanged version, its existing tag still serves the released schema; use the generated local
file to match the development executable.

The schema checks configuration structure; polyfw additionally validates Cargo workspace
membership and targets at runtime.
Most project commands validate every registered BSP before acting, so a broken BSP can prevent
a command for another board from starting.

Add generated files to the consuming project's `.gitignore`:

```gitignore
/.polyfw-state/
/target/
/.vscode/launch.json
```

Then inspect the registry and run the first checks:

```sh
polyfw info
polyfw info --board led-demo
polyfw info --board buzzer-demo
polyfw select led-demo --release
polyfw verify --all-boards
polyfw build --all-boards --release
```

Connect the board before running `polyfw flash`. See [hardware commands](usage.md#flash-and-monitor)
for probe selection and image freshness.

## Select local dependencies for checks

`config.local-dependencies` filters reachable Cargo path packages for `fmt`, `lint`, `test`,
`verify` and detailed `info` output. It matches the canonical directory containing each package's
`Cargo.toml`. Patterns are a union; relative patterns start at the directory holding `polyfw.toml`.

```toml
[config]
local-dependencies = ["./**", "../common/**"]
```

| Pattern | Matching package directories |
| --- | --- |
| `"shared"` | Only `shared` itself |
| `"shared/*"` | Immediate children, such as `shared/control` |
| `"shared/**"` | `shared` and descendants at any depth |
| `"../common/**"` | A sibling directory and its descendants |

`*` and `?` stay within one directory component; `**` spans zero or more complete components.
Character classes such as `[ab]` and `[!ab]` are supported. Matching is case-sensitive and includes
hidden directories. `.` and trailing separators are normalized; `..` must precede the first
wildcard. Existing literal prefixes are canonicalized, so a symlink outside the project is not
covered by the default pattern. Include its physical directory explicitly when needed.

The selected firmware is always checked. An empty array selects no dependency packages; valid
unmatched patterns select nothing, while malformed patterns fail before Cargo runs. Registry and
Git dependencies are excluded. The globs do not scan directories, and traversal continues through
excluded packages. They select polyfw's checks; Cargo still builds whatever dependencies those
packages require. See [check planning](design.md#check-planning) for host and MCU classification.

In this example, `polyfw verify led-demo` checks the LED package; sharing a workspace does not
make `buzzer-demo` its dependency. Use `polyfw verify --all-boards` to check both firmware packages.

## Add boards and workspaces

To add another programmable board, add a `boards.ID` table referring to its package and BSP.
Several boards can share a package and binary. For a different consuming workspace or hardware
configuration, add another BSP and give that workspace its own Cargo configuration and Rust pin.
A BSP workspace can be virtual, and its ID need not be a Cargo package name.

Run `polyfw info`, `polyfw verify --all-boards` and `polyfw build --all-boards --release` after
changing the registry. The project root need not be a Cargo workspace or Git repository.

## Choose who manages VS Code configuration

By default, saving a selection replaces the entire `.vscode/launch.json`, and clearing the
selection deletes it. For a project with a manually maintained launch file, opt out before saving:

```toml
[config.vscode]
enabled = false
```

With generation disabled, saving and clearing touch only selection state. Existing editor files
remain as they are, including a launch file generated before the option was disabled.
See [VS Code debugging](usage.md#debug-with-vs-code) for the generated sessions.

## Optional just wrappers

Projects can expose familiar recipes while keeping polyfw installed separately:

```just
[private]
_polyfw *args:
    polyfw --project-root "{{justfile_directory()}}" {{args}}

build *args:
    just _polyfw build {{args}}
```

The absolute project root makes the wrapper independent of the caller's working directory.
Forward other recipes in the same way. A later explicit `--project-root` overrides an earlier
value if the caller needs to operate on another project.
