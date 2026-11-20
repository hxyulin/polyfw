#![forbid(unsafe_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

mod support;

struct Example {
    root: PathBuf,
}

impl Example {
    fn new(name: &str) -> Self {
        static NEXT_COPY: AtomicUsize = AtomicUsize::new(0);
        let copy = NEXT_COPY.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "polyfw-example-{name}-{}-{copy}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        copy_project(
            &Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples")
                .join(name),
            &root,
        );
        Self { root }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_polyfw"));
        command
            .current_dir(&self.root)
            .args(args)
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("CARGO_BUILD_TARGET")
            .env_remove("CARGO_TARGET_DIR")
            .env_remove("CARGO_BUILD_BUILD_DIR")
            .env_remove("CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUSTFLAGS")
            .env("RUSTUP_TOOLCHAIN", "polyfw-test-unavailable")
            .env("RUSTUP_AUTO_INSTALL", "0")
            .env("CARGO_NET_OFFLINE", "true")
            .env("NO_COLOR", "1");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        succeeded(self.command(args).output().unwrap())
    }

    fn json(&self, args: &[&str]) -> serde_json::Value {
        serde_json::from_slice(&self.run(args).stdout).unwrap()
    }

    fn assert_test_fails(&self, scope: &str, marker: &str) {
        let output = self.command(&["test", scope]).output().unwrap();
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(marker),
            "stdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn add_failing_test(&self, path: &str, marker: &str) {
        let path = self.root.join(path);
        let source = fs::read_to_string(&path).unwrap();
        fs::write(
            path,
            format!("{source}\n#[test]\nfn scope_sentinel() {{\n    panic!({marker:?});\n}}\n"),
        )
        .unwrap();
    }
}

