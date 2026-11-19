#![forbid(unsafe_code)]

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[test]
fn firmware_paths_and_cpu_information_follow_the_effective_cargo_configuration() {
    let root = std::env::temp_dir().join(format!("polyfw-config-{}", std::process::id()));
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join(".cargo")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"application\"\nversion = \"0.1.0\"\n[workspace]\n",
    )
    .unwrap();
    fs::write(root.join("src/lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        root.join("rust-toolchain.toml"),
        include_str!("../rust-toolchain.toml"),
    )
    .unwrap();
    fs::write(
        root.join(".cargo/config.toml"),
        "[build]\ntarget = \"thumbv7em-none-eabihf\"\n\
         [target.thumbv7em-none-eabihf]\nrustflags = \"-C target-cpu=cortex-m7\"\n",
    )
    .unwrap();
    fs::write(
        root.join("polyfw.toml"),
        "config-version = \"0.1.0\"\nmin-polyfw-version = \"0.1.0\"\n[bsps.model]\nworkspace = \".\"\nchip = \"test-chip\"\n\
         [boards.board]\ndescription = \"Fixture\"\nbsp = \"model\"\n\
         package = \"application\"\n",
    )
    .unwrap();
    let info = |environment: &[(&str, &str)]| {
        let output = Command::new(env!("CARGO_BIN_EXE_polyfw"))
            .arg("--project-root")
            .arg(&root)
            .args(["info", "--json"])
            .env_remove("CARGO_BUILD_TARGET")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env_remove("RUSTFLAGS")
            .env_remove("CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUSTFLAGS")
            .envs(environment.iter().copied())
            .output()
            .unwrap();
        succeeded(&output);
        serde_json::from_slice::<serde_json::Value>(&output.stdout).unwrap()
    };
    let configured = info(&[]);
    assert_eq!(
        configured["bsps"]["model"]["target"],
        "thumbv7em-none-eabihf"
    );
    assert_eq!(configured["bsps"]["model"]["cpu"], "cortex-m7");
    let overridden = info(&[
        ("CARGO_BUILD_TARGET", "thumbv8m.main-none-eabihf"),
        ("RUSTFLAGS", "--codegen=target-cpu=cortex-m33"),
    ]);
    assert_eq!(
        overridden["bsps"]["model"]["target"],
        "thumbv8m.main-none-eabihf"
    );
    assert_eq!(overridden["bsps"]["model"]["cpu"], "cortex-m33");
    let artifact = &overridden["boards"]["board"]["artifacts"][0]["path"];
    assert!(
        Path::new(artifact.as_str().unwrap())
            .ends_with("target/firmware/model/thumbv8m.main-none-eabihf/debug/application")
    );
    assert!(info(&[("RUSTFLAGS", "")])["bsps"]["model"]["cpu"].is_null());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cargo_commands_use_each_workspaces_pin_instead_of_the_tools_inherited_toolchain() {
    let root = std::env::temp_dir()
        .canonicalize()
        .unwrap()
        .join(format!("polyfw-toolchains-{}", std::process::id()));
    let firmware = root.join("firmware");
    let common = root.join("common");
    let toolchain = Command::new("rustup")
        .args(["show", "active-toolchain"])
        .output()
        .unwrap();
    succeeded(&toolchain);
    let toolchain = String::from_utf8(toolchain.stdout).unwrap();
    let toolchain = toolchain.split_whitespace().next().unwrap();
    let host = Command::new("rustc")
        .args(["--print", "host-tuple"])
        .output()
        .unwrap();
    succeeded(&host);
    let host = String::from_utf8(host.stdout).unwrap();
    let host = host.trim();

    for project in [&firmware, &common] {
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(project.join(".cargo")).unwrap();
        fs::write(
            project.join("rust-toolchain.toml"),
            format!("[toolchain]\nchannel = {toolchain:?}\n"),
        )
        .unwrap();
        fs::write(
            project.join(".cargo/config.toml"),
            format!(
                "[build]\ntarget = {host:?}\n\
                 [target.'cfg(target_family = \"unix\")']\nrustflags = \"--cfg workspace_pin\"\n"
            ),
        )
        .unwrap();
        let package = project.file_name().unwrap().to_str().unwrap();
        fs::write(
            project.join("Cargo.toml"),
            format!(
                "[package]\nname = {package:?}\nversion = \"0.1.0\"\n\
                 edition = \"2024\"\n[workspace]\n"
            ),
        )
        .unwrap();
        fs::write(project.join("src/lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
    }
    let manifest = firmware.join("Cargo.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest).unwrap()
            + "[dependencies]\ncommon = { path = \"../common\" }\n",
    )
    .unwrap();
    fs::write(firmware.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(
        firmware.join("polyfw.toml"),
        "config-version = \"0.1.0\"\nmin-polyfw-version = \"0.1.0\"\n[config]\nlocal-dependencies = [\"../common\"]\n\
             [bsps.model]\nworkspace = \".\"\n\
             chip = \"test-chip\"\n\
             [boards.board]\ndescription = \"Fixture\"\nbsp = \"model\"\n\
             package = \"firmware\"\n",
    )
    .unwrap();
    for project in [&firmware, &common] {
        succeeded(
            &Command::new("cargo")
                .current_dir(project)
                .args(["generate-lockfile", "--offline"])
                .output()
                .unwrap(),
        );
    }

    for action in ["verify", "build"] {
        succeeded(
            &Command::new(env!("CARGO_BIN_EXE_polyfw"))
                .current_dir(&common)
                .arg("--project-root")
                .arg(&firmware)
                .args([action, "board"])
                .env("RUSTUP_TOOLCHAIN", "polyfw-test-unavailable")
                .env("RUSTC", "rustc")
                .env("RUSTUP_AUTO_INSTALL", "0")
                .env("CARGO_NET_OFFLINE", "true")
                .output()
                .unwrap(),
        );
    }
    assert!(
        firmware
            .join("target/firmware/model")
            .join(host)
            .join("debug")
            .join(format!("firmware{}", std::env::consts::EXE_SUFFIX))
            .is_file()
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn schema_is_printed_or_written_without_loading_the_operating_project() {
    let root = std::env::temp_dir().join(format!("polyfw-schema-{}", std::process::id()));
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("polyfw.toml"), "invalid registry").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_polyfw"))
        .current_dir(&root)
        .args(["schema", "--project-root", "missing-project"])
        .output()
        .unwrap();
    succeeded(&output);
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        include_str!("../polyfw.schema.json")
    );
    assert!(!root.join("polyfw.schema.json").exists());
    let output = Command::new(env!("CARGO_BIN_EXE_polyfw"))
        .current_dir(&root)
        .args(["schema", "--output", "schema files/project.json"])
        .output()
        .unwrap();
    succeeded(&output);
    assert!(output.stdout.is_empty());
    assert_eq!(
        fs::read_to_string(root.join("schema files/project.json")).unwrap(),
        include_str!("../polyfw.schema.json")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn one_tool_keeps_two_projects_selections_and_debug_files_independent() {
    let root = std::env::temp_dir().join(format!("polyfw-projects-{}", std::process::id()));
    let firmware = root.join("firmware");
    let bsp = root.join("bsp");
    for project in [&firmware, &bsp] {
        fs::create_dir_all(project.join("src")).unwrap();
        fs::create_dir_all(project.join(".cargo")).unwrap();
        fs::write(
            project.join(".cargo/config.toml"),
            "[build]\ntarget = \"thumbv7em-none-eabihf\"\n",
        )
        .unwrap();
        fs::write(
            project.join("Cargo.toml"),
            "[package]\nname = \"application\"\nversion = \"0.1.0\"\n[workspace]\n",
        )
        .unwrap();
        fs::write(project.join("src/lib.rs"), "#![forbid(unsafe_code)]\n").unwrap();
        fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
        fs::write(
            project.join("polyfw.toml"),
            "config-version = \"0.1.0\"\nmin-polyfw-version = \"0.1.0\"\n[bsps.model]\nworkspace = \".\"\n\
             chip = \"STM32H723VGTx\"\n\
             [boards.board]\ndescription = \"Fixture\"\nbsp = \"model\"\n\
             package = \"application\"\n",
        )
        .unwrap();
    }
    let select = |caller: &Path, project: &Path| {
        Command::new(env!("CARGO_BIN_EXE_polyfw"))
            .current_dir(caller)
            .arg("--project-root")
            .arg(root.join("missing"))
            .arg("--project-root")
            .arg(project)
            .args(["select", "board", "--release"])
            .output()
            .unwrap()
    };
    succeeded(&select(&bsp, &firmware));
    assert!(firmware.join(".polyfw-state/selected").is_file());
    assert!(!bsp.join(".polyfw-state/selected").exists());
    assert!(!bsp.join(".vscode/launch.json").exists());
    succeeded(&select(&firmware, &bsp));
    let saved = fs::read(bsp.join(".polyfw-state/selected")).unwrap();
    let launch = fs::read(bsp.join(".vscode/launch.json")).unwrap();
    let clear = Command::new(env!("CARGO_BIN_EXE_polyfw"))
        .current_dir(&bsp)
        .arg("--project-root")
        .arg(&firmware)
        .args(["select", "--clear"])
        .output()
        .unwrap();
    succeeded(&clear);
    assert!(!firmware.join(".polyfw-state/selected").exists());
    assert!(!firmware.join(".vscode/launch.json").exists());
    assert_eq!(fs::read(bsp.join(".polyfw-state/selected")).unwrap(), saved);
    assert_eq!(fs::read(bsp.join(".vscode/launch.json")).unwrap(), launch);
    fs::remove_dir_all(root).unwrap();
}

fn succeeded(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
