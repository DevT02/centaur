# Developer productivity roadmap

The goal is a short, safe loop from local change to reviewed AI-assisted patch. Priorities below favor faster feedback and fewer recovery steps over feature count.

The complete user-workflow research, shared architecture, and ranked implementation
backlog are maintained in [USER_WORKFLOWS.md](USER_WORKFLOWS.md).

## Shipped foundation

- **Unified 1-Step Setup (`centaur install`)**: Auto-configures MCP servers and GUI slash commands across Antigravity, Claude, ChatGPT, Cursor, Windsurf, VS Code, and Gemini CLI.
- **No-Copy Web MCP**: Authenticated loopback Streamable HTTP for remote connectors, with the existing stdio server compatible with private MCP tunnels.
- **System Doctor (`centaur doctor`)**: Comprehensive diagnostic command checking workspace canonical paths, Git state, storage permissions, clipboard availability, and client MCP configurations.
- **VS Code Extension (`editors/vscode`)**: 1-click status bar button and global `Ctrl+Alt+V` keyboard shortcut for applying patches directly within VS Code.
- **Transactional Safety & Redaction**: Strict path containment, pre-write validation, workspace-scoped undo snapshots, and export credential redaction.
- **Trustworthy Patch Review**: Complete-payload parsing, successful `NO_CHANGES` handling, exact diff approval, source-drift rejection, drift-safe undo, and first-class standard input.
- **Contributor Quality Gate**: Cross-platform locked tests plus enforced Rustfmt and warning-free Clippy checks.
- **Contributor Onboarding**: A runnable setup path, code and test map, issue forms, pull request checklist, security reporting path, and reproducible visual-source guidance.
- **Guided Activation**: `centaur task` chooses context, the terminal and VS Code surfaces use progressive review, and `centaur check` detects manifest-backed verification commands without running them implicitly.
- **Release Build Automation**: Version tags validate against `Cargo.toml`, build four native binary archives and a VSIX, generate SHA-256 checksums, and create a GitHub release.

## Active Priorities

### 1. JSON & Machine-Readable Output (`--format json`)

Add `--format json` to `centaur --export`, patch application, `centaur audit`, and `centaur doctor`. The shipped `--stdin` and `--yes` flags already provide explicit non-interactive input and approval; JSON completes the stable operation contract without requiring third-party tools to parse terminal text.

### 2. Multi-Line & Indentation-Resilient Patch Matching

Enhance the Search/Replace matcher to handle subtle line-wrapping, trailing whitespace differences, and indentation shifts gracefully before falling back to local Ollama LLM repair.

### 3. Prebuilt Multi-Platform Binary Releases

Run the first controlled version tag through the prepared release workflow,
inspect each downloaded artifact on its target platform, and then decide whether
to add signing and Marketplace publication. The workflow already builds Windows
x86_64, macOS ARM64/x86_64, Linux x86_64, checksums, and a packaged VSIX.

Before publishing a release:

- Decide whether crates.io is a supported distribution channel; if it is, add an explicit Cargo package allowlist and verify the packaged README assets.
- Enable GitHub private vulnerability reporting or replace the security-report link with another verified private channel.

- Measure export time, attachment count, and parse success locally before optimizing; avoid telemetry by default.

## Decision rules

- Prefer one source of truth: `cli.rs` for Clap metadata, shared export code for all front ends, and one documented quality gate.
- Keep cloud uploads user-driven unless a separate opt-in integration is explicitly designed.
- Add dependencies only when the standard library or an existing crate cannot solve the measured problem cleanly.
- Treat security, rollback, accessibility, and input validation as requirements, not optional polish.

## References

- [Cargo locked builds](https://doc.rust-lang.org/cargo/commands/cargo-test.html#manifest-options)
- [GitHub's Rust CI guidance](https://docs.github.com/en/actions/tutorials/build-and-test-code/rust)
- [rustup toolchain files](https://rust-lang.github.io/rustup/overrides.html#the-toolchain-file)
- [AGENTS.md open format](https://agents.md/)
