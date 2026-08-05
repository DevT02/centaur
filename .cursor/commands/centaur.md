# Centaur

Centaur reads a local repository and writes reviewed edits back to it. Every edit is a
Search/Replace block; all blocks are validated before any of them are written, and a
successful apply records an undo snapshot.

## Installation & Setup

To set up Centaur's MCP server and GUI slash commands across all supported AI clients in one step:

```sh
centaur install
```

For specific clients or custom target folders:
```sh
centaur skill install --client all                     # install skills across all clients
centaur mcp install --client antigravity               # configure MCP for Antigravity
centaur skill install --output path/to/action.md       # custom output file
```

## First Interaction when Invoking /centaur

When the user types `/centaur`, Centaur is already installed. Immediately ask the user what task they would like to accomplish and which context export mode to run:
1. **`changed`** (Recommended for active work): Read modified & untracked Git files.
2. **`full`**: Read the entire workspace.
3. **`compact`**: Read full workspace with repetitive generated content summarized.

If the user already specified a task in their `/centaur` prompt, immediately fetch context (`get_context` or `centaur --export`) and propose Search/Replace patches.

## Pick the path that matches this session

**If the `centaur` MCP tools are available** (`get_context`, `apply_patch`, `undo`), use them.
They are the whole loop and need no terminal:

1. `get_context` — read the workspace. Use `mode: "changed"` when the user is working on
   uncommitted edits, `mode: "full"` otherwise, and `paths` to narrow a large project.
   Large reads come back in numbered parts; keep calling with the next `part` until done.
2. Write the edits as Search/Replace blocks and pass them to `apply_patch`. Use
   `dry_run: true` first when the change is wide or the Search text is not obviously unique.
3. `undo` reverts the last apply if the result is wrong.

The workspace is fixed at install time and is not a tool argument. If `apply_patch` writes
to the wrong project, the fix is `centaur install --workspace <path>` (or `centaur mcp install --client <id> --workspace <path>`) and a client restart.

**If you can run shell commands**, call the CLI directly:

```sh
centaur --export --mode changed --task "<what to do>"   # pack context for a web AI
centaur --clipboard                                     # apply the AI's reply
centaur --dry-run --clipboard                           # validate without writing
centaur undo                                            # revert the last apply
centaur audit                                           # scan for leaked credentials
centaur install                                         # auto-configure MCP & skills
```

Editors that read and write files themselves do not need `--export`. Use `centaur undo`
and `centaur audit`, and edit directly.

## Block format

```
File: src/main.rs
<<<<<<< SEARCH
exact lines from the current file
=======
replacement lines
>>>>>>> REPLACE
```

- Paths are workspace-relative. No `./`, no absolute paths.
- SEARCH must match the file byte for byte. Include 2-3 surrounding lines so it matches once.
- Empty SEARCH creates a new file.
- Never wrap blocks in backticks.

## Failure modes

- `search text not found` — re-read the file with `get_context`; you are patching a stale copy.
- `matches more than once` — add surrounding lines until the match is unique.
- `refused: traversal` — the path leaves the workspace. It will not be written.

If a read reports possible credentials, tell the user, do not echo the values, and offer
`redact: true`.
