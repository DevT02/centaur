# Developer productivity roadmap

The goal is a short, safe loop from local change to reviewed AI-assisted patch. Priorities below favor faster feedback and fewer recovery steps over feature count.

## Shipped foundation

- Locked test suite on Linux and Windows through GitHub Actions
- Checked-in Rust 1.97.1 toolchain with Rustfmt and Clippy
- Root `AGENTS.md` with commands, code map, and safety invariants
- Validated export modes; typos fail instead of silently becoming a full export
- User, contributor, and architecture documentation

## P0: remove daily friction

### Prebuilt releases

Publish checksummed binaries for Windows, macOS, and Linux from tagged GitHub releases. `cargo install` should remain available, but end users should not need a Rust compiler. Add package-manager distribution only after the release artifacts are reliable.

### Stable automation output

Add `--format json` for export summaries, audits, dry runs, history, and apply results. Pair it with an explicit non-interactive confirmation flag so editors and agents can compose Centaur without scraping emoji-rich terminal output. Keep interactive confirmation as the default.

### Clean formatting and lint baseline

Land one dedicated Rustfmt-only change, fix the existing Clippy warnings, then make `cargo fmt --check` and `cargo clippy -- -D warnings` required CI gates. Keeping this separate avoids hiding behavior changes in a large mechanical diff.

## P1: make the CLI self-explanatory

### Generated shell completions and man pages

Generate Bash, Zsh, Fish, and PowerShell completions from the Clap command definition so flags and values stay in sync with `--help`. Generate a man page from the same source rather than maintaining another handwritten command reference.

### `centaur doctor`

Report clipboard availability, Git status, config/template paths, Ollama availability, writable history storage, and active limits in one read-only command. Include direct recovery commands for failed checks.

### Cross-platform release testing

Extend CI to macOS before publishing macOS binaries. Add smoke tests for clipboard initialization and opening export directories on each supported OS without making those desktop-dependent checks part of ordinary unit tests.

## P1: strengthen recovery

### Roll back mid-write failures automatically

Planning is all-or-nothing and undo snapshots are created before writes, but an I/O failure during the multi-file write loop can still leave a partially applied session. Restore already-written files automatically before returning failure, then test the failure path.

### Clarify history storage

History files use TOML serialization behind `.json` names. Migrate them to honest `.toml` names with backward-compatible reads so debugging and external tooling are not misled.

## P2: improve long-term confidence

- Add parser and path fuzzing once the stable CLI/output contract exists.
- Add dependency update automation with grouped, tested pull requests.
- Record export schema versions in `manifest.json` before third-party integrations depend on it.
- Measure export time, attachment count, and parse success locally before optimizing; avoid telemetry by default.

## Decision rules

- Prefer one source of truth: Clap for CLI metadata, shared export code for all front ends, and one documented test command.
- Keep cloud uploads user-driven unless a separate opt-in integration is explicitly designed.
- Add dependencies only when the standard library or an existing crate cannot solve the measured problem cleanly.
- Treat security, rollback, accessibility, and input validation as requirements, not optional polish.

## References

- [Cargo locked builds](https://doc.rust-lang.org/cargo/commands/cargo-test.html#manifest-options)
- [GitHub's Rust CI guidance](https://docs.github.com/en/actions/tutorials/build-and-test-code/rust)
- [rustup toolchain files](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file)
- [AGENTS.md open format](https://agents.md/)
