//! MCP servers for local stdio clients and remote Streamable HTTP clients.
//!
//! The workspace root comes from the launching client's config and is deliberately
//! not a tool parameter: every path-safety check in `patch.rs` is derived from that
//! root, so letting the model pick it would void the "never write outside the
//! workspace" invariant.
//!
//! Nothing here may print to stdout except JSON-RPC. Diagnostics go to stderr,
//! which MCP clients treat as a log stream.

use crate::export::{collect_files, scan_files};
use crate::git::ExportMode;
use crate::history::PatchSessionRecord;
use crate::pack::{PackOptions, pack_files_dynamic};
use crate::patch::{ApplyResult, apply_blocks_transactional, is_safe_path};
use crate::secrets::redact_secrets;
use crate::{PatchPayload, parse_patch_payload};
use serde_json::{Value, json};
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Used only when a client omits its own version during initialize.
const DEFAULT_PROTOCOL_VERSION: &str = "2026-11-05";

/// Characters of repository text per `get_context` response. A tool result lands
/// directly in the model's context, so a whole repository is paged rather than
/// returned at once; roughly 65k tokens per part at Centaur's 3-chars-per-token
/// estimate.
const CONTEXT_PART_CHARS: usize = 200_000;

const MAX_HTTP_HEADERS: usize = 64 * 1024;
const MAX_HTTP_BODY: usize = 8 * 1024 * 1024;

pub fn serve(workspace: &Path) -> Result<(), String> {
    let root = workspace.canonicalize().map_err(|e| {
        format!(
            "Could not resolve workspace '{}': {}",
            workspace.display(),
            e
        )
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
                send(
                    &mut stdout,
                    &error_response(Value::Null, -32700, &e.to_string()),
                )?;
                continue;
            }
        };

        if let Some(response) = handle_message(&root, &request) {
            send(&mut stdout, &response)?;
        }
    }

    Ok(())
}

/// Serve the same MCP tools over Streamable HTTP. TLS belongs at the tunnel or
/// reverse-proxy boundary; Centaur deliberately binds only to loopback so the
/// repository is never exposed directly on the LAN.
pub fn serve_http(
    workspace: &Path,
    bind: SocketAddr,
    bearer_token: &str,
    allowed_origins: &[String],
) -> Result<(), String> {
    if !bind.ip().is_loopback() {
        return Err(format!(
            "Remote MCP must bind to loopback, not {}. Put an authenticated HTTPS tunnel or reverse proxy in front of it.",
            bind.ip()
        ));
    }
    if bearer_token.len() < 32 {
        return Err("CENTAUR_MCP_TOKEN must contain at least 32 characters.".to_string());
    }

    let root = workspace.canonicalize().map_err(|e| {
        format!(
            "Could not resolve workspace '{}': {}",
            workspace.display(),
            e
        )
    })?;
    let listener = TcpListener::bind(bind)
        .map_err(|e| format!("Could not listen for remote MCP on {}: {}", bind, e))?;
    eprintln!(
        "centaur mcp: serving {} at http://{}/mcp",
        root.display(),
        bind
    );

    for connection in listener.incoming() {
        match connection {
            Ok(mut stream) => {
                let response = match read_http_request(&mut stream) {
                    Ok(request) => {
                        handle_http_request(&root, bearer_token, allowed_origins, &request)
                    }
                    Err(e) => HttpResponse::text(400, "Bad Request", &e),
                };
                if let Err(e) = write_http_response(&mut stream, &response) {
                    eprintln!("centaur mcp: response failed: {}", e);
                }
            }
            Err(e) => eprintln!("centaur mcp: connection failed: {}", e),
        }
    }

    Ok(())
}

fn handle_message(root: &Path, request: &Value) -> Option<Value> {
    // Notifications carry no id. Answering one desyncs strict clients.
    let id = request.get("id")?;
    Some(dispatch(root, id, request))
}

struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn header_count(&self, name: &str) -> usize {
        self.headers
            .iter()
            .filter(|(key, _)| key.eq_ignore_ascii_case(name))
            .count()
    }
}

struct HttpResponse {
    status: u16,
    reason: &'static str,
    content_type: &'static str,
    headers: Vec<(&'static str, String)>,
    body: Vec<u8>,
}

impl HttpResponse {
    fn text(status: u16, reason: &'static str, text: &str) -> Self {
        Self {
            status,
            reason,
            content_type: "text/plain; charset=utf-8",
            headers: Vec::new(),
            body: text.as_bytes().to_vec(),
        }
    }

