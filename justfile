set quiet
set no-exit-message

[default]
help:
    just --list

schema:
    cargo run --locked -- schema --output polyfw.schema.json

check:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings
    cargo test --locked
    cargo build --locked --release

fix:
    cargo fmt
    cargo clippy --locked --all-targets --fix --allow-dirty --allow-staged
