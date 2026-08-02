//! MCP stdio server. Exposes Centaur's transactional apply and undo to GUI chat
//! clients that have no shell of their own (Claude Desktop and similar).
//!
//! The workspace root comes from the launching client's config and is deliberately
//! not a tool parameter: every path-safety check in `patch.rs` is derived from that
//! root, so letting the model pick it would void the "never write outside the
//! workspace" invariant.
//!
//! Nothing here may print to stdout except JSON-RPC. Diagnostics go to stderr,
//! which MCP clients treat as a log stream.

use crate::history::PatchSessionRecord;
use crate::patch::{apply_blocks_transactional, ApplyResult};
use crate::{count_search_markers, parse_blocks};
use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
use std::path::Path;

/// Used only when a client omits its own version during initialize.
const DEFAULT_PROTOCOL_VERSION: &str = "2024-11-05";

pub fn serve(workspace: &Path) -> Result<(), String> {
    let root = workspace.canonicalize().map_err(|e| {
        format!("Could not resolve workspace '{}': {}", workspace.display(), e)
    })?;

    eprintln!("centaur mcp: serving {}", root.display());

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        if line.trim().is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(e) => {
                send(&mut stdout, &error_response(Value::Null, -32700, &e.to_string()))?;
                continue;
            }
        };

        // Notifications carry no id. Answering one desyncs strict clients.
        let Some(id) = request.get("id").cloned() else {
            continue;
        };

        let response = dispatch(&root, &id, &request);
        send(&mut stdout, &response)?;
    }

    Ok(())
}

fn dispatch(root: &Path, id: &Value, request: &Value) -> Value {
    let method = request.get("method").and_then(Value::as_str).unwrap_or_default();
    let params = request.get("params").cloned().unwrap_or(Value::Null);

    match method {
        "initialize" => {
            // Echo the client's version when it sends one; a mismatch here is the
            // most common reason a server fails to connect at all.
            let version = params
                .get("protocolVersion")
                .and_then(Value::as_str)
                .unwrap_or(DEFAULT_PROTOCOL_VERSION);
            result_response(
                id,
                json!({
                    "protocolVersion": version,
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "centaur", "version": env!("CARGO_PKG_VERSION") }
                }),
            )
        }
        "tools/list" => result_response(id, json!({ "tools": tool_definitions() })),
        "tools/call" => {
            let name = params.get("name").and_then(Value::as_str).unwrap_or_default();
            let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let (text, is_error) = call_tool(root, name, &arguments);
            result_response(
                id,
                json!({
                    "content": [{ "type": "text", "text": text }],
                    "isError": is_error
                }),
            )
        }
        _ => error_response(id.clone(), -32601, &format!("Unknown method: {}", method)),
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "apply_patch",
            "description": "Apply Search/Replace blocks to the Centaur workspace. Every block is \
                            validated before anything is written; if one block fails, none are \
                            applied. A successful apply records an undo snapshot.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "text": {
                        "type": "string",
                        "description": "Text containing one or more blocks in the form: a \
                                        'File: <relative path>' line, then <<<<<<< SEARCH, the \
                                        exact text to find, =======, the replacement, and \
                                        >>>>>>> REPLACE. Leave SEARCH empty to create a file."
                    },
                    "dry_run": {
                        "type": "boolean",
                        "description": "Validate and report without writing. Default false."
                    }
                },
                "required": ["text"]
            }
        },
        {
            "name": "undo",
            "description": "Revert a patch session in the Centaur workspace, restoring the files \
                            as they were before that apply.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "session_id": {
                        "type": "string",
                        "description": "Session id to revert, or 'latest'. Default 'latest'."
                    }
                }
            }
        }
    ])
}

/// Returns (report, is_error). Tool failures are reported in-band with `isError` so
/// the model can read the reason and correct itself; only protocol faults become
/// JSON-RPC errors.
pub(crate) fn call_tool(root: &Path, name: &str, arguments: &Value) -> (String, bool) {
    match name {
        "apply_patch" => apply_patch(root, arguments),
        "undo" => undo(root, arguments),
        _ => (format!("Unknown tool: {}", name), true),
    }
}

