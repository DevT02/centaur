# AGENTS.md

## Project

The Clipboard Centaur is a Rust CLI that exports local repository context for browser-based AI tools and safely applies their Search/Replace responses. Keep changes small, deterministic, and local-first.

## Commands

- Build: `cargo build --locked`
- Test: `cargo test --all-targets --locked`
- Focus a test: `cargo test <test_name> --locked`
- CLI help: `cargo run -- --help`
- Format a touched Rust file: `rustfmt --edition 2024 path/to/file.rs`
- Inspect lints: `cargo clippy --all-targets --all-features`

The test command is the required gate. Do not introduce a repository-wide formatting-only diff alongside functional work; the current formatting baseline is tracked in [`docs/ROADMAP.md`](https://github.com/DevT02/centaur/blob/master/docs/ROADMAP.md).

## Code map

- `src/cli.rs`: CLI parsing, help text, and parser consistency tests
- `src/main.rs`: process exit behavior and orchestration
- `src/lib.rs`: Search/Replace parsing and cleanup
- `src/export.rs`: shared export pipeline
- `src/pack.rs`: file walking, ordering, batching, and manifests
- `src/patch.rs`: path validation, patch planning, and writes
- `src/history.rs`: workspace-scoped undo history
- `src/secrets.rs`: export scanning and redaction
- `src/ui.rs`: interactive terminal workflows
- `src/mcp.rs`: MCP stdio server for GUI chat clients
- `src/config.rs` and `src/prompt.rs`: persistent settings and prompt templates

## Invariants

- Never write outside the active workspace, including through symlinked parents.
- Validate every patch before writing any patch.
- Refuse to write when the undo snapshot cannot be created.
- Keep dry runs side-effect free.
- Redaction may change only a temporary export copy, never source files.
- Preserve CRLF and UTF-8 BOM behavior covered by regression tests.
- Keep history scoped to the workspace that created it.

## Change rules

- Trace all callers before changing shared parsing, export, patch, or history behavior.
- Prefer the standard library and existing dependencies.
- Add one focused regression test for each behavior change.
- Update user documentation when flags, defaults, output, or safety behavior changes.
