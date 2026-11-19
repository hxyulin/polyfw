use std::path::Path;
use std::process::{Command, ExitCode};

use crate::Result;
use crate::cli::{BoardArgs, Task};
use crate::info::Report;
use crate::model::Selection;
use crate::probe::{self, Action};
use crate::profile::Profile;
use crate::registry::{Context, unique_by_bsp};
use crate::runner::{self, run_command_with_progress};
use crate::scope::{self, PackageScope, WorkspaceScope};
use crate::ui::{self, Progress, Ui};

pub(crate) fn run(context: Context, task: Task, ui: &Ui) -> Result<ExitCode> {
    let selections = match task.board_args() {
        Some(boards) => resolve(&context, ui, &boards, task.profile_override())?,
        None => Vec::new(),
    };
    let scopes = match &task {
        Task::Fmt(_) | Task::Lint(_) | Task::Test(_) | Task::Verify(_) => {
            scope::resolve(&context, &selections)?
        }
        Task::Schema { .. }
        | Task::Info { .. }
        | Task::Select { .. }
        | Task::Clean(_)
        | Task::Build { .. }
        | Task::Probes
        | Task::Run { .. }
        | Task::Flash { .. }
        | Task::Attach { .. }
        | Task::Reset(_) => Vec::new(),
    };
    match task {
        Task::Schema { output } => crate::registry::write_schema(output.as_deref()),
        Task::Info { board, json } => info(&context, ui, board, json),
        Task::Select {
            board,
            clear,
            profile,
        } => {
            return select(&context, ui, board.as_deref(), clear, profile.explicit());
        }
        Task::Clean(boards) => clean(&context, ui, &selections, boards.all_boards),
        Task::Build { .. } => build(&context, ui, &selections),
        Task::Probes => probe::list(&context.root, ui),
        Task::Run {
            target, monitor, ..
        } => probe::run(
            &context,
            ui,
            &selections[0],
            Action::Run { monitor: &monitor },
            &target,
        ),
        Task::Flash { target, .. } => {
            probe::run(&context, ui, &selections[0], Action::Flash, &target)
        }
        Task::Attach {
            target, monitor, ..
        } => probe::run(
            &context,
            ui,
            &selections[0],
            Action::Attach { monitor: &monitor },
            &target,
        ),
        Task::Reset(target) => probe::run(&context, ui, &selections[0], Action::Reset, &target),
        Task::Fmt(_) => run_stages(ui, fmt_stages(&context, ui, &scopes, false)),
        Task::Lint(_) => run_stages(ui, lint_stages(&context, ui, &scopes)),
        Task::Test(_) => run_stages(ui, test_stages(&context, ui, &scopes)),
        Task::Verify(_) => {
            let mut stages = fmt_stages(&context, ui, &scopes, true);
            stages.extend(lint_stages(&context, ui, &scopes));
            stages.extend(test_stages(&context, ui, &scopes));
            run_stages(ui, stages)
        }
    }?;
    Ok(ExitCode::SUCCESS)
}

fn info(context: &Context, ui: &Ui, board: Option<Option<String>>, json: bool) -> Result<()> {
    let show_saved_selection = !matches!(board, Some(None));
    let board = match board {
        None => None,
        Some(Some(board)) => Some(board),
        Some(None) => {
            let saved = context.read_saved_selection()?;
            Some(ui.prompt_board(
                &context.registry,
                saved.as_ref().map(|saved| saved.board.as_str()),
                "pass a board name with `polyfw info --board <BOARD>`",
            )?)
        }
    };
    let report = Report::load(context, board.as_deref())?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        ui.print_info(&report, show_saved_selection);
    }
    Ok(())
}

fn select(
    context: &Context,
    ui: &Ui,
    board: Option<&str>,
    clear: bool,
    profile: Option<Profile>,
) -> Result<ExitCode> {
    if clear {
        let mut exit = ExitCode::SUCCESS;
        for (path, result) in context.clear_selection() {
            ui.removal(path, &result);
            if result.is_err_and(|error| error.kind() != std::io::ErrorKind::NotFound) {
                exit = ExitCode::FAILURE;
            }
        }
        return Ok(exit);
    }

    let board = match board {
        Some(board) => board.to_owned(),
        None => {
            let saved = context.read_selection()?;
            ui.prompt_board(
                &context.registry,
                saved.as_ref().map(|saved| saved.board.as_str()),
                "pass a board name or run polyfw select in a terminal",
            )?
        }
    };
    context.save_selection(&board, profile.as_ref())?;
    ui.selected(&board, profile.as_ref());
    Ok(ExitCode::SUCCESS)
}

fn clean(context: &Context, ui: &Ui, selections: &[Selection], all_boards: bool) -> Result<()> {
    if all_boards {
        let selections = unique_by_bsp(selections);
        let mut progress = Progress::new(ui, selections.len());
        for selection in selections {
            let boards = context
                .registry
                .boards
                .iter()
                .filter(|(_, board)| board.bsp == selection.board.bsp)
                .map(|(id, _)| id.as_str())
                .collect::<Vec<_>>();
            ui.cleaning_bsp(&selection.board.bsp, &boards);

            let command = clean_command(context, selection, true);
            run_command_with_progress(
                ui,
                &mut progress,
                &ui.label("clean", &[&selection.board.bsp], ui::Target::None),
                command,
            )?;
        }
    } else {
        let selection = selections
            .first()
            .expect("single-board resolution returns one selection");
        let boards = context
            .registry
            .boards
            .iter()
            .filter(|(_, board)| {
                board.bsp == selection.board.bsp && board.package == selection.board.package
            })
            .map(|(id, _)| id.as_str())
            .collect::<Vec<_>>();
        ui.cleaning_package(&selection.board.package, &selection.board.bsp, &boards);

        let command = clean_command(context, selection, false);
        let mut progress = Progress::new(ui, 1);
        run_command_with_progress(
            ui,
            &mut progress,
            &ui.label("clean", &[&selection.id], ui::Target::None),
            command,
        )?;
    }
    Ok(())
}

