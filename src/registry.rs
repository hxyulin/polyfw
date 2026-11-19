use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::mem::DropGuard;
use std::path::{Component, Path, PathBuf};

use crate::Result;
use crate::cargo_config;
use crate::local_dependencies::LocalDependencies;
use crate::model::{Metadata, Registry, SavedSelection, Selection};
use crate::profile::Profile;
use crate::runner;
use crate::vscode;

const REGISTRY_FILE: &str = "polyfw.toml";
const SELECTION_FILE: &str = ".polyfw-state/selected";

pub(crate) fn project_root(explicit: Option<&Path>, working_directory: &Path) -> Result<PathBuf> {
    let path = if let Some(explicit) = explicit {
        working_directory.join(explicit)
    } else {
        let mut path = None;
        for directory in working_directory.ancestors() {
            let registry = directory.join(REGISTRY_FILE);
            match fs::symlink_metadata(&registry) {
                Ok(_) => {
                    path = Some(directory.to_owned());
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!("could not inspect {}: {error}", registry.display()).into());
                }
            }
        }
        path.ok_or_else(|| {
            format!(
                "no {REGISTRY_FILE} found in {} or its ancestors; use --project-root <DIR>",
                working_directory.display()
            )
        })?
    };
    let root = path.canonicalize().map_err(|error| {
        format!(
            "invalid project root {}: {error}; use --project-root <DIR>",
            path.display()
        )
    })?;
    if !root.is_dir() {
        return Err(format!("project root is not a directory: {}", root.display()).into());
    }
    Ok(root)
}

pub(crate) fn schema_source() -> Result<String> {
    let schema = schemars::generate::SchemaSettings::draft07()
        .into_generator()
        .into_root_schema_for::<Registry>();
    Ok(format!("{}\n", serde_json::to_string_pretty(&schema)?))
}

pub(crate) fn write_schema(output: Option<&Path>) -> Result<()> {
    let source = schema_source()?;
    match output {
        Some(path) => write_file(path, source.as_bytes()),
        None => Ok(std::io::stdout().lock().write_all(source.as_bytes())?),
    }
}

pub(crate) struct Context {
    pub(crate) root: PathBuf,
    pub(crate) registry: Registry,
}

impl Context {
    pub(crate) fn load(root: PathBuf) -> Result<Self> {
        let mut context = Self::load_config(root)?;
        context.resolve_cargo()?;
        Ok(context)
    }

    pub(crate) fn load_config(root: PathBuf) -> Result<Self> {
        let path = root.join(REGISTRY_FILE);
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        let registry: Registry = toml::from_str(&source)
            .map_err(|error| format!("invalid registry {}: {error}", path.display()))?;
        registry.validate_structure(&root)?;
        Ok(Self { root, registry })
    }

    pub(crate) fn resolve_cargo(&mut self) -> Result<()> {
        self.registry.resolve_cargo(&self.root)
    }

    pub(crate) fn selection(&self, id: &str) -> Result<Selection> {
        let board = self
            .registry
            .boards
            .get(id)
            .ok_or_else(|| format!("unknown board: {id}; run polyfw info for valid names"))?;
        let bsp = self
            .registry
            .bsps
            .get(&board.bsp)
            .ok_or_else(|| format!("board {id} refers to unknown BSP {}", board.bsp))?;
        Ok(Selection {
            id: id.to_owned(),
            board: board.clone(),
            bsp: bsp.clone(),
            profile: None,
        })
    }

    pub(crate) fn read_selection(&self) -> Result<Option<SavedSelection>> {
        let saved = self.read_saved_selection()?;
        if let Some(saved) = &saved {
            self.selection(&saved.board).map_err(|_| {
                format!(
                    "saved board {} at {} is no longer registered;\n    \
                     run `polyfw select [BOARD]` to replace it \
                     or `polyfw select --clear` to reset selection",
                    saved.board,
                    self.selection_path().display()
                )
            })?;
        }
        Ok(saved)
    }

