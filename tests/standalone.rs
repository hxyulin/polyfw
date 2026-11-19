#![forbid(unsafe_code)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

#[test]
fn help_version_and_schema_work_without_a_project_or_rust_tools() {
    let fixture = Fixture::new("project-free");
    fs::write(fixture.root.join("polyfw.toml"), "invalid TOML").unwrap();
    for args in [vec![], vec!["--help"], vec!["help", "build"]] {
        let output = fixture.command().args(args).output().unwrap();
        succeeded(&output);
        assert!(String::from_utf8(output.stdout).unwrap().contains("polyfw"));
    }
    let output = fixture.command().arg("--version").output().unwrap();
    succeeded(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        concat!("polyfw ", env!("CARGO_PKG_VERSION"))
    );
    let output = fixture
        .command()
        .args(["schema", "--project-root", "missing"])
        .output()
        .unwrap();
    succeeded(&output);
    assert_eq!(output.stdout, include_bytes!("../polyfw.schema.json"));
    let output = fixture
        .command()
        .args(["schema", "--output", "polyfw.schema.json"])
        .output()
        .unwrap();
    succeeded(&output);
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read(fixture.root.join("polyfw.schema.json")).unwrap(),
        include_bytes!("../polyfw.schema.json")
    );
}

#[test]
fn a_project_command_outside_a_project_reports_how_to_select_one() {
    let fixture = Fixture::new("missing-project");
    let output = fixture.command().arg("info").output().unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("no polyfw.toml found"), "{error}");
    assert!(error.contains("--project-root <DIR>"), "{error}");
}

#[test]
fn configuration_versions_are_checked_before_tools_state_and_unknown_fields() {
    for (label, declarations, expected) in [
        (
            "missing-versions",
            "",
            "missing required top-level config-version",
        ),
        (
            "missing-minimum",
            "config-version = \"0.1.0\"\n",
            "missing required top-level min-polyfw-version",
        ),
        (
            "wrong-version-type",
            "config-version = 1\nmin-polyfw-version = \"0.1.0\"\n",
            "config-version must be a string",
        ),
        (
            "invalid-version",
            "config-version = \"0.1\"\nmin-polyfw-version = \"0.1.0\"\n",
            "not a full SemVer",
        ),
        (
            "inconsistent-versions",
            "config-version = \"0.1.0\"\nmin-polyfw-version = \"999.0.0\"\n",
            "exceeds config-version",
        ),
        (
            "old-config",
            "config-version = \"0.0.0\"\nmin-polyfw-version = \"0.0.0\"\n",
            "is too old",
        ),
        (
            "old-cli",
            "config-version = \"999.0.0\"\nmin-polyfw-version = \"999.0.0\"\n",
            "Upgrade the CLI",
        ),
    ] {
        let fixture = Fixture::new(label);
        fixture.project(true);
        let path = fixture.root.join("polyfw.toml");
        let source = fs::read_to_string(&path).unwrap();
        let body = source.split_once("[config.vscode]").unwrap().1;
        fs::write(
            &path,
            format!("future-field = true\n{declarations}[config.vscode]{body}"),
        )
        .unwrap();
        fs::create_dir_all(fixture.root.join(".polyfw-state")).unwrap();
        fs::write(
            fixture.root.join(".polyfw-state/selected"),
            "original state",
        )
        .unwrap();
        fs::create_dir_all(fixture.root.join(".vscode")).unwrap();
        fs::write(
            fixture.root.join(".vscode/launch.json"),
            "original editor file",
        )
        .unwrap();

        for args in [
            vec!["info", "--json"],
            vec!["select", "--clear"],
            vec!["reset", "board"],
            vec!["reset", "board", "--select"],
            vec!["build", "--select", "board"],
        ] {
            let output = fixture.command().args(args).output().unwrap();
            assert!(!output.status.success());
            assert!(output.stdout.is_empty());
            let error = String::from_utf8(output.stderr).unwrap();
            assert!(error.contains(expected), "{error}");
            assert!(error.contains(&path.display().to_string()), "{error}");
            assert!(error.contains(env!("CARGO_PKG_VERSION")), "{error}");
            assert!(!error.contains("unknown field"), "{error}");
            assert_eq!(
                fs::read_to_string(fixture.root.join(".polyfw-state/selected")).unwrap(),
                "original state"
            );
            assert_eq!(
                fs::read_to_string(fixture.root.join(".vscode/launch.json")).unwrap(),
                "original editor file"
            );
        }
    }
}