    fn json(status: u16, reason: &'static str, value: &Value) -> Self {
        Self {
            status,
            reason,
            content_type: "application/json",
            headers: Vec::new(),
            body: value.to_string().into_bytes(),
        }
    }

    fn empty(status: u16, reason: &'static str) -> Self {
        Self {
            status,
            reason,
            content_type: "application/json",
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    fn with_header(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.headers.push((name, value.into()));
        self
    }
}

fn read_http_request(stream: &mut TcpStream) -> Result<HttpRequest, String> {
    stream
        .set_read_timeout(Some(Duration::from_secs(15)))
        .map_err(|e| e.to_string())?;

    let mut bytes = Vec::new();
    let mut chunk = [0_u8; 8192];
    let header_end = loop {
        let read = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("Connection closed before the HTTP headers were complete.".to_string());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if let Some(end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
        if bytes.len() > MAX_HTTP_HEADERS {
            return Err("HTTP headers are too large.".to_string());
        }
    };

    let header_text = std::str::from_utf8(&bytes[..header_end - 4])
        .map_err(|_| "HTTP headers are not valid UTF-8.".to_string())?;
    let mut lines = header_text.split("\r\n");
    let mut request_line = lines
        .next()
        .ok_or_else(|| "Missing HTTP request line.".to_string())?
        .split_whitespace();
    let method = request_line
        .next()
        .ok_or_else(|| "Missing HTTP method.".to_string())?
        .to_string();
    let path = request_line
        .next()
        .ok_or_else(|| "Missing HTTP path.".to_string())?
        .to_string();
    if request_line.next().is_none() || request_line.next().is_some() {
        return Err("Malformed HTTP request line.".to_string());
    }

    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| "Malformed HTTP header.".to_string())?;
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    if headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("transfer-encoding"))
    {
        return Err("Transfer-Encoding is not supported.".to_string());
    }
    let content_lengths: Vec<_> = headers
        .iter()
        .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| {
            value
                .parse::<usize>()
                .map_err(|_| "Invalid Content-Length header.".to_string())
        })
        .collect::<Result<_, _>>()?;
    if content_lengths.len() > 1 {
        return Err("Multiple Content-Length headers are not allowed.".to_string());
    }
    let content_length = content_lengths.first().copied().unwrap_or(0);
    if content_length > MAX_HTTP_BODY {
        return Err(format!(
            "HTTP body exceeds the {} byte limit.",
            MAX_HTTP_BODY
        ));
    }

    while bytes.len() - header_end < content_length {
        let read = stream.read(&mut chunk).map_err(|e| e.to_string())?;
        if read == 0 {
            return Err("Connection closed before the HTTP body was complete.".to_string());
        }
        bytes.extend_from_slice(&chunk[..read]);
        if bytes.len() - header_end > MAX_HTTP_BODY {
            return Err(format!(
                "HTTP body exceeds the {} byte limit.",
                MAX_HTTP_BODY
            ));
        }
    }