fn apply_patch(root: &Path, arguments: &Value) -> (String, bool) {
    let Some(text) = arguments.get("text").and_then(Value::as_str) else {
        return ("Missing required argument: text".to_string(), true);
    };

    let blocks = parse_blocks(text);
    if blocks.is_empty() {
        return (
            "No Search/Replace blocks found. Each edit needs a 'File: <path>' line followed by \
             <<<<<<< SEARCH / ======= / >>>>>>> REPLACE."
                .to_string(),
            true,
        );
    }

    let dry_run = arguments.get("dry_run").and_then(Value::as_bool).unwrap_or(false);

    let mut report = String::new();
    let markers = count_search_markers(text);
    if markers > blocks.len() {
        report.push_str(&format!(
            "warning: {} of {} blocks had malformed delimiters and were skipped\n",
            markers - blocks.len(),
            markers
        ));
    }

    match apply_blocks_transactional(root, &blocks, dry_run) {
        Ok(results) => {
            let write_failed = results.iter().any(|r| matches!(r, ApplyResult::IoError(..)));
            for result in &results {
                report.push_str(&describe(result));
                report.push('\n');
            }
            if write_failed {
                report.push_str("Some writes failed. Call undo to restore the workspace.");
            } else if !dry_run {
                report.push_str("Undo snapshot recorded; call undo to revert this session.");
            }
            (report, write_failed)
        }
        Err((failures, message)) => {
            report.push_str(&message);
            report.push('\n');
            for failure in &failures {
                report.push_str(&describe(failure));
                report.push('\n');
            }
            (report, true)
        }
    }
}

fn undo(root: &Path, arguments: &Value) -> (String, bool) {
    let session_id = arguments
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("latest");

    match PatchSessionRecord::revert_session(root, session_id) {
        Ok(restored) if restored.is_empty() => {
            ("Session reverted; no files needed restoring.".to_string(), false)
        }
        Ok(restored) => (restored.join("\n"), false),
        Err(e) => (e, true),
    }
}

fn describe(result: &ApplyResult) -> String {
    match result {
        ApplyResult::Created(path) => format!("created {}", path),
        ApplyResult::Updated(path) => format!("updated {}", path),
        ApplyResult::DryRunSimulated(path) => format!("would patch {}", path),
        ApplyResult::MatchNotFound(path) => {
            format!("search text not found in {}", path)
        }
        ApplyResult::AmbiguousMatch(path) => format!(
            "search text matches more than once in {}; include more surrounding lines",
            path
        ),
        ApplyResult::IoError(path, err) => format!("write failed for {}: {}", path, err),
        ApplyResult::SecurityError(err) => format!("refused: {}", err),
    }
}

fn result_response(id: &Value, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error_response(id: Value, code: i32, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

fn send(out: &mut io::Stdout, response: &Value) -> Result<(), String> {
    writeln!(out, "{}", response).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::use_scratch_home;
    use tempfile::tempdir;

    #[test]
    fn traversal_block_is_refused_instead_of_written() {
        use_scratch_home();
        let dir = tempdir().unwrap();
        let inside = dir.path().join("keep.txt");
        std::fs::write(&inside, "original").unwrap();

        let text = "File: keep.txt\n<<<<<<< SEARCH\noriginal\n=======\npatched\n>>>>>>> REPLACE\n\n\
                    File: ../escape.txt\n<<<<<<< SEARCH\n=======\nowned\n>>>>>>> REPLACE\n";

        let (report, is_error) = call_tool(dir.path(), "apply_patch", &json!({ "text": text }));

        assert!(is_error, "traversal must fail the call: {}", report);
        assert!(report.contains("traversal"), "report should name the cause: {}", report);
        // The transaction aborts as a whole, so the legal block must not land either.
        assert_eq!(std::fs::read_to_string(&inside).unwrap(), "original");
    }
}
