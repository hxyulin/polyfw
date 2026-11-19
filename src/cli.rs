use std::num::NonZeroU32;
use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Styles};
use clap::{
    Args, ColorChoice, Command, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum,
};

use crate::profile::Profile;
use crate::ui::color_enabled;

/// Build, check, flash and monitor embedded Rust firmware registered in polyfw.toml.
///
/// A board ID names a programmable target. Its BSP identifies the consuming Cargo
/// workspace and probe chip. Cargo owns compiler settings, dependencies and profiles.
#[derive(Debug, Parser)]
#[command(
    name = "polyfw",
    bin_name = "polyfw",
    version,
    after_long_help = "Project and selection:
  Find polyfw.toml in the current directory or an ancestor, or pass --project-root DIR.
  Pass BOARD for one invocation, or omit it to use the saved board and profile.
  If no board is saved, choose interactively; invalid saved state is an error.
  Use select or --select to save a board. Saving keeps only an explicit profile.
  Build/check/clean commands accept --all-boards; hardware commands use one board.
  Pickers require a terminal. Use BOARD or --all-boards in scripts.
  run, flash and attach use an existing image: run build after changing firmware source.

Examples:
  polyfw info
  polyfw select controller --release
  polyfw verify
  polyfw build
  polyfw probes
  polyfw run
  polyfw verify --all-boards
  polyfw help build",
    styles = Styles::styled()
        .header(AnsiColor::BrightGreen.on_default())
        .usage(AnsiColor::BrightGreen.on_default())
        .literal(AnsiColor::BrightCyan.on_default())
        .placeholder(AnsiColor::Cyan.on_default())
        .context(AnsiColor::BrightBlack.on_default())
        .context_value(AnsiColor::BrightCyan.on_default())
)]
pub(crate) struct Cli {
    /// Operate on the directory containing polyfw.toml.
    ///
    /// Relative paths start at the caller's working directory. Otherwise, search
    /// that directory and its ancestors for the nearest polyfw.toml. An invalid
    /// selected project is an error. Help, version, schema and probes ignore this option.
    #[arg(
        long,
        value_name = "DIR",
        global = true,
        overrides_with = "project_root",
        verbatim_doc_comment
    )]
    pub(crate) project_root: Option<PathBuf>,

    /// Choose automatic colors, always-on colors or plain output.
    #[arg(
        long,
        value_enum,
        default_value_t = ColorChoice::Auto,
        global = true,
        verbatim_doc_comment
    )]
    pub(crate) color: ColorChoice,

    #[command(subcommand)]
    pub(crate) command: Option<Task>,
}

impl Cli {
    pub(crate) fn parse_with_color() -> (Self, Command) {
        let args: Vec<_> = std::env::args_os().collect();
        // Clap exits on help before returning matches, so its color policy must be set first.
        let color = args
            .iter()
            .enumerate()
            .skip(1)
            .take_while(|(_, arg)| *arg != "--")
            .find_map(|(index, arg)| {
                let arg = arg.to_str()?;
                let value = if arg == "--color" {
                    args.get(index + 1)?.to_str()?
                } else {
                    arg.strip_prefix("--color=")?
                };
                ColorChoice::from_str(value, false).ok()
            })
            .unwrap_or(ColorChoice::Auto);
        let mut command = Self::command().color(if color_enabled(color) {
            ColorChoice::Always
        } else {
            ColorChoice::Never
        });
        let matches = command
            .try_get_matches_from_mut(args)
            .unwrap_or_else(|error| error.exit());
        let cli = Self::from_arg_matches(&matches).unwrap_or_else(|error| error.exit());
        (cli, command)
    }
}

#[derive(Clone, Debug, Args)]
pub(crate) struct BoardArgs {
    /// Board ID for this invocation; otherwise use the saved board or a picker.
    ///
    /// Use an ID from polyfw info. A named or picked board is transient unless
    /// --select is passed. The picker opens only when no board is saved; stale or
    /// malformed saved state is an error, including with a one-time BOARD override.
    /// Replace it with polyfw select BOARD or clear it with polyfw select --clear.
    /// Pickers require a terminal; name a board in scripts.
    #[arg(verbatim_doc_comment)]
    pub(crate) board: Option<String>,
    /// Save the named board, or always open the picker and save its choice.
    ///
    /// Save before running the action. Without BOARD, the picker opens even if a
    /// board is already saved and requires a terminal. State is stored in
    /// .polyfw-state/selected; saving retains only an explicitly supplied profile.
    #[arg(short, long, verbatim_doc_comment)]
    pub(crate) select: bool,
    /// Operate on every registered board, ignoring saved board/profile state.
    ///
    /// Include every registered board. Conflicts with BOARD and --select.
    #[arg(
        short = 'a',
        long = "all-boards",
        conflicts_with_all = ["board", "select"],
        verbatim_doc_comment
    )]
    pub(crate) all_boards: bool,
}