    Ok(HttpRequest {
        method,
        path,
        headers,
        body: bytes[header_end..header_end + content_length].to_vec(),
    })
}

fn handle_http_request(
    root: &Path,
    bearer_token: &str,
    allowed_origins: &[String],
    request: &HttpRequest,
) -> HttpResponse {
    if request.path.split('?').next() != Some("/mcp") {
        return HttpResponse::text(404, "Not Found", "Not found.");
    }
    if request.header_count("origin") > 1 {
        return HttpResponse::text(403, "Forbidden", "Multiple Origin headers are not allowed.");
    }
    if let Some(origin) = request.header("origin")
        && !allowed_origins.iter().any(|allowed| allowed == origin)
    {
        return HttpResponse::text(403, "Forbidden", "Origin is not allowed.");
    }
    if request.method == "GET" {
        return HttpResponse::text(
            405,
            "Method Not Allowed",
            "This server does not provide an SSE event stream.",
        )
        .with_header("Allow", "POST");
    }
    if request.method != "POST" {
        return HttpResponse::text(405, "Method Not Allowed", "Use POST for MCP messages.")
            .with_header("Allow", "POST");
    }
    if request.header_count("authorization") != 1
        || !valid_bearer_token(request.header("authorization"), bearer_token)
    {
        return HttpResponse::text(401, "Unauthorized", "A valid bearer token is required.")
            .with_header("WWW-Authenticate", "Bearer realm=\"centaur\"");
    }
    if !request
        .header("content-type")
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    {
        return HttpResponse::text(415, "Unsupported Media Type", "Use application/json.");
    }

    let message: Value = match serde_json::from_slice::<Value>(&request.body) {
        Ok(value) if value.is_object() => value,
        Ok(_) => {
            return HttpResponse::json(
                400,
                "Bad Request",
                &error_response(Value::Null, -32600, "JSON-RPC message must be an object"),
            );
        }
        Err(e) => {
            return HttpResponse::json(
                400,
                "Bad Request",
                &error_response(Value::Null, -32700, &e.to_string()),
            );
        }
    };

    match handle_message(root, &message) {
        Some(response) => HttpResponse::json(200, "OK", &response),
        None => HttpResponse::empty(202, "Accepted"),
    }
}

fn valid_bearer_token(header: Option<&str>, expected: &str) -> bool {
    let Some((scheme, token)) = header.and_then(|value| value.split_once(' ')) else {
        return false;
    };
    scheme.eq_ignore_ascii_case("bearer") && constant_time_eq(token.as_bytes(), expected.as_bytes())
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

fn write_http_response(stream: &mut TcpStream, response: &HttpResponse) -> Result<(), String> {
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        response.status,
        response.reason,
        response.content_type,
        response.body.len()
    )
    .map_err(|e| e.to_string())?;
    for (name, value) in &response.headers {
        write!(stream, "{}: {}\r\n", name, value).map_err(|e| e.to_string())?;
    }
    write!(stream, "\r\n").map_err(|e| e.to_string())?;
    stream
        .write_all(&response.body)
        .map_err(|e| e.to_string())?;
    stream.flush().map_err(|e| e.to_string())
}

fn dispatch(root: &Path, id: &Value, request: &Value) -> Value {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
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
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let arguments = params
                .get("arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
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
            "name": "get_context",
            "description": "Read the Centaur workspace: returns the project's directory map and \
                            file contents, ready to reason over. Call this before apply_patch so \
                            the Search text you write matches the file exactly. Large projects \
                            come back in numbered parts; keep calling with the next 'part' until \
                            you have them all.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "mode": {
                        "type": "string",
                        "enum": ["full", "changed", "staged", "compact"],
                        "description": "full: every eligible file (default). changed: files \
                                        modified or untracked in Git. staged: staged files only. \
                                        compact: full, but generated and repetitive files are \
                                        replaced by short summaries."
                    },
                    "paths": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Restrict a full or compact read to these workspace-relative \
                                        files and directories. Omit to read everything."
                    },
                    "part": {
                        "type": "integer",
                        "description": "Which part to return, starting at 1. Default 1."
                    },
                    "redact": {
                        "type": "boolean",
                        "description": "Replace detected credentials with placeholders. Default \
                                        false; a patch whose Search text covers a redacted line \
                                        will not match."
                    }
                }
            },
            "annotations": {
                "title": "Read workspace context",
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        },
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
            },
            "annotations": {
                "title": "Apply workspace patch",
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": false,
                "openWorldHint": false
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
            },
            "annotations": {
                "title": "Undo workspace patch",
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": false,
                "openWorldHint": false
            }
        }
    ])
}

/// Returns (report, is_error). Tool failures are reported in-band with `isError` so
/// the model can read the reason and correct itself; only protocol faults become
/// JSON-RPC errors.
pub(crate) fn call_tool(root: &Path, name: &str, arguments: &Value) -> (String, bool) {
    match name {
        "get_context" => get_context(root, arguments),
        "apply_patch" => apply_patch(root, arguments),
        "undo" => undo(root, arguments),
        _ => (format!("Unknown tool: {}", name), true),
    }
}

