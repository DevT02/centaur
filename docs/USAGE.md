# Usage guide

## The workflow

Centaur has two halves: export context for a browser-based AI, then validate and apply the AI's response locally.

```sh
cd path/to/your/project
centaur --export --mode changed --task "Describe the exact change"
```

Paste the copied prompt into the AI conversation and attach every generated `centaur_context_part*.txt` file. If Centaur creates batch folders, send them in numeric order and wait for the model to acknowledge each batch.

Copy the model's complete response, then preview and apply it:

```sh
centaur --dry-run --clipboard
centaur --clipboard
```

Use `centaur undo` to restore the latest patch session in the current workspace.

## Choosing context

| Goal | Command |
| --- | --- |
| Send the full repository | `centaur --export` |
| Send current Git work | `centaur --export --mode changed` |
| Send only staged files | `centaur --export --mode staged` |
| Summarize repetitive generated data | `centaur --export --mode compact` |
| Send selected paths | `centaur --export src tests/parser.rs` |
| Add an explicit task | `centaur --export --task "Fix issue #42"` |
| Redact likely secrets from the upload copy | `centaur --export --redact` |

`full` is the default. Invalid mode names are rejected; they never fall back to a full export.

Changed and staged modes include common project manifests so the model still receives build context. Deleted files are omitted because there is no remaining content to upload.

## Applying a response

Centaur accepts responses from the clipboard or a file:

```sh
centaur --clipboard
centaur --file response.txt
centaur --stdin < response.txt
```

The expected block format is:

```text
File: src/example.rs
<<<<<<< SEARCH
fn old() {}
=======
fn new() {}
>>>>>>> REPLACE
```

To create a file, leave `SEARCH` empty. Search text should be unique and include enough surrounding context to match only once.

Before writing, Centaur validates the complete payload, rejects malformed blocks and ambiguous or unsafe paths, shows the exact computed diff, and asks for confirmation. If any block fails parsing or validation, none of the planned blocks are written. The writer also rejects source files changed after review. An exact `NO_CHANGES` response is a successful no-op. `--dry-run` performs the same planning and diff rendering without modifying the workspace or history.

Piped input cannot answer an interactive prompt. Use `--yes` only after the producer and payload have been reviewed, or use `--dry-run` for a side-effect-free check:

```sh
generate-centaur-patch | centaur --stdin --dry-run
generate-centaur-patch | centaur --stdin --yes
```

## Connecting web clients without copying

The MCP server exposes `get_context`, `apply_patch`, and `undo` over either local stdio or authenticated Streamable HTTP.

For ChatGPT web, use [OpenAI Secure MCP Tunnel](https://developers.openai.com/api/docs/guides/secure-mcp-tunnels) and configure its local stdio command as:

```sh
centaur mcp serve --workspace /absolute/path/to/project
```

For Claude web, Gemini Spark, or another remote MCP client, start the HTTP transport:

```powershell
$env:CENTAUR_MCP_TOKEN = "a-random-secret-with-at-least-32-characters"
centaur mcp serve --workspace C:\absolute\path\to\project --http 127.0.0.1:3765
```

Expose `http://127.0.0.1:3765/mcp` through an OAuth-capable HTTPS tunnel or reverse proxy. The proxy must forward `Authorization: Bearer <CENTAUR_MCP_TOKEN>` to Centaur. Add the resulting HTTPS `/mcp` URL as a custom connector in Claude or as a custom app in Gemini Spark.

HTTP mode refuses non-loopback bindings. Requests without the token receive `401 Unauthorized`; requests with an unexpected `Origin` receive `403 Forbidden`. If a trusted proxy must add an Origin header, repeat `--allow-origin <exact-origin>` for each allowed value.

## Configuration

Create the default configuration and print its location:

```sh
centaur config init
centaur config path
```

Defaults:

```toml
[export]
copy_prompt = true
open_export_directory = true
prompt_mode = "first"
max_attachments_per_message = 20
max_attachment_chars = 5000000
context_token_budget = 200000
```

Set `CENTAUR_HOME` to keep configuration, prompt templates, and history in a portable or isolated directory.

## Updating

Run:

    centaur update

The updater invokes Cargo against Centaur's explicit HTTPS repository with the
repository lockfile and replaces the installed binary on PATH. It never pulls, builds,
or installs the repository in the current working directory. This remains a
source-based update path; signed prebuilt releases are tracked separately in the
roadmap.

## Prompt templates

Centaur keeps separate templates for single-message and batched uploads:

```sh
centaur prompt show
centaur prompt edit --single
centaur prompt edit
centaur prompt reset
```

Custom templates should preserve `{{SESSION_ID}}`, `{{TOTAL_PARTS}}`, `{{TOTAL_BATCHES}}`, and `{{USER_TASK}}` where applicable. The built-in templates require final responses to contain only Centaur patch data, or exactly `NO_CHANGES` when no file edits are needed. `prompt reset` removes both custom templates and restores built-in defaults.

## Security and privacy

- Centaur never initiates a cloud upload. A configured remote MCP client can receive the workspace content returned by `get_context`, so enable remote access only for a client and workspace you trust.
- Secret scanning is heuristic. Review the export before uploading it even when `--redact` reports no findings.
- `--redact` changes a temporary copy only. It does not clean secrets from the source repository or Git history.
- Keep `CENTAUR_MCP_TOKEN` out of command lines, configuration files, logs, and source control. Terminate public HTTPS and OAuth at the tunnel or reverse proxy.
- Patch paths are constrained to the current workspace, including checks against traversals and symlink escapes.
- Undo records live under Centaur's configuration directory, are scoped to their original workspace, and refuse to overwrite files changed after the patch.
- Older undo records that do not contain the applied-state check are refused automatically; their original content remains available in the history record for manual recovery.
- Ollama fallback is optional and local. It requires explicit non-interactive approval, turns repaired files back into a single validated transaction, and records an undo snapshot: `centaur --llm auto --clipboard --yes`.

## Troubleshooting

Start with a local health check:

```sh
centaur doctor
```

Use `centaur doctor --redact-paths` when sharing the output in an issue. It replaces workspace, Centaur home, history, and integration paths with safe labels; still review the result before posting it.

### No patch blocks were found

Copy the complete model response. Confirm it contains `File:`, `<<<<<<< SEARCH`, `=======`, and `>>>>>>> REPLACE` delimiter lines. The default Centaur prompt tells the model not to wrap the blocks in an outer code fence.

### A search block is ambiguous

Ask the model to include more unchanged lines around the target. Centaur intentionally refuses to guess when the same search text occurs more than once.

### A search block does not match

Export fresh context after local edits, then ask the model to regenerate its response. Use `--llm auto` only as an optional fallback.

### The changed or staged export is unexpected

Run `git status` or `git diff --cached --name-only` in the same directory. Use explicit paths with a full export when Git state is not the desired scope.

### Clipboard access fails

Save the model response to a UTF-8 text file and run `centaur --file response.txt`.