    pub(crate) fn read_saved_selection(&self) -> Result<Option<SavedSelection>> {
        let path = self.selection_path();
        let value = match fs::read_to_string(&path) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(format!("{}: {error}", path.display()).into()),
        };
        let saved = if self.registry.boards.contains_key(value.trim()) {
            SavedSelection {
                board: value.trim().to_owned(),
                profile: None,
            }
        } else {
            toml::from_str::<SavedSelection>(&value).map_err(|error| {
                format!(
                    "invalid saved selection at {}: {error}; run polyfw select --clear",
                    path.display()
                )
            })?
        };
        Ok(Some(saved))
    }

    pub(crate) fn selection_path(&self) -> PathBuf {
        self.root.join(SELECTION_FILE)
    }

    pub(crate) fn save_selection(&self, id: &str, profile: Option<&Profile>) -> Result<()> {
        let mut selection = self.selection(id)?;
        selection.profile = profile.cloned();
        let value = toml::to_string(&SavedSelection {
            board: id.to_owned(),
            profile: profile.cloned(),
        })?;
        let path = self.selection_path();
        if !self.registry.config.vscode.enabled {
            return write_file(&path, value.as_bytes());
        }
        let launch = vscode::configuration(&selection)?;
        let launch_path = self.root.join(vscode::LAUNCH_FILE);
        let previous_launch = match fs::read(&launch_path) {
            Ok(value) => Some(value),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(format!("{}: {error}", launch_path.display()).into()),
        };
        write_file(&launch_path, launch.as_bytes())?;
        if let Err(error) = write_file(&path, value.as_bytes()) {
            let rollback = match previous_launch {
                Some(previous) => write_file(&launch_path, &previous),
                None => fs::remove_file(&launch_path).map_err(Into::into),
            };
            rollback.map_err(|rollback| {
                format!(
                    "{error}; restoring {} also failed: {rollback}",
                    launch_path.display()
                )
            })?;
            return Err(error);
        }
        Ok(())
    }

    pub(crate) fn clear_selection(&self) -> Vec<(&'static str, std::io::Result<()>)> {
        let mut files = Vec::with_capacity(2);
        if self.registry.config.vscode.enabled {
            files.push(vscode::LAUNCH_FILE);
        }
        files.push(SELECTION_FILE);
        files
            .into_iter()
            .map(|file| (file, fs::remove_file(self.root.join(file))))
            .collect()
    }
}

fn write_file(path: &Path, value: &[u8]) -> Result<()> {
    fs::create_dir_all(path.parent().ok_or("generated file path has no parent")?)?;
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    let cleanup = DropGuard::new(&temporary, |path| {
        let _ = fs::remove_file(path);
    });
    let result = fs::write(&temporary, value).and_then(|()| fs::rename(&temporary, path));
    if let Err(error) = result {
        return Err(format!("could not write {}: {error}", path.display()).into());
    }
    DropGuard::dismiss(cleanup);
    Ok(())
}

impl Registry {
    fn validate_structure(&self, root: &Path) -> Result<()> {
        LocalDependencies::new(root, &self.config.local_dependencies)?;
        if self.bsps.is_empty() {
            return Err("registry has no BSPs".into());
        }
        if self.boards.is_empty() {
            return Err("registry has no boards".into());
        }
        for name in self.bsps.keys().chain(self.boards.keys()) {
            if !valid_name(name) {
                return Err(format!("invalid registry name: {name}").into());
            }
        }
        for bsp in self.bsps.values() {
            relative_path(root, &bsp.workspace, "BSP workspace")?;
        }
        for (name, board) in &self.boards {
            if !self.bsps.contains_key(&board.bsp) {
                return Err(format!("board {name} refers to unknown BSP {}", board.bsp).into());
            }
        }
        Ok(())
    }

