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

Before writing, Centaur validates every block, rejects ambiguous or unsafe paths, shows a summary, and asks for confirmation. If any block fails validation, none of the planned blocks are written. `--dry-run` performs the same planning without modifying the workspace or history.

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

## Prompt templates

Centaur keeps separate templates for single-message and batched uploads:

```sh
centaur prompt show
centaur prompt edit --single
centaur prompt edit
centaur prompt reset
```

Custom templates should preserve `{{SESSION_ID}}`, `{{TOTAL_PARTS}}`, `{{TOTAL_BATCHES}}`, and `{{USER_TASK}}` where applicable. `prompt reset` removes both custom templates and restores built-in defaults.

## Security and privacy

- Centaur itself does not send files to a cloud API; you choose what to upload in the browser.
- Secret scanning is heuristic. Review the export before uploading it even when `--redact` reports no findings.
- `--redact` changes a temporary copy only. It does not clean secrets from the source repository or Git history.
- Patch paths are constrained to the current workspace, including checks against traversals and symlink escapes.
- Undo records live under Centaur's configuration directory and are scoped to their original workspace.
- Ollama fallback is optional and local. Enable it only when deterministic matching fails: `centaur --llm auto --clipboard`.

## Troubleshooting

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
