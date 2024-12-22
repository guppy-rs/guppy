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

# Build docs for crates and direct dependencies
rustdoc:
    @# Ignore clap since we currently depend on both clap 2, 3, and 4 -- we
    @# should really fix this at some point.
    @
    @# Also, cargo-compare currently pulls in cargo which bloats build times massively.
    @RUSTC_BOOTSTRAP=1 RUSTDOCFLAGS='--cfg=doc_cfg' \
        cargo tree --depth 1 -e normal --prefix none --workspace --exclude cargo-compare \
        | grep -v '^clap v[23].' \
        | grep -v '^cargo-compare v' \
        | gawk '{ gsub(" v", "@", $0); printf("%s\n", $1); }' \
        | xargs printf -- '-p %s\n' \
        | xargs cargo doc --no-deps --lib --all-features