    fn resolve_cargo(&mut self, root: &Path) -> Result<()> {
        let mut metadata = BTreeMap::new();
        for (name, bsp) in &self.bsps {
            let workspace = relative_path(root, &bsp.workspace, "BSP workspace")?;
            let manifest = workspace.join("Cargo.toml");
            if !manifest.is_file() {
                return Err(format!(
                    "bsp {name} has no workspace manifest: {}",
                    manifest.display()
                )
                .into());
            }
            metadata.insert(name.clone(), cargo_metadata(&workspace, false)?);
        }

        let mut binaries = BTreeMap::new();
        for (name, board) in &mut self.boards {
            let bsp = self
                .bsps
                .get(&board.bsp)
                .ok_or_else(|| format!("board {name} refers to unknown BSP {}", board.bsp))?;
            let bsp_metadata = metadata
                .get(&board.bsp)
                .expect("metadata was loaded for every BSP");
            let package = bsp_metadata
                .workspace_package(&board.package)
                .ok_or_else(|| {
                    format!(
                        "board {name} package is not in BSP {}: {}",
                        board.bsp, board.package
                    )
                })?;
            let workspace = canonical(&root.join(&bsp.workspace))?;
            let metadata_workspace = canonical(Path::new(&bsp_metadata.workspace_root))?;
            if metadata_workspace != workspace {
                return Err(format!(
                    "board {name} package is outside BSP workspace {}",
                    bsp.workspace
                )
                .into());
            }
            if !package
                .targets
                .iter()
                .any(|target| target.kind.iter().any(|kind| kind == "lib"))
            {
                return Err(format!(
                    "board {name} package {} has no library target",
                    board.package
                )
                .into());
            }
            let targets = package
                .targets
                .iter()
                .filter(|target| target.kind.iter().any(|kind| kind == "bin"))
                .map(|target| target.name.as_str())
                .collect::<Vec<_>>();
            let bin = match board.bin.as_deref() {
                Some(bin) if targets.contains(&bin) => bin,
                Some(bin) => {
                    return Err(format!(
                        "board {name} package {} has no binary target named {bin}",
                        board.package
                    )
                    .into());
                }
                None => match targets.as_slice() {
                    [bin] => *bin,
                    [] => {
                        return Err(format!(
                            "board {name} package {} has no binary targets",
                            board.package
                        )
                        .into());
                    }
                    bins => {
                        return Err(format!(
                            "board {name} package {} has multiple binary targets: {}; \
                             set bin in {REGISTRY_FILE}",
                            board.package,
                            bins.join(", ")
                        )
                        .into());
                    }
                },
            };
            if let Some((previous_board, previous_package)) = binaries.insert(
                (board.bsp.as_str(), bin),
                (name.as_str(), board.package.as_str()),
            ) && previous_package != board.package.as_str()
            {
                return Err(format!(
                    "boards {previous_board} ({previous_package}) and {name} ({}) use binary {bin} \
                     in BSP {}; their ELF paths would collide",
                    board.package, board.bsp
                )
                .into());
            }
            board.resolved_bin = bin.to_owned();
        }
        for (name, bsp) in &mut self.bsps {
            let workspace = root.join(&bsp.workspace);
            let config = cargo_config::resolve(&workspace)
                .map_err(|error| format!("bsp {name}: {error}"))?;
            bsp.target = config.target;
            bsp.cpu = config.cpu;
        }
        Ok(())
    }
}

pub(crate) fn cargo_metadata(workspace: &Path, dependencies: bool) -> Result<Metadata> {
    let mut command = runner::workspace_cargo(workspace);
    command
        .arg("metadata")
        .arg("--locked")
        .arg("--format-version")
        .arg("1");
    if !dependencies {
        command.arg("--no-deps");
    }
    let output = command.output()?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata failed for {}: {}",
            workspace.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    Ok(serde_json::from_slice::<Metadata>(&output.stdout)?)
}

fn relative_path(root: &Path, value: &str, label: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(format!("{label} must be a relative path inside the project: {value}").into());
    }
    Ok(root.join(path))
}

fn canonical(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()).into())
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.as_bytes()[0].is_ascii_lowercase()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !name.ends_with('-')
        && !name.contains("--")
}