fn clean_command(context: &Context, selection: &Selection, all_boards: bool) -> Command {
    let mut command = runner::cargo_command(&context.root, &selection.board.bsp, &selection.bsp);
    command.arg("clean");
    if !all_boards {
        command.arg("-p").arg(&selection.board.package);
    }
    command
}

fn build(context: &Context, ui: &Ui, selections: &[Selection]) -> Result<()> {
    let mut progress = Progress::new(ui, selections.len());
    for selection in selections {
        let command = build_command(context, selection);
        let board = match &selection.profile {
            Some(profile) => format!("{} ({})", selection.id, profile.name()),
            None => selection.id.clone(),
        };
        let label = ui.label("build", &[&board], ui::Target::Embedded);
        run_command_with_progress(ui, &mut progress, &label, command)?;
    }
    Ok(())
}

fn build_command(context: &Context, selection: &Selection) -> Command {
    let mut command = runner::cargo_command(&context.root, &selection.board.bsp, &selection.bsp);
    command.arg("build").arg("-p").arg(&selection.board.package);
    if let Some(bin) = &selection.board.bin {
        command.arg("--bin").arg(bin);
    }
    command.arg("--locked");
    if let Some(profile) = &selection.profile {
        command.arg("--profile").arg(profile.name());
    }
    command
}

struct Stage(String, Command);

fn run_stages(ui: &Ui, stages: Vec<Stage>) -> Result<()> {
    let mut progress = Progress::new(ui, stages.len());
    for Stage(label, command) in stages {
        run_command_with_progress(ui, &mut progress, &label, command)?;
    }
    Ok(())
}

fn fmt_stages(context: &Context, ui: &Ui, scopes: &[WorkspaceScope], check: bool) -> Vec<Stage> {
    let mut stages = Vec::new();
    for scope in scope::host_scopes(scopes) {
        let mut command = host_command(context, scope.workspace);
        command.arg("fmt");
        let names = add_packages(&mut command, scope.packages, |package| &package.name);
        let action = if check {
            command.arg("--check");
            "format check"
        } else {
            "format"
        };
        stages.push(Stage(ui.label(action, &names, ui::Target::None), command));
    }
    stages
}

fn lint_stages(context: &Context, ui: &Ui, scopes: &[WorkspaceScope]) -> Vec<Stage> {
    let mut stages = Vec::new();
    for scope in scopes {
        if scope.packages.is_empty() {
            continue;
        }
        let mut command = runner::cargo_command(&context.root, &scope.bsp_id, &scope.bsp);
        command.arg("clippy");
        let names = add_packages(
            &mut command,
            scope.packages.iter().filter(|package| package.embedded),
            |package| &package.id,
        );
        if !names.is_empty() {
            command
                .args(["--lib", "--bins"])
                .args(["--locked", "--", "-D", "warnings"]);
            stages.push(Stage(
                ui.label("lint", &names, ui::Target::Embedded),
                command,
            ));
        }
    }
    for scope in scope::host_scopes(scopes) {
        let mut host = host_command(context, scope.workspace);
        host.arg("clippy");
        let names = add_packages(&mut host, scope.packages, |package| &package.host_id);
        host.args([
            "--lib",
            "--tests",
            "--target",
            "host-tuple",
            "--locked",
            "--",
            "-D",
            "warnings",
        ]);
        stages.push(Stage(ui.label("lint", &names, ui::Target::Host), host));
    }
    stages
}

fn test_stages(context: &Context, ui: &Ui, scopes: &[WorkspaceScope]) -> Vec<Stage> {
    let mut stages = Vec::new();
    for scope in scope::host_scopes(scopes) {
        let mut command = host_command(context, scope.workspace);
        command.arg("test");
        let names = add_packages(&mut command, scope.packages, |package| &package.host_id);
        command.args(["--target", "host-tuple", "--locked"]);
        stages.push(Stage(ui.label("test", &names, ui::Target::Host), command));
    }
    stages
}

fn host_command(context: &Context, workspace: &Path) -> Command {
    for (id, bsp) in &context.registry.bsps {
        let directory = context.root.join(&bsp.workspace);
        if directory == workspace || directory.canonicalize().is_ok_and(|path| path == workspace) {
            return runner::cargo_command(&context.root, id, bsp);
        }
    }
    runner::workspace_cargo(workspace)
}

fn add_packages<'a>(
    command: &mut Command,
    packages: impl IntoIterator<Item = &'a PackageScope>,
    spec: fn(&PackageScope) -> &str,
) -> Vec<&'a str> {
    packages
        .into_iter()
        .map(|package| {
            let name = package.name.as_str();
            command.arg("-p").arg(spec(package));
            name
        })
        .collect()
}

