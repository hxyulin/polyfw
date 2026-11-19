use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Serialize;

use crate::Result;
use crate::model::SavedSelection;
use crate::profile::Profile;
use crate::registry::{Context, unique_by_bsp};
use crate::runner::firmware_elf_path;
use crate::scope::{self, PackageScope};

#[derive(Serialize)]
pub(crate) struct Report {
    pub(crate) repository_root: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) saved_selection: Option<SavedSelection>,
    pub(crate) bsps: BTreeMap<String, Bsp>,
    pub(crate) boards: BTreeMap<String, Board>,
}

#[derive(Serialize)]
pub(crate) struct Bsp {
    pub(crate) registered_boards: usize,
    pub(crate) workspace: PathBuf,
    pub(crate) target: String,
    pub(crate) cpu: Option<String>,
    pub(crate) chip: String,
}

#[derive(Serialize)]
pub(crate) struct Board {
    pub(crate) description: String,
    pub(crate) bsp: String,
    pub(crate) package: String,
    pub(crate) bin: String,
    pub(crate) artifacts: Vec<Artifact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) local_dependencies: Option<Vec<PackageScope>>,
}

#[derive(Serialize)]
pub(crate) struct Artifact {
    pub(crate) profile: Profile,
    pub(crate) path: PathBuf,
    pub(crate) exists: bool,
}

