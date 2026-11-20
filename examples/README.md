# Examples

These small host projects demonstrate polyfw configuration and serve as integration-test inputs.
They use `host-tuple`, the `native` CPU and local path dependencies. Their programs only print
messages; Rust, rustfmt and Clippy are sufficient to run them.

| Project | What it exercises |
| --- | --- |
| [minimal](minimal) | One package: project discovery, selection, builds and artifact paths |
| [multi-firmware](multi-firmware) | Two firmware packages sharing a library: switching and dependency scopes |
| [multi-workspace](multi-workspace) | Three workspaces, external firmware members, shared dependencies and a two-bin package |

Each directory is a self-contained consuming project. Copy the whole directory when trying one
outside this checkout. Each Cargo workspace includes its own lockfile and Rust pin.

## Run an example

With polyfw installed, enter an example directory and run:

```sh
polyfw info
polyfw verify --all-firmwares
polyfw build --all-firmwares --release
```

From the polyfw repository, use the development binary instead:

```sh
just run --project-root examples/minimal info
just run --project-root examples/minimal select hello --release
just run --project-root examples/minimal build
just run --project-root examples/minimal info --firmware hello --json
```

The JSON report gives the built executable's path. Run that executable directly to see `hello`.
For `multi-firmware`, select `led-demo` or `buzzer-demo`; their executables print their names and a
shared value. For `multi-workspace`, select `first`, `second`, `multi-one` or `multi-two` as
described below.

The host platform's `test-chip` is a placeholder for the required chip field. Editor generation
is disabled. For hardware commands, configure an embedded target, CPU and real probe-rs chip as
described in [Project configuration](../docs/configuration.md).

## Multiple workspaces

`multi-workspace` keeps shared libraries in the root workspace and firmware packages outside
their owning BSP workspaces:

```text
multi-workspace/
├── Cargo.toml                 # members = ["shared/*"], exclude = ["bsp", "firmwares"]
├── Cargo.lock
├── rust-toolchain.toml
├── polyfw.toml
├── shared/
│   ├── common/
│   ├── first-only/
│   └── second-only/
├── bsp/
│   ├── a/                     # workspace: first, multi; its own lockfile and Rust pin
│   └── b/                     # workspace: second; its own lockfile and Rust pin
└── firmwares/
    ├── first/
    ├── second/
    └── multi/                 # src/bin/one.rs and src/bin/two.rs
```

Each firmware package explicitly sets `package.workspace` to its BSP workspace. Path dependencies
are declared in that workspace's `workspace.dependencies` and inherited by the firmware packages.
The shared libraries belong to the root workspace throughout.

| Firmware ID | Package | BSP workspace | Shared dependencies |
| --- | --- | --- | --- |
| `first` | `first` | `bsp/a` | `common`, `first-only` |
| `second` | `second` | `bsp/b` | `common`, `second-only` |
| `multi-one` | `multi` | `bsp/a` | `common` |
| `multi-two` | `multi` | `bsp/a` | `common` |

The two registrations for `multi` specify different `bin` values. Select and build either one:

```sh
polyfw select multi-one --release
polyfw build
polyfw select multi-two --release
polyfw build
```

Platforms a and b use `bsp/a` and `bsp/b` as their working directories, with separate output roots
at `target/firmware/a` and `target/firmware/b`. Shared-library host checks use the root workspace's
native `target` directory. The BSP directories are workspace containers; all programs run on the
host and print which dependencies they use.

## Run the integration tests

```sh
cargo test --locked --test examples
```

The tests copy these projects into temporary directories, invoke the built polyfw executable,
and build and run the reported host artifacts. They check external workspace membership and
independent builds of a package's two bins. Failing tests introduced in the copies check that
polyfw includes reachable shared dependencies and excludes unrelated packages. The tests run
without network access and are included in `just check`.
