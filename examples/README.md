# Examples

These small Cortex-M projects demonstrate polyfw configuration and serve as integration-test
inputs. They use the real `thumbv7em-none-eabihf` target, Cortex-M7 or Cortex-M4 CPUs, and STM32
chip identifiers. Minimal `cortex-m-rt` startup code and linker layouts produce ARM ELF files;
the firmware loops call library functions. The libraries support host tests.

| Example platform | CPU | probe-rs chip |
| --- | --- | --- |
| `minimal`, `multi-firmware`, `multi-workspace` platform a (DM-MC02) | `cortex-m7` | `STM32H723VGTx` |
| `multi-workspace` platform b (RM-C) | `cortex-m4` | `STM32F407IG` |

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
rustup target add thumbv7em-none-eabihf
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

The JSON report gives the linked ARM ELF's path. For `multi-firmware`, select `led-demo` or
`buzzer-demo`. For `multi-workspace`, select `first`, `second`, `multi-one` or `multi-two` as
described below.

Each firmware binary disables its host test harness; library tests run on the host while builds
and target-side Clippy use the MCU target. The linker layouts reserve the first 384 KiB of flash
and 128 KiB of RAM (DM-MC02 DTCM or RM-C SRAM). The matching Cargo configuration lives beside each
layout and supplies the target, CPU and linker script. Each firmware package's build script adds
the absolute memory-layout directory to the linker search path, including for external members.

These examples validate cross-compilation, linking and polyfw orchestration. Board execution is
a separate manual check. Editor generation is disabled. See
[Project configuration](../docs/configuration.md) for hardware and editor setup.

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
│   ├── a/                     # DM-MC02 workspace: first, multi; .cargo/config.toml + memory.x
│   └── b/                     # RM-C workspace: second; .cargo/config.toml + memory.x
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
native `target` directory. Each BSP directory contains its Cargo configuration, memory layout,
lockfile and Rust pin; the firmware packages remain outside it.

## Run the integration tests

```sh
just fetch-examples
cargo test --locked --test examples
```

`just fetch-examples` downloads the runtime and macro dependencies using each workspace's pin
and lockfile. `just check` runs this preparation automatically, including on a cold CI checkout.
Once dependencies and the target are installed, `CARGO_NET_OFFLINE=true just check` works offline.

The tests copy these projects into temporary directories, invoke the built polyfw executable,
and check that reported artifacts are linked 32-bit ARM ELF executables with Thumb entry points
in flash. They also check platform CPU/chip values, external workspace membership and independent
builds of a package's two bins. Failing host tests introduced in the copies check that polyfw
includes reachable shared dependencies and excludes unrelated packages. Test subprocesses run
without network access and are included in `just check`.
