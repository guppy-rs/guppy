# Contributing to cargo-guppy

## Pull Requests

We actively welcome your pull requests. If you have a new feature in mind, please discuss the feature in an issue to
ensure that your contributions will be accepted.

1. Fork the repo and create your branch from `main`.
2. If you've added code that should be tested, add tests.
3. If you've changed APIs, update the documentation.
4. Ensure the test suite passes:
  a. Install [cargo nextest](https://nexte.st/book/pre-built-binaries) if you haven't already.
  b. Run `cargo nextest run --all-features`.
  c. Run `cargo test --doc --all-features` to run doctests.
5. Run `cargo xfmt` to automatically format your changes (CI will let you know if you missed this).

## Issues

We use GitHub issues to track public bugs. Please ensure your description is clear and has sufficient instructions to be
able to reproduce the issue.

## License

By contributing to `cargo-guppy`, you agree that your contributions will be dual-licensed under the terms of the
[`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE) files in the root directory of this source
tree.
