# Multiple workspaces example

Three Cargo workspaces with shared libraries, external firmware members and a package with two binaries.

## Layout

```text
multi-workspace/
├── Cargo.toml                 # root workspace: shared/*
├── polyfw.toml
├── shared/
│   ├── common/                # used by all firmware packages
│   ├── first-only/            # used by first
│   └── second-only/           # used by second
├── bsp/
│   ├── a/                     # workspace: first, multi
│   └── b/                     # workspace: second
└── firmwares/
    ├── first/
    ├── second/
    └── multi/                 # src/bin/one.rs and src/bin/two.rs
```

Each firmware package sets `package.workspace` to its owning BSP workspace. The shared libraries
belong to the root workspace.

## Platforms and firmwares

[polyfw.toml](polyfw.toml) defines two platforms:

| Platform | Board | CPU | Chip | Workspace |
| --- | --- | --- | --- | --- |
| `a` | DM-MC02 | `cortex-m7` | `STM32H723VGTx` | `bsp/a` |
| `b` | RM-C | `cortex-m4` | `STM32F407IG` | `bsp/b` |

Firmware IDs match their binary names; `multi` provides two selectable binaries:

| Firmware | Package | Platform |
| --- | --- | --- |
| `first` | `first` | `a` |
| `second` | `second` | `b` |
| `multi-one` | `multi` | `a` |
| `multi-two` | `multi` | `a` |
