use std::path::{Path, PathBuf};

use cargo_config2::{Config, ResolveOptions, TargetTripleRef};

use crate::{Result, runner};

pub(crate) struct Configuration {
    pub(crate) target: String,
    pub(crate) cpu: Option<String>,
}

pub(crate) fn resolve(workspace: &Path) -> Result<Configuration> {
    let output = runner::workspace_command(workspace, "rustup")
        .args(["which", "cargo"])
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "could not select Cargo for {}: {}",
            workspace.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        )
        .into());
    }
    let cargo = PathBuf::from(String::from_utf8(output.stdout)?.trim());
    // An absolute Cargo path also selects its adjacent rustc for configuration probes.
    let mut config =
        Config::load_with_options(workspace, ResolveOptions::default().cargo(cargo.clone()))?;
    // A bare RUSTC would otherwise invoke rustup with the tool's inherited override.
    if let Some(rustc) = &mut config.build.rustc
        && (rustc == Path::new("rustc") || rustc == Path::new("rustc.exe"))
    {
        *rustc = cargo.with_file_name(format!("rustc{}", std::env::consts::EXE_SUFFIX));
    }
    read(&config).map_err(|error| {
        format!(
            "invalid Cargo configuration in {}: {error}",
            workspace.display()
        )
        .into()
    })
}

fn read(config: &Config) -> Result<Configuration> {
    let requested = config.build_target_for_cli(std::iter::empty::<&str>())?;
    if requested.len() != 1 {
        return Err(format!(
            "expected one explicit build.target (or CARGO_BUILD_TARGET), got {requested:?}"
        )
        .into());
    }
    let targets = config.build_target_for_config(std::iter::empty::<&str>())?;
    let target: TargetTripleRef<'_> = if targets[0].triple() == "host-tuple" {
        config.host_triple()?.into()
    } else {
        (&targets[0]).into()
    };
    let flags = config.rustflags(&target)?.unwrap_or_default();
    Ok(Configuration {
        target: target.triple().to_owned(),
        cpu: target_cpu(&flags.flags)?,
    })
}

fn target_cpu(flags: &[String]) -> Result<Option<String>> {
    let mut cpu = None;
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        if flag.starts_with('@') {
            return Err("cannot determine the target CPU from rustflags response files".into());
        }
        let option = if flag == "-C" || flag == "--codegen" {
            flags.next().map(String::as_str)
        } else {
            flag.strip_prefix("-C")
                .or_else(|| flag.strip_prefix("--codegen="))
        };
        if let Some(("target-cpu" | "target_cpu", value)) =
            option.and_then(|option| option.split_once('='))
        {
            if value.is_empty() {
                return Err("target-cpu must not be empty".into());
            }
            cpu = Some(value.to_owned());
        }
    }
    Ok(cpu)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn the_last_cpu_option_wins_and_other_flags_do_not_choose_a_cpu() {
        for options in [
            "-C target-cpu=cortex-m4",
            "-Ctarget-cpu=cortex-m4",
            "--codegen target-cpu=cortex-m4",
            "--codegen=target-cpu=cortex-m4",
            "-Ctarget_cpu=cortex-m4",
        ] {
            let flags = format!("-Ctarget-cpu=cortex-m7 {options} -Clink-arg=-Tlink.x");
            let flags = flags
                .split_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>();
            assert_eq!(target_cpu(&flags).unwrap().as_deref(), Some("cortex-m4"));
        }
        assert_eq!(target_cpu(&["-Clink-arg=-Tlink.x".into()]).unwrap(), None);
        assert!(target_cpu(&["@compiler-flags".into()]).is_err());
        assert!(target_cpu(&["-Ctarget-cpu=".into()]).is_err());
    }

    #[test]
    fn firmware_requires_one_explicit_target_and_host_tuple_uses_the_real_directory() {
        let root = fixture("targets");
        for target in ["", "target = []", "target = [\"first\", \"second\"]"] {
            fs::write(
                root.join(".cargo/config.toml"),
                format!("[build]\n{target}\n"),
            )
            .unwrap();
            assert!(read(&isolated(&root, &[])).is_err());
        }
        fs::write(
            root.join(".cargo/config.toml"),
            "[build]\ntarget = \"host-tuple\"\n",
        )
        .unwrap();
        let config = read(&isolated(&root, &[])).unwrap();
        assert_eq!(config.target, "x86_64-unknown-linux-gnu");
        assert_eq!(config.cpu, None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inner_string_flags_replace_the_parent_and_environment_flags_keep_their_precedence() {
        let root = fixture("hierarchy");
        let child = root.join("child");
        fs::create_dir_all(child.join(".cargo")).unwrap();
        for (directory, cpu) in [(&root, "cortex-m7"), (&child, "cortex-m4")] {
            fs::write(
                directory.join(".cargo/config.toml"),
                format!(
                    "[build]\ntarget = \"thumbv7em-none-eabihf\"\n\
                     [target.thumbv7em-none-eabihf]\n\
                     rustflags = \"-Ctarget-cpu={cpu} -Clink-arg=-Tlink.x\"\n"
                ),
            )
            .unwrap();
        }
        assert_eq!(
            read(&isolated(&child, &[])).unwrap().cpu.as_deref(),
            Some("cortex-m4")
        );
        let config = isolated(
            &child,
            &[(
                "CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUSTFLAGS",
                "-Ctarget-cpu=cortex-m7",
            )],
        );
        assert_eq!(read(&config).unwrap().cpu.as_deref(), Some("cortex-m7"));
        let flags = config
            .rustflags("thumbv7em-none-eabihf")
            .unwrap()
            .unwrap()
            .flags;
        assert_eq!(
            flags
                .iter()
                .filter(|flag| *flag == "-Clink-arg=-Tlink.x")
                .count(),
            1
        );
        let config = isolated(&child, &[("RUSTFLAGS", "-Ctarget-cpu=cortex-m33")]);
        assert_eq!(read(&config).unwrap().cpu.as_deref(), Some("cortex-m33"));
        let config = isolated(
            &child,
            &[
                ("RUSTFLAGS", "-Ctarget-cpu=cortex-m33"),
                ("CARGO_ENCODED_RUSTFLAGS", ""),
            ],
        );
        assert_eq!(read(&config).unwrap().cpu, None);
        fs::remove_dir_all(root).unwrap();
    }

    fn isolated(root: &Path, environment: &[(&str, &str)]) -> Config {
        Config::load_with_options(
            root,
            ResolveOptions::default()
                .cargo_home(None::<PathBuf>)
                .env(environment.iter().copied())
                .host_triple("x86_64-unknown-linux-gnu"),
        )
        .unwrap()
    }

    fn fixture(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "polyfw-cargo-config-{label}-{}",
            std::process::id()
        ));
        fs::create_dir_all(root.join(".cargo")).unwrap();
        root
    }
}
