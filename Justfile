# Note: help messages should be 1 line long as required by just.

# Print a help message.
help:
    just --list

# Run the nightly check command: `just nightly check` or `just nightly clippy --fix`
nightly arg1 *args:
    cargo +nightly {{arg1}} {{args}} --all-features --all-targets --config .cargo/nightly-config.toml

# Run with nightly features enabled: `just bootstrap check`, `just bootstrap +beta clippy`
bootstrap arg1 *args:
    RUSTC_BOOTSTRAP=1 cargo {{arg1}} {{args}} --all-features --all-targets --config .cargo/nightly-config.toml