impl Report {
    pub(crate) fn load(context: &Context, board: Option<&str>) -> Result<Self> {
        let saved_selection = context.read_saved_selection()?;
        let selections = match board {
            Some(id) => vec![context.selection(id)?],
            None => context
                .registry
                .boards
                .keys()
                .map(|id| context.selection(id))
                .collect::<Result<Vec<_>>>()?,
        };
        let mut profiles = vec!["dev".parse::<Profile>()?, "release".parse()?];
        if let Some(profile) = saved_selection
            .as_ref()
            .and_then(|saved| saved.profile.as_ref())
            .filter(|profile| !profiles.contains(profile))
        {
            profiles.push(profile.clone());
        }

        let bsps = unique_by_bsp(&selections)
            .into_iter()
            .map(|selection| {
                let bsp = &selection.bsp;
                (
                    selection.board.bsp.clone(),
                    Bsp {
                        registered_boards: context
                            .registry
                            .boards
                            .values()
                            .filter(|board| board.bsp == selection.board.bsp)
                            .count(),
                        workspace: context.root.join(&bsp.workspace),
                        target: bsp.target.clone(),
                        cpu: bsp.cpu.clone(),
                        chip: bsp.chip.clone(),
                    },
                )
            })
            .collect();
        let mut boards = BTreeMap::new();
        for mut selection in selections {
            let local_dependencies = if board.is_some() {
                let scope = scope::resolve(context, std::slice::from_ref(&selection))?
                    .into_iter()
                    .next()
                    .expect("one board has one BSP scope");
                Some(
                    scope
                        .packages
                        .into_iter()
                        .filter(|package| !scope.firmware.contains(&package.manifest_path))
                        .collect(),
                )
            } else {
                None
            };
            let artifacts = profiles
                .iter()
                .map(|profile| {
                    selection.profile = Some(profile.clone());
                    let path = firmware_elf_path(&context.root, &selection);
                    Artifact {
                        profile: profile.clone(),
                        exists: path.is_file(),
                        path,
                    }
                })
                .collect();
            boards.insert(
                selection.id,
                Board {
                    description: selection.board.description,
                    bsp: selection.board.bsp,
                    package: selection.board.package,
                    bin: selection.board.resolved_bin,
                    artifacts,
                    local_dependencies,
                },
            );
        }
        Ok(Self {
            repository_root: context.root.clone(),
            saved_selection,
            bsps,
            boards,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use super::*;

    #[test]
    fn an_overview_checks_elf_files_without_resolving_the_dependency_graph() {
        let context = fixture("overview");
        let artifact_dir = context
            .root
            .join("target/firmware/test/thumbv7em-none-eabihf");
        fs::create_dir_all(artifact_dir.join("debug")).unwrap();
        fs::write(artifact_dir.join("debug/robot-image"), "existing ELF").unwrap();
        fs::create_dir_all(artifact_dir.join("release/robot-image")).unwrap();
        fs::remove_file(context.root.join("bsp/test/host-helper/Cargo.toml")).unwrap();

        let report = Report::load(&context, None).unwrap();
        assert_eq!(report.boards.len(), 2);
        assert_eq!(report.bsps["test"].registered_boards, 2);
        assert!(report.saved_selection.is_none());
        assert_eq!(report.boards["one"].bin, "robot-image");
        for board in report.boards.values() {
            assert!(board.local_dependencies.is_none());
            assert_eq!(board.artifacts.len(), 2);
            assert!(
                board
                    .artifacts
                    .iter()
                    .all(|artifact| artifact.path.is_absolute())
            );
        }
        let artifacts = &report.boards["one"].artifacts;
        assert_eq!(artifacts[0].profile.name(), "dev");
        assert_eq!(artifacts[0].path, artifact_dir.join("debug/robot-image"));
        assert!(artifacts[0].exists);
        assert_eq!(artifacts[1].profile.name(), "release");
        assert!(!artifacts[1].exists);
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["boards"]["one"]["artifacts"][1]["exists"], false);
        assert!(Report::load(&context, Some("one")).is_err());
        assert!(!context.selection_path().exists());
        assert!(!context.root.join(crate::vscode::LAUNCH_FILE).exists());
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn board_details_list_local_dependencies_without_changing_selection() {
        let context = fixture("detail with spaces");
        context
            .save_selection("two", Some(&"bench-debug".parse().unwrap()))
            .unwrap();
        let state = fs::read(context.selection_path()).unwrap();
        let launch_path = context.root.join(crate::vscode::LAUNCH_FILE);
        let launch = fs::read(&launch_path).unwrap();

        let report = Report::load(&context, Some("one")).unwrap();
        assert_eq!(report.boards.len(), 1);
        assert_eq!(report.bsps.len(), 1);
        let board = &report.boards["one"];
        let packages = board
            .local_dependencies
            .as_ref()
            .unwrap()
            .iter()
            .map(|package| (package.name.as_str(), package.embedded))
            .collect::<Vec<_>>();
        assert_eq!(
            packages,
            [
                ("common", true),
                ("common-leaf", true),
                ("host-helper", false)
            ]
        );
        assert_eq!(board.artifacts.len(), 3);
        assert_eq!(board.artifacts[2].profile.name(), "bench-debug");
        assert_eq!(
            board.artifacts[2].path,
            context
                .root
                .join("target/firmware/test/thumbv7em-none-eabihf/bench-debug/robot-image")
        );
        let saved = report.saved_selection.as_ref().unwrap();
        assert_eq!(saved.board, "two");
        assert_eq!(saved.profile.as_ref().unwrap().name(), "bench-debug");
        assert_eq!(fs::read(context.selection_path()).unwrap(), state);
        assert_eq!(fs::read(launch_path).unwrap(), launch);
        assert!(!context.root.join("target").exists());

        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["boards"]["one"]["bin"], "robot-image");
        assert_eq!(json["bsps"]["test"]["target"], "thumbv7em-none-eabihf");
        let manifest_path = |directory: &str| {
            PathBuf::from("bsp")
                .join("test")
                .join(directory)
                .join("Cargo.toml")
        };
        assert_eq!(
            json["boards"]["one"]["local_dependencies"],
            serde_json::json!([
                {
                    "package": "common",
                    "manifest_path": manifest_path("common"),
                    "target": "embedded"
                },
                {
                    "package": "common-leaf",
                    "manifest_path": manifest_path("leaf"),
                    "target": "embedded"
                },
                {
                    "package": "host-helper",
                    "manifest_path": manifest_path("host-helper"),
                    "target": "host"
                },
            ])
        );
        assert!(json["boards"].get("two").is_none());
        assert!(
            json["boards"]["one"]
                .get("workspace_dependencies")
                .is_none()
        );
        let report = Report::load(&context, Some("two")).unwrap();
        assert!(
            report.boards["two"]
                .local_dependencies
                .as_ref()
                .unwrap()
                .is_empty()
        );
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(
            json["boards"]["two"]["local_dependencies"],
            serde_json::json!([])
        );
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn json_omits_absent_fields_and_preserves_explicit_profiles() {
        let context = fixture("saved profiles");
        let json = serde_json::to_value(Report::load(&context, None).unwrap()).unwrap();
        assert!(json.get("saved_selection").is_none());
        for profile in [None, Some("dev"), Some("release"), Some("bench-debug")] {
            context
                .save_selection("one", profile.map(|name| name.parse().unwrap()).as_ref())
                .unwrap();
            let report = Report::load(&context, None).unwrap();
            let expected_count = if profile == Some("bench-debug") { 3 } else { 2 };
            for board in report.boards.values() {
                assert_eq!(board.artifacts.len(), expected_count);
            }
            let json = serde_json::to_value(report).unwrap();
            assert_eq!(json["saved_selection"]["board"], "one");
            let expected_profile = profile.map(serde_json::Value::from);
            assert_eq!(
                json["saved_selection"].get("profile"),
                expected_profile.as_ref()
            );
            assert!(json["boards"]["one"].get("local_dependencies").is_none());
        }
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn info_lists_current_boards_without_discarding_a_stale_saved_selection() {
        let context = fixture("stale selection");
        let saved = "board = \"removed-board\"\nprofile = \"release\"\n";
        fs::create_dir_all(context.selection_path().parent().unwrap()).unwrap();
        fs::write(context.selection_path(), saved).unwrap();
        for board in [None, Some("one")] {
            let report = Report::load(&context, board).unwrap();
            assert!(report.boards.contains_key("one"));
            assert!(!report.boards.contains_key("removed-board"));
            let json = serde_json::to_value(report).unwrap();
            assert_eq!(json["saved_selection"]["board"], "removed-board");
            assert_eq!(json["saved_selection"]["profile"], "release");
            assert_eq!(fs::read_to_string(context.selection_path()).unwrap(), saved);
        }
        let error = context.read_selection().unwrap_err().to_string();
        assert!(error.contains("removed-board"), "{error}");
        assert!(error.contains("polyfw select --clear"), "{error}");
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn an_info_query_reports_invalid_saved_state_and_unknown_boards() {
        let context = fixture("errors");
        assert!(
            Report::load(&context, Some("unknown"))
                .err()
                .unwrap()
                .to_string()
                .contains("unknown board: unknown")
        );
        fs::create_dir_all(context.selection_path().parent().unwrap()).unwrap();
        fs::write(context.selection_path(), "invalid = [").unwrap();
        let error = Report::load(&context, Some("one")).err().unwrap();
        assert!(error.to_string().contains("invalid saved selection"));
        assert_eq!(
            fs::read_to_string(context.selection_path()).unwrap(),
            "invalid = ["
        );
        fs::remove_dir_all(context.root).unwrap();
    }

    fn fixture(label: &str) -> Context {
        let root = std::env::temp_dir().join(format!("polyfw-info-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let workspace = root.join("bsp/test");
        fs::create_dir_all(workspace.join(".cargo")).unwrap();
        fs::write(
            workspace.join(".cargo/config.toml"),
            "[build]\ntarget = \"thumbv7em-none-eabihf\"\n\
             [target.thumbv7em-none-eabihf]\nrustflags = \"-Ctarget-cpu=cortex-m4\"\n",
        )
        .unwrap();
        for (directory, name, extra) in [
            (
                "board",
                "board-fw",
                "[[bin]]\nname = \"robot-image\"\npath = \"src/main.rs\"\n\
                 [dependencies]\ncommon = { path = \"../common\" }\n\
                 [build-dependencies]\nhost-helper = { path = \"../host-helper\" }\n",
            ),
            ("other", "other-fw", ""),
            (
                "common",
                "common",
                "[dependencies]\ncommon-leaf = { path = \"../leaf\" }\n",
            ),
            ("leaf", "common-leaf", ""),
            ("host-helper", "host-helper", ""),
            ("unused", "unused", ""),
        ] {
            let package = workspace.join(directory);
            fs::create_dir_all(package.join("src")).unwrap();
            fs::write(
                package.join("Cargo.toml"),
                format!(
                    "[package]\nname = \"{name}\"\nversion = \"0.1.0\"\n\
                     edition = \"2024\"\n{extra}"
                ),
            )
            .unwrap();
            fs::write(package.join("src/lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
            if matches!(directory, "board" | "other") {
                fs::write(package.join("src/main.rs"), "fn main() {}\n").unwrap();
            }
        }
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nresolver = \"3\"\n\
             members = [\"board\", \"other\", \"common\", \"leaf\", \"host-helper\", \"unused\"]\n",
        )
        .unwrap();
        fs::write(
            root.join("polyfw.toml"),
            "[bsps.test]\nworkspace = \"bsp/test\"\n\
             chip = \"test-chip\"\n\
             [boards.one]\ndescription = \"first board\"\nbsp = \"test\"\n\
             package = \"board-fw\"\n\
             [boards.two]\ndescription = \"other board\"\nbsp = \"test\"\n\
             package = \"other-fw\"\n",
        )
        .unwrap();
        let output = Command::new("cargo")
            .current_dir(&workspace)
            .args(["generate-lockfile", "--offline"])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        Context::load(root).unwrap()
    }
}