#[derive(Debug, Default, Args)]
pub(crate) struct ProbeArgs {
    /// Board ID; otherwise use the saved board or pick one if no board is saved.
    pub(crate) board: Option<String>,
    /// Save BOARD before acting, or always open the picker and save its choice.
    #[arg(short, long)]
    pub(crate) select: bool,
    /// Use probe VID:PID[:SERIAL] for this session; otherwise let probe-rs choose.
    #[arg(long, value_name = "ID")]
    pub(crate) probe: Option<String>,
    /// Set debug interface speed in kHz for this session; otherwise use probe-rs settings.
    #[arg(long = "speed", value_name = "KHZ")]
    pub(crate) speed_khz: Option<NonZeroU32>,
    /// Assert the target's reset line while connecting.
    #[arg(long)]
    pub(crate) connect_under_reset: bool,
}

#[derive(Debug, Default, Args)]
pub(crate) struct ProfileArgs {
    /// Choose the dev profile explicitly; equivalent to --profile dev.
    #[arg(long, conflicts_with_all = ["release", "profile"])]
    pub(crate) dev: bool,
    /// Choose the release profile; equivalent to --profile release.
    #[arg(short = 'r', long, conflicts_with = "profile")]
    pub(crate) release: bool,
    /// Use this Cargo profile instead of the saved profile.
    ///
    /// Single-board actions otherwise inherit the saved profile. Saving a selection
    /// and --all-boards use only an explicit profile. Without a resolved profile,
    /// omit the Cargo flag and use its default.
    #[arg(long, value_name = "NAME")]
    pub(crate) profile: Option<Profile>,
}

impl ProfileArgs {
    pub(crate) fn explicit(&self) -> Option<Profile> {
        if self.release {
            Some("release".parse().expect("release is a valid profile name"))
        } else if self.dev {
            Some("dev".parse().expect("dev is a valid profile name"))
        } else {
            self.profile.clone()
        }
    }
}

#[derive(Debug, Default, Args)]
pub(crate) struct MonitorArgs {
    /// Choose RTT buffer behavior; the default drops whole messages when full.
    #[arg(long, value_enum, default_value_t = RttChannelMode::NoBlockSkip)]
    pub(crate) rtt_channel_mode: RttChannelMode,
    /// Disable probe-rs capture of target resets.
    #[arg(long)]
    pub(crate) no_catch_reset: bool,
    /// Disable probe-rs capture of HardFault exceptions.
    #[arg(long)]
    pub(crate) no_catch_hardfault: bool,
}

#[derive(Clone, Copy, Debug, Default, ValueEnum)]
pub(crate) enum RttChannelMode {
    #[default]
    NoBlockSkip,
    NoBlockTrim,
    BlockIfFull,
}

impl RttChannelMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::NoBlockSkip => "no-block-skip",
            Self::NoBlockTrim => "no-block-trim",
            Self::BlockIfFull => "block-if-full",
        }
    }
}

