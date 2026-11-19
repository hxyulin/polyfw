use std::path::Path;
use std::process::Command;

pub fn workspace_cargo(directory: &Path) -> Command {
    let mut command = Command::new("cargo");
    command
        .current_dir(directory)
        .env_remove("RUSTUP_TOOLCHAIN");
    command
}
