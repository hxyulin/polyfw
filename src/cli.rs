use std::num::NonZeroU32;
use std::path::PathBuf;

use clap::builder::styling::{AnsiColor, Styles};
use clap::{
    Args, ColorChoice, Command, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum,
};

use crate::profile::Profile;
use crate::ui::color_enabled;

/// Build, check, flash and monitor registered embedded Rust firmware.
///
/// A board ID, such as sentry-chassis, names one physical programmable target.
/// Its BSP, such as dm-mc02, supplies the workspace and hardware configuration.
#[derive(Debug, Parser)]
#[command(
    name = "polyfw",
    bin_name = "polyfw",
    version,
    after_long_help = "Board selection:
  Pass BOARD for this invocation, or omit it to use the saved default.
  If no default is saved, choose a board interactively.
  Commands accepting --select can save BOARD, or choose and save if BOARD is omitted.
  Build/check/clean commands accept --all-boards; hardware commands operate on one board.
  Interactive selection requires a terminal. Pass BOARD in scripts.

Examples:
  polyfw info
  polyfw select sentry-chassis
  polyfw build --release
  polyfw probes
  polyfw run --release
  polyfw verify
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
    /// Project directory containing polyfw.toml.
    ///
    /// Relative paths start at the caller's working directory. Without this option, search
    /// that directory and its ancestors for the nearest polyfw.toml. Report an error if
    /// no project is found. Project-free commands ignore this option.
    #[arg(
        long,
        value_name = "DIR",
        global = true,
        overrides_with = "project_root",
        verbatim_doc_comment
    )]
    pub(crate) project_root: Option<PathBuf>,

    /// Control output colors.
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
    /// Board ID; use the saved default or prompt if omitted.
    ///
    /// Use an ID listed by polyfw info. An explicit or prompted board applies
    /// only to this invocation unless --select is passed. Without BOARD, use the
    /// saved default, or open the selector if no valid default is saved.
    /// Interactive selection requires a terminal; pass BOARD in scripts.
    #[arg(verbatim_doc_comment)]
    pub(crate) board: Option<String>,
    /// Save BOARD as the default, or choose and save a board.
    ///
    /// With BOARD, save it before running the command. Without BOARD, always open
    /// the selector, even if a default is already saved. The selector requires a
    /// terminal. The default is stored in .polyfw-state/selected.
    #[arg(short, long, verbatim_doc_comment)]
    pub(crate) select: bool,
    /// Run this command for every registered board.
    ///
    /// Include every board listed by polyfw info instead of using the saved
    /// default. Cannot be combined with BOARD or --select.
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
    /// Board ID; use the saved default or prompt if omitted.
    pub(crate) board: Option<String>,
    /// Save BOARD as the default, or choose and save a board.
    #[arg(short, long)]
    pub(crate) select: bool,
    /// Probe selector VID:PID[:SERIAL]; otherwise let probe-rs select the probe.
    #[arg(long, value_name = "ID")]
    pub(crate) probe: Option<String>,
    /// Debug interface speed in kHz; otherwise use probe-rs configuration or defaults.
    #[arg(long = "speed", value_name = "KHZ")]
    pub(crate) speed_khz: Option<NonZeroU32>,
    /// Connect while asserting the target's reset line.
    #[arg(long)]
    pub(crate) connect_under_reset: bool,
}

