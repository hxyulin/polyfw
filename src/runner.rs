use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{Bsp, Selection};
use crate::profile::Profile;
use crate::ui::{Progress, Ui};

pub(crate) fn workspace_cargo(workspace: &Path) -> Command {
    workspace_command(workspace, "cargo")
}

pub(crate) fn workspace_command(workspace: &Path, program: &str) -> Command {
    let mut command = Command::new(program);
    command
        .current_dir(workspace)
        .env_remove("RUSTUP_TOOLCHAIN");
    command
}

pub(crate) fn cargo_command(root: &Path, bsp_id: &str, bsp: &Bsp) -> Command {
    let mut command = workspace_cargo(&root.join(&bsp.workspace));
    command.env("CARGO_TARGET_DIR", firmware_target_dir(root, bsp_id));
    command
}

fn firmware_target_dir(root: &Path, bsp_id: &str) -> PathBuf {
    root.join("target/firmware").join(bsp_id)
}

pub(crate) fn firmware_elf_path(root: &Path, selection: &Selection) -> PathBuf {
    firmware_target_dir(root, &selection.board.bsp)
        .join(&selection.bsp.target)
        .join(
            selection
                .profile
                .as_ref()
                .map_or("debug", Profile::directory),
        )
        .join(&selection.board.resolved_bin)
}

fn render_command(command: &Command) -> String {
    std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|argument| argument.to_string_lossy())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(crate) fn run_command(ui: &Ui, mut command: Command) -> io::Result<()> {
    let rendered = render_command(&command);
    ui.command(&rendered);
    let status = command.status()?;
    if status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!("{rendered} failed with {status}")))
    }
}

pub(crate) fn run_command_with_progress(
    ui: &Ui,
    progress: &mut Progress<'_>,
    label: &str,
    command: Command,
) -> io::Result<()> {
    let step = progress.start(label);
    let result = run_command(ui, command);
    progress.finish(step, result.is_ok());
    result
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::model::Board;
    use clap::ColorChoice;

    #[test]
    fn temporary_projects_use_the_project_toolchain_with_an_unusable_ancestor() {
        let root =
            std::env::temp_dir().join(format!("polyfw-fixture-ancestor-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let _cleanup = std::mem::DropGuard::new(&root, |root| {
            let _ = fs::remove_dir_all(root);
        });
        let missing = toml::Value::String(root.join("missing-toolchain").display().to_string());
        fs::write(
            root.join("rust-toolchain.toml"),
            format!("[toolchain]\npath = {missing}\n"),
        )
        .unwrap();
        for test in [
            "commands::tests::host_lint_rejects_mistakes_in_both_bsp_and_firmware_tests",
            "scope::tests::a_board_can_verify_with_std_helpers_without_checking_unrelated_packages",
            "scope::tests::same_named_dependencies_in_different_workspaces_are_both_verified",
            "scope::tests::external_directory_patterns_include_packages_and_their_own_host_helpers",
            "scope::tests::lint_checks_other_bins_in_the_selected_firmware_package",
        ] {
            let output = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", test, "--nocapture"])
                .env("TMPDIR", &root)
                .env("TMP", &root)
                .env("TEMP", &root)
                .env("RUSTUP_AUTO_INSTALL", "0")
                .env("CARGO_NET_OFFLINE", "true")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "{test}:\n{stdout}\n{stderr}");
            assert!(stdout.contains("1 passed"), "{test}:\n{stdout}\n{stderr}");
        }
    }

    #[test]
    fn command_runners_preserve_launch_errors_and_failed_exit_statuses() {
        let ui = Ui::new(ColorChoice::Never);
        for with_progress in [false, true] {
            for missing in [false, true] {
                let mut command = if missing {
                    Command::new(
                        std::env::temp_dir()
                            .join(format!("polyfw-missing-executable-{}", std::process::id())),
                    )
                } else {
                    Command::new("rustc")
                };
                command.arg("--polyfw-invalid-argument");
                let rendered = render_command(&command);
                let result = if with_progress {
                    run_command_with_progress(
                        &ui,
                        &mut Progress::new(&ui, 1),
                        "test command",
                        command,
                    )
                } else {
                    run_command(&ui, command)
                };
                let error = result.unwrap_err();
                if missing {
                    assert_eq!(error.kind(), io::ErrorKind::NotFound);
                    assert!(error.raw_os_error().is_some());
                } else {
                    assert_eq!(error.kind(), io::ErrorKind::Other);
                    assert!(error.to_string().contains("failed with"), "{error}");
                    assert!(error.to_string().starts_with(&rendered), "{error}");
                }
            }
        }
    }

    #[test]
    fn firmware_paths_select_the_bsp_target_profile_and_resolved_binary() {
        let root = Path::new("repository");
        let mut selection = Selection {
            id: "controller".into(),
            profile: None,
            board: Board {
                description: "test board".into(),
                bsp: "board-family".into(),
                package: "firmware-package".into(),
                bin: None,
                resolved_bin: "firmware-package".into(),
            },
            bsp: Bsp {
                workspace: "bsp/test".into(),
                target: "thumbv7em-none-eabihf".into(),
                cpu: Some("cortex-m4".into()),
                chip: "test-chip".into(),
            },
        };
        let target_dir = root.join("target/firmware/board-family/thumbv7em-none-eabihf");
        assert_eq!(
            firmware_elf_path(root, &selection),
            target_dir.join("debug/firmware-package")
        );
        selection.board.resolved_bin = "robot-image".into();
        selection.profile = Some("release".parse().unwrap());
        assert_eq!(
            firmware_elf_path(root, &selection),
            target_dir.join("release/robot-image")
        );
        selection.profile = Some("dev".parse().unwrap());
        assert_eq!(
            firmware_elf_path(root, &selection),
            target_dir.join("debug/robot-image")
        );
    }

    #[test]
    fn cargo_uses_the_bsp_target_and_compiler_configuration() {
        let host = Command::new("rustc")
            .args(["--print", "host-tuple"])
            .output()
            .unwrap();
        assert!(host.status.success());
        let target = String::from_utf8(host.stdout).unwrap().trim().to_owned();
        let root = std::env::temp_dir().join(format!("polyfw-flags-{}", std::process::id()));
        let workspace = root.join("bsp/test");
        fs::create_dir_all(workspace.join(".cargo")).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"test-board\"\nversion = \"0.1.0\"\n[lib]\npath = \"lib.rs\"\n",
        )
        .unwrap();
        fs::write(workspace.join("lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
        fs::write(
            workspace.join(".cargo/config.toml"),
            format!(
                "[build]\ntarget = {target:?}\n[target.{target}]\n\
                 rustflags = [\"--cfg\", \"bsp_configuration\"]\n"
            ),
        )
        .unwrap();
        let bsp = Bsp {
            workspace: "bsp/test".into(),
            target,
            cpu: Some("generic".into()),
            chip: "test-chip".into(),
        };
        let mut command = cargo_command(&root, "board-family", &bsp);
        let target_dir = root.join("target/firmware/board-family");
        assert_eq!(command.get_current_dir(), Some(workspace.as_path()));
        assert_eq!(
            command
                .get_envs()
                .find(|(key, _)| *key == "CARGO_TARGET_DIR")
                .and_then(|(_, value)| value)
                .map(Path::new),
            Some(target_dir.as_path())
        );

        let output = command
            .args(["rustc", "--quiet", "--lib", "--", "--print", "cfg"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let cfg = String::from_utf8(output.stdout).unwrap();
        assert!(cfg.lines().any(|line| line == "bsp_configuration"));

        fs::remove_dir_all(root).unwrap();
    }
}