/// Reads the workspace back to the model. Without this the server is write-only:
/// a client with no filesystem of its own could apply patches but had no way to
/// see what it was patching, so the user still had to run a CLI export by hand.
fn get_context(root: &Path, arguments: &Value) -> (String, bool) {
    let mode = match arguments
        .get("mode")
        .and_then(Value::as_str)
        .unwrap_or("full")
    {
        "full" => ExportMode::Full,
        "changed" => ExportMode::Changed,
        "staged" => ExportMode::Staged,
        "compact" => ExportMode::Compact,
        other => {
            return (
                format!(
                    "Unknown mode '{}'. Use full, changed, staged, or compact.",
                    other
                ),
                true,
            );
        }
    };

    // These paths come from the model, so they get the containment check that patch
    // targets get. collect_files would otherwise walk an absolute path out of the
    // workspace, which is exactly the invariant a fixed workspace root exists to hold.
    let mut paths = Vec::new();
    if let Some(values) = arguments.get("paths").and_then(Value::as_array) {
        for value in values {
            let Some(text) = value.as_str() else {
                return ("Every entry in 'paths' must be a string.".to_string(), true);
            };
            match is_safe_path(root, text) {
                Ok(path) => paths.push(path.to_string_lossy().into_owned()),
                Err(e) => return (format!("refused: {}", e), true),
            }
        }
    }

    let files = collect_files(root, mode, &paths);
    if files.is_empty() {
        return (
            "No files matched. In 'changed' or 'staged' mode this means the Git working tree is \
             clean; use mode 'full' to read the whole project."
                .to_string(),
            false,
        );
    }

    let warnings = scan_files(&files);
    let redact = arguments
        .get("redact")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    // Packing is repeated per part rather than cached, so paging a repository is
    // quadratic in the number of parts. At repository scale the walk is far cheaper
    // than the model's own read of the result; cache by session id if that changes.
    let result = pack_files_dynamic(
        files,
        root,
        PackOptions {
            max_attachment_chars: CONTEXT_PART_CHARS,
            // One part per response. The model pages with `part` instead of batching.
            max_attachments_per_message: 1,
            is_compact_mode: mode == ExportMode::Compact,
            project_root_name: root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            ..PackOptions::default()
        },
    );

    let part = arguments.get("part").and_then(Value::as_u64).unwrap_or(1) as usize;
    let Some(chunk) = result.chunks.get(part.saturating_sub(1)) else {
        return (
            format!(
                "Part {} does not exist; this read has {} part(s).",
                part,
                result.chunks.len()
            ),
            true,
        );
    };

    let mut report = String::new();

    // Only on the first part: repeating these on every page wastes context.
    if part <= 1 {
        if !warnings.is_empty() && !redact {
            report.push_str(
                "warning: possible credentials in this context. Do not repeat these values back, \
                 and tell the user they were sent. Call again with redact=true to mask them.\n",
            );
            for warning in &warnings {
                report.push_str(&format!(
                    "  {}: {}\n",
                    warning.file_path, warning.pattern_name
                ));
            }
            report.push('\n');
        }
        // The directory map lists these, but their contents are absent — without
        // saying so the model reads the gap as an empty file.
        if !result.summary.skipped_files.is_empty() {
            report.push_str("Listed in the directory map but not included:\n");
            for skipped in &result.summary.skipped_files {
                report.push_str(&format!("  {}: {}\n", skipped.path, skipped.reason));
            }
            report.push('\n');
        }
    }

    report.push_str(&if redact {
        redact_secrets(&chunk.content)
    } else {
        chunk.content.clone()
    });

    if !chunk.is_final {
        report.push_str(&format!(
            "\n\n(Part {} of {}. Call get_context again with part={} for the rest.)",
            part,
            result.summary.total_parts,
            part + 1
        ));
    }

    (report, false)
}

fn apply_patch(root: &Path, arguments: &Value) -> (String, bool) {
    let Some(text) = arguments.get("text").and_then(Value::as_str) else {
        return ("Missing required argument: text".to_string(), true);
    };

    let blocks = match parse_patch_payload(text) {
        Ok(PatchPayload::NoChanges) => {
            return (
                "No changes requested; the workspace was left untouched.".to_string(),
                false,
            );
        }
        Ok(PatchPayload::Blocks(blocks)) => blocks,
        Err(error) => return (error, true),
    };

    let dry_run = arguments
        .get("dry_run")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut report = String::new();

    match apply_blocks_transactional(root, &blocks, dry_run) {
        Ok(results) => {
            let write_failed = results
                .iter()
                .any(|r| matches!(r, ApplyResult::IoError(..)));
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
        Ok(restored) if restored.is_empty() => (
            "Session reverted; no files needed restoring.".to_string(),
            false,
        ),
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
        ApplyResult::SourceChanged(path) => {
            format!("source changed after review: {}; create a fresh plan", path)
        }
        ApplyResult::IoError(path, err) => format!("write failed for {}: {}", path, err),
        ApplyResult::SecurityError(err) => format!("refused: {}", err),
    }
}

/// One MCP client's configuration file. Clients differ only in where the file
/// lives and what the server map is called, so the same entry works everywhere.
pub(crate) struct ClientTarget {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) format: Format,
    pub(crate) path: Option<PathBuf>,
}

