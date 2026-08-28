# Architecture

Centaur is a local bridge. MCP clients can call workspace-scoped tools directly; the export workflow remains available when a client has no MCP connection.

```mermaid
flowchart LR
    A["Local workspace"] --> B["Centaur MCP tools"]
    B --> C["stdio client"]
    B --> D["loopback HTTP"]
    D --> E["authenticated HTTPS tunnel"]
    C --> F["AI client"]
    E --> F
    F --> B
    B --> G["Validate every patch"]
    G --> H["Undo snapshot"]
    H --> A
```

## Export pipeline

1. `main.rs` or `ui.rs` selects paths, an `ExportMode`, and limits. `cli.rs` is the source of truth for command-line metadata.
2. `export::collect_files` resolves full, changed, staged, or compact scope.
3. `export::scan_files` reports likely credentials; `--redact` creates sanitized temporary copies.
4. `pack::pack_files_dynamic` orders files, omits unsupported data, splits oversized context, and builds the manifest.
5. `export::run` writes a unique temporary export directory and copies the rendered prompt when configured.

`src/export.rs` is the shared implementation. Keep CLI and interactive flows routed through it so redaction, filtering, and output behavior cannot drift apart.

## Apply pipeline

1. `lib::parse_patch_payload` accepts exactly `NO_CHANGES` or a complete set of parseable Search/Replace blocks from clipboard, file, or standard input.
2. `patch::plan_blocks_transactional` validates paths and computes every resulting file in memory.
3. `review::render_patch_plan` renders the exact in-memory before/after result for approval.
4. Any malformed, missing, ambiguous, or unsafe block aborts the complete payload before disk writes.
5. `patch::apply_planned_transactional` verifies that every reviewed source is still current.
6. `history::PatchSessionRecord` stores both original and applied content before writes begin.
7. Files are written through temporary paths.
8. Undo history is workspace-scoped and checked for later edits before a session can be restored.

## MCP transports

1. `mcp::serve` reads newline-delimited JSON-RPC over stdio for local clients and private tunnel helpers.
2. `mcp::serve_http` accepts JSON-RPC POST requests at `/mcp` for remote clients.
3. Both transports call the same dispatcher and therefore expose identical tools and safety behavior.
4. HTTP binds only to a loopback address, requires a bearer token, limits request sizes, and rejects origins unless explicitly allowed.
5. An external tunnel or reverse proxy supplies public HTTPS and provider-compatible OAuth. Centaur never opens a public listener itself.

## Module map

| Module | Responsibility |
| --- | --- |
| `cli.rs` | Clap arguments, subcommands, help text, and parser consistency tests |
| `main.rs` | Process exit behavior and CLI orchestration |
| `ui.rs` | Interactive terminal dashboard and wizards |
| `lib.rs` | Public API and tolerant response parser |
| `export.rs` | Shared export orchestration and temporary redaction copies |
| `git.rs` | Git-aware export scopes |
| `pack.rs` | Walking, prioritization, token estimates, batching, and manifest output |
| `patch.rs` | Workspace path security, matching, planning, and writes |
| `history.rs` | Persistent, workspace-scoped undo records |
| `secrets.rs` | Credential-pattern detection and redaction |
| `prompt.rs` | Default/custom prompt templates and placeholder rendering |
| `config.rs` | Persistent export settings and `CENTAUR_HOME` |
| `review.rs` | Per-file patch summaries and exact diff rendering |
| `doctor.rs` | Core health checks and optional integration diagnostics |
| `mcp.rs` | Shared MCP tools plus stdio and authenticated Streamable HTTP transports |

## Safety invariants

- Every target must remain under the canonical workspace root.
- New files must not escape through an existing symlinked ancestor.
- Every block must validate before any planned block is written.
- A failed undo snapshot prevents all writes.
- Dry runs must not write files or history.
- Redaction must never mutate the source workspace.
- Undo sessions must be visible only from the workspace that created them.
- The MCP workspace root comes from the launching client's configuration, never from a tool argument.
- Remote HTTP must bind to loopback and authenticate every MCP POST before dispatch.
- Existing CRLF line endings and UTF-8 BOM behavior must remain stable.

Tests for these boundaries belong in `tests/regressions.rs` or `tests/intricate_edge_cases.rs`. CLI exit behavior belongs in `tests/exit_codes.rs`; Git status parsing belongs in `tests/git_changed.rs`.
