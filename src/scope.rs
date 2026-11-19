use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use serde::{Serialize, Serializer};

use crate::Result;
use crate::local_dependencies::LocalDependencies;
use crate::model::{Bsp, DependencyKind, Metadata, MetadataPackage, Selection};
use crate::registry::{Context, cargo_metadata, unique_by_bsp};
use crate::runner;

pub(crate) struct WorkspaceScope {
    pub(crate) bsp_id: String,
    pub(crate) bsp: Bsp,
    pub(crate) firmware: BTreeSet<PathBuf>,
    pub(crate) packages: Vec<PackageScope>,
}

#[derive(Serialize)]
pub(crate) struct PackageScope {
    #[serde(rename = "package")]
    pub(crate) name: String,
    pub(crate) manifest_path: PathBuf,
    #[serde(rename = "target", serialize_with = "serialize_target")]
    pub(crate) embedded: bool,
    #[serde(skip)]
    pub(crate) id: String,
    #[serde(skip)]
    pub(crate) host_id: String,
    #[serde(skip)]
    pub(crate) workspace: PathBuf,
}

pub(crate) struct HostScope<'a> {
    pub(crate) workspace: &'a Path,
    pub(crate) packages: Vec<&'a PackageScope>,
}

pub(crate) fn host_scopes(scopes: &[WorkspaceScope]) -> Vec<HostScope<'_>> {
    let mut workspaces = BTreeMap::<&Path, BTreeMap<&Path, &PackageScope>>::new();
    for package in scopes.iter().flat_map(|scope| &scope.packages) {
        workspaces
            .entry(&package.workspace)
            .or_default()
            .insert(&package.manifest_path, package);
    }
    workspaces
        .into_iter()
        .map(|(workspace, packages)| {
            let mut packages = packages.into_values().collect::<Vec<_>>();
            packages.sort_by_key(|package| (&package.name, &package.manifest_path));
            HostScope {
                workspace,
                packages,
            }
        })
        .collect()
}