pub(crate) enum Format {
    /// Editable in place. The payload is the name of the server map.
    Json(&'static str),
    /// Codex-style TOML. These files carry comments and hand-tuned formatting that a
    /// parse-and-rewrite would flatten, so print the snippet and let the user paste it.
    TomlSnippet,
}

pub(crate) fn known_clients(workspace: &Path) -> Vec<ClientTarget> {
    let home = dirs::home_dir();
    let in_home = |parts: &[&str]| {
        home.as_ref()
            .map(|h| parts.iter().fold(h.clone(), |path, part| path.join(part)))
    };
    // config_dir is %APPDATA% on Windows, ~/Library/Application Support on macOS,
    // and ~/.config on Linux, which is where Claude Desktop looks on each.
    let config = dirs::config_dir();

    vec![
        ClientTarget {
            id: "claude-desktop",
            label: "Claude Desktop",
            format: Format::Json("mcpServers"),
            path: config.map(|c| c.join("Claude").join("claude_desktop_config.json")),
        },
        ClientTarget {
            id: "antigravity",
            label: "Antigravity",
            format: Format::Json("mcpServers"),
            path: in_home(&[".gemini", "antigravity", "mcp_config.json"]),
        },
        ClientTarget {
            id: "antigravity-ide",
            label: "Antigravity IDE",
            format: Format::Json("mcpServers"),
            path: in_home(&[".gemini", "antigravity-ide", "mcp_config.json"]),
        },
        ClientTarget {
            id: "cursor",
            label: "Cursor",
            format: Format::Json("mcpServers"),
            path: in_home(&[".cursor", "mcp.json"]),
        },
        ClientTarget {
            id: "windsurf",
            label: "Windsurf",
            format: Format::Json("mcpServers"),
            path: in_home(&[".codeium", "windsurf", "mcp_config.json"]),
        },
        ClientTarget {
            id: "gemini-cli",
            label: "Gemini CLI",
            format: Format::Json("mcpServers"),
            path: in_home(&[".gemini", "settings.json"]),
        },
        ClientTarget {
            id: "vscode",
            label: "VS Code (this project)",
            format: Format::Json("servers"),
            path: Some(workspace.join(".vscode").join("mcp.json")),
        },
        ClientTarget {
            id: "codex",
            label: "ChatGPT desktop and Codex CLI",
            format: Format::TomlSnippet,
            path: in_home(&[".codex", "config.toml"]),
        },
    ]
}

fn client_listing(workspace: &Path) -> String {
    let mut out = String::from("Known MCP clients:\n\n");
    for client in known_clients(workspace) {
        let (state, location) = match &client.path {
            Some(path) if path.exists() => ("found   ", path.display().to_string()),
            Some(path) => ("new file", path.display().to_string()),
            None => ("n/a     ", "could not resolve a home directory".to_string()),
        };
        let note = match client.format {
            Format::Json(_) => "",
            Format::TomlSnippet => "  (prints a snippet to paste)",
        };
        out.push_str(&format!(
            "  {:<16} {}  {}{}\n",
            client.id, state, location, note
        ));
    }
    out.push_str("\nInstall with:  centaur mcp install --client <id>\n");
    out.push_str("Any other client:  centaur mcp install --config <path to its config file>\n");
    out
}

/// Adds (or updates) the Centaur entry in a client's MCP configuration. Every other
/// key in the file is preserved: these files hold the user's other servers.
pub fn install(
    client: Option<&str>,
    config: Option<&Path>,
    workspace: &Path,
    name: &str,
) -> Result<String, String> {
    let workspace = std::path::absolute(workspace).map_err(|e| {
        format!(
            "Could not resolve workspace '{}': {}",
            workspace.display(),
            e
        )
    })?;
    if !workspace.is_dir() {
        return Err(format!(
            "Workspace is not a directory: {}",
            workspace.display()
        ));
    }

    if client == Some("all") {
        let targets = known_clients(&workspace);
        let mut reports = Vec::new();
        for target in targets {
            if let Ok(rep) = install(Some(target.id), None, &workspace, name) {
                reports.push(format!("[{}]\n{}", target.id, rep));
            }
        }
        return Ok(format!(
            "Installed Centaur MCP configuration for all clients:\n\n{}",
            reports.join("\n\n")
        ));
    }

    let executable = std::env::current_exe()
        .map_err(|e| format!("Could not resolve the centaur executable: {}", e))?;

    let (path, key) = match (client, config) {
        (Some(id), _) => {
            let target = known_clients(&workspace)
                .into_iter()
                .find(|c| c.id == id)
                .ok_or_else(|| {
                    format!("Unknown client '{}'.\n\n{}", id, client_listing(&workspace))
                })?;
            let path = target.path.ok_or_else(|| {
                format!(
                    "Could not resolve the configuration path for {}.",
                    target.label
                )
            })?;
            match target.format {
                Format::Json(key) => (path, key),
                Format::TomlSnippet => {
                    return Ok(toml_snippet(&path, name, &executable, &workspace));
                }
            }
        }
        (None, Some(path)) => (path.to_path_buf(), "mcpServers"),
        (None, None) => return Ok(client_listing(&workspace)),
    };

    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("Could not read {}: {}", path.display(), e)),
    };

    // Several clients ship this file empty rather than absent.
    let mut root: Value = if existing.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(&existing).map_err(|e| {
            format!(
                "{} is not valid JSON ({}). Fix or move it, then run this again.",
                path.display(),
                e
            )
        })?
    };

    if !root.is_object() {
        return Err(format!(
            "{} does not contain a JSON object.",
            path.display()
        ));
    }

    let replaced = root
        .get(key)
        .and_then(|servers| servers.get(name))
        .is_some();

    let servers = root
        .get(key)
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    root[key] = servers;
    root[key][name] = json!({
        "command": executable.to_string_lossy(),
        "args": ["mcp", "serve", "--workspace", workspace.to_string_lossy()]
    });

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create {}: {}", parent.display(), e))?;
    }
    let rendered = serde_json::to_string_pretty(&root).map_err(|e| e.to_string())?;
    fs::write(&path, rendered).map_err(|e| format!("Could not write {}: {}", path.display(), e))?;

    Ok(format!(
        "{} entry '{}' in {}\n  command:   {}\n  workspace: {}\n\nRestart the client so it picks up the change.",
        if replaced { "Updated" } else { "Added" },
        name,
        path.display(),
        executable.display(),
        workspace.display()
    ))
}

