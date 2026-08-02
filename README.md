<p align="center">
  <img src="centaur_logo.svg" alt="The Clipboard Centaur" width="520" />
</p>

<h1 align="center">The Clipboard Centaur</h1>

<p align="center">
  Use browser-based AI coding assistants with your local repository without giving them shell access.
</p>

## About

Centaur is a simpler way to move code from a local folder into a browser-based AI session and bring reviewed edits back. It packages the right files, copies a precise prompt, validates the response, previews every change, and keeps an undo snapshot.

Use the web AI access you already have instead of paying for an API-driven agent loop. You keep terminal access local while the model gets enough project context for implementation, review, and system-design work, not just blind vibe coding.

## Why Centaur?

- **You stay in control.** Review every affected file and line count before anything is written.
- **Your tools stay separate.** The model receives only the files you export and never gets direct terminal access.
- **Mistakes are reversible.** Each applied patch creates a local history entry that `centaur undo` can restore.
- **Exports are safer.** Centaur warns about likely credentials and can redact them from the temporary upload copy.
- **Large repositories fit.** Context is split into attachment-sized batches with a manifest and token estimate.
- **Save tokens for architecture.** Get past context limits when system planning. The model can spend its tokens making the best design decisions instead of wasting them on terminal output and tool overhead.

## Install

Centaur requires the [current stable Rust toolchain](https://www.rust-lang.org/tools/install). Contributors using `rustup` automatically get the repository's tested toolchain.

```sh
cargo install --git https://github.com/DevT02/centaur.git
```

To build the current checkout instead:

```sh
git clone https://github.com/DevT02/centaur.git
cd centaur
cargo install --path .
```

## Quick start

From the repository you want the AI to edit:

```sh
# 1. Export changed and untracked files, plus your task.
centaur --export --mode changed --task "Add keyboard navigation to the command menu"
```

Centaur opens the export folder and copies the workflow prompt. Paste that prompt into your web AI, attach the generated `centaur_context_part*.txt` files, and send the message.

When the AI replies with search/replace blocks, copy the entire response and run:

```sh
# 2. Validate, preview, and apply the clipboard response.
centaur --clipboard

# 3. Revert the latest applied patch if needed.
centaur undo
```

Running `centaur` with no arguments opens the interactive workspace UI for the same workflow.

## Export modes

| Mode | What it exports |
| --- | --- |
| `full` | All eligible files under the selected paths (default) |
| `changed` | Modified and untracked Git files |
| `staged` | Staged Git files |
| `compact` | Full context, replacing highly repetitive generated content with short summaries |

Pass specific files or directories after `--export` to narrow a full or compact export:

```sh
centaur --export src tests/regressions.rs --task "Fix the parser regression"
```

Before uploading sensitive code, use `--redact`. It rewrites only the temporary export copy; your workspace is untouched.

```sh
centaur --export --mode changed --redact
```

## Command reference

| Command | Action |
| --- | --- |
| `centaur` / `centaur ui` | Open the interactive workspace UI |
| `centaur --export [paths...]` | Create upload-ready context files and copy the workflow prompt |
| `centaur --clipboard` | Read, preview, and apply patch blocks from the clipboard |
| `centaur --file response.txt` | Read patch blocks from a file |
| `centaur --dry-run --clipboard` | Validate and preview clipboard patches without writing |
| `centaur undo [session-id]` | Revert a patch session; defaults to the latest |
| `centaur history` | Browse patch history for the current workspace |
| `centaur audit` | Scan the workspace for likely leaked credentials |
| `centaur prompt show\|copy\|edit\|reset` | Manage the workflow prompt templates |
| `centaur config init\|path` | Create the default config or print its location |
| `centaur --llm auto --clipboard` | Let a local Ollama model repair malformed patch blocks as a fallback |
| `centaur mcp --workspace <path>` | Serve the apply and undo tools to an MCP client over stdio |

Run `centaur --help` for every option and default.

## GUI clients

Chat applications that have no shell of their own, such as Claude Desktop, can apply Centaur patches through the Model Context Protocol.

### 1. Install the binary and find its full path

```sh
cargo install --git https://github.com/DevT02/centaur.git --force
```

Print the installed location with `where centaur` on Windows or `which centaur` on macOS and Linux. It is normally `C:\Users\<you>\.cargo\bin\centaur.exe` or `~/.cargo/bin/centaur`.

Use that full path in the next step. Desktop applications are not launched from a terminal and do not inherit your shell's `PATH`, so a bare `"centaur"` usually fails to start.

### 2. Register the server with your client

Claude Desktop reads `%APPDATA%\Claude\claude_desktop_config.json` on Windows and `~/Library/Application Support/Claude/claude_desktop_config.json` on macOS. Create the file if it does not exist, or add the `centaur` entry to the `mcpServers` object already there.

```json
{
  "mcpServers": {
    "centaur": {
      "command": "C:/Users/you/.cargo/bin/centaur.exe",
      "args": ["mcp", "--workspace", "C:/path/to/your/project"]
    }
  }
}
```

Write Windows paths with forward slashes or with doubled backslashes. JSON rejects a single backslash.

Each entry serves exactly one project. To work across several repositories, add one entry per repository under a distinct name such as `centaur-api` and `centaur-web`.

### 3. Restart the client

MCP servers start when the application does, so a running client will not pick up the change. After the restart, `apply_patch` and `undo` appear in its tool list.

### What the model can and cannot do

It can apply Search/Replace blocks and revert them. It cannot choose the workspace: the directory in your configuration is the only one the server will write to, and paths that try to escape it are refused. Every apply validates all blocks before writing any of them, and records an undo snapshot first. `undo` restores the most recent session unless you name an older one.

Editors that already run shell commands, such as Cursor or Antigravity, do not need any of this. They can call the CLI directly.

## Documentation

- [Usage guide](https://github.com/DevT02/centaur/blob/master/docs/USAGE.md): recipes, configuration, prompt customization, and troubleshooting
- [Architecture](https://github.com/DevT02/centaur/blob/master/docs/ARCHITECTURE.md): data flow, module map, and safety invariants
- [Contributing](https://github.com/DevT02/centaur/blob/master/CONTRIBUTING.md): setup, checks, and pull request expectations
- [Productivity roadmap](https://github.com/DevT02/centaur/blob/master/docs/ROADMAP.md): prioritized improvements for releases, automation, and AI-assisted development

## Development

```sh
cargo test --all-targets --locked
cargo run -- --help
```

## License

MIT
