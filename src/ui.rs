use std::fmt::Display;
use std::io::{self, IsTerminal};
use std::time::{Duration, Instant};

use clap::ColorChoice;
use comfy_table::{CellAlignment, ContentArrangement, Table, presets};
use dialoguer::FuzzySelect;
use dialoguer::console::{self, Term, style, truncate_str};
use dialoguer::theme::ColorfulTheme;

use crate::Result;
use crate::info::{Board, Report};
use crate::model::Registry;
use crate::profile::Profile;

pub(crate) struct Ui {
    unicode: bool,
    width: usize,
}

pub enum Target {
    Host,
    Embedded,
    None,
}

pub(crate) fn color_enabled(color: ColorChoice) -> bool {
    match color {
        ColorChoice::Always => true,
        ColorChoice::Never => false,
        ColorChoice::Auto => {
            io::stdout().is_terminal()
                && io::stderr().is_terminal()
                && std::env::var_os("NO_COLOR").is_none()
        }
    }
}

impl Ui {
    pub(crate) fn new(color: ColorChoice) -> Self {
        let stdout_is_terminal = io::stdout().is_terminal();
        let stderr_is_terminal = io::stderr().is_terminal();
        let enabled = color_enabled(color);
        console::set_colors_enabled(enabled);
        console::set_colors_enabled_stderr(enabled);

        Self {
            unicode: stdout_is_terminal && stderr_is_terminal,
            width: usize::from(Term::stdout().size().1).max(20),
        }
    }

    pub(crate) fn error(&self, error: &dyn Display) {
        eprintln!("{} {error}", style("error:").red().bold());
    }

    pub(crate) fn selected(&self, board: &str, profile: Option<&Profile>) {
        let profile = profile
            .map(|profile| format!(" ({})", profile.name()))
            .unwrap_or_default();
        println!(
            "{} {}{profile}",
            style("selected").green().bold(),
            style(board).bold()
        );
    }

    pub(crate) fn removal(&self, path: &str, result: &io::Result<()>) {
        match result {
            Ok(()) => println!("{} {path}", style("removed").green().bold()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                println!("{} {path}", style("not found").yellow());
            }
            Err(error) => eprintln!("{} {path}: {error}", style("failed").red().bold()),
        }
    }

    pub(crate) fn cleaning_bsp(&self, bsp: &str, boards: &[&str]) {
        println!(
            "{} {} (boards: {})",
            style("cleaning BSP").cyan().bold(),
            style(bsp).yellow().bold(),
            boards.join(", ")
        );
    }

    pub(crate) fn cleaning_package(&self, package: &str, bsp: &str, boards: &[&str]) {
        println!(
            "{} {} (BSP: {}; boards: {})",
            style("cleaning package").cyan().bold(),
            style(package).yellow().bold(),
            style(bsp).dim(),
            boards.join(", ")
        );
    }

    pub(crate) fn label(&self, action: &str, packages: &[&str], target: Target) -> String {
        let target = match target {
            Target::Host => "(host)",
            Target::Embedded => "(emb)",
            Target::None => "",
        };
        format!("{} {}: {}", action, target, packages.join(", "))
    }

    pub(crate) fn print_info(&self, report: &Report, show_saved_selection: bool) {
        if let Some(saved) = report
            .saved_selection
            .as_ref()
            .filter(|_| show_saved_selection)
        {
            let profile = saved
                .profile
                .as_ref()
                .map(|profile| format!(" ({})", profile.name()))
                .unwrap_or_default();
            println!("saved selection: {}{profile}\n", saved.board);
        }
        if let Some((id, board)) = report
            .boards
            .iter()
            .find(|(_, board)| board.local_dependencies.is_some())
        {
            self.print_board_info(report, id, board);
            return;
        }
        let bsp_rows = report
            .bsps
            .iter()
            .map(|(name, bsp)| {
                vec![
                    name.clone(),
                    bsp.registered_boards.to_string(),
                    bsp.workspace
                        .strip_prefix(&report.repository_root)
                        .unwrap_or(&bsp.workspace)
                        .display()
                        .to_string(),
                    bsp.target.clone(),
                    bsp.cpu.clone().unwrap_or_else(|| "compiler default".into()),
                    bsp.chip.clone(),
                ]
            })
            .collect::<Vec<_>>();
        self.section("bsps", Some(bsp_rows.len()), false);
        self.print_table(
            &["name", "boards", "workspace", "target", "cpu", "chip"],
            &bsp_rows,
        );

        let mut headers = vec!["status".to_owned(), "name".to_owned(), "bsp".to_owned()];
        if let Some(board) = report.boards.values().next() {
            headers.extend(
                board
                    .artifacts
                    .iter()
                    .map(|artifact| format!("{} ELF", artifact.profile.name())),
            );
        }
        headers.push("description".to_owned());
        let board_rows = report
            .boards
            .iter()
            .map(|(name, board)| {
                let saved = report
                    .saved_selection
                    .as_ref()
                    .filter(|saved| saved.board == *name);
                let mut row = if let Some(saved) = saved {
                    let profile = saved
                        .profile
                        .as_ref()
                        .map(|profile| format!(" ({})", profile.name()))
                        .unwrap_or_default();
                    vec![
                        style(format!("[saved]{profile}"))
                            .green()
                            .bold()
                            .to_string(),
                        style(name).yellow().to_string(),
                        style(&board.bsp).yellow().to_string(),
                    ]
                } else {
                    vec![String::new(), name.clone(), board.bsp.clone()]
                };
                row.extend(
                    board
                        .artifacts
                        .iter()
                        .map(|artifact| artifact_status(artifact.exists)),
                );
                row.push(if saved.is_some() {
                    style(&board.description).yellow().to_string()
                } else {
                    board.description.clone()
                });
                row
            })
            .collect::<Vec<_>>();
        self.section("boards", Some(board_rows.len()), true);
        self.print_table(
            &headers.iter().map(String::as_str).collect::<Vec<_>>(),
            &board_rows,
        );
    }

