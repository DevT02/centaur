pub mod config;
pub mod doctor;
pub mod export;
pub mod git;
pub mod history;
pub mod mcp;
pub mod pack;
pub mod patch;
pub mod prompt;
pub mod review;
pub mod secrets;
pub mod skill;
pub mod ui;
pub mod update;

use regex::Regex;
use std::sync::LazyLock;

pub use config::CentaurConfig;
pub use git::ExportMode;
pub use pack::{PackOptions, PackResult, pack_files_dynamic};
pub use patch::{
    ApplyResult, MemoryPatchPlan, apply_blocks_transactional, apply_planned_transactional,
    plan_blocks_transactional,
};
pub use prompt::{get_prompt_template, render_prompt};
pub use review::{PatchFileSummary, render_patch_plan, summarize_patch_blocks};

#[cfg(test)]
pub(crate) mod test_support {
    /// Point CENTAUR_HOME at a scratch directory so tests never write patch history
    /// into the developer's real config directory.
    pub(crate) fn use_scratch_home() {
        use std::sync::Once;
        static INIT: Once = Once::new();
        INIT.call_once(|| {
            let home =
                std::env::temp_dir().join(format!("centaur_test_home_{}", std::process::id()));
            let _ = std::fs::create_dir_all(&home);
            // Safety: set once, before any test thread reads CENTAUR_HOME.
            unsafe { std::env::set_var("CENTAUR_HOME", &home) };
        });
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct PatchBlock {
    pub file_path: String,
    pub search: String,
    pub replace: String,
}

#[derive(Debug, PartialEq, Clone)]
pub enum PatchPayload {
    NoChanges,
    Blocks(Vec<PatchBlock>),
}

/// Parse one complete model response. A payload is accepted only when every
/// apparent Search/Replace block parses, so a malformed edit can never be
/// silently dropped while the remaining blocks are applied.
pub fn parse_patch_payload(text: &str) -> Result<PatchPayload, String> {
    if text.trim() == "NO_CHANGES" {
        return Ok(PatchPayload::NoChanges);
    }

    let blocks = parse_blocks(text);
    let markers = count_search_markers(text);

    if blocks.is_empty() {
        return Err(
            "No valid Search/Replace blocks found. Expected a Centaur patch payload or exactly NO_CHANGES."
                .to_string(),
        );
    }

    if markers != blocks.len() {
        return Err(format!(
            "Refusing the entire payload: parsed {} of {} apparent Search/Replace blocks. Fix every delimiter and try again.",
            blocks.len(),
            markers
        ));
    }

    Ok(PatchPayload::Blocks(blocks))
}

// Compiled once; parse_blocks runs several times per TUI launch.
static BLOCK_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?sm)^[ \t]*(?i:File|Path):[ \t]*([^\r\n]+?)[ \t]*\r?\n^[ \t]*<<<<<<< SEARCH[ \t]*\r?\n(.*?)\r?\n^[ \t]*=======[ \t]*\r?\n(.*?)\r?\n^[ \t]*>>>>>>> REPLACE[ \t]*|(?sm)^[ \t]*(?i:File|Path):[ \t]*([^\r\n]+?)[ \t]*\r?\n^[ \t]*<<<<<<< SEARCH[ \t]*\r?\n^[ \t]*=======[ \t]*\r?\n(.*?)\r?\n^[ \t]*>>>>>>> REPLACE[ \t]*").unwrap()
});

pub fn parse_blocks(text: &str) -> Vec<PatchBlock> {
    let mut raw_blocks = Vec::new();
    for cap in BLOCK_RE.captures_iter(text) {
        if let Some(file_mat) = cap.get(1) {
            raw_blocks.push(PatchBlock {
                file_path: file_mat.as_str().trim().to_string(),
                search: cap.get(2).map(|m| m.as_str()).unwrap_or("").to_string(),
                replace: cap.get(3).map(|m| m.as_str()).unwrap_or("").to_string(),
            });
        } else if let Some(file_mat) = cap.get(4) {
            raw_blocks.push(PatchBlock {
                file_path: file_mat.as_str().trim().to_string(),
                search: String::new(),
                replace: cap.get(5).map(|m| m.as_str()).unwrap_or("").to_string(),
            });
        }
    }

    if raw_blocks.is_empty() {
        raw_blocks = parse_blocks_state_machine(text);
    }

    raw_blocks
        .into_iter()
        .filter_map(clean_patch_block)
        .collect()
}

