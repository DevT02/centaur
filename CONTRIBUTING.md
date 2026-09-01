# Contributing

Thanks for improving The Clipboard Centaur. The project is intentionally local-first: users must be able to see and control what leaves or changes their workspace.

## Start here

Install [Git](https://git-scm.com/downloads), [rustup](https://www.rust-lang.org/tools/install), and [Node.js 22](https://nodejs.org/), then run:

```sh
git clone https://github.com/DevT02/centaur.git
cd centaur
cargo test --all-targets --locked
cargo run -- --help
```

The crate supports Rust 1.97.1 or newer. The checked-in `rust-toolchain.toml` selects that tested compiler and installs Rustfmt and Clippy, so contributors using `rustup` do not need to choose a version manually.

For a first contribution, documentation, error recovery text, focused regression tests, and small usability fixes are good places to start. A pull request should solve one clear user problem.

## Repository tour

| Area | Start here | Tests |
| --- | --- | --- |
| CLI syntax and help | `src/cli.rs` | Unit tests in the same file |
| CLI orchestration and exit codes | `src/main.rs` | `tests/exit_codes.rs` |
| Export selection and batching | `src/export.rs`, `src/pack.rs`, `src/git.rs` | `tests/git_changed.rs`, `tests/regressions.rs` |
| Patch parsing and safe writes | `src/lib.rs`, `src/patch.rs` | `tests/intricate_edge_cases.rs`, `tests/regressions.rs` |
| Review and undo | `src/review.rs`, `src/history.rs` | Unit tests and `tests/regressions.rs` |
| Interactive terminal | `src/ui.rs` | Unit tests plus manual keyboard checks |
| Project check detection | `src/verification.rs` | Unit tests in the same file |
| MCP and client setup | `src/mcp.rs`, `src/skill.rs`, `src/doctor.rs` | Unit tests in each module |
| VS Code extension | `editors/vscode/` | `node editors/vscode/extension.test.js` |

Read [the architecture guide](docs/ARCHITECTURE.md) before changing shared parsing, export, patch, history, or MCP behavior. Its safety invariants are acceptance criteria, not implementation suggestions.

## Make a change

Keep the patch small and trace every caller of shared behavior. Prefer the standard library and existing dependencies.

Run one focused test while iterating:

```sh
cargo test <test_name> --locked
```

Add the smallest regression test that would have caught a behavior bug. New errors should return a non-zero exit code and tell the user how to recover.

If a flag, default, output line, setup step, or safety behavior changes, update the README and usage guide in the same pull request.

## Quality gate

Run the same checks as CI before opening a pull request:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features --locked -- -D warnings
cargo test --all-targets --locked
node --check editors/vscode/extension.js
node editors/vscode/extension.test.js
```

Do not mix a repository-wide formatting rewrite into an unrelated behavior change.

Those checks only see your own platform. Code behind `#[cfg(target_os = ...)]` can
compile clean locally and fail on another runner, so if you touch a platform-gated
path, check the other targets before pushing:

```sh
rustup target add x86_64-unknown-linux-gnu
cargo clippy --all-targets --all-features --locked --target x86_64-unknown-linux-gnu -- -D warnings
```

## Documentation and screenshots

- Prefer commands that a reader can paste and run.
- Use project-relative paths and remove usernames, tokens, private repository names, and unrelated applications from screenshots.
- Capture real Centaur output. Do not recreate terminal output in an image editor.
- Keep important text readable at the width used in the README.
- Store final images in `docs/screenshots/` and keep reproducible visual sources and capture notes in `docs/visuals/`.
- Use `centaur doctor --redact-paths` for shareable diagnostics, then check the output manually.
- Run `centaur audit` before publishing a diagnostic bundle or screenshot fixture.

See [the visual guide](docs/visuals/README.md) for the current capture sizes and fixture checklist.

## Pull request checklist

- The change solves one clear problem without speculative abstractions.
- Tests cover changed behavior and the complete quality gate passes.
- Safety boundaries and dry runs remain fail-closed and side-effect free.
- User-visible changes are documented.
- New local links and screenshots resolve from the document that references them.
- The pull request explains the user impact and verification, not only the implementation.

Use the repository issue forms for bugs and feature proposals. Report vulnerabilities through [GitHub private vulnerability reporting](https://github.com/DevT02/centaur/security/advisories/new), not a public issue.