    fn print_board_info(&self, report: &Report, id: &str, board: &Board) {
        let bsp = &report.bsps[&board.bsp];
        self.section("board", None, false);
        let fields = [
            ("name", id.to_owned()),
            ("description", board.description.clone()),
            ("bsp", board.bsp.clone()),
            ("workspace", bsp.workspace.display().to_string()),
            ("target", bsp.target.clone()),
            (
                "cpu",
                bsp.cpu.clone().unwrap_or_else(|| "compiler default".into()),
            ),
            ("chip", bsp.chip.clone()),
            ("package", board.package.clone()),
            ("bin", board.bin.clone()),
        ];
        let rows = fields
            .into_iter()
            .map(|(key, value)| vec![key.to_owned(), value])
            .collect::<Vec<_>>();
        self.print_table(&["field", "value"], &rows);

        self.section("artifacts", Some(board.artifacts.len()), true);
        let rows = board
            .artifacts
            .iter()
            .map(|artifact| {
                vec![
                    artifact.profile.name().to_owned(),
                    artifact_status(artifact.exists),
                    artifact.path.display().to_string(),
                ]
            })
            .collect::<Vec<_>>();
        self.print_table(&["profile", "ELF", "path"], &rows);

        if let Some(packages) = &board.local_dependencies {
            self.section("local dependencies", Some(packages.len()), true);
            let rows = packages
                .iter()
                .map(|package| {
                    vec![
                        package.name.clone(),
                        if package.embedded { "embedded" } else { "host" }.to_owned(),
                        package.manifest_path.display().to_string(),
                    ]
                })
                .collect::<Vec<_>>();
            self.print_table(&["package", "target", "manifest"], &rows);
        }
    }

    pub(crate) fn prompt_board(
        &self,
        registry: &Registry,
        saved: Option<&str>,
        no_terminal_hint: &str,
    ) -> Result<String> {
        if !Term::stderr().is_term() {
            return Err(format!("no board selected; {no_terminal_hint}").into());
        }
        let board_width = registry
            .boards
            .keys()
            .map(|name| name.chars().count())
            .max()
            .unwrap_or(5)
            .min(24);
        let bsp_width = registry
            .boards
            .values()
            .map(|board| board.bsp.chars().count())
            .max()
            .unwrap_or(3)
            .min(12);
        let description_width = self
            .width
            .saturating_sub(board_width + bsp_width + 8) // 8 is the width of the marker
            .min(30);
        let names: Vec<String> = registry
            .boards
            .iter()
            .map(|(id, board)| {
                let marker = if saved == Some(id.as_str()) {
                    " [saved]"
                } else {
                    ""
                };
                format!(
                    "{:<board_width$}  {:<bsp_width$}  {:<description_width$}{}",
                    id,
                    board.bsp,
                    truncate_str(
                        &board.description,
                        description_width,
                        if self.unicode { "…" } else { "..." },
                    ),
                    marker
                )
            })
            .collect();
        let initial = saved
            .and_then(|id| registry.boards.keys().position(|candidate| candidate == id))
            .unwrap_or(0);
        let theme = ColorfulTheme {
            prompt_prefix: style("?".to_owned()).cyan().bold(),
            success_prefix: style("ok".to_owned()).green(),
            error_prefix: style("!".to_owned()).red(),
            active_item_prefix: style(">".to_owned()).cyan().bold(),
            ..ColorfulTheme::default()
        };
        eprintln!(
            "{}",
            style("type to filter; arrows to move; Enter to choose; Esc to cancel").dim()
        );
        let Some(index) = FuzzySelect::with_theme(&theme)
            .with_prompt("board")
            .items(&names)
            .default(initial)
            .max_length(14)
            .highlight_matches(true)
            .report(false)
            .interact_opt()?
        else {
            return Err("selection cancelled".into());
        };
        registry
            .boards
            .keys()
            .nth(index)
            .cloned()
            .ok_or_else(|| "selection index out of range".into())
    }

    fn section(&self, title: &str, count: Option<usize>, blank_before: bool) {
        if blank_before {
            println!();
        }
        let title = style(title).cyan().bold();
        match count {
            Some(count) => println!("{title} {}", style(format!("({count})")).dim()),
            None => println!("{title}"),
        }
    }

