set quiet
set no-exit-message

# List repository recipes (default).
[default]
help:
    just --list

# Run the development CLI; pass arguments directly, or omit them for CLI help.
[positional-arguments]
run *args:
    cargo run --locked -- "$@"

# Regenerate assets/polyfw.schema.json from the current Rust model.
schema:
    just run schema --output assets/polyfw.schema.json

# Check formatting, Clippy and tests, then build the release executable.
check:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked
    cargo build --locked --release

# Format Rust sources and apply Clippy fixes; review the resulting diff.
fix:
    cargo fmt
    cargo clippy --locked --all-targets --fix --allow-dirty --allow-staged