impl Drop for Example {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
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

fn succeeded(output: Output) -> Output {
    assert!(
        output.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn assert_arm_artifact(report: &serde_json::Value, id: &str, profile: &str) {
    let firmware = &report["firmwares"][id];
    assert_eq!(firmware["target"], "thumbv7em-none-eabihf");
    let artifact = firmware["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["profile"] == profile)
        .unwrap();
    assert_eq!(artifact["exists"], true);
    let path = Path::new(artifact["path"].as_str().unwrap());
    assert!(path.is_absolute());
    assert_eq!(
        path,
        Path::new(firmware["target_dir"].as_str().unwrap())
            .join("thumbv7em-none-eabihf")
            .join(if profile == "dev" { "debug" } else { profile })
            .join(firmware["bin"].as_str().unwrap())
    );
    let elf = fs::read(path).unwrap();
    assert!(
        elf.len() >= 52,
        "ELF header missing from {}",
        path.display()
    );
    assert_eq!(&elf[..6], b"\x7fELF\x01\x01", "32-bit little-endian ELF");
    assert_eq!(&elf[16..18], &[2, 0], "ELF must be an executable");
    assert_eq!(&elf[18..20], &[40, 0], "ELF must target ARM");
    let entry = u32::from_le_bytes(elf[24..28].try_into().unwrap());
    assert_eq!(entry & 1, 1, "Cortex-M entry point must use Thumb mode");
    assert!(
        (0x0800_0000..0x0806_0000).contains(&entry),
        "entry point {entry:#x} must lie in the configured flash region"
    );
}

#[test]
fn minimal_example_discovers_selects_and_builds_the_reported_artifact() {
    let example = Example::new("minimal");
    let output = succeeded(
        example
            .command(&["info", "--json"])
            .current_dir(example.root.join("src"))
            .output()
            .unwrap(),
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        Path::new(report["repository_root"].as_str().unwrap()),
        example.root
    );
    assert!(report["saved_selection"].is_null());
    assert_eq!(report["firmwares"]["hello"]["package"], "hello");
    for artifact in report["firmwares"]["hello"]["artifacts"]
        .as_array()
        .unwrap()
    {
        assert_eq!(artifact["exists"], false);
    }

    example.run(&["select", "hello", "--release"]);
    example.run(&["build"]);
    let report = example.json(&["info", "--json"]);
    assert_eq!(report["saved_selection"]["firmware"], "hello");
    assert_eq!(report["saved_selection"]["profile"], "release");
    assert_arm_artifact(&report, "hello", "release");
    assert_eq!(report["platforms"]["dm-mc02"]["cpu"], "cortex-m7");
    assert_eq!(report["platforms"]["dm-mc02"]["chip"], "STM32H723VGTx");
    assert!(!example.root.join(".vscode/launch.json").exists());
    example.run(&["verify", "--all-firmwares"]);
    example.run(&["select", "--clear"]);
    assert!(!example.root.join(".polyfw-state/selected").exists());
}

#[test]
fn multi_firmware_example_switches_and_checks_only_the_selected_scope() {
    let example = Example::new("multi-firmware");
    let report = example.json(&["info", "--json"]);
    assert_eq!(report["firmwares"].as_object().unwrap().len(), 2);
    for id in ["led-demo", "buzzer-demo"] {
        let details = example.json(&["info", "--firmware", id, "--json"]);
        let dependencies = details["firmwares"][id]["local_dependencies"]
            .as_array()
            .unwrap();
        assert_eq!(dependencies.len(), 1);
        assert_eq!(dependencies[0]["package"], "common");
    }

    example.run(&["select", "led-demo", "--release"]);
    example.run(&["build", "--all-firmwares", "--release"]);
    let report = example.json(&["info", "--json"]);
    assert_eq!(report["saved_selection"]["firmware"], "led-demo");
    for id in ["led-demo", "buzzer-demo"] {
        assert_arm_artifact(&report, id, "release");
    }
    example.run(&["select", "buzzer-demo", "--dev"]);
    example.run(&["build"]);
    let report = example.json(&["info", "--json"]);
    assert_eq!(report["saved_selection"]["firmware"], "buzzer-demo");
    assert_eq!(report["saved_selection"]["profile"], "dev");
    assert_arm_artifact(&report, "buzzer-demo", "dev");
    assert_eq!(
        report["firmwares"]["led-demo"]["artifacts"][0]["exists"],
        false
    );
    example.run(&["verify", "--all-firmwares"]);

    example.add_failing_test(
        "firmwares/buzzer-demo/src/lib.rs",
        "unselected firmware test",
    );
    example.run(&["test", "led-demo"]);
    example.assert_test_fails("--all-firmwares", "unselected firmware test");
}

#[test]
fn multi_workspace_example_reports_external_members_and_shared_scopes() {
    let example = Example::new("multi-workspace");
    let report = example.json(&["info", "--json"]);
    assert_eq!(report["firmwares"].as_object().unwrap().len(), 4);
    assert_eq!(report["platforms"]["a"]["registered_firmwares"], 3);
    assert_eq!(report["platforms"]["b"]["registered_firmwares"], 1);
    for (platform, cpu, chip) in [
        ("a", "cortex-m7", "STM32H723VGTx"),
        ("b", "cortex-m4", "STM32F407IG"),
    ] {
        assert_eq!(
            report["platforms"][platform]["target"],
            "thumbv7em-none-eabihf"
        );
        assert_eq!(report["platforms"][platform]["cpu"], cpu);
        assert_eq!(report["platforms"][platform]["chip"], chip);
    }
    for (id, platform, package, expected) in [
        ("first", "a", "first", &["common", "first-only"][..]),
        ("second", "b", "second", &["common", "second-only"][..]),
        ("multi-one", "a", "multi", &["common"][..]),
        ("multi-two", "a", "multi", &["common"][..]),
    ] {
        let details = example.json(&["info", "--firmware", id, "--json"]);
        let firmware = &details["firmwares"][id];
        assert_eq!(firmware["platform"], platform);
        assert_eq!(firmware["package"], package);
        assert_eq!(firmware["bin"], id);
        assert_eq!(
            Path::new(firmware["workspace"].as_str().unwrap()),
            example.root.join("bsp").join(platform)
        );
        assert_eq!(firmware["working_dir"], firmware["workspace"]);
        assert_eq!(
            Path::new(firmware["target_dir"].as_str().unwrap()),
            example.root.join("target/firmware").join(platform)
        );
        let dependencies = firmware["local_dependencies"].as_array().unwrap();
        assert_eq!(
            dependencies
                .iter()
                .map(|dependency| dependency["package"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        for dependency in dependencies {
            let name = dependency["package"].as_str().unwrap();
            assert_eq!(
                Path::new(dependency["manifest_path"].as_str().unwrap()),
                Path::new("shared").join(name).join("Cargo.toml")
            );
        }
    }

    example.run(&["verify", "--all-firmwares"]);
    let cargo = succeeded(
        support::workspace_cargo(&example.root)
            .arg("-vV")
            .output()
            .unwrap(),
    );
    let version = String::from_utf8(cargo.stdout).unwrap();
    let host = version
        .lines()
        .find_map(|line| line.strip_prefix("host: "))
        .unwrap();
    assert!(
        example
            .root
            .join("target")
            .join(host)
            .join("debug")
            .is_dir()
    );
}

#[test]
fn multi_workspace_example_builds_selected_bins_and_all_firmwares() {
    let example = Example::new("multi-workspace");
    example.run(&["select", "multi-one", "--release"]);
    example.run(&["build"]);
    let report = example.json(&["info", "--json"]);
    assert_arm_artifact(&report, "multi-one", "release");
    for id in ["first", "second", "multi-two"] {
        assert_eq!(report["firmwares"][id]["artifacts"][1]["exists"], false);
    }

    example.run(&["select", "multi-two", "--release"]);
    example.run(&["build"]);
    let report = example.json(&["info", "--json"]);
    assert_arm_artifact(&report, "multi-two", "release");
    assert_arm_artifact(&report, "multi-one", "release");

    example.run(&["build", "--all-firmwares", "--release"]);
    let report = example.json(&["info", "--json"]);
    assert_eq!(report["saved_selection"]["firmware"], "multi-two");
    assert_eq!(report["saved_selection"]["profile"], "release");
    for id in ["first", "second", "multi-one", "multi-two"] {
        assert_arm_artifact(&report, id, "release");
    }
}

#[test]
fn multi_workspace_example_tests_only_reachable_shared_libraries() {
    let example = Example::new("multi-workspace");
    example.add_failing_test("shared/first-only/src/lib.rs", "first-only test");
    example.assert_test_fails("first", "first-only test");
    example.run(&["test", "second"]);
    example.add_failing_test("shared/second-only/src/lib.rs", "second-only test");
    example.assert_test_fails("second", "second-only test");
    for id in ["multi-one", "multi-two"] {
        example.run(&["test", id]);
    }

    example.add_failing_test("shared/common/src/lib.rs", "common test");
    for id in ["multi-one", "multi-two"] {
        example.assert_test_fails(id, "common test");
    }
}