/// Count SEARCH markers, tolerating the delimiter typos models actually make
/// (wrong chevron count, stray spacing). Compared against the number of blocks
/// parsed, this reveals edits that would otherwise be dropped in silence.
pub fn count_search_markers(text: &str) -> usize {
    static MARKER_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?m)^[ \t]*<{4,9}[ \t]*SEARCH\b").unwrap());
    MARKER_RE.find_iter(text).count()
}

pub fn clean_patch_block(mut block: PatchBlock) -> Option<PatchBlock> {
    let mut path = block.file_path.trim().to_string();

    // Strip surrounding quotes or backticks
    if ((path.starts_with('`') && path.ends_with('`'))
        || (path.starts_with('"') && path.ends_with('"'))
        || (path.starts_with('\'') && path.ends_with('\'')))
        && path.len() >= 2
    {
        path = path[1..path.len() - 1].trim().to_string();
    }

    // Strip leading ./ or .\ or / or \
    while path.starts_with("./") || path.starts_with(".\\") {
        path = path[2..].to_string();
    }
    while path.starts_with('/') || path.starts_with('\\') {
        path = path[1..].to_string();
    }

    // Reject literal placeholder paths outputted by confused LLMs
    if path.is_empty()
        || path.contains('<')
        || path.contains('>')
        || path.eq_ignore_ascii_case("relative file path")
        || path.eq_ignore_ascii_case("path/to/file")
    {
        return None;
    }

    // In prose formats a fence line is content, not packaging. Stripping it silently
    // corrupted every patch that touched a fenced block in a Markdown file.
    let is_prose = ["md", "markdown", "mdx", "rst", "txt", "adoc"]
        .iter()
        .any(|ext| path.to_lowercase().ends_with(&format!(".{}", ext)));

    block.file_path = path;

    if !is_prose {
        // Clean stray code fence lines (e.g. ```, ```rust, ```text) from search and replace
        block.search = strip_stray_code_fences(&block.search);
        block.replace = strip_stray_code_fences(&block.replace);
    }

    Some(block)
}

fn strip_stray_code_fences(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return content.to_string();
    }

    let mut start = 0;
    let mut end = lines.len();

    // Strip leading fence line if present (e.g. ```, ```rust, ```text)
    if lines[0].trim().starts_with("```") {
        start += 1;
    }

    // Strip trailing fence line if present (e.g. ```)
    if end > start && lines[end - 1].trim().starts_with("```") {
        end -= 1;
    }

    if start == 0 && end == lines.len() {
        content.to_string()
    } else {
        lines[start..end].join("\n")
    }
}