#[derive(Debug, Subcommand)]
pub(crate) enum Task {
    /// Print or write JSON Schema for this version's polyfw.toml.
    ///
    /// Works without a project, Rust installation or hardware tools.
    Schema {
        /// Write to FILE instead of stdout; relative paths start at the current directory.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },

    /// Inspect registered boards, existing images and the saved selection.
    ///
    /// List boards and their referenced BSPs, with dev/release ELF presence and any
    /// different saved profile. --board [BOARD] adds one board's resolved configuration,
    /// ELF paths and local check dependencies. Omit BOARD after the flag to pick one
    /// without saving. A stale saved board is reported; malformed state is an error.
    /// This query does not build, write state/editor files or connect to hardware.
    /// ELF presence establishes neither source freshness nor a match with running firmware.
    #[command(
        visible_alias = "i",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw info
  polyfw info --board
  polyfw info --board controller
  polyfw info --board controller --json"
    )]
    Info {
        /// Show one board's details; omit BOARD to pick interactively without saving.
        #[arg(long, value_name = "BOARD")]
        board: Option<Option<String>>,
        /// Write the selected view as JSON to stdout; prompts use stderr.
        #[arg(long)]
        json: bool,
    },

    /// Save a board and explicit profile, or clear the saved selection.
    ///
    /// Save BOARD in .polyfw-state/selected for later commands. Saving without a
    /// profile clears the previous profile. With no BOARD, always open the picker;
    /// this requires a terminal. When editor generation is enabled, saving also replaces
    /// the whole VS Code launch file.
    /// --clear removes saved state and the managed launch file without needing Cargo.
    #[command(
        visible_alias = "s",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw select controller --release
  polyfw select
  polyfw select --clear"
    )]
    Select {
        /// Save this board ID; otherwise open the picker.
        board: Option<String>,
        /// Remove saved selection and the managed VS Code launch file.
        ///
        /// Does not parse the old state or load Cargo workspaces. Editor files are
        /// untouched when generation is disabled. Conflicts with BOARD and profile flags.
        #[arg(
            long,
            conflicts_with_all = ["board", "dev", "release", "profile"],
            verbatim_doc_comment
        )]
        clear: bool,
        #[command(flatten)]
        profile: ProfileArgs,
    },

    /// Clean a firmware package or all BSP firmware caches.
    ///
    /// One board cleans its whole package, including other binaries and boards sharing
    /// it in the BSP. --all-boards cleans each BSP's full cache, including dependencies.
    /// The notice lists affected boards. All cleaning stays under target/firmware/<bsp>.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw clean controller
  polyfw clean --all-boards"
    )]
    Clean(BoardArgs),

    /// Build the selected board or every registered board with Cargo.
    ///
    /// Build each board separately from its BSP workspace with --locked and that
    /// workspace's target/settings. An explicit registry bin selects --bin; otherwise
    /// keep Cargo's default targets. Store artifacts under target/firmware/<bsp>.
    /// Inherit a saved profile for one board, or override it with a profile flag.
    /// --all-boards uses only the explicitly supplied profile.
    #[command(
        visible_alias = "b",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw build
  polyfw build --select controller --release
  polyfw build --all-boards --release"
    )]
    Build {
        #[command(flatten)]
        boards: BoardArgs,
        #[command(flatten)]
        profile: ProfileArgs,
    },

    /// List connected probes using probe-rs; no project or Rust tools required.
    Probes,

    /// Flash and verify an existing ELF, then start and monitor RTT/defmt.
    ///
    /// Use the resolved board/profile image without building. Run polyfw build after
    /// source changes. Omitted probe and speed options are delegated to probe-rs.
    /// Ctrl-C ends the probe-rs session; firmware may continue running.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw run controller
  polyfw run --release
  polyfw run controller --probe VID:PID:SERIAL --speed 4000"
    )]
    Run {
        #[command(flatten)]
        target: ProbeArgs,
        #[command(flatten)]
        profile: ProfileArgs,
        #[command(flatten)]
        monitor: MonitorArgs,
    },

    /// Flash and verify an existing ELF, reset the board, then exit.
    ///
    /// Use the resolved board/profile image without building. Run polyfw build after
    /// source changes. This command does not start an RTT monitor.
    #[command(
        visible_aliases = ["f", "download", "d"],
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw flash controller --release
  polyfw flash --select controller"
    )]
    Flash {
        #[command(flatten)]
        target: ProbeArgs,
        #[command(flatten)]
        profile: ProfileArgs,
    },

    /// Attach an RTT/defmt monitor using the matching existing ELF.
    ///
    /// Does not build or flash. The resolved board/profile ELF must match the image
    /// already running. Connecting can briefly halt the core; --connect-under-reset
    /// asserts the reset line.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw attach controller
  polyfw attach --release --no-catch-reset"
    )]
    Attach {
        #[command(flatten)]
        target: ProbeArgs,
        #[command(flatten)]
        profile: ProfileArgs,
        #[command(flatten)]
        monitor: MonitorArgs,
    },

    /// Reset one board without building, flashing or requiring an ELF.
    ///
    /// Needs project configuration and probe-rs. Saving with --select also needs Cargo
    /// when VS Code generation is enabled.
    Reset(ProbeArgs),

    /// Rewrite formatting for firmware and selected local dependencies.
    ///
    /// Run cargo fmt in place for the firmware and reachable path packages matched
    /// by config.local-dependencies. Include whole packages, including other binaries,
    /// once per owning workspace. Use verify to check formatting without rewriting.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw fmt controller
  polyfw fmt --all-boards"
    )]
    Fmt(BoardArgs),

    /// Check firmware and selected local dependencies with MCU and host Clippy.
    ///
    /// Deny warnings. Check runtime libraries and binaries for the MCU, then selected
    /// libraries and tests for the host. config.local-dependencies filters reachable
    /// path packages, including local test helpers. Other binaries in a package are
    /// included. Group MCU checks per BSP and host checks per owning workspace,
    /// using workspace defaults and deduplicating packages with --all-boards.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw lint controller
  polyfw lint --all-boards"
    )]
    Lint(BoardArgs),

    /// Run host tests and doctests for firmware and selected local dependencies.
    ///
    /// No board hardware is required. Run each package once in its owning workspace
    /// with --target host-tuple, workspace defaults and dev-dependencies. Use Cargo's
    /// default test targets, including other binaries when their test settings allow.
    /// config.local-dependencies filters reachable path packages and local test helpers.
    #[command(
        visible_alias = "t",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw test controller
  polyfw test --all-boards"
    )]
    Test(BoardArgs),

    /// Check formatting, run MCU/host Clippy, then run host tests.
    ///
    /// Check without rewriting source, deny lint warnings, and stop at the first
    /// failure. Include selected firmware and reachable path packages matched by
    /// config.local-dependencies across workspaces. --all-boards checks every board.
    /// Does not build a firmware image. Cargo may need network access for dependencies.
    #[command(
        visible_alias = "v",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw verify
  polyfw verify controller
  polyfw verify --all-boards"
    )]
    Verify(BoardArgs),
}