#[test]
fn a_compatible_version_does_not_bypass_strict_configuration_validation() {
    let fixture = Fixture::new("compatible-unknown-field");
    fixture.project(true);
    let path = fixture.root.join("polyfw.toml");
    let source = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("future-field = true\n{source}")).unwrap();
    let output = fixture
        .command()
        .args(["select", "--clear"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("unknown field `future-field`"), "{error}");
}

#[test]
fn clearing_selection_needs_no_cargo_and_obeys_the_projects_vscode_setting() {
    for enabled in [true, false] {
        let fixture = Fixture::new(&format!("clear-{enabled}"));
        fixture.project(enabled);
        fs::create_dir_all(fixture.root.join(".polyfw-state")).unwrap();
        fs::write(fixture.root.join(".polyfw-state/selected"), "invalid state").unwrap();
        fs::create_dir_all(fixture.root.join(".vscode")).unwrap();
        let launch = fixture.root.join(".vscode/launch.json");
        fs::write(&launch, "handwritten configuration").unwrap();
        let output = fixture
            .command()
            .args(["select", "--clear"])
            .output()
            .unwrap();
        succeeded(&output);
        assert!(!fixture.root.join(".polyfw-state/selected").exists());
        if enabled {
            assert!(!launch.exists());
        } else {
            assert_eq!(
                fs::read_to_string(launch).unwrap(),
                "handwritten configuration"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn listing_probes_uses_only_probe_rs_and_ignores_project_configuration() {
    let fixture = Fixture::new("probes");
    fixture.probe();
    fs::write(fixture.root.join("polyfw.toml"), "invalid TOML").unwrap();
    let output = fixture
        .command()
        .args(["probes", "--project-root", "missing"])
        .output()
        .unwrap();
    succeeded(&output);
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "list\n");
}

#[cfg(unix)]
#[test]
fn resetting_needs_no_cargo_unless_saving_an_enabled_debug_configuration() {
    for enabled in [true, false] {
        let fixture = Fixture::new(&format!("reset-{enabled}"));
        fixture.project(enabled);
        fixture.probe();
        let output = fixture.command().args(["reset", "board"]).output().unwrap();
        succeeded(&output);
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            "reset\n--chip\ntest-chip\n"
        );
        assert!(!fixture.root.join(".polyfw-state/selected").exists());

        let output = fixture
            .command()
            .args(["reset", "board", "--select"])
            .output()
            .unwrap();
        if enabled {
            assert!(!output.status.success());
            assert!(!fixture.root.join(".polyfw-state/selected").exists());
        } else {
            succeeded(&output);
            assert_eq!(
                fs::read_to_string(fixture.root.join(".polyfw-state/selected")).unwrap(),
                "board = \"board\"\n"
            );
        }
        assert!(!fixture.root.join(".vscode/launch.json").exists());
    }
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "polyfw-standalone-{label}-{} with spaces",
            std::process::id()
        ));
        fs::create_dir_all(root.join("tools")).unwrap();
        Self { root }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_polyfw"));
        command
            .current_dir(&self.root)
            .env("PATH", self.root.join("tools"))
            .env("RUSTUP_TOOLCHAIN", "polyfw-test-unavailable")
            .env("RUSTUP_AUTO_INSTALL", "0")
            .env("CARGO_NET_OFFLINE", "true")
            .env("NO_COLOR", "1");
        command
    }

    fn project(&self, vscode: bool) {
        fs::write(
            self.root.join("polyfw.toml"),
            format!(
                "config-version = \"0.1.0\"\nmin-polyfw-version = \"0.1.0\"\n[config.vscode]\nenabled = {vscode}\n\
                 [bsps.model]\nworkspace = \"missing\"\nchip = \"test-chip\"\n\
                 [boards.board]\ndescription = \"Fixture\"\nbsp = \"model\"\n\
                 package = \"application\"\n"
            ),
        )
        .unwrap();
    }

    #[cfg(unix)]
    fn probe(&self) {
        use std::os::unix::fs::PermissionsExt;

        let path = self.root.join("tools/probe-rs");
        fs::write(&path, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