fn parse_blocks_state_machine(text: &str) -> Vec<PatchBlock> {
    let mut blocks = Vec::new();
    let mut current_file = String::new();
    let mut current_search = String::new();
    let mut current_replace = String::new();

    #[derive(PartialEq)]
    enum State {
        LookingForFile,
        LookingForSearch,
        InSearch,
        InReplace,
    }

    let mut state = State::LookingForFile;
    let lines = text.replace("\r\n", "\n");
    let lines_iter = lines.split('\n');

    for line in lines_iter {
        let trimmed = line.trim();
        match state {
            State::LookingForFile => {
                if trimmed.starts_with("File:")
                    || trimmed.starts_with("file:")
                    || trimmed.starts_with("Path:")
                {
                    let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        current_file = parts[1].trim().to_string();
                        state = State::LookingForSearch;
                    }
                }
            }
            State::LookingForSearch => {
                if trimmed == "<<<<<<< SEARCH" {
                    current_search.clear();
                    state = State::InSearch;
                } else if trimmed.starts_with("File:")
                    || trimmed.starts_with("file:")
                    || trimmed.starts_with("Path:")
                {
                    let parts: Vec<&str> = trimmed.splitn(2, ':').collect();
                    if parts.len() == 2 {
                        current_file = parts[1].trim().to_string();
                    }
                }
            }
            State::InSearch => {
                if trimmed == "=======" {
                    state = State::InReplace;
                    current_replace.clear();
                    if current_search.ends_with('\n') {
                        current_search.pop();
                    }
                } else {
                    current_search.push_str(line);
                    current_search.push('\n');
                }
            }
            State::InReplace => {
                if trimmed == ">>>>>>> REPLACE" {
                    if current_replace.ends_with('\n') {
                        current_replace.pop();
                    }
                    blocks.push(PatchBlock {
                        file_path: current_file.clone(),
                        search: current_search.clone(),
                        replace: current_replace.clone(),
                    });
                    state = State::LookingForFile;
                } else {
                    current_replace.push_str(line);
                    current_replace.push('\n');
                }
            }
        }
    }

    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_standard_block() {
        let input = r#"
File: src/main.rs
<<<<<<< SEARCH
fn old() {}
=======
fn new() {}
>>>>>>> REPLACE
"#;
        let blocks = parse_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].file_path, "src/main.rs");
        assert_eq!(blocks[0].search, "fn old() {}");
        assert_eq!(blocks[0].replace, "fn new() {}");
    }

    #[test]
    fn test_parse_windows_newlines() {
        let input = "File: src/main.rs\r\n<<<<<<< SEARCH\r\nfn old() {}\r\n=======\r\nfn new() {}\r\n>>>>>>> REPLACE\r\n";
        let blocks = parse_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].file_path, "src/main.rs");
    }

    #[test]
    fn test_markdown_fences_are_content_not_packaging() {
        let input = "File: README.md\n<<<<<<< SEARCH\n```sh\ncargo build\n```\n=======\n```sh\ncargo build --release\n```\n>>>>>>> REPLACE\n";
        let blocks = parse_blocks(input);
        assert_eq!(blocks[0].search, "```sh\ncargo build\n```");
        assert_eq!(blocks[0].replace, "```sh\ncargo build --release\n```");
    }

    #[test]
    fn test_dropped_blocks_are_countable() {
        // Second block has a malformed delimiter, so only one parses.
        let input = "File: a.rs\n<<<<<<< SEARCH\nold\n=======\nnew\n>>>>>>> REPLACE\n\n\
                     File: b.rs\n<<<<<< SEARCH\nold b\n=======\nnew b\n>>>>>> REPLACE\n";
        assert_eq!(parse_blocks(input).len(), 1);
        assert_eq!(
            count_search_markers(input),
            2,
            "both markers should be seen"
        );
    }

    #[test]
    fn complete_payload_parser_accepts_an_explicit_noop() {
        assert_eq!(
            parse_patch_payload("  NO_CHANGES\r\n").unwrap(),
            PatchPayload::NoChanges
        );
    }

    #[test]
    fn complete_payload_parser_rejects_a_partially_malformed_response() {
        let input = "File: a.rs\n<<<<<<< SEARCH\nold\n=======\nnew\n>>>>>>> REPLACE\n\n\
                     File: b.rs\n<<<<<< SEARCH\nold b\n=======\nnew b\n>>>>>> REPLACE\n";

        let error = parse_patch_payload(input).unwrap_err();
        assert!(error.contains("parsed 1 of 2"), "{error}");
    }

    #[test]
    fn test_path_normalization_and_stray_fence_cleaning() {
        let input = r#"
File: ./src/main.rs
<<<<<<< SEARCH
```rust
fn old() {}
```
=======
fn new() {}
>>>>>>> REPLACE

File: <relative file path>
<<<<<<< SEARCH
placeholder
=======
replacement
>>>>>>> REPLACE
"#;
        let blocks = parse_blocks(input);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].file_path, "src/main.rs");
        assert_eq!(blocks[0].search, "fn old() {}");
        assert_eq!(blocks[0].replace, "fn new() {}");
    }
}
