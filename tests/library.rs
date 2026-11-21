#![forbid(unsafe_code)]

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use polyfw::{Profile, Project};

#[path = "support/cargo_env.rs"]
mod cargo_env;

/// Set in the child process that runs a test in an isolated Cargo environment.
const ISOLATED: &str = "POLYFW_LIBRARY_TEST_ISOLATED";

/// Runs `test` in a child test process without ambient Cargo flags, target or output
/// directories, since the library runs Cargo with the caller's environment. Returns false
/// in the parent, which only checks the child's result.
fn in_isolated_cargo_environment(test: &str) -> bool {
    if std::env::var_os(ISOLATED).is_some() {
        return true;
    }
    let output =
        cargo_env::isolate_cargo_environment(Command::new(std::env::current_exe().unwrap()))
            .args(["--exact", test, "--test-threads=1"])
            .env(ISOLATED, "1")
            .output()
            .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stdout}\n{stderr}");
    assert!(stdout.contains("1 passed"), "{stdout}\n{stderr}");
    false
}

/// A copy of an example project at `root`, inside an otherwise empty directory.
struct Example {
    outer: PathBuf,
    root: PathBuf,
}

impl Example {
    fn new(name: &str) -> Self {
        let outer = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("polyfw-library-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&outer);
        let root = outer.join("project");
        copy_project(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples")
                .join(name),
            &root,
        );
        Self { outer, root }
    }
}

impl Drop for Example {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.outer);
    }
}

fn copy_project(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if matches!(
            entry.file_name().to_str(),
            Some("target" | ".polyfw-state" | ".vscode")
        ) {
            continue;
        }
        let path = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_project(&entry.path(), &path);
        } else {
            fs::copy(entry.path(), path).unwrap();
        }
    }
}

#[test]
fn a_project_reports_firmwares_selection_and_a_buildable_command() {
    if !in_isolated_cargo_environment(
        "a_project_reports_firmwares_selection_and_a_buildable_command",
    ) {
        return;
    }
    let example = Example::new("multi-firmware");
    let project = Project::discover(example.root.join("firmwares/led-demo/src")).unwrap();
    assert_eq!(project.root(), example.root);

    let ids: Vec<String> = project.firmwares().map(|f| f.id().to_owned()).collect();
    assert_eq!(ids, ["buzzer-demo", "led-demo"]);
    let platforms: Vec<String> = project.platforms().map(|p| p.id().to_owned()).collect();
    assert_eq!(platforms, ["dm-mc02"]);

    let firmware = project.firmware("led-demo").unwrap();
    let platform = firmware.platform();
    assert_eq!(platform.chip(), "STM32H723VGTx");
    assert_eq!(platform.cpu(), "cortex-m7");
    assert_eq!(firmware.target(), "thumbv7em-none-eabihf");
    assert_eq!(firmware.package(), "led-demo");
    assert_eq!(firmware.bin(), "led-demo");
    assert_eq!(firmware.workspace(), example.root);

    let release: Profile = "release".parse().unwrap();
    let elf = firmware.elf_path(Some(&release));
    assert_eq!(
        elf,
        firmware
            .target_dir()
            .join("thumbv7em-none-eabihf/release/led-demo")
    );
    assert!(
        firmware.elf_path(None).ends_with("debug/led-demo"),
        "{}",
        firmware.elf_path(None).display()
    );

    let error = project.firmware("missing").unwrap_err();
    assert!(
        error.to_string().contains("unknown firmware: missing"),
        "{error}"
    );

    assert_eq!(project.saved_selection().unwrap(), None);
    project.save_selection("led-demo", Some(&release)).unwrap();
    let saved = project.saved_selection().unwrap().unwrap();
    assert_eq!(saved.firmware(), "led-demo");
    assert_eq!(saved.profile(), Some(&release));
    project.clear_selection().unwrap();
    assert_eq!(project.saved_selection().unwrap(), None);
    project.clear_selection().unwrap();

    firmware.check_platform_consistency().unwrap();
    let mut command = firmware.build_command(Some(&release));
    assert_eq!(command.get_current_dir(), Some(firmware.working_dir()));
    let args: Vec<&OsStr> = command.get_args().collect();
    for expected in [
        "build",
        "-p",
        "led-demo",
        "--locked",
        "--profile",
        "release",
    ] {
        assert!(args.contains(&OsStr::new(expected)), "{args:?}");
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    // ELF files start with 0x7f followed by "ELF".
    assert_eq!(&fs::read(&elf).unwrap()[..4], b"\x7fELF");
}

#[test]
fn clearing_a_selection_reports_every_file_it_cannot_remove() {
    if !in_isolated_cargo_environment("clearing_a_selection_reports_every_file_it_cannot_remove") {
        return;
    }
    let example = Example::new("multi-firmware");
    let config = example.root.join("polyfw.toml");
    let source = fs::read_to_string(&config).unwrap();
    fs::write(&config, source.replace("enabled = false", "enabled = true")).unwrap();
    let project = Project::open(&example.root).unwrap();
    project.save_selection("led-demo", None).unwrap();

    let files = [".vscode/launch.json", ".polyfw-state/selected"];
    for file in files {
        let path = example.root.join(file);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
    }
    let error = project.clear_selection().unwrap_err().to_string();
    for file in files {
        assert!(error.contains(file), "{error}");
    }
}

#[test]
fn discovery_resolves_parent_components_before_searching() {
    if !in_isolated_cargo_environment("discovery_resolves_parent_components_before_searching") {
        return;
    }
    let example = Example::new("multi-firmware");
    let project = Project::discover(example.root.join("firmwares/..")).unwrap();
    assert_eq!(project.root(), example.root);
    // `project/..` is the outer directory, which has no polyfw.toml above it.
    let error = Project::discover(example.root.join("..")).unwrap_err();
    assert!(
        error.to_string().contains("no polyfw.toml found"),
        "{error}"
    );
}

#[test]
fn opening_a_directory_without_a_configuration_fails() {
    let directory =
        std::env::temp_dir().join(format!("polyfw-library-empty-{}", std::process::id()));
    fs::create_dir_all(&directory).unwrap();
    let error = Project::open(&directory).unwrap_err();
    fs::remove_dir_all(&directory).unwrap();
    assert!(error.to_string().contains("polyfw.toml"), "{error}");
}
