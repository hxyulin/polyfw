#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

mod support;

struct Project {
    root: PathBuf,
    project: PathBuf,
    workspace: PathBuf,
}

impl Project {
    fn new(label: &str) -> Self {
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("polyfw-platforms-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let project = root.join("project");
        let workspace = root.join("workspace");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(workspace.join(".cargo")).unwrap();
        for directory in [&root, &workspace] {
            fs::write(
                directory.join("rust-toolchain.toml"),
                include_str!("../rust-toolchain.toml"),
            )
            .unwrap();
        }
        fs::write(
            workspace.join("Cargo.toml"),
            "[workspace]\nmembers = [\"led-demo\", \"buzzer-demo\"]\nresolver = \"3\"\n",
        )
        .unwrap();
        for name in ["led-demo", "buzzer-demo"] {
            let directory = workspace.join(name);
            fs::create_dir_all(directory.join("src")).unwrap();
            fs::write(directory.join("Cargo.toml"), format!("[package]\nname = {name:?}\nversion = \"0.1.0\"\nedition = \"2024\"\n[[bin]]\nname = \"demo-image\"\npath = \"src/main.rs\"\n")).unwrap();
            fs::write(
                directory.join("src/lib.rs"),
                "#![forbid(unsafe_code)]\npub fn value() -> bool { true }\n",
            )
            .unwrap();
            fs::write(
                directory.join("src/main.rs"),
                format!("fn main() {{ println!({name:?}); }}\n"),
            )
            .unwrap();
        }
        succeed(
            support::workspace_cargo(&workspace)
                .args(["generate-lockfile", "--offline"])
                .output()
                .unwrap(),
        );
        let fixture = Self {
            root,
            project,
            workspace,
        };
        fixture.config("", "", "");
        fixture
    }

    fn config(&self, platform: &str, led: &str, buzzer: &str) {
        fs::write(self.project.join("polyfw.toml"), format!(
            "config-version = \"0.2.0\"\nmin-polyfw-version = \"0.2.0\"\n\
             [platforms.demo-board]\ntarget = \"host-tuple\"\ncpu = \"native\"\nchip = \"test-chip\"\nworking-dir = \"../workspace\"\n{platform}\n\
             [firmwares.led-demo]\ndescription = \"LED\"\nplatform = \"demo-board\"\npackage = \"led-demo\"\n{led}\n\
             [firmwares.buzzer-demo]\ndescription = \"Buzzer\"\nplatform = \"demo-board\"\npackage = \"buzzer-demo\"\n{buzzer}\n"
        )).unwrap();
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_polyfw"));
        command
            .current_dir(&self.project)
            .args(args)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_BUILD_DIR")
            .env("CARGO_NET_OFFLINE", "true")
            .env("RUSTUP_AUTO_INSTALL", "0")
            .env("NO_COLOR", "1");
        command
    }

    fn info(&self) -> serde_json::Value {
        serde_json::from_slice(&succeed(self.command(&["info", "--json"]).output().unwrap()).stdout)
            .unwrap()
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn succeed(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn directory_inheritance_and_native_defaults_use_cargos_actual_workspace_and_outputs() {
    let fixture = Project::new("directories");
    let buzzer = fixture.root.join("images/buzzer");
    fixture.config(
        "build-dir = \"../cache\"\ntarget-dir = \"../images/default\"",
        "target-dir = \"../images/led\"",
        &format!(
            "target-dir = {}",
            toml::Value::String(buzzer.display().to_string())
        ),
    );
    for name in ["cache", "images/led", "images/buzzer"] {
        fs::create_dir_all(fixture.root.join(name)).unwrap();
    }
    let info = fixture.info();
    for (id, output) in [
        ("led-demo", fixture.root.join("images/led")),
        ("buzzer-demo", buzzer),
    ] {
        let firmware = &info["firmwares"][id];
        assert_eq!(
            Path::new(firmware["working_dir"].as_str().unwrap()),
            fixture.workspace
        );
        assert_eq!(
            Path::new(firmware["workspace"].as_str().unwrap()),
            fixture.workspace
        );
        assert_eq!(
            Path::new(firmware["build_dir"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            fixture.root.join("cache")
        );
        assert_eq!(
            Path::new(firmware["target_dir"].as_str().unwrap())
                .canonicalize()
                .unwrap(),
            output
        );
        assert_eq!(firmware["check_platform"], true);
    }
    fixture.config("", "", "");
    fs::write(
        fixture.workspace.join(".cargo/config.toml"),
        "[build]\ntarget-dir = \"../cargo-output\"\nbuild-dir = \"../cargo-cache\"\n",
    )
    .unwrap();
    for name in ["cargo-output", "cargo-cache"] {
        fs::create_dir_all(fixture.root.join(name)).unwrap();
    }
    let info = fixture.info();
    assert_eq!(
        Path::new(
            info["firmwares"]["led-demo"]["target_dir"]
                .as_str()
                .unwrap()
        )
        .canonicalize()
        .unwrap(),
        fixture.root.join("cargo-output")
    );
    assert_eq!(
        Path::new(info["firmwares"]["led-demo"]["build_dir"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        fixture.root.join("cargo-cache")
    );
}

#[test]
fn consistency_defaults_to_true_and_false_overrides_without_analyzing_unselected_firmware() {
    let fixture = Project::new("checking");
    let output = fixture
        .command(&["build", "led-demo"])
        .env("RUSTFLAGS", "-Ctarget-cpu=polyfw-mismatched-cpu")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("effective CPU \"polyfw-mismatched-cpu\"")
    );
    let output = fixture
        .command(&["build", "led-demo"])
        .env("CARGO_ENCODED_RUSTFLAGS", "")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("cannot determine an explicit effective target CPU")
    );
    fixture.config("check-platform = true", "check-platform = false", "");
    let output = succeed(
        fixture
            .command(&["build", "led-demo"])
            .env("CARGO_ENCODED_RUSTFLAGS", "")
            .output()
            .unwrap(),
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("consistency check"));
    assert_eq!(
        fixture.info()["firmwares"]["led-demo"]["check_platform"],
        false
    );
    assert_eq!(
        fixture.info()["firmwares"]["buzzer-demo"]["check_platform"],
        true
    );
    fixture.config("check-platform = false", "check-platform = true", "");
    let output = fixture
        .command(&["build", "led-demo"])
        .env("RUSTFLAGS", "")
        .output()
        .unwrap();
    assert!(!output.status.success());
    succeed(
        fixture
            .command(&["test", "led-demo"])
            .env("RUSTFLAGS", "")
            .output()
            .unwrap(),
    );
}

#[test]
fn disabled_checking_leaves_includes_and_conditional_flags_to_native_cargo() {
    let fixture = Project::new("native-config");
    fs::write(fixture.workspace.join(".cargo/config.toml"), "include = [\"flags.toml\"]\n[target.'cfg(all())']\nrustflags = [\"--cfg\", \"conditional_flags\"]\n").unwrap();
    fs::write(
        fixture.workspace.join(".cargo/flags.toml"),
        "[build]\nrustflags = \"--cfg suppressed_build_flags\"\n",
    )
    .unwrap();
    for name in ["led-demo", "buzzer-demo"] {
        fs::write(fixture.workspace.join(name).join("src/lib.rs"), "#[cfg(not(conditional_flags))]\ncompile_error!(\"native conditional flags were lost\");\npub fn value() -> bool { true }\n").unwrap();
    }
    let output = fixture.command(&["build", "led-demo"]).output().unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("cannot reliably verify configuration includes"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    fixture.config("check-platform = true", "check-platform = false", "");
    succeed(fixture.command(&["build", "led-demo"]).output().unwrap());
}

#[test]
fn shared_intermediates_keep_same_named_final_images_independent() {
    let fixture = Project::new("split-outputs");
    fixture.config(
        "build-dir = \"../cache\"",
        "target-dir = \"../images/led\"",
        "target-dir = \"../images/buzzer\"",
    );
    for id in ["led-demo", "buzzer-demo", "led-demo"] {
        succeed(fixture.command(&["build", id]).output().unwrap());
    }
    let info = fixture.info();
    for id in ["led-demo", "buzzer-demo"] {
        let image = info["firmwares"][id]["artifacts"][0]["path"]
            .as_str()
            .unwrap();
        let output = succeed(Command::new(image).output().unwrap());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), id);
        assert_eq!(info["firmwares"][id]["artifacts"][0]["exists"], true);
    }
}

#[test]
fn external_members_can_supply_the_working_directory_without_changing_workspace_ownership() {
    let fixture = Project::new("external-member");
    let member = fixture.root.join("external/led-demo");
    fs::create_dir_all(member.parent().unwrap()).unwrap();
    fs::rename(fixture.workspace.join("led-demo"), &member).unwrap();
    let manifest = member.join("Cargo.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("[package]", "[package]\nworkspace = \"../../workspace\""),
    )
    .unwrap();
    fs::write(
        fixture.workspace.join("Cargo.toml"),
        "[workspace]\nmembers = [\"../external/led-demo\", \"buzzer-demo\"]\nresolver = \"3\"\n",
    )
    .unwrap();
    succeed(
        support::workspace_cargo(&fixture.workspace)
            .args(["generate-lockfile", "--offline"])
            .output()
            .unwrap(),
    );
    fixture.config(
        "",
        &format!(
            "working-dir = {}",
            toml::Value::String(member.display().to_string())
        ),
        "",
    );
    let info = fixture.info();
    assert_eq!(
        Path::new(info["firmwares"]["led-demo"]["workspace"].as_str().unwrap()),
        fixture.workspace
    );
    assert_eq!(
        Path::new(
            info["firmwares"]["led-demo"]["working_dir"]
                .as_str()
                .unwrap()
        ),
        member
    );
    succeed(fixture.command(&["build", "led-demo"]).output().unwrap());
}