#[derive(Debug, Default, Args)]
pub(crate) struct ProfileArgs {
    /// Use Cargo's dev profile; shorthand for --profile dev.
    #[arg(long, conflicts_with_all = ["release", "profile"])]
    pub(crate) dev: bool,
    /// Use Cargo's release profile; shorthand for --profile release.
    #[arg(short = 'r', long, conflicts_with = "profile")]
    pub(crate) release: bool,
    /// Cargo profile; otherwise inherit the saved profile, except when saving a selection.
    ///
    /// An unspecified profile is not saved or passed to Cargo; Cargo defaults to dev.
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
    /// RTT buffer policy; no-block-skip drops whole messages when the buffer is full.
    #[arg(long, value_enum, default_value_t = RttChannelMode::NoBlockSkip)]
    pub(crate) rtt_channel_mode: RttChannelMode,
    /// Let the target continue through resets without debugger capture.
    #[arg(long)]
    pub(crate) no_catch_reset: bool,
    /// Disable debugger capture of HardFault exceptions.
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
    /// Print the project configuration's JSON Schema.
    ///
    /// Does not load the registry or inspect BSP workspaces.
    Schema {
        /// Write to this file instead of stdout, relative to the current directory.
        #[arg(short, long, value_name = "FILE")]
        output: Option<PathBuf>,
    },