impl Task {
    pub(crate) fn profile_override(&self) -> Option<Profile> {
        match self {
            Self::Select { profile, .. }
            | Self::Build { profile, .. }
            | Self::Run { profile, .. }
            | Self::Flash { profile, .. }
            | Self::Attach { profile, .. } => profile.explicit(),
            Self::Schema { .. }
            | Self::Info { .. }
            | Self::Clean(_)
            | Self::Probes
            | Self::Reset(_)
            | Self::Fmt(_)
            | Self::Lint(_)
            | Self::Test(_)
            | Self::Verify(_) => None,
        }
    }

    pub(crate) fn board_args(&self) -> Option<BoardArgs> {
        match self {
            Self::Schema { .. } | Self::Info { .. } | Self::Select { .. } | Self::Probes => None,
            Self::Clean(boards)
            | Self::Build { boards, .. }
            | Self::Fmt(boards)
            | Self::Lint(boards)
            | Self::Test(boards)
            | Self::Verify(boards) => Some(boards.clone()),
            Self::Run { target, .. }
            | Self::Flash { target, .. }
            | Self::Attach { target, .. }
            | Self::Reset(target) => Some(BoardArgs {
                board: target.board.clone(),
                select: target.select,
                all_boards: false,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[test]
    fn the_project_root_is_accepted_before_or_after_every_command() {
        for command in Cli::command().get_subcommands() {
            for args in [
                vec![
                    "polyfw",
                    "--project-root",
                    "../another project",
                    command.get_name(),
                ],
                vec![
                    "polyfw",
                    command.get_name(),
                    "--project-root",
                    "../another project",
                ],
            ] {
                let cli = Cli::try_parse_from(args).unwrap();
                assert_eq!(cli.project_root, Some(PathBuf::from("../another project")));
            }
        }
        assert!(
            Cli::try_parse_from(["polyfw", "info"])
                .unwrap()
                .project_root
                .is_none()
        );
        assert!(Cli::try_parse_from(["polyfw", "info", "--project-root"]).is_err());
    }

    #[test]
    fn info_distinguishes_an_overview_a_picker_and_a_named_board() {
        for command in ["info", "i"] {
            for (flags, expected_board, expected_json) in [
                (vec![], None, false),
                (vec!["--json"], None, true),
                (vec!["--board"], None, false),
                (vec!["--board", "--json"], None, true),
                (vec!["--json", "--board"], None, true),
                (
                    vec!["--board", "sentry-chassis"],
                    Some("sentry-chassis"),
                    false,
                ),
                (
                    vec!["--board", "sentry-chassis", "--json"],
                    Some("sentry-chassis"),
                    true,
                ),
            ] {
                let has_board_flag = flags.contains(&"--board");
                let mut args = vec!["polyfw", command];
                args.extend(flags);
                let task = Cli::try_parse_from(args).unwrap().command.unwrap();
                assert!(task.board_args().is_none());
                assert!(task.profile_override().is_none());
                let Task::Info { board, json } = task else {
                    panic!("expected an info query");
                };
                assert_eq!(board.is_some(), has_board_flag);
                assert_eq!(
                    board.as_ref().and_then(|board| board.as_deref()),
                    expected_board
                );
                assert_eq!(json, expected_json);
            }
        }
        for flags in [
            vec!["sentry-chassis"],
            vec!["--profile", "release"],
            vec!["-r"],
            vec!["--release"],
            vec!["--dev"],
            vec!["--select"],
            vec!["--all-boards"],
        ] {
            let mut args = vec!["polyfw", "info"];
            args.extend(flags);
            assert!(Cli::try_parse_from(args).is_err());
        }
    }

    #[test]
    fn image_commands_accept_cargo_profile_flags_and_reject_conflicts_and_path_names() {
        for command in ["select", "build", "run", "flash", "attach"] {
            let cli = Cli::try_parse_from(["polyfw", command]).unwrap();
            assert!(cli.command.unwrap().profile_override().is_none());
            for flag in ["-r", "--release"] {
                let cli = Cli::try_parse_from(["polyfw", command, flag]).unwrap();
                assert_eq!(
                    cli.command.unwrap().profile_override().unwrap().name(),
                    "release"
                );
                let error = Cli::try_parse_from(["polyfw", command, flag, "--profile", "release"])
                    .unwrap_err();
                assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
            }
            let cli = Cli::try_parse_from(["polyfw", command, "--dev"]).unwrap();
            assert_eq!(
                cli.command.unwrap().profile_override().unwrap().name(),
                "dev"
            );
            for flags in [
                vec!["-r"],
                vec!["--release"],
                vec!["--profile", "dev"],
                vec!["--profile", "release"],
                vec!["--profile", "bench-debug"],
            ] {
                let mut args = vec!["polyfw", command, "--dev"];
                args.extend(flags);
                let error = Cli::try_parse_from(args).unwrap_err();
                assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
            }
            let cli = Cli::try_parse_from(["polyfw", command, "--profile", "bench-debug"]).unwrap();
            assert_eq!(
                cli.command.unwrap().profile_override().unwrap().name(),
                "bench-debug"
            );
            assert!(Cli::try_parse_from(["polyfw", command, "--profile", "../image"]).is_err());
        }
        for flags in [vec!["-r"], vec!["--dev"], vec!["--profile", "dev"]] {
            let mut args = vec!["polyfw", "select", "--clear"];
            args.extend(flags);
            let error = Cli::try_parse_from(args).unwrap_err();
            assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
        }
    }

    #[test]
    fn board_commands_accept_select_with_or_without_a_board() {
        for command in ["build", "clean", "fmt", "lint", "test", "verify"] {
            for flag in ["-s", "--select"] {
                let cli = Cli::try_parse_from(["polyfw", command, flag, "sentry-chassis"])
                    .expect("select with a board must parse");
                let boards = cli.command.as_ref().unwrap().board_args().unwrap();
                assert!(boards.select);
                assert_eq!(boards.board.as_deref(), Some("sentry-chassis"));

                let cli = Cli::try_parse_from(["polyfw", command, flag])
                    .expect("select without a board must parse");
                let boards = cli.command.as_ref().unwrap().board_args().unwrap();
                assert!(boards.select);
                assert!(boards.board.is_none());
            }
        }
    }

    #[test]
    fn all_boards_conflicts_with_a_board_and_select() {
        for command in ["build", "clean", "fmt", "lint", "test", "verify"] {
            for argument in ["--select", "sentry-chassis"] {
                let error = Cli::try_parse_from(["polyfw", command, "--all-boards", argument])
                    .expect_err("all boards must reject single-board options");
                assert_eq!(error.kind(), clap::error::ErrorKind::ArgumentConflict);
            }
        }
    }

    #[test]
    fn hardware_commands_select_one_board_and_reject_all_boards() {
        for command in ["run", "flash", "attach", "reset"] {
            let cli =
                Cli::try_parse_from(["polyfw", command, "--select", "sentry-chassis"]).unwrap();
            let boards = cli.command.as_ref().unwrap().board_args().unwrap();
            assert!(boards.select);
            assert_eq!(boards.board.as_deref(), Some("sentry-chassis"));
            assert!(!boards.all_boards);
            assert!(Cli::try_parse_from(["polyfw", command, "--all-boards"]).is_err());
            assert!(Cli::try_parse_from(["polyfw", command, "--speed", "0"]).is_err());
        }
        for argument in ["sentry-chassis", "--select", "--probe", "--speed"] {
            assert!(Cli::try_parse_from(["polyfw", "probes", argument]).is_err());
        }
        assert!(Cli::try_parse_from(["polyfw", "reset", "--release"]).is_err());
    }

    #[test]
    fn flash_aliases_and_reset_preserve_their_options() {
        for (command, flash) in [
            ("f", true),
            ("download", true),
            ("d", true),
            ("reset", false),
        ] {
            let mut args = vec![
                "polyfw",
                command,
                "sentry-chassis",
                "--select",
                "--probe",
                "1234:5678:serial",
                "--speed",
                "4000",
                "--connect-under-reset",
            ];
            if flash {
                args.push("--release");
            }
            let cli = Cli::try_parse_from(args).unwrap();
            let target = match cli.command.unwrap() {
                Task::Flash { target, profile } => {
                    assert!(flash);
                    assert_eq!(profile.explicit().unwrap().name(), "release");
                    target
                }
                Task::Reset(target) => {
                    assert!(!flash);
                    target
                }
                Task::Schema { .. }
                | Task::Info { .. }
                | Task::Select { .. }
                | Task::Clean(_)
                | Task::Build { .. }
                | Task::Probes
                | Task::Run { .. }
                | Task::Attach { .. }
                | Task::Fmt(_)
                | Task::Lint(_)
                | Task::Test(_)
                | Task::Verify(_) => panic!("unexpected command for {command}"),
            };
            assert_eq!(target.board.as_deref(), Some("sentry-chassis"));
            assert!(target.select);
            assert_eq!(target.probe.as_deref(), Some("1234:5678:serial"));
            assert_eq!(target.speed_khz.unwrap().get(), 4000);
            assert!(target.connect_under_reset);
        }
    }

    #[test]
    fn run_and_attach_default_to_nonblocking_logs_and_accept_native_monitor_controls() {
        for command in ["run", "attach"] {
            let cli = Cli::try_parse_from(["polyfw", command]).unwrap();
            let (target, profile, monitor) = monitor_task(cli.command.unwrap());
            assert!(profile.explicit().is_none());
            assert!(target.probe.is_none());
            assert!(target.speed_khz.is_none());
            assert_eq!(monitor.rtt_channel_mode.as_str(), "no-block-skip");
            assert!(!monitor.no_catch_reset);
            assert!(!monitor.no_catch_hardfault);

            let cli = Cli::try_parse_from([
                "polyfw",
                command,
                "--release",
                "--probe",
                "1234:5678:serial",
                "--speed",
                "4000",
                "--connect-under-reset",
                "--rtt-channel-mode",
                "block-if-full",
                "--no-catch-reset",
                "--no-catch-hardfault",
            ])
            .unwrap();
            let (target, profile, monitor) = monitor_task(cli.command.unwrap());
            assert_eq!(profile.explicit().unwrap().name(), "release");
            assert_eq!(target.probe.as_deref(), Some("1234:5678:serial"));
            assert_eq!(target.speed_khz.unwrap().get(), 4000);
            assert!(target.connect_under_reset);
            assert_eq!(monitor.rtt_channel_mode.as_str(), "block-if-full");
            assert!(monitor.no_catch_reset);
            assert!(monitor.no_catch_hardfault);
        }
        for command in ["flash", "reset"] {
            assert!(Cli::try_parse_from(["polyfw", command, "--no-catch-reset"]).is_err());
        }
    }

    fn monitor_task(task: Task) -> (ProbeArgs, ProfileArgs, MonitorArgs) {
        match task {
            Task::Run {
                target,
                profile,
                monitor,
            }
            | Task::Attach {
                target,
                profile,
                monitor,
            } => (target, profile, monitor),
            Task::Schema { .. }
            | Task::Info { .. }
            | Task::Select { .. }
            | Task::Clean(_)
            | Task::Build { .. }
            | Task::Probes
            | Task::Flash { .. }
            | Task::Reset(_)
            | Task::Fmt(_)
            | Task::Lint(_)
            | Task::Test(_)
            | Task::Verify(_) => panic!("expected a monitor command"),
        }
    }
}