fn serialize_target<S: Serializer>(
    embedded: &bool,
    serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
    serializer.serialize_str(if *embedded { "embedded" } else { "host" })
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Platform {
    Embedded,
    Host,
}

pub(crate) fn resolve(context: &Context, selections: &[Selection]) -> Result<Vec<WorkspaceScope>> {
    let mut resolver = Resolver::new(context)?;
    let mut scopes = Vec::new();
    for selection in unique_by_bsp(selections) {
        scopes.push(resolver.board_scope(context, selection, selections)?);
    }
    Ok(scopes)
}

struct Resolver {
    root: PathBuf,
    local_dependencies: LocalDependencies,
    metadata: BTreeMap<PathBuf, Rc<Metadata>>,
    manifests: BTreeMap<PathBuf, PathBuf>,
    owners: BTreeMap<PathBuf, (PathBuf, String)>,
}

impl Resolver {
    fn new(context: &Context) -> Result<Self> {
        let root = canonical(&context.root)?;
        let local_dependencies =
            LocalDependencies::new(&root, &context.registry.config.local_dependencies)?;
        Ok(Self {
            root,
            local_dependencies,
            metadata: BTreeMap::new(),
            manifests: BTreeMap::new(),
            owners: BTreeMap::new(),
        })
    }

    fn path_manifest(&mut self, package: &MetadataPackage) -> Result<Option<PathBuf>> {
        if package.source.is_some() {
            return Ok(None);
        }
        if let Some(manifest) = self.manifests.get(&package.manifest_path) {
            return Ok(Some(manifest.clone()));
        }
        let manifest = canonical(&package.manifest_path)?;
        self.manifests
            .insert(package.manifest_path.clone(), manifest.clone());
        Ok(Some(manifest))
    }

    fn metadata(&mut self, workspace: &Path) -> Result<Rc<Metadata>> {
        let workspace = canonical(workspace)?;
        if let Some(metadata) = self.metadata.get(&workspace) {
            return Ok(Rc::clone(metadata));
        }
        let metadata = Rc::new(cargo_metadata(&workspace, true)?);
        for package in &metadata.packages {
            if metadata.workspace_members.contains(&package.id)
                && let Some(manifest) = self.path_manifest(package)?
            {
                self.owners
                    .insert(manifest, (workspace.clone(), package.id.clone()));
            }
        }
        self.metadata.insert(workspace, Rc::clone(&metadata));
        Ok(metadata)
    }

    fn owner(&mut self, manifest: &Path) -> Result<(PathBuf, String)> {
        if let Some(owner) = self.owners.get(manifest) {
            return Ok(owner.clone());
        }
        let output =
            runner::workspace_cargo(manifest.parent().ok_or("package manifest has no parent")?)
                .args([
                    "locate-project",
                    "--workspace",
                    "--message-format",
                    "plain",
                    "--manifest-path",
                ])
                .arg(manifest)
                .output()?;
        if !output.status.success() {
            return Err(format!(
                "cargo locate-project failed for {}: {}",
                manifest.display(),
                String::from_utf8_lossy(&output.stderr).trim()
            )
            .into());
        }
        let owner_manifest = PathBuf::from(String::from_utf8(output.stdout)?.trim());
        self.metadata(
            owner_manifest
                .parent()
                .ok_or("workspace manifest has no parent")?,
        )?;
        self.owners.get(manifest).cloned().ok_or_else(|| {
            format!(
                "package is not a member of its owning workspace: {}",
                manifest.display()
            )
            .into()
        })
    }

    fn board_scope(
        &mut self,
        context: &Context,
        selection: &Selection,
        selections: &[Selection],
    ) -> Result<WorkspaceScope> {
        let workspace = canonical(&context.root.join(&selection.bsp.workspace))?;
        let metadata = self.metadata(&workspace)?;
        let mut pending = Vec::new();
        let mut firmware = BTreeSet::new();
        for board in selections
            .iter()
            .filter(|board| board.board.bsp == selection.board.bsp)
        {
            let package = metadata
                .workspace_package(&board.board.package)
                .ok_or_else(|| {
                    format!(
                        "firmware package is not in BSP workspace: {}",
                        board.board.package
                    )
                })?;
            firmware.insert(canonical(&package.manifest_path)?);
            pending.extend([
                (workspace.clone(), package.id.clone(), Platform::Embedded),
                (workspace.clone(), package.id.clone(), Platform::Host),
            ]);
        }
        let mut visited = BTreeSet::new();
        let mut packages = BTreeMap::<PathBuf, PackageScope>::new();
        while let Some((workspace, id, platform)) = pending.pop() {
            let metadata = self.metadata(&workspace)?;
            let package = metadata
                .packages
                .iter()
                .find(|package| package.id == id)
                .ok_or_else(|| format!("Cargo metadata has no package: {id}"))?;
            let platform = if package
                .targets
                .iter()
                .any(|target| target.kind.iter().any(|kind| kind == "proc-macro"))
            {
                Platform::Host
            } else {
                platform
            };
            if !visited.insert((workspace.clone(), id.clone(), platform)) {
                continue;
            }
            if let Some(manifest) = self.path_manifest(package)?
                && (firmware.contains(&manifest)
                    || self
                        .local_dependencies
                        .contains(manifest.parent().ok_or("package manifest has no parent")?)?)
            {
                if let Some(entry) = packages.get_mut(&manifest) {
                    if platform == Platform::Embedded {
                        entry.embedded = true;
                        entry.id = id.clone();
                    }
                } else {
                    let (owner, host_id) = self.owner(&manifest)?;
                    // The BSP graph omits non-member packages' dev-dependencies.
                    pending.push((owner.clone(), host_id.clone(), Platform::Host));
                    packages.insert(
                        manifest.clone(),
                        PackageScope {
                            name: package.name.clone(),
                            manifest_path: project_relative(&self.root, &manifest),
                            embedded: platform == Platform::Embedded,
                            id: id.clone(),
                            host_id,
                            workspace: owner,
                        },
                    );
                }
            }
            let graph = metadata
                .resolve
                .as_ref()
                .ok_or("Cargo metadata has no dependency graph")?;
            let node = graph
                .nodes
                .iter()
                .find(|node| node.id == id)
                .ok_or_else(|| format!("Cargo metadata has no dependency node: {id}"))?;
            for dependency in &node.deps {
                for kind in &dependency.dep_kinds {
                    let platform = match kind.kind {
                        None => platform,
                        Some(DependencyKind::Build | DependencyKind::Dev) => Platform::Host,
                    };
                    pending.push((workspace.clone(), dependency.pkg.clone(), platform));
                }
            }
        }
        let mut packages = packages.into_values().collect::<Vec<_>>();
        packages.sort_by(|left, right| {
            (&left.name, &left.manifest_path).cmp(&(&right.name, &right.manifest_path))
        });
        Ok(WorkspaceScope {
            bsp_id: selection.board.bsp.clone(),
            bsp: selection.bsp.clone(),
            firmware: firmware
                .into_iter()
                .map(|manifest| project_relative(&self.root, &manifest))
                .collect(),
            packages,
        })
    }
}

fn project_relative(root: &Path, path: &Path) -> PathBuf {
    let common = root
        .components()
        .zip(path.components())
        .take_while(|(left, right)| left == right)
        .count();
    if common == 0 {
        return path.to_owned();
    }
    let mut relative = root
        .components()
        .skip(common)
        .map(|_| "..")
        .collect::<PathBuf>();
    relative.extend(path.components().skip(common));
    relative
}

fn canonical(path: &Path) -> Result<PathBuf> {
    path.canonicalize()
        .map_err(|error| format!("{}: {error}", path.display()).into())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use clap::ColorChoice;

    use super::*;

    #[test]
    fn directory_patterns_filter_dependencies_without_dropping_selected_firmware() {
        let context = fixture("directory-patterns");
        let registry_path = context.root.join("polyfw.toml");
        let source = fs::read_to_string(&registry_path).unwrap();
        for (patterns, expected) in [
            (vec![], vec!["board-a"]),
            (vec!["shared"], vec!["board-a"]),
            (vec!["missing/**"], vec!["board-a"]),
            (vec!["shared/*"], vec!["board-a", "outside"]),
            (vec!["./shared/outside/"], vec!["board-a", "outside"]),
            (
                vec!["shared/**"],
                vec!["board-a", "outside", "outside-test-helper"],
            ),
            (vec!["bsp/test/common-b"], vec!["board-a", "common-b"]),
        ] {
            fs::write(
                &registry_path,
                format!("{source}\n[config]\nlocal-dependencies = {patterns:?}\n"),
            )
            .unwrap();
            let context = Context::load(context.root.clone()).unwrap();
            let scopes = resolve(&context, &[context.selection("one").unwrap()]).unwrap();
            let names = scopes[0]
                .packages
                .iter()
                .map(|package| package.name.as_str())
                .collect::<Vec<_>>();
            assert_eq!(names, expected, "{patterns:?}");
            assert!(
                scopes[0]
                    .firmware
                    .contains(Path::new("bsp/test/board-a/Cargo.toml"))
            );
            let report = crate::info::Report::load(&context, Some("one")).unwrap();
            assert_eq!(
                report.boards["one"]
                    .local_dependencies
                    .as_ref()
                    .unwrap()
                    .len(),
                expected.len() - 1
            );
        }
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn external_directory_patterns_include_packages_and_their_own_host_helpers() {
        let context = fixture("external-patterns");
        let external = context.root.with_extension("external");
        let _ = fs::remove_dir_all(&external);
        fs::rename(context.root.join("shared/outside"), &external).unwrap();
        let common = context.root.join("bsp/test/common-b");
        let manifest = external.join("Cargo.toml");
        let source = fs::read_to_string(&manifest).unwrap();
        fs::write(
            &manifest,
            source.replace("\"../../bsp/test/common-b\"", &format!("{common:?}")),
        )
        .unwrap();
        let board = context.root.join("bsp/test/board-a/Cargo.toml");
        let source = fs::read_to_string(&board).unwrap();
        fs::write(
            board,
            source.replace("\"../../../shared/outside\"", &format!("{external:?}")),
        )
        .unwrap();
        lock(&context.root.join("bsp/test"));
        lock(&external);
        let registry = context.root.join("polyfw.toml");
        let source = fs::read_to_string(&registry).unwrap();
        let relative = Path::new("..").join(external.file_name().unwrap());
        for pattern in [relative.join("**"), external.join("**")] {
            fs::write(
                &registry,
                format!("{source}\n[config]\nlocal-dependencies = [\"./**\", {pattern:?}]\n"),
            )
            .unwrap();
            let context = Context::load(context.root.clone()).unwrap();
            let report = crate::info::Report::load(&context, Some("one")).unwrap();
            let packages = report.boards["one"].local_dependencies.as_ref().unwrap();
            let outside = packages
                .iter()
                .find(|package| package.name == "outside")
                .unwrap();
            assert!(outside.embedded);
            assert_eq!(outside.manifest_path, relative.join("Cargo.toml"));
            let helper = packages
                .iter()
                .find(|package| package.name == "outside-test-helper")
                .unwrap();
            assert!(!helper.embedded);
            assert_eq!(
                helper.manifest_path,
                relative.join("test-helper/Cargo.toml")
            );
        }
        fs::remove_dir_all(context.root).unwrap();
        fs::remove_dir_all(external).unwrap();
    }
    use crate::cli::{BoardArgs, Task};
    use crate::commands;
    use crate::ui::Ui;

    #[test]
    fn a_board_scope_follows_all_dependency_kinds_and_keeps_platform_uses_separate() {
        let context = fixture("dependencies");
        let board = context.selection("one").unwrap();
        let scopes = resolve(&context, &[board]).unwrap();
        assert_eq!(scopes.len(), 1);
        let packages = scopes[0]
            .packages
            .iter()
            .map(|package| (package.name.as_str(), package.embedded))
            .collect::<Vec<_>>();
        assert_eq!(
            packages,
            [
                ("board-a", true),
                ("build-helper", false),
                ("common-a", true),
                ("common-b", true),
                ("dev-helper", false),
                ("host-leaf", false),
                ("macro-helper", false),
                ("mixed", true),
                ("optional-common", true),
                ("outside", true),
                ("outside-test-helper", false),
            ]
        );
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn all_boards_share_one_scope_and_check_each_common_package_once() {
        let context = fixture("all-boards");
        let boards = [
            context.selection("one").unwrap(),
            context.selection("two").unwrap(),
        ];
        let scopes = resolve(&context, &boards).unwrap();
        assert_eq!(scopes.len(), 1);
        let names = scopes[0]
            .packages
            .iter()
            .map(|package| package.name.as_str())
            .collect::<Vec<_>>();
        let unique = names.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(names.len(), unique.len());
        assert_eq!(names.len(), 13);
        assert!(unique.contains("board-b"));
        assert!(unique.contains("other-only"));
        assert!(!unique.contains("unused"));
        assert!(unique.contains("outside"));
        assert!(unique.contains("outside-test-helper"));
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn firmware_bins_in_the_same_package_share_one_check_scope() {
        let context = fixture("shared-package");
        let first = context.selection("one").unwrap();
        let second = context.selection("one-alternate").unwrap();
        assert_eq!(first.board.resolved_bin, "board-a");
        assert_eq!(second.board.resolved_bin, "board-a-alternate");
        assert_ne!(first.id, second.id);
        let expected = resolve(&context, std::slice::from_ref(&first)).unwrap();
        let expected_packages = expected[0]
            .packages
            .iter()
            .map(|package| (package.name.as_str(), package.embedded))
            .collect::<Vec<_>>();
        for selections in [vec![second.clone()], vec![first, second]] {
            let scopes = resolve(&context, &selections).unwrap();
            assert_eq!(scopes.len(), 1);
            let packages = scopes[0]
                .packages
                .iter()
                .map(|package| (package.name.as_str(), package.embedded))
                .collect::<Vec<_>>();
            assert_eq!(packages, expected_packages);
            assert_eq!(
                packages
                    .iter()
                    .filter(|(name, _)| *name == "board-a")
                    .count(),
                1
            );
        }
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn lint_checks_other_bins_in_the_selected_firmware_package() {
        const MODE: &str = "FLEET_XTASK_OTHER_BIN_CHILD";
        if std::env::var_os(MODE).is_none() {
            let output = Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "scope::tests::lint_checks_other_bins_in_the_selected_firmware_package",
                    "--nocapture",
                ])
                .env(MODE, "1")
                .env("CARGO_TERM_COLOR", "never")
                .output()
                .unwrap();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert!(output.status.success(), "{stdout}\n{stderr}");
            assert!(
                stderr.contains("error: another firmware binary"),
                "{stdout}\n{stderr}"
            );
            assert!(!stderr.contains("E0463"), "{stdout}\n{stderr}");
            return;
        }
        let context = fixture("lint-other-bin");
        let root = context.root.clone();
        let path = root.join("bsp/test/board-a/src/alternate.rs");
        let source = fs::read_to_string(&path).unwrap();
        fs::write(
            path,
            format!("{source}\ncompile_error!(\"another firmware binary\");\n"),
        )
        .unwrap();
        let error = commands::run(
            context,
            Task::Lint(BoardArgs {
                board: Some("one".into()),
                select: false,
                all_boards: false,
            }),
            &Ui::new(ColorChoice::Never),
        )
        .unwrap_err();
        assert!(error.to_string().starts_with("cargo clippy"), "{error}");
        assert!(error.to_string().contains("failed with"), "{error}");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_board_can_verify_with_std_helpers_without_checking_unrelated_packages() {
        let context = fixture("verify");
        let root = context.root.clone();
        let _ = commands::run(
            context,
            Task::Verify(BoardArgs {
                board: Some("one".into()),
                select: false,
                all_boards: false,
            }),
            &Ui::new(ColorChoice::Never),
        )
        .unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn registry_and_git_sources_are_excluded_before_their_manifest_paths_are_read() {
        let context = fixture("sources");
        let mut resolver = Resolver::new(&context).unwrap();
        let mut package = MetadataPackage {
            id: "third-party".into(),
            name: "third-party".into(),
            source: None,
            manifest_path: context.root.join("missing/Cargo.toml"),
            targets: Vec::new(),
        };
        for source in [
            "registry+https://example.invalid/index",
            "git+https://example.invalid/repo",
        ] {
            package.source = Some(source.into());
            assert!(resolver.path_manifest(&package).unwrap().is_none());
            assert!(resolver.manifests.is_empty());
        }
        package.source = None;
        assert!(resolver.path_manifest(&package).is_err());
        fs::remove_dir_all(context.root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn canonical_manifest_locations_determine_which_symlinked_packages_are_local() {
        use std::os::unix::fs::symlink;

        let context = fixture("symlinks");
        let external = context.root.with_extension("external");
        fs::create_dir_all(&external).unwrap();
        fs::write(external.join("Cargo.toml"), "").unwrap();
        symlink(&external, context.root.join("escape")).unwrap();
        symlink(
            context.root.join("shared/outside"),
            context.root.join("alias"),
        )
        .unwrap();
        let mut resolver = Resolver::new(&context).unwrap();
        let mut package = MetadataPackage {
            id: "symlinked".into(),
            name: "symlinked".into(),
            source: None,
            manifest_path: context.root.join("escape/Cargo.toml"),
            targets: Vec::new(),
        };
        let manifest = resolver.path_manifest(&package).unwrap().unwrap();
        assert!(
            !resolver
                .local_dependencies
                .contains(manifest.parent().unwrap())
                .unwrap()
        );
        for pattern in [
            external.clone(),
            context.root.join("escape"),
            context
                .root
                .join("escape/..")
                .join(external.file_name().unwrap()),
        ] {
            let explicit =
                LocalDependencies::new(&context.root, &[pattern.to_str().unwrap().into()]).unwrap();
            assert!(explicit.contains(manifest.parent().unwrap()).unwrap());
        }
        package.manifest_path = context.root.join("alias/Cargo.toml");
        assert_eq!(
            resolver.path_manifest(&package).unwrap(),
            Some(canonical(&context.root.join("shared/outside/Cargo.toml")).unwrap())
        );
        fs::remove_dir_all(context.root).unwrap();
        fs::remove_dir_all(external).unwrap();
    }

    #[test]
    fn traversal_through_an_outside_package_still_reaches_its_local_dependencies() {
        let context = fixture("outside-project");
        let external = context.root.with_extension("external");
        let _ = fs::remove_dir_all(&external);
        fs::rename(context.root.join("shared/outside"), &external).unwrap();
        let common = context.root.join("bsp/test/common-b");
        fs::write(
            external.join("Cargo.toml"),
            format!(
                "[package]\nname = \"outside\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [workspace]\nresolver = \"3\"\n\
             [dependencies]\ncommon-b = {{ path = {common:?} }}\n"
            ),
        )
        .unwrap();
        let board = context.root.join("bsp/test/board-a/Cargo.toml");
        let manifest = fs::read_to_string(&board).unwrap();
        fs::write(
            board,
            manifest.replace("\"../../../shared/outside\"", &format!("{external:?}")),
        )
        .unwrap();
        lock(&context.root.join("bsp/test"));
        let scopes = resolve(&context, &[context.selection("one").unwrap()]).unwrap();
        assert!(
            scopes[0]
                .packages
                .iter()
                .any(|package| package.name == "common-b" && package.embedded)
        );
        assert!(
            !scopes[0]
                .packages
                .iter()
                .any(|package| package.name.starts_with("outside"))
        );
        fs::remove_dir_all(context.root).unwrap();
        fs::remove_dir_all(external).unwrap();
    }

    #[test]
    fn a_reached_firmware_library_is_checked_even_when_its_board_is_not_selected() {
        let context = fixture("firmware-dependency");
        let manifest = context.root.join("bsp/test/board-a/Cargo.toml");
        let source = fs::read_to_string(&manifest).unwrap();
        fs::write(
            manifest,
            source.replacen(
                "[dependencies]",
                "[dependencies]\nboard-b = { path = \"../board-b\" }",
                1,
            ),
        )
        .unwrap();
        lock(&context.root.join("bsp/test"));
        let scopes = resolve(&context, &[context.selection("one").unwrap()]).unwrap();
        assert!(
            scopes[0]
                .packages
                .iter()
                .any(|package| package.name == "board-b" && package.embedded)
        );
        assert!(
            scopes[0]
                .packages
                .iter()
                .any(|package| package.name == "other-only")
        );
        fs::remove_dir_all(context.root).unwrap();
    }

    #[test]
    fn same_named_dependencies_in_different_workspaces_are_both_verified() {
        let context = fixture("duplicate-names");
        let duplicate = context.root.join("shared/duplicate");
        fs::create_dir_all(duplicate.join("src")).unwrap();
        fs::write(
            duplicate.join("Cargo.toml"),
            "[package]\nname = \"board-a\"\nversion = \"0.2.0\"\nedition = \"2024\"\n\
             [workspace]\nresolver = \"3\"\n",
        )
        .unwrap();
        fs::write(
            duplicate.join("src/lib.rs"),
            "#![no_std]\n#![forbid(unsafe_code)]\n",
        )
        .unwrap();
        let manifest = context.root.join("bsp/test/board-a/Cargo.toml");
        let source = fs::read_to_string(&manifest).unwrap();
        fs::write(
            manifest,
            source.replacen(
                "[dependencies]",
                "[dependencies]\n\
                 second-common = { package = \"board-a\", path = \"../../../shared/duplicate\" }",
                1,
            ),
        )
        .unwrap();
        lock(&duplicate);
        lock(&context.root.join("bsp/test"));
        let scopes = resolve(&context, &[context.selection("one").unwrap()]).unwrap();
        let duplicates = scopes[0]
            .packages
            .iter()
            .filter(|package| package.name == "board-a")
            .collect::<Vec<_>>();
        assert_eq!(duplicates.len(), 2);
        assert_ne!(duplicates[0].manifest_path, duplicates[1].manifest_path);
        assert_ne!(duplicates[0].id, duplicates[1].id);
        assert!(duplicates.iter().all(|package| package.embedded));
        let report = crate::info::Report::load(&context, Some("one")).unwrap();
        assert_eq!(
            report.boards["one"]
                .local_dependencies
                .as_ref()
                .unwrap()
                .iter()
                .filter(|package| package.name == "board-a")
                .count(),
            1
        );
        let root = context.root.clone();
        let _ = commands::run(
            context,
            Task::Verify(BoardArgs {
                board: Some("one".into()),
                select: false,
                all_boards: false,
            }),
            &Ui::new(ColorChoice::Never),
        )
        .unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn owning_workspace_default_features_add_only_host_checks_to_the_board_scope() {
        let context = fixture("owner-defaults");
        let outside = context.root.join("shared/outside");
        let helper = outside.join("default-helper");
        fs::create_dir_all(helper.join("src")).unwrap();
        fs::write(
            helper.join("Cargo.toml"),
            "[package]\nname = \"default-helper\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            helper.join("src/lib.rs"),
            "#![no_std]\n#![forbid(unsafe_code)]\n",
        )
        .unwrap();
        let manifest = outside.join("Cargo.toml");
        let source = fs::read_to_string(&manifest).unwrap();
        fs::write(
            manifest,
            source.replace(
                "[dependencies]",
                "[dependencies]\n\
            default-helper = { path = \"default-helper\", optional = true }",
            ) + "\n[features]\ndefault = [\"dep:default-helper\"]\n",
        )
        .unwrap();
        let manifest = context.root.join("bsp/test/board-a/Cargo.toml");
        let source = fs::read_to_string(&manifest).unwrap();
        fs::write(
            manifest,
            source.replace(
                "outside = { path = \"../../../shared/outside\" }",
                "outside = { path = \"../../../shared/outside\", default-features = false }",
            ),
        )
        .unwrap();
        lock(&outside);
        lock(&context.root.join("bsp/test"));
        let scopes = resolve(&context, &[context.selection("one").unwrap()]).unwrap();
        let helper = scopes[0]
            .packages
            .iter()
            .find(|package| package.name == "default-helper")
            .unwrap();
        assert!(!helper.embedded);
        fs::remove_dir_all(context.root).unwrap();
    }

    fn lock(workspace: &Path) {
        let output = runner::workspace_cargo(workspace)
            .args(["generate-lockfile", "--offline"])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn fixture(label: &str) -> Context {
        // Cargo must see one path for dependencies reached both relatively and absolutely.
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("polyfw-scope-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let workspace = root.join("bsp/test");
        fs::create_dir_all(workspace.join(".cargo")).unwrap();
        fs::write(
            root.join("rust-toolchain.toml"),
            include_str!("../rust-toolchain.toml"),
        )
        .unwrap();
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nresolver = \"3\"\n\
             members = [\n\
             \"board-a\", \"board-b\", \"common-a\", \"common-b\",\n\
             \"build-helper\", \"dev-helper\", \"macro-helper\", \"host-leaf\",\n\
             \"mixed\", \"optional-common\", \"disabled-optional\", \"other-only\", \"unused\",\n\
             ]\n",
        )
        .unwrap();
        fs::write(
            workspace.join(".cargo/config.toml"),
            "[build]\ntarget = \"thumbv7em-none-eabihf\"\n",
        )
        .unwrap();
        let library = "#![no_std]\n#![forbid(unsafe_code)]\npub fn value() -> u8 { 0 }\n";
        let helper = "//! ```\n//! assert!(host_leaf::host_value().is_empty());\n//! ```\n\
                      #![forbid(unsafe_code)]\npub fn host_value() -> String { String::new() }\n";
        let unused = "#![forbid(unsafe_code)]\ncompile_error!(\"unselected package\");\n";
        for (name, manifest, source) in [
            (
                "board-a",
                "[dependencies]\noutside = { path = \"../../../shared/outside\" }\n\
                 macro-helper = { path = \"../macro-helper\" }\n\
                 mixed = { path = \"../mixed\" }\n\
                 [target.'cfg(target_os = \"none\")'.dependencies]\n\
                 drivers = { package = \"common-a\", path = \"../common-a\" }\n\
                 [build-dependencies]\nbuild-helper = { path = \"../build-helper\" }\n\
                 [dev-dependencies]\ndev-helper = { path = \"../dev-helper\" }\n",
                library,
            ),
            (
                "board-b",
                "[dependencies]\ncommon-a = { path = \"../common-a\", features = [\"extra\"] }\n\
                 other-only = { path = \"../other-only\" }\n",
                unused,
            ),
            (
                "common-a",
                "[dependencies]\n\
                 optional-common = { path = \"../optional-common\", optional = true }\n\
                 disabled-optional = { path = \"../disabled-optional\", optional = true }\n\
                 [features]\nextra = [\"dep:optional-common\"]\n\
                 [dev-dependencies]\nmixed = { path = \"../mixed\" }\n",
                library,
            ),
            ("common-b", "", library),
            (
                "build-helper",
                "[dependencies]\nhost-leaf = { path = \"../host-leaf\" }\n",
                "#![forbid(unsafe_code)]\npub fn value() -> String { host_leaf::host_value() }\n",
            ),
            (
                "dev-helper",
                "[dependencies]\nhost-leaf = { path = \"../host-leaf\" }\n\
                 mixed = { path = \"../mixed\" }\n",
                "#![forbid(unsafe_code)]\npub fn value() -> String { host_leaf::host_value() }\n",
            ),
            (
                "macro-helper",
                "[lib]\nproc-macro = true\n\
                 [dependencies]\nhost-leaf = { path = \"../host-leaf\" }\n",
                "#![forbid(unsafe_code)]\nextern crate proc_macro;\n\
                 #[proc_macro]\n\
                 pub fn noop(input: proc_macro::TokenStream) -> proc_macro::TokenStream {\n\
                 let _ = host_leaf::host_value();\ninput\n}\n",
            ),
            ("host-leaf", "", helper),
            ("mixed", "", library),
            ("optional-common", "", library),
            ("disabled-optional", "", unused),
            ("other-only", "", unused),
            ("unused", "", unused),
        ] {
            let directory = workspace.join(name);
            fs::create_dir_all(directory.join("src")).unwrap();
            fs::write(
                directory.join("Cargo.toml"),
                format!(
                    "[package]\nname = {name:?}\nversion = \"0.1.0\"\n\
                     edition = \"2024\"\n{manifest}"
                ),
            )
            .unwrap();
            fs::write(directory.join("src/lib.rs"), source).unwrap();
        }
        for name in ["board-a", "board-b"] {
            let directory = workspace.join(name);
            let manifest = fs::read_to_string(directory.join("Cargo.toml")).unwrap();
            fs::write(
                directory.join("Cargo.toml"),
                format!(
                    "{manifest}\n[[bin]]\nname = {name:?}\npath = \"src/main.rs\"\ntest = false\n"
                ),
            )
            .unwrap();
            fs::write(
                directory.join("src/main.rs"),
                "#![no_std]\n#![no_main]\n#![forbid(unsafe_code)]\n\
                 #[panic_handler]\nfn panic(_: &core::panic::PanicInfo<'_>) -> ! {\n\
                 loop { core::hint::spin_loop(); }\n}\n",
            )
            .unwrap();
        }
        let manifest = workspace.join("board-a/Cargo.toml");
        let source = fs::read_to_string(&manifest).unwrap();
        fs::write(
            manifest,
            format!(
                "{source}\n[[bin]]\nname = \"board-a-alternate\"\n\
                 path = \"src/alternate.rs\"\ntest = false\n"
            ),
        )
        .unwrap();
        fs::copy(
            workspace.join("board-a/src/main.rs"),
            workspace.join("board-a/src/alternate.rs"),
        )
        .unwrap();
        fs::write(
            workspace.join("board-a/build.rs"),
            "#![forbid(unsafe_code)]\nfn main() { let _ = build_helper::value(); }\n",
        )
        .unwrap();
        let outside = root.join("shared/outside");
        fs::create_dir_all(outside.join("src")).unwrap();
        fs::write(
            outside.join("rust-toolchain.toml"),
            include_str!("../rust-toolchain.toml"),
        )
        .unwrap();
        fs::write(
            outside.join("Cargo.toml"),
            "[package]\nname = \"outside\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\
             [workspace]\nresolver = \"3\"\nmembers = [\"test-helper\"]\n\
             [dependencies]\ncommon-b = { path = \"../../bsp/test/common-b\" }\n\
             [dev-dependencies]\n\
             outside-test-helper = { path = \"test-helper\" }\n",
        )
        .unwrap();
        fs::write(
            outside.join("src/lib.rs"),
            format!(
                "{library}\n#[cfg(test)]\nmod tests {{\n#[test]\n\
                 fn the_helper_is_available_to_dependency_tests() {{\n\
                 assert!(outside_test_helper::value().is_empty());\n}}\n}}\n"
            ),
        )
        .unwrap();
        fs::create_dir_all(outside.join("test-helper/src")).unwrap();
        fs::write(
            outside.join("test-helper/Cargo.toml"),
            "[package]\nname = \"outside-test-helper\"\nversion = \"0.1.0\"\n\
             edition = \"2024\"\n",
        )
        .unwrap();
        fs::write(
            outside.join("test-helper/src/lib.rs"),
            "#![forbid(unsafe_code)]\npub fn value() -> String { String::new() }\n",
        )
        .unwrap();
        fs::write(
            root.join("polyfw.toml"),
            "config-version = \"0.1.0\"\nmin-polyfw-version = \"0.1.0\"\n[bsps.board-family]\nworkspace = \"bsp/test\"\n\
             chip = \"test\"\n\
             [boards.one]\ndescription = \"first board\"\n\
             bsp = \"board-family\"\npackage = \"board-a\"\nbin = \"board-a\"\n\
             [boards.one-alternate]\ndescription = \"same package, another binary\"\n\
             bsp = \"board-family\"\npackage = \"board-a\"\nbin = \"board-a-alternate\"\n\
             [boards.two]\ndescription = \"second board\"\n\
             bsp = \"board-family\"\npackage = \"board-b\"\n",
        )
        .unwrap();
        for directory in [&workspace, &outside] {
            for arguments in [["generate-lockfile", "--offline"], ["fmt", "--all"]] {
                let output = runner::workspace_cargo(directory)
                    .args(arguments)
                    .output()
                    .unwrap();
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
        Context::load(root).unwrap()
    }
}
