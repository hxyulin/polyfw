# Cargo cache and colliding binary artifacts

Investigated 2026-11-20 using Cargo 1.100.0 (`3acc79c0c`, 2026-11-05) and
rustc 1.100.0 on Linux. These are Cargo findings, not accepted polyfw policies.

## Native directories and freshness

Cargo defaults its target and build directories to the workspace's `target` directory.
`--target-dir`, `CARGO_TARGET_DIR` and `build.target-dir` can change the target directory;
the build directory has separate configuration. Explicit `--target TRIPLE` puts final
executables under `TRIPLE/PROFILE/`; `dev` uses `debug`. Omitting an override preserves
native Cargo directory selection. Intermediate layout is internal and changed in 1.100.
[Cargo build cache](https://doc.rust-lang.org/cargo/reference/build-cache.html)

Cargo uses unit identities, fingerprints and filesystem checks to decide whether compilation
is needed. Relevant inputs include compiler, profile, target kind, enabled features,
dependencies and compiler flags. Package identity distinguishes cached units, without
guaranteeing unique public executable filenames. Freshness tracking has documented limitations.
[Cargo fingerprint implementation](https://doc.rust-lang.org/stable/nightly-rustc/cargo/compiler/fingerprint/index.html)

## Same binary name in different packages

Cargo's collision checker warns about conflicting build outputs. Duplicate binary names
across packages are not universally rejected by `cargo build`. The upstream tracking issue
explicitly includes workspace targets with identical names and shared target directories;
the diagnostic says this could become a hard error. Documentation collisions have separate
error handling, so `cargo doc` behavior should not be generalized to builds.
[Collision checker](https://doc.rust-lang.org/stable/nightly-rustc/src/cargo/compiler/build_runner/mod.rs.html#609-746),
[Cargo issue #6313](https://github.com/rust-lang/cargo/issues/6313)

## Empirical probe

A temporary virtual workspace contained dependency-free `led-demo` and `buzzer-demo`
packages, each declaring `[[bin]] name = "demo-image"`; their programs printed their
package names. It carried the repository Rust pin and a valid lockfile. A temporary Rust
launcher copied `workspace_cargo` and `workspace_command` exactly from
[runner.rs](../../src/runner.rs), preserving workspace cwd and clearing inherited
`RUSTUP_TOOLCHAIN`. Cargo ran offline through that launcher.

Each invocation used:

```sh
cargo build --locked --message-format=json \
  --target x86_64-unknown-linux-gnu --target-dir DIR SELECTION
```

The workspace build used a clean directory. The following package builds shared another
clean directory. Every invocation exited successfully; the output executable was run after
each build.

| Selection | JSON `fresh` | Executable printed | Cargo collision warning |
| --- | --- | --- | --- |
| `--workspace` | Both `false` | `led-demo` in this run | Yes |
| `-p led-demo` | `false` | `led-demo` | No |
| `-p buzzer-demo` | `false` | `buzzer-demo` | No |
| `-p led-demo` again | `true` | `led-demo` | No |

Both packages' JSON `executable` and `filenames` reported the same public path:
`DIR/x86_64-unknown-linux-gnu/debug/demo-image`. Package-specific binaries remained under
`debug/build/PACKAGE/UNIT_HASH/out/demo_image` in this Cargo version. The final A build
restored A's public executable despite reporting `fresh=true`. Cargo defines that flag as
reuse without running rustc.
[Cargo artifact JSON](https://doc.rust-lang.org/cargo/reference/external-tools.html#artifact-messages)

Shared intermediate reuse and persistent final-image identity are therefore separate
questions. After B, the public path contained B while A's cached output remained.

## Separate final directories with one build directory

`build.build-dir` (`CARGO_BUILD_BUILD_DIR`) stabilized in Cargo 1.91; 1.100 changed its
internal layout. The setting defaults to target-dir. No `--build-dir` flag was used.
[Cargo configuration](https://doc.rust-lang.org/cargo/reference/config.html#buildbuild-dir),
[Cargo changelog](https://doc.rust-lang.org/cargo/CHANGELOG.html#cargo-191-2025-10-30)

An extended fixture added one shared path library. With one absolute build-dir and
destination-specific `--target-dir`, every build succeeded without warnings:

| Package → destination | Library `fresh` | Binary `fresh` |
| --- | --- | --- |
| A → targetA | `false` | `false` |
| B → targetB | `true` | `false` |
| A → targetA | `true` | `true` |
| A → targetC | `true` | `true` |

All existing destination executables retained their expected outputs after each build:
A at targetA, B at targetB, A at targetC. Cargo populated previously absent targetC
without recompilation. Inodes showed targetA/targetC hardlinked to cached A; targetB linked
cached B. The launcher performed no copying.

Metadata with `CARGO_TARGET_DIR=targetA` reported `target_directory=targetA` and
`build_directory=shared-build`. Cargo 1.100 emits the latter field; its documentation
still marks it unstable.
[Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html#output-format)

Both probes used runnable host binaries, not MCU firmware or hardware.
