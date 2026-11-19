# Repository instructions

## Load context for the task

- Before changing commands, selection, profiles, hardware behavior or JSON output, read the
  relevant sections of [Usage](docs/usage.md) and [Design](docs/design.md).
- Before changing configuration validation, Cargo resolution or dependency scopes, read
  [Project configuration](docs/configuration.md) and [Design](docs/design.md).
- Before changing test fixtures, generated documentation or verification commands, read
  [Development](docs/development.md).
- Before changing versions, packaging, release scripts/workflows or publishing, read
  [Releasing](docs/releasing.md).

## Preserve repository invariants

- Construct Cargo children through `runner::workspace_cargo` / `workspace_command`; run them
  from their owning workspace and clear inherited `RUSTUP_TOOLCHAIN`. Use `cargo_command`
  when BSP firmware-cache isolation is required. Apply this to metadata and fixture setup too.
- Use `runner::firmware_elf_path` for ELF lookup, reports and editor generation so every
  consumer shares the build/profile layout.
- Keep help, version and schema independent of projects and Rust tools. Probe listing needs
  only probe-rs. Preserve the lighter loading paths for selection clearing and reset.
- Keep selection/editor rollback and disabled-editor behavior consistent with the state-write
  contract in Design. A later action failure retains an already successful selection save.
- Edit configuration Schema descriptions in `src/model.rs`; run `just schema` and commit
  `polyfw.schema.json`. CLI help comes from `src/cli.rs`.

## Complete the change

Use the change-specific checks in [Development](docs/development.md#validate-changes).
Update the owning English guide when its contract changes, and link to it instead of copying
its explanation into other pages. Keep this file focused on repository-specific instructions.
