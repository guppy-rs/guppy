# Changelog

All notable changes to this project will be documented in this file.

## [0.9.29-rc.1] - 2024-02-04

### Added

- The default `hakari.toml` template for new projects now suggests Apple Silicon
  (`aarch64-apple-darwin`) as well.

### Changed

- Builtin platforms updated to Rust 1.75.

### Fixed

- Consider dev-dependencies of proc-macro crates -- previously, we weren't doing so.

This may change some checked-in workspace-hacks, but it is a bugfix and not a breaking change.
Testing against several real-world workspace-hacks, only one of them changed.

## [0.9.28] - 2023-10-03

### Added

New config option `workspace-hack-line-style`, with three possible values:

- _"full"_: `my-workspace-hack = { version = "0.1", path = ... }`. This is the default and a
  good way to get started.

- _"version-only"_: `my-workspace-hack = { version = "0.1" }`. Specifying versions this way is
  useful if you've published a stub crate to crates.io (see the [publishing](crate::publishing)
  section). You can use this in combination with a `patch` directive in the root `Cargo.toml`:

  ```toml
  [patch.crates-io.my-workspace-hack]
  path = "workspace-hack"
  ```

- _"workspace-dotted"_: `my-workspace-hack.workspace = true`. To use this, define a workspace
  dependency in the root `Cargo.toml`:

  ```toml
  [workspace.dependencies]
  my-workspace-hack = { version = "0.1", path = "workspace-hack" }

  # or, along with a patch directive:
  my-workspace-hack = { version = "0.1" }
  ```

### Changed

- MSRV updated to Rust 1.70.
- Builtin platforms updated to Rust 1.73.

## [0.9.27] - 2023-07-29

### Changed

- Builtin platforms updated to Rust 1.71.

## [0.9.26] - 2023-06-25

### Changed

- Internal dependency updates: updated guppy to 0.17.0.

## [0.9.25] - 2023-06-19

### Fixed

- `target_os = "none"` is now correctly evaluated.

### Changed

- MSRV updated to Rust 1.66.
- Builtin platforms updated to Rust 1.70.

## [0.9.24] - 2023-04-15

### Fixed