fn toml_snippet(path: &Path, name: &str, executable: &Path, workspace: &Path) -> String {
    // Backslashes are an escape in TOML basic strings, so emit forward slashes.
    let quote = |p: &Path| p.display().to_string().replace('\\', "/");
    format!(
        "Add this to {}:\n\n\
         [mcp_servers.{}]\n\
         command = \"{}\"\n\
         args = [\"mcp\", \"serve\", \"--workspace\", \"{}\"]\n\n\
         Written by hand because that file keeps comments and formatting a rewrite would lose.\n\
         ChatGPT desktop can also add this from its own MCP settings panel: choose STDIO, then\n\
         give it the command and arguments above.",
        path.display(),
        name,
        quote(executable),
        quote(workspace)
    )
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
        assert!(
            report.contains("traversal"),
            "report should name the cause: {}",
            report
        );
        // The transaction aborts as a whole, so the legal block must not land either.
        assert_eq!(std::fs::read_to_string(&inside).unwrap(), "original");
    }

    #[test]
    fn apply_tool_accepts_no_changes_and_rejects_partial_payloads() {
        let dir = tempdir().unwrap();
        let target = dir.path().join("keep.txt");
        fs::write(&target, "original").unwrap();

        let (report, is_error) =
            call_tool(dir.path(), "apply_patch", &json!({ "text": "NO_CHANGES" }));
        assert!(!is_error, "{}", report);
        assert!(report.contains("left untouched"));

        let malformed = "File: keep.txt\n<<<<<<< SEARCH\noriginal\n=======\npatched\n>>>>>>> REPLACE\n\n\
                         File: other.txt\n<<<<<< SEARCH\n=======\ncreated\n>>>>>> REPLACE\n";
        let (report, is_error) =
            call_tool(dir.path(), "apply_patch", &json!({ "text": malformed }));
        assert!(is_error, "{}", report);
        assert!(report.contains("parsed 1 of 2"), "{}", report);
        assert_eq!(fs::read_to_string(target).unwrap(), "original");
    }

    #[test]
    fn get_context_returns_file_contents_and_refuses_to_leave_the_workspace() {
        let dir = tempdir().unwrap();
        std::fs::write(
            dir.path().join("hello.rs"),
            "fn main() { println!(\"hi\"); }",
        )
        .unwrap();

        let (report, is_error) = call_tool(dir.path(), "get_context", &json!({ "mode": "full" }));
        assert!(!is_error, "{}", report);
        assert!(
            report.contains("hello.rs"),
            "directory map missing: {}",
            report
        );
        assert!(report.contains("println!"), "file body missing: {}", report);

        let (report, is_error) = call_tool(
            dir.path(),
            "get_context",
            &json!({ "paths": ["../elsewhere"] }),
        );
        assert!(is_error, "traversal must be refused: {}", report);
    }

    #[test]
    fn install_keeps_the_rest_of_the_client_config() {
        let dir = tempdir().unwrap();
        let config = dir.path().join("client.json");
        fs::write(
            &config,
            r#"{"preferences":{"theme":"dark"},"mcpServers":{"playwright":{"command":"npx"}}}"#,
        )
        .unwrap();

        let report = install(None, Some(&config), dir.path(), "centaur").unwrap();
        assert!(report.contains("Added"), "{}", report);

        let written: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(written["preferences"]["theme"], "dark");
        assert_eq!(written["mcpServers"]["playwright"]["command"], "npx");
        assert_eq!(written["mcpServers"]["centaur"]["args"][1], "serve");
    }

    #[test]
    fn install_handles_an_empty_config_file() {
        // Antigravity ships mcp_config.json as a zero-byte file.
        let dir = tempdir().unwrap();
        let config = dir.path().join("mcp_config.json");
        fs::write(&config, "").unwrap();

        install(None, Some(&config), dir.path(), "centaur").unwrap();

        let written: Value = serde_json::from_str(&fs::read_to_string(&config).unwrap()).unwrap();
        assert!(written["mcpServers"]["centaur"]["command"].is_string());
    }

    #[test]
    fn http_transport_requires_auth_and_marks_write_tools() {
        let dir = tempdir().unwrap();
        let token = "0123456789abcdef0123456789abcdef";
        let body = br#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#.to_vec();
        let request = |authorization: Option<&str>, origin: Option<&str>| {
            let mut headers = vec![("Content-Type".to_string(), "application/json".to_string())];
            if let Some(value) = authorization {
                headers.push(("Authorization".to_string(), value.to_string()));
            }
            if let Some(value) = origin {
                headers.push(("Origin".to_string(), value.to_string()));
            }
            HttpRequest {
                method: "POST".to_string(),
                path: "/mcp".to_string(),
                headers,
                body: body.clone(),
            }
        };

        let unauthorized = handle_http_request(dir.path(), token, &[], &request(None, None));
        assert_eq!(unauthorized.status, 401);

        let rejected_origin = handle_http_request(
            dir.path(),
            token,
            &[],
            &request(
                Some(&format!("Bearer {}", token)),
                Some("https://example.com"),
            ),
        );
        assert_eq!(rejected_origin.status, 403);

        let response = handle_http_request(
            dir.path(),
            token,
            &[],
            &request(Some(&format!("Bearer {}", token)), None),
        );
        assert_eq!(response.status, 200);
        let message: Value = serde_json::from_slice(&response.body).unwrap();
        let tools = message["result"]["tools"].as_array().unwrap();
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
        assert_eq!(tools[1]["annotations"]["destructiveHint"], true);
    }

    #[test]
    fn http_transport_refuses_non_loopback_bindings() {
        let dir = tempdir().unwrap();
        let result = serve_http(
            dir.path(),
            "0.0.0.0:3765".parse().unwrap(),
            "0123456789abcdef0123456789abcdef",
            &[],
        );
        assert!(result.unwrap_err().contains("loopback"));
    }

    #[test]
    fn http_transport_handles_a_real_loopback_request() {
        let dir = tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let request = read_http_request(&mut stream).unwrap();
            let response =
                handle_http_request(&root, "0123456789abcdef0123456789abcdef", &[], &request);
            write_http_response(&mut stream, &response).unwrap();
        });

        let body = r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#;
        let mut client = TcpStream::connect(address).unwrap();
        write!(
            client,
            "POST /mcp HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer 0123456789abcdef0123456789abcdef\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();

        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("\"name\":\"get_context\""));
    }
}
