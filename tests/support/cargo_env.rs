use std::process::Command;

pub(crate) fn isolate_cargo_environment(mut command: Command) -> Command {
    for key in [
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "RUSTDOCFLAGS",
        "CARGO_ENCODED_RUSTDOCFLAGS",
        "CARGO_BUILD_RUSTDOCFLAGS",
        "CARGO_BUILD_TARGET",
        "CARGO_TARGET_DIR",
        "CARGO_BUILD_TARGET_DIR",
        "CARGO_BUILD_BUILD_DIR",
    ] {
        command.env_remove(key);
    }

    for (key, _) in std::env::vars_os() {
        let key = if cfg!(windows) {
            key.to_ascii_uppercase()
        } else {
            key
        };
        let Some(key) = key.to_str() else {
            continue;
        };
        let target_flags = key.starts_with("CARGO_TARGET_")
            && (key.ends_with("_RUSTFLAGS") || key.ends_with("_RUSTDOCFLAGS"));
        let profile_flags = key.starts_with("CARGO_PROFILE_") && key.ends_with("_RUSTFLAGS");
        if target_flags || profile_flags {
            command.env_remove(key);
        }
    }
    command
}