Canonicalize paths correctly on Windows ([#70]).

## [0.9.23] - 2023-01-18

### Added

Introduced a new `dep-format-version`, version 4, with a change to always sort outputs alphabetically. This
matches the order produced by [cargo-sort](https://crates.io/crates/cargo-sort) ([#65]).

[#65]: https://github.com/guppy-rs/guppy/issues/65

## [0.9.22] - 2023-01-18

(Publishing this release was cancelled due to a bug in it.)

## [0.9.21] - 2023-01-14

### Fixed

Update README.md with fixed install instructions.

## [0.9.20] - 2023-01-14

### Fixed

Fixed install instructions.

## [0.9.19] - 2023-01-14

### Added

Release binaries are now available on GitHub Releases for quicker installation locally and in CI.

You can install release binaries:

- using `cargo binstall` with `cargo binstall cargo-hakari`
- in GitHub Actions CI, using:

  ```yml
  - name: Install cargo-hakari
    uses: taiki-e/install-action@v2
    with:
      tool: cargo-hakari
  ```

## [0.9.18] - 2023-01-08

### Added

Introduced a new `dep-format-version`, version 3, with these changes:

- Always elide build metadata from version strings (e.g. with the semver string `5.4.0+g7f361a3`, don't show the bit after the + sign). Thanks [Nikhil Benesch](https://github.com/guppy-rs/guppy/pull/57) for your first contribution!
- Remove private features from the workspace-hack's Cargo.toml. This should simplify the output greatly.

### Changed

- MSRV updated to Rust 1.62.
- Builtin target platforms updated to Rust 1.66.

## [0.9.17] - 2022-12-04

### Fixed

- Fixed a panic in rare circumstances ([#38]).

[#38]: https://github.com/guppy-rs/guppy/issues/38

## [0.9.16] - 2022-11-07

### Added

- cargo-hakari now works with `cfg()` specifications that contain `target_abi` in them.

## [0.9.15] - 2022-09-30

### Changed

- Repository location update.
- MSRV updated to Rust 1.58.

Thanks to [Carol Nichols](https://github.com/carols10cents) for her contributions to this release!

## [0.9.14] - 2022-05-29

### Changed

- Dependency updates: in particular, guppy updated to 0.14.2.

## [0.9.13] - 2022-03-14

### Changed

- Support for weak and namespaced features.
- Target platforms updated to Rust 1.59.
- MSRV updated to Rust 1.56.

## [0.9.12] - 2022-02-06

### Fixed

- A small fix to Cargo build simulations ([#596](https://github.com/facebookincubator/cargo-guppy/issues/596)). This is not a breaking change to the hakari output because it is a bugfix.

## [0.9.11] - 2021-12-08

- Reverted the changes in version 0.9.9 because of [#524](https://github.com/facebookincubator/cargo-guppy/issues/524).

## [0.9.10] - 2021-12-06

### Added

- A new `explain` command prints out information about why a dependency is in the workspace-hack.

### Changed

- The `verify` command now uses `explain` to print out information about failing crates.

## [0.9.9] - 2021-11-28

### Added

- Support for using the already-published [workspace-hack crate](https://crates.io/crates/workspace-hack)
  on crates.io, which makes publishing seamless for new users.

### Changed

- `cargo hakari init`: the default crate name is always `workspace-hack` now.
  - This makes publishing seamless for new users.

## [0.9.8] - 2021-11-27

### Added

- Support for [publishing a dummy workspace-hack](https://docs.rs/cargo-hakari/latest/cargo_hakari/publishing). This is an
  alternate publishing method that integrates better with existing workflows.
- New config option `dep-format-version`, to control `workspace-hack = ...` lines in other `Cargo.toml`s.
  - Newly initialized workspaces have `dep-format-version = "2"`.
  - Version 2 is required for the alternate publishing method.

### Changed

- The default config file location is now `.config/hakari.toml`. `.guppy/hakari.toml` continues to be supported
  as a fallback, so existing users are unaffected.

## [0.9.7] - 2021-11-25

(This release was yanked because it contained a few bugs.)

## [0.9.6] - 2021-10-09

### Fixed

- Backed out the [algorithmic improvement](https://github.com/facebookincubator/cargo-guppy/pull/468) from earlier
  because it didn't handle some edge cases.
- Also simulate builds with dev-dependencies disabled.
- Remove empty sections from the output.

## [0.9.5] - 2021-10-04

### Added

- Support for alternate registries through the `[registries]` section in the config.
  - This is a temporary workaround until [Cargo issue #9052](https://github.com/rust-lang/cargo/issues/9052) is resolved.
- Enable ANSI color output on Windows.

### Fixed

- [Fixed some workspace-hack contents missing in an edge case.](https://github.com/facebookincubator/cargo-guppy/pull/476)

### Optimized

- An [algorithmic improvement](https://github.com/facebookincubator/cargo-guppy/pull/468) in `hakari` makes computation up to 33% faster.

## [0.9.4] - 2021-10-04

### Fixed

- Fixed the configuration example in the readme.

## [0.9.3] - 2021-10-03

### Changed

- The new `"auto"` strategy for the `unify-target-host` option is now the default.
- Updated documentation.

### Fixed

- Fix a rustdoc issue.

## [0.9.2] - 2021-10-01

This was tagged, but never released due to
[docs.rs and rustc nightly issues](https://github.com/rust-lang/docs.rs/issues/1510).

## [0.9.1] - 2021-10-01

### Fixed

- Fix invocation as a cargo plugin.

## [0.9.0] - 2021-10-01

Initial release.

[0.9.29-rc.1]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.29-rc.1
[0.9.28]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.28
[0.9.27]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.27
[0.9.26]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.26
[0.9.25]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.25
[0.9.24]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.24
[0.9.23]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.23
[0.9.22]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.22
[0.9.21]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.21
[0.9.20]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.20
[0.9.19]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.19
[0.9.18]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.18
[0.9.17]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.17
[0.9.16]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.16
[0.9.15]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.15
[0.9.14]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.14
[0.9.13]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.13
[0.9.12]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.12
[0.9.11]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.11
[0.9.10]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.10
[0.9.9]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.9
[0.9.8]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.8
[0.9.7]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.7
[0.9.6]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.6
[0.9.5]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.5
[0.9.4]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.4
[0.9.3]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.3
[0.9.2]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.2
[0.9.1]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.1
[0.9.0]: https://github.com/guppy-rs/guppy/releases/tag/cargo-hakari-0.9.0
