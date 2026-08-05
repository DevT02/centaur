# Developer productivity roadmap

The goal is a short, safe loop from local change to reviewed AI-assisted patch. Priorities below favor faster feedback and fewer recovery steps over feature count.

## Shipped foundation

- **Unified 1-Step Setup (`centaur install`)**: Auto-configures MCP servers and GUI slash commands across Antigravity, Claude, ChatGPT, Cursor, Windsurf, VS Code, and Gemini CLI.
- **System Doctor (`centaur doctor`)**: Comprehensive diagnostic command checking workspace canonical paths, Git state, storage permissions, clipboard availability, and client MCP configurations.
- **VS Code Extension (`editors/vscode`)**: 1-click status bar button and global `Ctrl+Alt+V` keyboard shortcut for applying patches directly within VS Code.
- **Transactional Safety & Redaction**: Strict path containment, pre-write validation, workspace-scoped undo snapshots, and export credential redaction.
- **Locked Test Suite**: Cross-platform test suite passing 35/35 unit and integration tests.

## Active Priorities

### 1. JSON & Machine-Readable Output (`--format json`)

Add `--format json` flag to `centaur --export`, `centaur --clipboard`, `centaur audit`, and `centaur doctor`. Pairs with a non-interactive `--yes` flag to allow seamless programmatic composition by third-party agents and editor extensions without parsing terminal UI text.

### 2. Multi-Line & Indentation-Resilient Patch Matching

Enhance the Search/Replace matcher to handle subtle line-wrapping, trailing whitespace differences, and indentation shifts gracefully before falling back to local Ollama LLM repair.

### 3. Prebuilt Multi-Platform Binary Releases

Automate GitHub Actions release matrix for prebuilt binaries (Windows x86_64, macOS ARM64/x86_64, Linux x86_64) and package the VS Code extension for the Visual Studio Marketplace (`vsce package`).
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