pub(crate) fn unique_by_bsp(selections: &[Selection]) -> Vec<&Selection> {
    let mut seen = BTreeSet::new();
    selections
        .iter()
        .filter(|selection| seen.insert(selection.board.bsp.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Board, Bsp};

    #[test]
    fn the_nearest_configuration_defines_the_project_root() {
        let root = std::env::temp_dir().join(format!("polyfw-discovery-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let nested = root.join("nested");
        let working = nested.join("source/deep");
        fs::create_dir_all(&working).unwrap();
        for directory in [&root, &nested] {
            fs::write(directory.join(REGISTRY_FILE), "invalid registry").unwrap();
        }
        let discovered = project_root(None, &working).unwrap();
        assert_eq!(discovered, nested.canonicalize().unwrap());
        let error = Context::load(discovered).err().unwrap().to_string();
        assert!(
            error.contains(&nested.join(REGISTRY_FILE).display().to_string()),
            "{error}"
        );
        fs::remove_file(nested.join(REGISTRY_FILE)).unwrap();
        fs::create_dir(nested.join(REGISTRY_FILE)).unwrap();
        let discovered = project_root(None, &working).unwrap();
        assert_eq!(discovered, nested.canonicalize().unwrap());
        assert!(Context::load(discovered).is_err());
        fs::remove_dir_all(nested.join(REGISTRY_FILE)).unwrap();
        assert_eq!(
            project_root(None, &working).unwrap(),
            root.canonicalize().unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn an_explicit_root_is_exact_and_invalid_paths_never_fall_back() {
        let root = std::env::temp_dir().join(format!("polyfw-explicit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("caller")).unwrap();
        fs::create_dir_all(root.join("project")).unwrap();
        fs::write(root.join(REGISTRY_FILE), "").unwrap();
        for explicit in [PathBuf::from("../project"), root.join("project")] {
            assert_eq!(
                project_root(Some(&explicit), &root.join("caller")).unwrap(),
                root.join("project").canonicalize().unwrap()
            );
        }
        for explicit in [root.join("missing"), root.join(REGISTRY_FILE)] {
            assert!(project_root(Some(&explicit), &root.join("caller")).is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_missing_configuration_reports_an_error_instead_of_using_the_tools_source() {
        let root = std::env::temp_dir().join(format!("polyfw-fallback-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("caller")).unwrap();
        let error = project_root(None, &root.join("caller")).unwrap_err();
        assert!(
            error.to_string().contains("no polyfw.toml found"),
            "{error}"
        );
        assert!(error.to_string().contains("--project-root"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loading_the_registry_rejects_a_registered_firmware_without_a_library() {
        let root = std::env::temp_dir().join(format!("polyfw-no-lib-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let workspace = root.join("bsp/test");
        configure(&workspace);
        fs::create_dir_all(workspace.join("src")).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"test-board\"\nversion = \"0.1.0\"\n[workspace]\n",
        )
        .unwrap();
        fs::write(workspace.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            root.join(REGISTRY_FILE),
            "[bsps.test]\nworkspace = \"bsp/test\"\n\
             chip = \"test-chip\"\n\
             [boards.test-board]\ndescription = \"test board\"\nbsp = \"test\"\n\
             package = \"test-board\"\n",
        )
        .unwrap();

        let result = Context::load(root.clone());
        fs::remove_dir_all(root).unwrap();
        let error = result
            .err()
            .expect("a binary-only firmware must be rejected");
        assert!(
            error
                .to_string()
                .contains("test-board has no library target"),
            "{error}"
        );
    }

    #[test]
    fn firmware_binaries_use_the_only_target_or_an_explicit_name() {
        let root = std::env::temp_dir().join(format!("polyfw-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let workspace = root.join("bsp/test");
        configure(&workspace);
        fs::create_dir_all(workspace.join("src/bin")).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"test-board\"\nversion = \"0.1.0\"\n[workspace]\n",
        )
        .unwrap();
        fs::write(workspace.join("src/lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
        fs::write(workspace.join("src/main.rs"), "fn main() {}\n").unwrap();
        let source = "[bsps.test]\nworkspace = \"bsp/test\"\n\
             chip = \"test-chip\"\n\
             [boards.test-board]\ndescription = \"test board\"\nbsp = \"test\"\n\
             package = \"test-board\"\n";
        fs::write(root.join(REGISTRY_FILE), source).unwrap();
        let context = Context::load(root.clone()).unwrap();
        assert_eq!(
            context.selection("test-board").unwrap().board.resolved_bin,
            "test-board"
        );
        assert!(context.registry.boards["test-board"].bin.is_none());

        fs::write(workspace.join("src/bin/robot-image.rs"), "fn main() {}\n").unwrap();
        fs::write(
            root.join(REGISTRY_FILE),
            format!("{source}bin = \"robot-image\"\n"),
        )
        .unwrap();
        let context = Context::load(root.clone()).unwrap();
        assert_eq!(
            context.selection("test-board").unwrap().board.resolved_bin,
            "robot-image"
        );

        for bin in ["missing-image", "../robot-image", ""] {
            fs::write(root.join(REGISTRY_FILE), format!("{source}bin = {bin:?}\n")).unwrap();
            let error = Context::load(root.clone()).err().unwrap();
            assert!(
                error.to_string().contains("has no binary target named"),
                "{error}"
            );
        }

        fs::write(root.join(REGISTRY_FILE), source).unwrap();
        fs::remove_file(workspace.join("src/main.rs")).unwrap();
        let context = Context::load(root.clone()).unwrap();
        let selection = context.selection("test-board").unwrap();
        assert!(selection.board.bin.is_none());
        assert_eq!(selection.board.resolved_bin, "robot-image");
        let elf = crate::runner::firmware_elf_path(&root, &selection);
        assert_eq!(
            elf,
            root.join("target/firmware/test/test-target/debug/robot-image")
        );
        context.save_selection("test-board", None).unwrap();
        let launch: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(vscode::LAUNCH_FILE)).unwrap())
                .unwrap();
        for configuration in launch["configurations"].as_array().unwrap() {
            assert_eq!(
                configuration["coreConfigs"][0]["programBinary"],
                "${workspaceFolder}/target/firmware/test/test-target/debug/robot-image"
            );
        }

        fs::remove_file(workspace.join("src/bin/robot-image.rs")).unwrap();
        let error = Context::load(root.clone()).err().unwrap();
        assert!(
            error.to_string().contains("has no binary targets"),
            "{error}"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn multiple_binaries_require_an_explicit_bin_even_when_one_matches_the_package() {
        let root =
            std::env::temp_dir().join(format!("polyfw-ambiguous-bin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let workspace = root.join("bsp/test");
        configure(&workspace);
        fs::create_dir_all(workspace.join("src/bin")).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[package]\nname = \"test-board\"\nversion = \"0.1.0\"\n[workspace]\n",
        )
        .unwrap();
        fs::write(workspace.join("src/lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
        fs::write(workspace.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(workspace.join("src/bin/robot-image.rs"), "fn main() {}\n").unwrap();
        let source = "[bsps.test]\nworkspace = \"bsp/test\"\n\
             chip = \"test-chip\"\n\
             [boards.test-board]\ndescription = \"test board\"\nbsp = \"test\"\n\
             package = \"test-board\"\n";
        fs::write(root.join(REGISTRY_FILE), source).unwrap();
        let error = Context::load(root.clone())
            .err()
            .expect("multiple binaries must require an explicit bin")
            .to_string();
        for expected in [
            "test-board",
            "robot-image",
            "multiple binary targets",
            "set bin",
        ] {
            assert!(error.contains(expected), "{error}");
        }

        fs::write(
            root.join(REGISTRY_FILE),
            format!("{source}bin = \"test-board\"\n"),
        )
        .unwrap();
        Context::load(root.clone()).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn binary_names_are_unique_across_packages_within_each_bsp() {
        let root =
            std::env::temp_dir().join(format!("polyfw-bin-collision-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let workspace = root.join("bsp/test");
        configure(&workspace);
        fs::create_dir_all(&workspace).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nmembers = [\"firmware-a\", \"firmware-b\"]\nresolver = \"3\"\n",
        )
        .unwrap();
        for name in ["firmware-a", "firmware-b"] {
            let directory = workspace.join(name);
            fs::create_dir_all(directory.join("src")).unwrap();
            fs::write(
                directory.join("Cargo.toml"),
                format!(
                    "[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\n\
                     [[bin]]\nname = \"shared-image\"\npath = \"src/main.rs\"\ntest = false\n"
                ),
            )
            .unwrap();
            fs::write(directory.join("src/lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
            fs::write(
                directory.join("src/main.rs"),
                "#![forbid(unsafe_code)]\nfn main() {}\n",
            )
            .unwrap();
        }
        let source = "[bsps.first-family]\nworkspace = \"bsp/test\"\n\
             chip = \"test\"\n\
             [bsps.second-family]\nworkspace = \"bsp/test\"\n\
             chip = \"test\"\n\
             [boards.one]\ndescription = \"first board\"\nbsp = \"first-family\"\n\
             package = \"firmware-a\"\nbin = \"shared-image\"\n\
             [boards.alias]\ndescription = \"same image\"\nbsp = \"first-family\"\n\
             package = \"firmware-a\"\nbin = \"shared-image\"\n\
             [boards.two]\ndescription = \"other package\"\nbsp = \"second-family\"\n\
             package = \"firmware-b\"\nbin = \"shared-image\"\n";
        for source in [
            source.to_owned(),
            source.replace("bin = \"shared-image\"\n", ""),
        ] {
            fs::write(root.join(REGISTRY_FILE), &source).unwrap();
            let context = Context::load(root.clone()).unwrap();
            assert_eq!(context.registry.boards.len(), 3);
            for board in context.registry.boards.keys() {
                assert_eq!(
                    context.selection(board).unwrap().board.resolved_bin,
                    "shared-image"
                );
            }

            fs::write(
                root.join(REGISTRY_FILE),
                source.replace("bsp = \"second-family\"", "bsp = \"first-family\""),
            )
            .unwrap();
            let error = Context::load(root.clone()).err().unwrap().to_string();
            for expected in [
                "firmware-a",
                "firmware-b",
                "shared-image",
                "first-family",
                "two",
            ] {
                assert!(error.contains(expected), "{error}");
            }
            assert!(error.contains("one") || error.contains("alias"), "{error}");
            assert!(error.contains("ELF paths would collide"), "{error}");
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_board_can_move_within_its_bsp_but_cannot_use_another_workspace() {
        let root = std::env::temp_dir().join(format!("polyfw-package-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let workspace = root.join("bsp/test");
        configure(&workspace);
        fs::create_dir_all(workspace.join("firmware")).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nmembers = [\"firmware\"]\nresolver = \"3\"\n",
        )
        .unwrap();
        fs::write(
            workspace.join("firmware/Cargo.toml"),
            "[package]\nname = \"test-board\"\nversion = \"0.1.0\"\n[lib]\npath = \"lib.rs\"\n",
        )
        .unwrap();
        fs::write(
            workspace.join("firmware/lib.rs"),
            "#![forbid(unsafe_code)]\n",
        )
        .unwrap();
        fs::create_dir_all(workspace.join("firmware/src")).unwrap();
        fs::write(workspace.join("firmware/src/main.rs"), "fn main() {}\n").unwrap();

        let mut registry: Registry = toml::from_str(
            "[bsps.test]\nworkspace = \"bsp/test\"\n\
             chip = \"test-chip\"\n\
             [boards.test-board]\ndescription = \"test board\"\nbsp = \"test\"\n\
             package = \"test-board\"\n",
        )
        .unwrap();
        registry.resolve_cargo(&root).unwrap();

        fs::rename(workspace.join("firmware"), workspace.join("relocated")).unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nmembers = [\"relocated\"]\nresolver = \"3\"\n",
        )
        .unwrap();
        registry.resolve_cargo(&root).unwrap();

        let other = root.join("bsp/other");
        fs::create_dir_all(&other).unwrap();
        fs::write(
            other.join("Cargo.toml"),
            "[package]\nname = \"other-board\"\nversion = \"0.1.0\"\n\
             [workspace]\n[lib]\npath = \"lib.rs\"\n",
        )
        .unwrap();
        fs::write(other.join("lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
        let mut bsp = registry.bsps["test"].clone();
        bsp.workspace = "bsp/other".into();
        registry.bsps.insert("other".into(), bsp);
        registry.boards.get_mut("test-board").unwrap().bsp = "other".into();
        let error = registry.resolve_cargo(&root).unwrap_err().to_string();
        assert!(
            error.contains("package is not in BSP other: test-board"),
            "{error}"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registry_names_are_lowercase_slug_names() {
        assert!(valid_name("sentry-chassis"));
        assert!(valid_name("bringup-dm-mc02"));
        assert!(!valid_name("Sentry"));
        assert!(!valid_name("sentry--chassis"));
        assert!(!valid_name("sentry-"));
    }

    #[test]
    fn compiler_settings_are_rejected_in_the_registry_instead_of_ignored() {
        for setting in ["target = \"thumbv7em-none-eabihf\"", "cpu = \"cortex-m7\""] {
            assert!(
                toml::from_str::<Registry>(&format!(
                    "boards = {{}}\n[bsps.test]\nworkspace = \".\"\nchip = \"test\"\n{setting}\n"
                ))
                .is_err()
            );
        }
    }

    fn configure(workspace: &Path) {
        fs::create_dir_all(workspace.join(".cargo")).unwrap();
        fs::write(
            workspace.join(".cargo/config.toml"),
            "[build]\ntarget = \"test-target\"\n\
             [target.test-target]\nrustflags = \"-Ctarget-cpu=test-cpu\"\n",
        )
        .unwrap();
    }

    #[test]
    fn dependency_patterns_default_to_the_project_and_invalid_patterns_fail_before_cargo() {
        let registry: Registry = toml::from_str("bsps = {}\nboards = {}\n").unwrap();
        assert_eq!(registry.config.local_dependencies, ["./**"]);
        let registry: Registry =
            toml::from_str("bsps = {}\nboards = {}\n[config]\nlocal-dependencies = []\n").unwrap();
        assert!(registry.config.local_dependencies.is_empty());
        for value in ["42", "\"shared/**\"", "[42]"] {
            assert!(
                toml::from_str::<Registry>(&format!(
                    "bsps = {{}}\nboards = {{}}\n[config]\nlocal-dependencies = {value}\n"
                ))
                .is_err()
            );
        }
        for pattern in ["[", "shared/**suffix", "*/../shared"] {
            let registry: Registry = toml::from_str(&format!(
                "bsps = {{}}\nboards = {{}}\n[config]\nlocal-dependencies = [{pattern:?}]\n"
            ))
            .unwrap();
            let error = registry
                .validate_structure(Path::new("."))
                .unwrap_err()
                .to_string();
            assert!(
                error.contains("invalid local-dependencies pattern"),
                "{error}"
            );
            assert!(error.contains(pattern), "{error}");
        }
    }

    #[test]
    fn tool_configuration_defaults_are_preserved_for_missing_and_partial_tables() {
        for config in [
            "",
            "[config]\n",
            "[config.vscode]\n",
            "[config.vscode]\nenabled = true\n",
        ] {
            let registry: Registry =
                toml::from_str(&format!("bsps = {{}}\nboards = {{}}\n{config}")).unwrap();
            assert_eq!(registry.config.local_dependencies, ["./**"]);
            assert!(registry.config.vscode.enabled);
        }
        let registry: Registry = toml::from_str(
            "bsps = {}\nboards = {}\n[config]\nlocal-dependencies = []\n\
             [config.vscode]\nenabled = false\n",
        )
        .unwrap();
        assert!(registry.config.local_dependencies.is_empty());
        assert!(!registry.config.vscode.enabled);
    }

    #[test]
    fn misplaced_unknown_and_wrongly_typed_configuration_options_are_rejected() {
        for config in [
            "local-dependencies = []\n",
            "[config]\nunknown = true\n",
            "[config]\nvscode = false\n",
            "[config.vscode]\nenabled = \"false\"\n",
            "[config.vscode]\nenable = false\n",
        ] {
            assert!(
                toml::from_str::<Registry>(&format!("bsps = {{}}\nboards = {{}}\n{config}"))
                    .is_err(),
                "{config}"
            );
        }
    }

    #[test]
    fn the_schema_identifies_the_package_version_and_draft07_dialect() {
        let schema: serde_json::Value = serde_json::from_str(&schema_source().unwrap()).unwrap();
        assert_eq!(
            schema["$id"],
            format!(
                "https://raw.githubusercontent.com/inaku-Gyan/polyfw/v{}/polyfw.schema.json",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert_eq!(schema["$schema"], "http://json-schema.org/draft-07/schema#");
    }

    #[test]
    fn the_registry_schema_matches_the_generated_schema() {
        assert_eq!(
            include_str!("../polyfw.schema.json"),
            schema_source().unwrap(),
            "registry schema is stale; run just schema"
        );
    }

    #[test]
    fn duplicate_bsp_work_is_removed_for_all_boards() {
        let bsp = Bsp {
            workspace: "bsp/dm-mc02".into(),
            target: "thumbv7em-none-eabihf".into(),
            cpu: Some("cortex-m7".into()),
            chip: "STM32H723VGTx".into(),
        };
        let board = |id: &str| Selection {
            id: id.into(),
            profile: None,
            board: Board {
                description: id.into(),
                bsp: "dm-mc02".into(),
                package: id.into(),
                bin: None,
                resolved_bin: id.into(),
            },
            bsp: bsp.clone(),
        };
        assert_eq!(unique_by_bsp(&[board("one"), board("two")]).len(), 1);
    }

    #[test]
    fn saving_a_valid_board_writes_the_selection_file_and_rejects_unknown_boards() {
        let root = std::env::temp_dir().join(format!("polyfw-selection-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();

        let context = Context {
            root: root.clone(),
            registry: Registry {
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
        };

        let profile = "release".parse().unwrap();
        context
            .save_selection("test-board", Some(&profile))
            .unwrap();
        let saved = context.read_selection().unwrap().unwrap();
        assert_eq!(saved.board, "test-board");
        assert_eq!(saved.profile.as_ref(), Some(&profile));
        assert!(
            context
                .save_selection("missing-board", Some(&profile))
                .is_err()
        );
        assert_eq!(
            context.read_selection().unwrap().unwrap().board,
            "test-board"
        );

        fs::write(context.selection_path(), "test-board\n").unwrap();
        assert!(context.read_selection().unwrap().unwrap().profile.is_none());
        fs::write(context.selection_path(), "board = \"test-board\"\n").unwrap();
        assert!(context.read_selection().unwrap().unwrap().profile.is_none());
        fs::write(
            context.selection_path(),
            "board = \"test-board\"\nprofile = \"dev\"\n",
        )
        .unwrap();
        assert_eq!(
            context
                .read_selection()
                .unwrap()
                .unwrap()
                .profile
                .as_ref()
                .map(Profile::name),
            Some("dev")
        );
        context.save_selection("test-board", None).unwrap();
        assert_eq!(
            fs::read_to_string(context.selection_path()).unwrap(),
            "board = \"test-board\"\n"
        );
        assert!(context.read_selection().unwrap().unwrap().profile.is_none());
        fs::write(
            context.selection_path(),
            "board = \"test-board\"\nprofile = \"../release\"\n",
        )
        .unwrap();
        assert!(context.read_selection().is_err());
        assert!(
            context
                .clear_selection()
                .iter()
                .all(|(_, result)| result.is_ok())
        );
        assert!(context.clear_selection().iter().all(|(_, result)| {
            result.as_ref().unwrap_err().kind() == std::io::ErrorKind::NotFound
        }));
        assert!(context.read_selection().unwrap().is_none());

        fs::remove_dir_all(root).unwrap();
    }
}