    /// Show boards, existing firmware artifacts and the saved selection.
    ///
    /// List BSPs and boards with dev/release ELF presence, plus the saved profile
    /// when different. With --board [BOARD], show one board's configuration, ELF paths
    /// and its configured local dependencies across workspaces. Omit BOARD after the flag to open
    /// the interactive selector without saving the choice. ELF presence does not establish
    /// source freshness or a match with the running firmware.
    /// This query never saves a selection, builds or connects to hardware.
    #[command(
        visible_alias = "i",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw info
  polyfw info --board
  polyfw info --board sentry-chassis
  polyfw info --board sentry-chassis --json"
    )]
    Info {
        /// Show board details; omit BOARD to choose interactively without saving.
        #[arg(long, value_name = "BOARD")]
        board: Option<Option<String>>,
        /// Print the corresponding view as JSON, without terminal formatting.
        #[arg(long)]
        json: bool,
    },

    /// Save or clear the default board and firmware profile.
    ///
    /// Save BOARD and any explicitly specified profile for later commands.
    /// Omit BOARD to open the interactive
    /// selector, even when a default is already saved; this requires a terminal.
    /// Use --clear to remove the default stored in .polyfw-state/selected.
    #[command(
        visible_alias = "s",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw select sentry-chassis
  polyfw select
  polyfw select --clear"
    )]
    Select {
        /// Board ID to save; omit it to open the selector.
        board: Option<String>,
        /// Clear the saved default board.
        ///
        /// Later board commands will prompt unless given BOARD or --all-boards.
        /// Cannot be combined with BOARD or profile flags.
        #[arg(
            long,
            conflicts_with_all = ["board", "dev", "release", "profile"],
            verbatim_doc_comment
        )]
        clear: bool,
        #[command(flatten)]
        profile: ProfileArgs,
    },

    /// Remove firmware build artifacts.
    ///
    /// For one board, run cargo clean for its firmware package and keep the BSP
    /// and dependency artifacts. With --all-boards, remove the full shared
    /// firmware cache for every BSP, including its dependency artifacts.
    /// Package cleaning also affects its other binaries and registered boards;
    /// the notice lists every affected board in the BSP.
    /// Cleaning is limited to target/firmware/<bsp>.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw clean sentry-chassis
  polyfw clean --all-boards"
    )]
    Clean(BoardArgs),

    /// Build firmware for one board or every registered board.
    ///
    /// Inherit the saved profile, or leave it to Cargo when none is saved or with --all-boards.
    /// Override with --dev, -r/--release or --profile NAME. An explicit registry bin selects
    /// that binary; otherwise use Cargo's default targets and infer the package's only binary
    /// for ELF commands.
    /// Each board builds separately
    /// in its BSP workspace, including with --all-boards. Artifacts are stored
    /// under target/firmware/<bsp>.
    #[command(
        visible_alias = "b",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw build
  polyfw build --select sentry-chassis --release
  polyfw build --all-boards --release"
    )]
    Build {
        #[command(flatten)]
        boards: BoardArgs,
        #[command(flatten)]
        profile: ProfileArgs,
    },

    /// List connected debug probes without selecting a board.
    Probes,

    /// Flash, verify and start an existing ELF, then monitor RTT/defmt logs.
    ///
    /// Select the saved profile's ELF, or override with --dev, -r/--release or --profile. Build
    /// explicitly with polyfw build first. Probe and speed selection are
    /// delegated to probe-rs when omitted. Ctrl-C ends the probe-rs session;
    /// it does not mean the target firmware has stopped.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw run sentry-chassis
  polyfw run --release
  polyfw run sentry-chassis --probe VID:PID:SERIAL --speed 4000"
    )]
    Run {
        #[command(flatten)]
        target: ProbeArgs,
        #[command(flatten)]
        profile: ProfileArgs,
        #[command(flatten)]
        monitor: MonitorArgs,
    },

    /// Flash an existing ELF, verify the image, then reset the board and exit.
    ///
    /// Select the saved profile's ELF, or override with --dev, -r/--release or --profile.
    /// Build explicitly with polyfw build first.
    #[command(
        visible_aliases = ["f", "download", "d"],
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw flash sentry-chassis --release
  polyfw flash --select sentry-chassis"
    )]
    Flash {
        #[command(flatten)]
        target: ProbeArgs,
        #[command(flatten)]
        profile: ProfileArgs,
    },

    /// Monitor RTT/defmt using an existing ELF without building or flashing.
    ///
    /// The selected board and profile's ELF must match the running firmware.
    /// Override with --dev, -r/--release or --profile NAME. A debug connection can briefly
    /// halt the core, and --connect-under-reset asserts the reset line.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw attach sentry-chassis
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

    /// Reset one board without building or flashing.
    Reset(ProbeArgs),

    /// Format a board's firmware and its local dependencies.
    ///
    /// Run cargo fmt and modify source files in place. Include the selected
    /// firmware and its reachable path dependencies matched by local-dependencies in the registry.
    /// Format each package's full set of targets, including its other binaries.
    /// Format each package once from its owning workspace, also with --all-boards.
    /// Use verify to check formatting without rewriting files.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw fmt sentry-chassis
  polyfw fmt --all-boards"
    )]
    Fmt(BoardArgs),

    /// Run Clippy for a board's firmware and its local dependencies.
    ///
    /// Check all binaries and libraries of firmware and local runtime packages
    /// for the MCU, including other binaries in the firmware's package. Then
    /// check all selected libraries and their unit tests on the host. Deny all warnings.
    /// Include reachable path dependencies matched by local-dependencies in the registry.
    /// Group MCU checks per BSP and host checks per owning workspace, using its defaults.
    /// Include local test helpers recursively; deduplicate host work with --all-boards.
    #[command(
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw lint sentry-chassis
  polyfw lint --all-boards"
    )]
    Lint(BoardArgs),

    /// Run host unit tests and doctests for a board and its local dependencies.
    ///
    /// Test the firmware library and its configured local path dependencies
    /// on the development machine; no board hardware is required. Include local test helpers
    /// recursively and use each owning workspace's defaults and dev-dependencies.
    /// Use Cargo's default test targets and library doctests for each package,
    /// including its other binaries when enabled by Cargo's test configuration.
    /// Test each selected package once in its owning workspace, also with --all-boards.
    #[command(
        visible_alias = "t",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw test sentry-chassis
  polyfw test --all-boards"
    )]
    Test(BoardArgs),

    /// Check formatting, run Clippy and run host tests.
    ///
    /// Check formatting without rewriting files, run MCU and host Clippy with
    /// warnings denied, then run host unit tests and doctests. Stop at the first
    /// failed command. Include the selected firmware and the local path dependencies matched by
    /// local-dependencies across workspaces; with --all-boards, check all registered boards.
    ///
    /// Release builds, dependency policy checks and repository file hooks are
    /// separate steps. Cargo may access the network to resolve dependencies.
    #[command(
        visible_alias = "v",
        verbatim_doc_comment,
        after_long_help = "Examples:
  polyfw verify
  polyfw verify sentry-chassis
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
