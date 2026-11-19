# Migrate an xtask project

Install the first version from a checkout of polyfw using `cargo install --path . --locked`.
Run that command from the polyfw checkout so its host target and Rust pin apply.

## Configuration and schema

Rename `xtask-registry.toml` to `polyfw.toml`. Keep the `bsps` and `boards` tables unchanged.
Move the top-level `local-dependencies` option into `[config]`:

```toml
#:schema ./polyfw.schema.json

[config]
local-dependencies = ["robots/**", "shared/**"]

[config.vscode]
enabled = true

[bsps.dm-mc02]
workspace = "."
chip = "STM32H723VGTx"

[boards.controller]
description = "Controller firmware"
bsp = "dm-mc02"
package = "controller-firmware"
```

Omitted dependency patterns still default to `["./**"]`. An empty array still selects no
dependencies, while the selected firmware is always checked. VS Code generation defaults to true;
set `config.vscode.enabled = false` to leave all editor files untouched.

Generate and commit a project-local schema:

```sh
polyfw schema --output polyfw.schema.json
```

Old registry filenames and top-level tool options are not accepted. The tool does not fall back
to a compiled-in source directory when no project is found.

## Project entry and state

Replace the source-building `_xtask` recipe with an installed-binary wrapper:

```just
[private]
_polyfw *args:
    polyfw --project-root "{{justfile_directory()}}" {{args}}

build *args:
    just _polyfw build {{args}}
```

Forward other existing recipes in the same way. Drop the wrapper's source working-directory
attribute, host profile and compilation cache. Existing user-facing `just build`, `just select`
and other recipes can keep their names and aliases.

Add `.polyfw-state/` to `.gitignore`. Run `polyfw select BOARD` to create a fresh default, or move
the existing `.xtask-state/selected` TOML file to `.polyfw-state/selected` to retain it. Firmware
artifacts remain in `target/firmware/<bsp>/<target>/<profile>/<bin>` and do not need rebuilding
solely because the tool was renamed.

Remove the old xtask source directory and source symlink after validating the new tool. Remove
their manifests from rust-analyzer linked projects and their checks from the consuming project's
CI. The standalone polyfw repository owns these checks. Consuming CI must install polyfw before
running recipes; first-version consumers can build from a pinned Git revision until releases
are available.

## Validate each consumer

From both the firmware project and its BSP project:

```sh
polyfw info --json
polyfw verify --all-boards
polyfw build --all-boards --release
```

Check each project's board list and target/CPU against its Cargo configuration. A select/clear in
one project must not change the other project's state or debug files. Existing profile behavior,
package checking scopes, hardware command arguments and whole-file VS Code management are retained.
Hardware verification of this extracted version is a separate bench check.
