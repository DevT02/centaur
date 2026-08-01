# Contributing

Thanks for improving The Clipboard Centaur. Changes should preserve its core promise: the user sees and controls what leaves or changes their workspace.

## Setup

Install [rustup](https://www.rust-lang.org/tools/install), clone the repository, and build it:

```sh
git clone https://github.com/DevT02/centaur.git
cd centaur
cargo build --locked
```

The checked-in `rust-toolchain.toml` selects the tested compiler and installs Rustfmt and Clippy.

## Before opening a pull request

```sh
cargo test --all-targets --locked
cargo clippy --all-targets --all-features
```

CI runs the locked test suite on Linux and Windows. Clippy currently reports known style debt, so fix warnings in code you touch without mixing a repository-wide cleanup into an unrelated change.

For behavior changes, add the smallest regression test that would have caught the bug. Security boundaries, patch transaction behavior, line endings, BOMs, Git path handling, and exit codes already have focused suites under `tests/`.

## Pull request checklist

- The change solves one clear problem and avoids speculative abstractions.
- Tests cover changed behavior and pass with `--locked`.
- New errors return a non-zero exit code and explain the recovery step.
- CLI flags, defaults, or output changes are reflected in the README and usage guide.
- Export and patch changes preserve the invariants in [the architecture guide](https://github.com/DevT02/centaur/blob/master/docs/ARCHITECTURE.md).

See [AGENTS.md](https://github.com/DevT02/centaur/blob/master/AGENTS.md) for the compact code map used by coding agents.