fn resolve(
    context: &Context,
    ui: &Ui,
    boards: &BoardArgs,
    profile: Option<Profile>,
) -> Result<Vec<Selection>> {
    if boards.all_boards {
        return context
            .registry
            .boards
            .keys()
            .map(|id| {
                let mut selection = context.selection(id)?;
                selection.profile = profile.clone();
                Ok(selection)
            })
            .collect();
    }

    let saved = if boards.select && boards.board.is_some() {
        None
    } else {
        context.read_selection()?
    };
    let id = match (boards.board.as_deref(), boards.select, saved.as_ref()) {
        (Some(id), _, _) => id.to_owned(),
        (None, false, Some(saved)) => saved.board.clone(),
        (None, _, saved) => ui.prompt_board(
            &context.registry,
            saved.map(|saved| saved.board.as_str()),
            "pass a board name or run polyfw select in a terminal",
        )?,
    };
    let mut selection = context.selection(&id)?;
    selection.profile = profile.or_else(|| {
        if boards.select {
            None
        } else {
            saved.and_then(|saved| saved.profile)
        }
    });
    if boards.select {
        context.save_selection(&id, selection.profile.as_ref())?;
        ui.selected(&id, selection.profile.as_ref());
    }
    Ok(vec![selection])
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::ffi::OsStr;
    use std::fs;
    use std::path::{Path, PathBuf};

    use clap::ColorChoice;
    use clap::Parser;

    use super::*;
    use crate::model::{Board, Bsp, Registry};

    #[test]
    fn info_requires_a_terminal_to_pick_even_when_a_board_is_saved() {
        const MODE: &str = "FLEET_XTASK_TEST_INFO_PICKER";
        if std::env::var_os(MODE).is_some() {
            for saved in [false, true] {
                for json in [false, true] {
                    let root = test_root(&format!("info-picker-{saved}-{json}"));
                    let context = test_context(root.clone());
                    if saved {
                        context
                            .save_selection("test-board", Some(&"release".parse().unwrap()))
                            .unwrap();
                    }
                    let state_path = context.selection_path();
                    let launch_path = root.join(crate::vscode::LAUNCH_FILE);
                    let state = fs::read(&state_path).ok();
                    let launch = fs::read(&launch_path).ok();
                    let mut args = vec!["polyfw", "info", "--board"];
                    if json {
                        args.push("--json");
                    }
                    let task = crate::cli::Cli::try_parse_from(args)
                        .unwrap()
                        .command
                        .unwrap();
                    let error = run(context, task, &Ui::new(ColorChoice::Never)).unwrap_err();
                    assert!(
                        error.to_string().contains("polyfw info --board <BOARD>"),
                        "{error}"
                    );
                    assert_eq!(fs::read(state_path).ok(), state);
                    assert_eq!(fs::read(launch_path).ok(), launch);
                    fs::remove_dir_all(root).unwrap();
                }
            }
            return;
        }

        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "commands::tests::info_requires_a_terminal_to_pick_even_when_a_board_is_saved",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(!stdout.contains("repository_root"), "{stdout}");
    }

    #[test]
    fn saving_a_selection_generates_matching_launch_and_attach_without_building() {
        let mut context = test_context(test_root("vscode profiles with spaces"));
        let board = context.registry.boards.get_mut("test-board").unwrap();
        board.bin = Some("robot-image".into());
        board.resolved_bin = "robot-image".into();
        let mut other_bsp = context.registry.bsps["test-bsp"].clone();
        other_bsp.chip = "another-chip".into();
        context.registry.bsps.insert("other-bsp".into(), other_bsp);
        let mut other_board = context.registry.boards["test-board"].clone();
        other_board.bsp = "other-bsp".into();
        context
            .registry
            .boards
            .insert("other-board".into(), other_board);
        for board in ["test-board", "other-board"] {
            for (profile, directory, suffix) in [
                (None, "debug", ""),
                (Some("dev"), "debug", " (dev)"),
                (Some("release"), "release", " (release)"),
                (Some("bench-debug"), "bench-debug", " (bench-debug)"),
            ] {
                let _ = select(
                    &context,
                    &Ui::new(ColorChoice::Never),
                    Some(board),
                    false,
                    profile.map(|profile| profile.parse().unwrap()),
                )
                .unwrap();
                let state: toml::Value =
                    toml::from_str(&fs::read_to_string(context.selection_path()).unwrap()).unwrap();
                assert_eq!(state.get("profile").and_then(toml::Value::as_str), profile);
                let value: serde_json::Value = serde_json::from_slice(
                    &fs::read(context.root.join(crate::vscode::LAUNCH_FILE)).unwrap(),
                )
                .unwrap();
                let configurations = value["configurations"].as_array().unwrap();
                assert_eq!(configurations.len(), 2);
                let selection = context.selection(board).unwrap();
                let expected_elf = format!(
                    "${{workspaceFolder}}/target/firmware/{}/thumbv7em-none-eabihf/\
                     {directory}/robot-image",
                    selection.board.bsp
                );
                for (configuration, request, label) in [
                    (&configurations[0], "launch", "Launch"),
                    (&configurations[1], "attach", "Attach"),
                ] {
                    assert_eq!(configuration["request"], request);
                    assert_eq!(configuration["name"], format!("{label} {board}{suffix}"));
                    assert_eq!(configuration["chip"], selection.bsp.chip);
                    assert_eq!(
                        configuration["coreConfigs"][0]["programBinary"],
                        expected_elf
                    );
                }
            }
        }
        let value: serde_json::Value = serde_json::from_slice(
            &fs::read(context.root.join(crate::vscode::LAUNCH_FILE)).unwrap(),
        )
        .unwrap();
        assert_eq!(value["version"], "0.2.0");
        let configurations = value["configurations"].as_array().unwrap();
        for configuration in configurations {
            assert_eq!(configuration["type"], "probe-rs-debug");
            assert_eq!(configuration["cwd"], "${workspaceFolder}");
            assert_eq!(configuration["wireProtocol"], "Swd");
            assert!(configuration.get("probe").is_none());
            assert!(configuration.get("speed").is_none());
            assert!(configuration.get("preLaunchTask").is_none());
            assert!(configuration.get("postDebugTask").is_none());
            let core = &configuration["coreConfigs"][0];
            assert_eq!(core["coreIndex"], 0);
            assert_eq!(core["rttEnabled"], true);
            assert_eq!(core["rttChannelFormats"][0]["channelNumber"], 0);
            assert_eq!(core["rttChannelFormats"][0]["dataFormat"], "Defmt");
            assert_eq!(core["rttChannelFormats"][0]["mode"], "NoBlockSkip");
        }
        let flashing = &configurations[0]["flashingConfig"];
        assert_eq!(flashing["flashingEnabled"], true);
        assert_eq!(flashing["verifyAfterFlashing"], true);
        assert_eq!(flashing["haltAfterReset"], true);
        assert!(configurations[1].get("flashingConfig").is_none());
        assert!(!context.root.join("target").exists());
        assert!(!context.root.join("bsp").exists());
        assert!(!context.root.join(".vscode/tasks.json").exists());
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn clearing_selection_removes_launch_but_preserves_other_editor_files_and_handles_orphans() {
        let context = test_context(test_root("clear-vscode"));
        context.save_selection("test-board", None).unwrap();
        let editor = context.root.join(".vscode");
        fs::write(editor.join("settings.json"), "{}\n").unwrap();
        fs::write(editor.join("tasks.json"), "{}\n").unwrap();
        let ui = Ui::new(ColorChoice::Never);
        let _ = select(&context, &ui, None, true, None).unwrap();
        assert!(!context.selection_path().exists());
        assert!(!editor.join("launch.json").exists());
        assert_eq!(
            fs::read_to_string(editor.join("settings.json")).unwrap(),
            "{}\n"
        );
        assert_eq!(
            fs::read_to_string(editor.join("tasks.json")).unwrap(),
            "{}\n"
        );
        fs::write(editor.join("launch.json"), "{}\n").unwrap();
        let removed = context.clear_selection();
        assert!(removed[0].1.is_ok());
        assert_eq!(
            removed[1].1.as_ref().unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        assert!(!editor.join("launch.json").exists());
        assert_eq!(
            select(&context, &ui, None, true, None).unwrap(),
            ExitCode::SUCCESS
        );
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn clearing_selection_attempts_every_file_and_returns_failure_after_an_error() {
        for failed_file in [crate::vscode::LAUNCH_FILE, ".polyfw-state/selected"] {
            let context = test_context(test_root(&format!("clear-failed-{failed_file}")));
            context.save_selection("test-board", None).unwrap();
            let failed_path = context.root.join(failed_file);
            fs::remove_file(&failed_path).unwrap();
            fs::create_dir(&failed_path).unwrap();

            assert_eq!(
                select(&context, &Ui::new(ColorChoice::Never), None, true, None).unwrap(),
                ExitCode::FAILURE
            );
            assert!(failed_path.is_dir());
            for file in [crate::vscode::LAUNCH_FILE, ".polyfw-state/selected"] {
                if file != failed_file {
                    assert!(!context.root.join(file).exists());
                }
            }
            fs::remove_dir_all(context.root).unwrap();
        }
    }

    #[test]
    fn disabling_vscode_leaves_existing_editor_files_untouched_when_saving_or_clearing() {
        let mut context = test_context(test_root("vscode-disabled"));
        context.save_selection("test-board", None).unwrap();
        let path = context.root.join(crate::vscode::LAUNCH_FILE);
        let previous = fs::read(&path).unwrap();
        context.registry.config.vscode.enabled = false;
        context
            .save_selection("test-board", Some(&"release".parse().unwrap()))
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), previous);
        assert_eq!(
            context
                .read_selection()
                .unwrap()
                .unwrap()
                .profile
                .unwrap()
                .name(),
            "release"
        );
        let exit = select(&context, &Ui::new(ColorChoice::Never), None, true, None).unwrap();
        assert_eq!(exit, ExitCode::SUCCESS);
        assert!(!context.selection_path().exists());
        assert_eq!(fs::read(&path).unwrap(), previous);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        context.save_selection("test-board", None).unwrap();
        assert_eq!(context.clear_selection().len(), 1);
        assert!(path.is_dir());
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn board_commands_that_save_a_selection_respect_the_vscode_switch() {
        use clap::Parser;

        for command in [
            "clean", "build", "fmt", "lint", "test", "verify", "run", "flash", "attach",
        ] {
            let mut context = test_context(test_root(&format!("vscode-disabled-{command}")));
            context.registry.config.vscode.enabled = false;
            fs::write(context.root.join(".vscode"), "not a directory").unwrap();
            let root = context.root.clone();
            let task =
                crate::cli::Cli::try_parse_from(["polyfw", command, "test-board", "--select"])
                    .unwrap()
                    .command
                    .unwrap();
            // Every action fails on this fixture after saving; the selected board remains saved.
            assert!(run(context, task, &Ui::new(ColorChoice::Never)).is_err());
            assert_eq!(
                fs::read_to_string(root.join(".polyfw-state/selected")).unwrap(),
                "board = \"test-board\"\n",
                "{command}"
            );
            assert_eq!(
                fs::read_to_string(root.join(".vscode")).unwrap(),
                "not a directory"
            );
            fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn a_failed_selection_save_restores_the_previous_launch_or_removes_the_new_one() {
        for existing in [false, true] {
            let context = test_context(test_root(&format!("vscode-rollback-{existing}")));
            let path = context.root.join(crate::vscode::LAUNCH_FILE);
            let previous = if existing {
                context.save_selection("test-board", None).unwrap();
                let previous = fs::read(&path).unwrap();
                fs::remove_file(context.selection_path()).unwrap();
                Some(previous)
            } else {
                None
            };
            fs::create_dir_all(context.selection_path()).unwrap();
            assert!(
                context
                    .save_selection("test-board", Some(&"release".parse().unwrap()))
                    .is_err()
            );
            if let Some(previous) = previous {
                assert_eq!(fs::read(path).unwrap(), previous);
            } else {
                assert!(!path.exists());
            }
            assert!(context.selection_path().is_dir());
            fs::remove_dir_all(context.root).unwrap();
        }
    }

    #[test]
    fn profiles_are_inherited_but_remain_unspecified_when_saving_or_building_all_boards() {
        let mut context = test_context(test_root("profile-selection"));
        let other = context.registry.boards["test-board"].clone();
        context.registry.boards.insert("other-board".into(), other);
        let ui = Ui::new(ColorChoice::Never);
        for (args, expected, saved_board, saved_profile) in [
            (
                vec!["build"],
                Some("release"),
                "test-board",
                Some("release"),
            ),
            (
                vec!["build", "other-board"],
                Some("release"),
                "test-board",
                Some("release"),
            ),
            (
                vec!["build", "other-board", "--profile", "dev"],
                Some("dev"),
                "test-board",
                Some("release"),
            ),
            (
                vec!["build", "other-board", "--select"],
                None,
                "other-board",
                None,
            ),
            (
                vec!["build", "other-board", "--select", "--profile", "dev"],
                Some("dev"),
                "other-board",
                Some("dev"),
            ),
            (
                vec![
                    "build",
                    "other-board",
                    "--select",
                    "--profile",
                    "bench-debug",
                ],
                Some("bench-debug"),
                "other-board",
                Some("bench-debug"),
            ),
            (
                vec!["build", "--all-boards"],
                None,
                "test-board",
                Some("release"),
            ),
            (
                vec!["build", "--all-boards", "--profile", "dev"],
                Some("dev"),
                "test-board",
                Some("release"),
            ),
        ] {
            context
                .save_selection("test-board", Some(&"release".parse().unwrap()))
                .unwrap();
            let task = crate::cli::Cli::try_parse_from(std::iter::once("polyfw").chain(args))
                .unwrap()
                .command
                .unwrap();
            let selections = resolve(
                &context,
                &ui,
                &task.board_args().unwrap(),
                task.profile_override(),
            )
            .unwrap();
            assert!(!selections.is_empty());
            for selection in selections {
                assert_eq!(selection.profile.as_ref().map(Profile::name), expected);
                let command = build_command(&context, &selection);
                let mut expected_args = vec!["build", "-p", "test-board", "--locked"];
                if let Some(profile) = expected {
                    expected_args.extend(["--profile", profile]);
                }
                assert_eq!(
                    command.get_args().collect::<Vec<_>>(),
                    expected_args.iter().map(OsStr::new).collect::<Vec<_>>()
                );
            }
            let saved = context.read_selection().unwrap().unwrap();
            assert_eq!(saved.board, saved_board);
            assert_eq!(saved.profile.as_ref().map(Profile::name), saved_profile);
            let launch: serde_json::Value = serde_json::from_slice(
                &fs::read(context.root.join(crate::vscode::LAUNCH_FILE)).unwrap(),
            )
            .unwrap();
            assert_eq!(
                launch["configurations"][0]["name"],
                match saved_profile {
                    Some(profile) => format!("Launch {saved_board} ({profile})"),
                    None => format!("Launch {saved_board}"),
                }
            );
            let state: toml::Value =
                toml::from_str(&fs::read_to_string(context.selection_path()).unwrap()).unwrap();
            assert_eq!(
                state.get("profile").and_then(toml::Value::as_str),
                saved_profile
            );
        }
        let _ = select(&context, &ui, Some("other-board"), false, None).unwrap();
        assert!(context.read_selection().unwrap().unwrap().profile.is_none());
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn a_custom_profile_builds_in_its_own_directory_even_when_it_inherits_release() {
        let mut context = host_test_context("custom-profile");
        let output = Command::new("rustc")
            .args(["--print", "host-tuple"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let target = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        context.registry.bsps.get_mut("test-bsp").unwrap().target = target.clone();
        let workspace = context.root.join("bsp/test");
        let manifest = workspace.join("Cargo.toml");
        let source = fs::read_to_string(&manifest).unwrap();
        fs::write(
            manifest,
            format!("{source}\n[profile.bench-debug]\ninherits = \"release\"\n"),
        )
        .unwrap();
        fs::write(
            workspace.join(".cargo/config.toml"),
            format!("[build]\ntarget = {target:?}\n"),
        )
        .unwrap();
        fs::write(workspace.join("board/src/main.rs"), "fn main() {}\n").unwrap();
        let mut selection = context.selection("test-board").unwrap();
        selection.profile = Some("bench-debug".parse().unwrap());
        build(&context, &Ui::new(ColorChoice::Never), &[selection.clone()]).unwrap();
        let elf = runner::firmware_elf_path(&context.root, &selection)
            .with_extension(std::env::consts::EXE_EXTENSION);
        assert!(elf.is_file(), "{}", elf.display());
        assert!(
            !context
                .root
                .join("target/firmware/test-bsp")
                .join(target)
                .join("release")
                .exists()
        );
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn grouped_checks_share_host_work_but_preserve_each_bsp_and_package_selection() {
        let mut context = test_context(test_root("grouped-checks"));
        let base_bsp = context.selection("test-board").unwrap().bsp;
        for id in ["first", "second"] {
            context.registry.bsps.insert(
                format!("{id}-bsp"),
                Bsp {
                    workspace: format!("bsp/{id}"),
                    ..base_bsp.clone()
                },
            );
        }
        let package = |name: &str, embedded, workspace: &str| PackageScope {
            name: name.into(),
            manifest_path: Path::new(workspace).join(name).join("Cargo.toml"),
            embedded,
            id: format!("{name}-bsp-id"),
            host_id: format!("{name}-owner-id"),
            workspace: context.root.join(workspace),
        };
        let scopes = [
            WorkspaceScope {
                bsp_id: "first-bsp".into(),
                bsp: context.registry.bsps["first-bsp"].clone(),
                firmware: Default::default(),
                packages: vec![
                    package("common-a", true, ""),
                    package("firmware-a", true, "bsp/first"),
                    package("firmware-b", true, "bsp/first"),
                    package("host-helper", false, ""),
                ],
            },
            WorkspaceScope {
                bsp_id: "second-bsp".into(),
                bsp: context.registry.bsps["second-bsp"].clone(),
                firmware: Default::default(),
                packages: vec![
                    package("common-a", true, ""),
                    package("host-only", false, "bsp/second"),
                ],
            },
            WorkspaceScope {
                bsp_id: "empty-bsp".into(),
                bsp: base_bsp,
                firmware: Default::default(),
                packages: Vec::new(),
            },
        ];
        let ui = Ui::new(ColorChoice::Never);
        let mut stages = lint_stages(&context, &ui, &scopes);
        stages.extend(test_stages(&context, &ui, &scopes));
        stages.extend(fmt_stages(&context, &ui, &scopes, true));
        let expected = [
            (
                "bsp/first",
                Some("first-bsp"),
                "lint (emb): common-a, firmware-a, firmware-b",
                "clippy -p common-a-bsp-id -p firmware-a-bsp-id -p firmware-b-bsp-id \
              --lib --bins --locked -- -D warnings",
            ),
            (
                "bsp/second",
                Some("second-bsp"),
                "lint (emb): common-a",
                "clippy -p common-a-bsp-id --lib --bins --locked -- -D warnings",
            ),
            (
                "",
                None,
                "lint (host): common-a, host-helper",
                "clippy -p common-a-owner-id -p host-helper-owner-id \
              --lib --tests --target host-tuple --locked -- -D warnings",
            ),
            (
                "bsp/first",
                Some("first-bsp"),
                "lint (host): firmware-a, firmware-b",
                "clippy -p firmware-a-owner-id -p firmware-b-owner-id \
              --lib --tests --target host-tuple --locked -- -D warnings",
            ),
            (
                "bsp/second",
                Some("second-bsp"),
                "lint (host): host-only",
                "clippy -p host-only-owner-id --lib --tests --target host-tuple \
                 --locked -- -D warnings",
            ),
            (
                "",
                None,
                "test (host): common-a, host-helper",
                "test -p common-a-owner-id -p host-helper-owner-id --target host-tuple --locked",
            ),
            (
                "bsp/first",
                Some("first-bsp"),
                "test (host): firmware-a, firmware-b",
                "test -p firmware-a-owner-id -p firmware-b-owner-id --target host-tuple --locked",
            ),
            (
                "bsp/second",
                Some("second-bsp"),
                "test (host): host-only",
                "test -p host-only-owner-id --target host-tuple --locked",
            ),
            (
                "",
                None,
                "format check : common-a, host-helper",
                "fmt -p common-a -p host-helper --check",
            ),
            (
                "bsp/first",
                Some("first-bsp"),
                "format check : firmware-a, firmware-b",
                "fmt -p firmware-a -p firmware-b --check",
            ),
            (
                "bsp/second",
                Some("second-bsp"),
                "format check : host-only",
                "fmt -p host-only --check",
            ),
        ];
        assert_eq!(stages.len(), expected.len());
        for (Stage(label, command), (directory, cache, expected_label, arguments)) in
            stages.into_iter().zip(expected)
        {
            let workspace = context.root.join(directory);
            assert_eq!(label, expected_label);
            assert_eq!(command.get_program(), OsStr::new("cargo"));
            assert_eq!(command.get_current_dir(), Some(workspace.as_path()));
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                arguments
                    .split_whitespace()
                    .map(OsStr::new)
                    .collect::<Vec<_>>()
            );
            let target_dir = cache.map(|id| context.root.join("target/firmware").join(id));
            assert_eq!(
                command
                    .get_envs()
                    .find(|(key, _)| *key == "CARGO_TARGET_DIR")
                    .and_then(|(_, value)| value),
                target_dir.as_deref().map(Path::as_os_str)
            );
        }
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn resolving_an_explicit_board_with_select_saves_it_before_the_command() {
        let root = test_root("select");
        let context = test_context(root.clone());
        let ui = Ui::new(ColorChoice::Never);

        let selections = resolve(
            &context,
            &ui,
            &BoardArgs {
                board: Some("test-board".into()),
                select: true,
                all_boards: false,
            },
            None,
        )
        .unwrap();

        assert_eq!(selections[0].id, "test-board");
        let saved = context.read_selection().unwrap().unwrap();
        assert_eq!(saved.board, "test-board");
        assert!(saved.profile.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolving_an_explicit_board_without_select_does_not_save_it() {
        let root = test_root("transient");
        let context = test_context(root.clone());
        let ui = Ui::new(ColorChoice::Never);

        let selections = resolve(
            &context,
            &ui,
            &BoardArgs {
                board: Some("test-board".into()),
                select: false,
                all_boards: false,
            },
            None,
        )
        .unwrap();

        assert_eq!(selections[0].id, "test-board");
        assert!(!context.selection_path().exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn clean_command_uses_the_bsp_workspace_and_firmware_target() {
        let root = test_root("clean-command");
        let context = test_context(root.clone());
        let selection = context.selection("test-board").unwrap();
        let command = clean_command(&context, &selection, false);
        let workspace = root.join("bsp/test");
        let target_dir = root.join("target/firmware/test-bsp");

        assert_eq!(command.get_program(), OsStr::new("cargo"));
        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [
                OsStr::new("clean"),
                OsStr::new("-p"),
                OsStr::new("test-board")
            ]
        );
        assert_eq!(command.get_current_dir(), Some(workspace.as_path()));
        assert_eq!(
            command
                .get_envs()
                .find(|(key, _)| *key == OsStr::new("CARGO_TARGET_DIR"))
                .and_then(|(_, value)| value)
                .map(Path::new),
            Some(target_dir.as_path())
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleaning_a_package_lists_all_affected_boards_in_that_bsp() {
        const MODE: &str = "FLEET_XTASK_TEST_CLEAN_OUTPUT";
        if std::env::var_os(MODE).is_some() {
            let mut context = host_test_context("clean-output");
            let board = context.registry.boards["test-board"].clone();
            context.registry.boards.insert(
                "secondary-board".into(),
                Board {
                    bin: Some("second-image".into()),
                    resolved_bin: "second-image".into(),
                    ..board.clone()
                },
            );
            context.registry.boards.insert(
                "unrelated-board".into(),
                Board {
                    package: "test-bsp".into(),
                    resolved_bin: "test-bsp".into(),
                    ..board.clone()
                },
            );
            let bsp = context.registry.bsps["test-bsp"].clone();
            context.registry.bsps.insert("other-family".into(), bsp);
            context.registry.boards.insert(
                "other-family-board".into(),
                Board {
                    bsp: "other-family".into(),
                    ..board
                },
            );
            let root = context.root.clone();
            let _ = run(
                context,
                Task::Clean(BoardArgs {
                    board: Some("test-board".into()),
                    select: false,
                    all_boards: false,
                }),
                &Ui::new(ColorChoice::Never),
            )
            .unwrap();
            fs::remove_dir_all(root).unwrap();
            return;
        }

        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "commands::tests::cleaning_a_package_lists_all_affected_boards_in_that_bsp",
                "--nocapture",
            ])
            .env(MODE, "1")
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert!(output.status.success(), "{stdout}\n{stderr}");
        assert!(
            stdout.contains(
                "cleaning package test-board (BSP: test-bsp; boards: secondary-board, test-board)"
            ),
            "{stdout}"
        );
        assert!(!stdout.contains("unrelated-board"), "{stdout}");
        assert!(!stdout.contains("other-family-board"), "{stdout}");
        assert!(stderr.contains("$ cargo clean -p test-board"), "{stderr}");
        assert!(!stderr.contains("--bin"), "{stderr}");
    }

    #[test]
    fn all_boards_clean_command_keeps_the_whole_bsp_target() {
        let root = test_root("clean-all-command");
        let context = test_context(root.clone());
        let selection = context.selection("test-board").unwrap();
        let command = clean_command(&context, &selection, true);

        assert_eq!(
            command.get_args().collect::<Vec<_>>(),
            [OsStr::new("clean")]
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_pass_a_binary_only_when_the_registry_explicitly_selects_it() {
        for bin in [None, Some("test-board"), Some("robot-image")] {
            let mut context = test_context(test_root("build-bin-argument"));
            let board = context.registry.boards.get_mut("test-board").unwrap();
            board.bin = bin.map(str::to_owned);
            board.resolved_bin = bin.unwrap_or("robot-image").into();
            let selection = context.selection("test-board").unwrap();
            let command = build_command(&context, &selection);
            let mut expected = vec!["build", "-p", "test-board"];
            if let Some(bin) = bin {
                expected.extend(["--bin", bin]);
            }
            expected.push("--locked");
            assert_eq!(
                command.get_args().collect::<Vec<_>>(),
                expected.iter().map(OsStr::new).collect::<Vec<_>>()
            );
            fs::remove_dir_all(context.root).unwrap();
        }
    }

    #[test]
    fn builds_compile_an_inferred_binary_or_only_the_explicit_binary() {
        let output = Command::new("rustc")
            .args(["--print", "host-tuple"])
            .output()
            .unwrap();
        assert!(output.status.success());
        let target = String::from_utf8(output.stdout).unwrap().trim().to_owned();
        for (index, bin) in [None, Some("robot-image")].into_iter().enumerate() {
            let name = bin.unwrap_or("robot-image");
            let context = host_test_context(&format!("build-{index}"));
            let workspace = context.root.join("bsp/test");
            fs::write(
                workspace.join(".cargo/config.toml"),
                format!("[build]\ntarget = {target:?}\n"),
            )
            .unwrap();
            fs::create_dir_all(workspace.join("board/src/bin")).unwrap();
            fs::write(
                workspace.join("board/src/bin").join(format!("{name}.rs")),
                "fn main() {}\n",
            )
            .unwrap();
            let mut registry = String::from(
                "config-version = \"0.1.0\"\nmin-polyfw-version = \"0.1.0\"\n[bsps.test-bsp]\nworkspace = \"bsp/test\"\n\
             chip = \"test-chip\"\n\
                 [boards.test-board]\ndescription = \"test board\"\nbsp = \"test-bsp\"\n\
                 package = \"test-board\"\n",
            );
            if let Some(bin) = bin {
                fs::write(
                    workspace.join("board/src/bin/unselected.rs"),
                    "compile_error!(\"an unrelated binary must not be built\");\n",
                )
                .unwrap();
                registry.push_str(&format!(
                    "bin = {bin:?}\n\
                     [boards.other-firmware]\ndescription = \"another binary\"\n\
                     bsp = \"test-bsp\"\npackage = \"test-board\"\nbin = \"unselected\"\n"
                ));
            }
            fs::write(context.root.join("polyfw.toml"), registry).unwrap();
            let context = Context::load(context.root).unwrap();

            let selection = context.selection("test-board").unwrap();
            assert_eq!(selection.board.resolved_bin, name);
            assert_eq!(selection.board.bin.as_deref(), bin);
            build(&context, &Ui::new(ColorChoice::Never), &[selection]).unwrap();
            assert!(
                context
                    .root
                    .join("target/firmware/test-bsp")
                    .join(&target)
                    .join("debug")
                    .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
                    .is_file()
            );
            fs::remove_dir_all(context.root).unwrap();
        }
    }

    #[test]
    fn host_lint_rejects_mistakes_in_both_bsp_and_firmware_tests() {
        for (package, path) in [
            ("test-bsp", "src/lib.rs"),
            ("test-board", "board/src/lib.rs"),
        ] {
            let context = host_test_context(&format!("test-lint-{package}"));
            fs::write(
                context.root.join("bsp/test").join(path),
                "#![no_std]\n#![forbid(unsafe_code)]\n\
                 #[cfg(test)]\nmod tests {\n\
                 #[test]\nfn an_empty_slice_has_no_elements() {\n\
                 let values: &[u8] = &[];\nassert!(values.len() == 0);\n}\n}\n",
            )
            .unwrap();
            let selection = context.selection("test-board").unwrap();
            let scopes = scope::resolve(&context, &[selection]).unwrap();
            let ui = Ui::new(ColorChoice::Never);
            let mut host_stages = 0;

            for Stage(label, mut command) in lint_stages(&context, &ui, &scopes) {
                let host = command.get_args().any(|argument| argument == "host-tuple");
                let output = command.output().unwrap();
                let stderr = String::from_utf8_lossy(&output.stderr);
                if host {
                    assert!(
                        !output.status.success(),
                        "{label} missed {package}'s test lint"
                    );
                    assert!(stderr.contains("clippy::len_zero"), "{label}: {stderr}");
                    host_stages += 1;
                } else {
                    assert!(output.status.success(), "{label}: {stderr}");
                }
            }
            assert_eq!(host_stages, 1, "both packages share one host lint stage");
            fs::remove_dir_all(context.root).unwrap();
        }
    }

    #[test]
    fn host_tests_reject_failing_doctests_in_both_bsp_and_firmware() {
        for (package, path) in [
            ("test-bsp", "src/lib.rs"),
            ("test-board", "board/src/lib.rs"),
        ] {
            let context = host_test_context(&format!("doctest-{package}"));
            fs::write(
                context.root.join("bsp/test").join(path),
                "//! ```\n//! assert!(false);\n//! ```\n\
                 #![no_std]\n#![forbid(unsafe_code)]\n",
            )
            .unwrap();
            let selection = context.selection("test-board").unwrap();
            let scopes = scope::resolve(&context, &[selection]).unwrap();
            let ui = Ui::new(ColorChoice::Never);
            let stages = test_stages(&context, &ui, &scopes);
            assert_eq!(stages.len(), 1, "both packages share one host test stage");

            for Stage(label, mut command) in stages {
                let output = command.output().unwrap();
                assert!(
                    !output.status.success(),
                    "{label} missed {package}'s failing doctest"
                );
                assert!(
                    String::from_utf8_lossy(&output.stderr).contains("doctest failed"),
                    "{label}: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            fs::remove_dir_all(context.root).unwrap();
        }
    }

    fn host_test_context(label: &str) -> Context {
        let root = test_root(label);
        let workspace = root.join("bsp/test");
        fs::create_dir_all(workspace.join("board/src")).unwrap();
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::create_dir_all(workspace.join(".cargo")).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"test-bsp\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [workspace]\nmembers = [\"board\"]\nresolver = \"3\"\n",
        )
        .unwrap();
        fs::write(
            workspace.join("board/Cargo.toml"),
            "[package]\nname = \"test-board\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [target.'cfg(target_os = \"none\")'.dependencies]\n\
             test-bsp = { path = \"..\" }\n",
        )
        .unwrap();
        let source = "#![no_std]\n#![forbid(unsafe_code)]\n\
             #[cfg(test)]\nmod tests {\n\
             #[test]\nfn an_empty_slice_has_no_elements() {\n\
             let values: &[u8] = &[];\nassert!(values.is_empty());\n}\n}\n";
        fs::write(workspace.join("src/lib.rs"), source).unwrap();
        fs::write(workspace.join("board/src/lib.rs"), source).unwrap();
        fs::write(
            workspace.join(".cargo/config.toml"),
            "[build]\ntarget = \"thumbv7em-none-eabihf\"\n",
        )
        .unwrap();
        let output = runner::workspace_cargo(&workspace)
            .args(["generate-lockfile", "--offline"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        test_context(root)
    }

    fn test_context(root: PathBuf) -> Context {
        Context {
            root,
            registry: Registry {
                config_version: "0.1.0".into(),
                min_polyfw_version: "0.1.0".into(),
                config: crate::model::Config::default(),
                bsps: BTreeMap::from([(
                    "test-bsp".into(),
                    Bsp {
                        workspace: "bsp/test".into(),
                        target: "thumbv7em-none-eabihf".into(),
                        cpu: Some("cortex-m7".into()),
                        chip: "test-chip".into(),
                    },
                )]),
                boards: BTreeMap::from([(
                    "test-board".into(),
                    Board {
                        description: "test board".into(),
                        bsp: "test-bsp".into(),
                        package: "test-board".into(),
                        bin: None,
                        resolved_bin: "test-board".into(),
                    },
                )]),
            },
        }
    }

    fn test_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("polyfw-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(
            root.join("rust-toolchain.toml"),
            include_str!("../rust-toolchain.toml"),
        )
        .unwrap();
        root
    }
}