    pub(crate) fn command(&self, command: &str) {
        eprintln!("  {} {}", style("$").yellow().bold(), style(command).cyan());
    }

    fn print_table(&self, headers: &[&str], rows: &[Vec<String>]) {
        for line in render_table(headers, rows, self.width).lines() {
            println!("  {line}");
        }
    }
}

pub(crate) struct Progress<'a> {
    ui: &'a Ui,
    total: usize,
    current: usize,
}

pub(crate) struct Step {
    index: usize,
    total: usize,
    started: Instant,
}

impl<'a> Progress<'a> {
    pub(crate) fn new(ui: &'a Ui, total: usize) -> Self {
        Self {
            ui,
            total,
            current: 0,
        }
    }

    pub(crate) fn start(&mut self, label: &str) -> Step {
        self.current += 1;
        let step = Step {
            index: self.current,
            total: self.total,
            started: Instant::now(),
        };
        let arrow = if self.ui.unicode { "▶" } else { ">" };
        eprintln!(
            "{} {} {}",
            style(format!("[{}/{}]", step.index, step.total))
                .cyan()
                .bold(),
            style(arrow).cyan().bold(),
            style(label).bold()
        );
        step
    }

    pub(crate) fn finish(&self, step: Step, success: bool) {
        let (marker, word) = if success {
            (self.success_marker(), "done")
        } else {
            (self.failure_marker(), "failed")
        };
        eprintln!(
            "  {} {} ({})",
            marker,
            style(word).bold(),
            style(format_duration(step.started.elapsed())).dim()
        );
    }

    fn success_marker(&self) -> String {
        if self.ui.unicode {
            style("✓").green().bold().to_string()
        } else {
            style("[ok]").green().bold().to_string()
        }
    }

    fn failure_marker(&self) -> String {
        if self.ui.unicode {
            style("✗").red().bold().to_string()
        } else {
            style("[fail]").red().bold().to_string()
        }
    }
}

fn artifact_status(exists: bool) -> String {
    if exists {
        style("present").green().to_string()
    } else {
        style("unbuilt").dim().to_string()
    }
}

fn render_table(headers: &[&str], rows: &[Vec<String>], width: usize) -> String {
    let mut table = Table::new();
    table
        .load_style(presets::NOTHING)
        .set_content_arrangement(ContentArrangement::Dynamic)
        .set_width(width.saturating_sub(2).min(u16::MAX as usize) as u16)
        .set_header(
            headers
                .iter()
                .map(|header| style(*header).dim().bold().to_string()),
        )
        .add_rows(rows.iter().cloned());
    for (header, column) in headers.iter().zip(table.column_iter_mut()) {
        column.set_padding((0, 2));
        if header.ends_with(" ELF") {
            column.set_cell_alignment(CellAlignment::Center);
        }
    }
    if let Some(column) = table.column_iter_mut().last() {
        column.set_padding((0, 0));
    }
    table.trim_fmt()
}

fn format_duration(duration: Duration) -> String {
    let milliseconds = duration.as_millis();
    if milliseconds < 1_000 {
        format!("{milliseconds}ms")
    } else if milliseconds < 60_000 {
        format!("{:.1}s", milliseconds as f64 / 1_000.0)
    } else {
        format!(
            "{}m {}s",
            milliseconds / 60_000,
            (milliseconds % 60_000) / 1_000
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_fit_narrow_terminals_without_losing_cell_contents() {
        let headers = ["name", "boards", "workspace", "target", "cpu", "chip"];
        let rows = vec![vec![
            "dm-mc02".into(),
            "12".into(),
            "bsp/dm-mc02".into(),
            "thumbv7em-none-eabihf".into(),
            "cortex-m7".into(),
            "STM32H723VGTx".into(),
        ]];
        for width in [20, 48, 80] {
            let rendered = render_table(&headers, &rows, width);
            assert!(
                rendered
                    .lines()
                    .all(|line| console::measure_text_width(line) + 2 <= width),
                "table exceeded {width} columns: {rendered}"
            );
            for cell in &rows[0] {
                let rendered = render_table(&["value"], &[vec![cell.clone()]], width);
                let compact: String = console::strip_ansi_codes(&rendered)
                    .chars()
                    .filter(|c| !c.is_whitespace())
                    .collect();
                assert!(compact.contains(cell), "lost {cell}: {rendered}");
            }
        }
    }

    #[test]
    fn styled_unicode_cells_wrap_using_their_terminal_display_width() {
        let styled = style("界界界界界界界界界界界界")
            .green()
            .force_styling(true)
            .to_string();
        let rendered = render_table(&["x"], &[vec![styled]], 20);
        assert!(
            rendered
                .lines()
                .all(|line| console::measure_text_width(line) + 2 <= 20)
        );
        assert_eq!(
            console::strip_ansi_codes(&rendered).matches('界').count(),
            12
        );
        assert!(rendered.contains("\x1b["));
    }

    #[test]
    fn durations_use_milliseconds_below_one_second() {
        assert_eq!(format_duration(Duration::from_millis(240)), "240ms");
        assert_eq!(format_duration(Duration::from_millis(1_240)), "1.2s");
    }
}
