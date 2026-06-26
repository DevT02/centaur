use regex::Regex;
use std::fs;
use std::path::Path;

#[derive(Debug, PartialEq, Clone)]
pub struct PatchBlock {
    pub file_path: String,
    pub search: String,
    pub replace: String,
}

pub fn parse_blocks(text: &str) -> Vec<PatchBlock> {
    // We use a regex that is flexible with whitespace, handles File/file/Path,
    // and correctly isolates the search and replace blocks even if indented or having trailing spaces.
    let pattern = r"(?sm)^[ \t]*(?:File|file|Path):[ \t]*([^\r\n]+?)[ \t]*\r?\n^[ \t]*<<<<<<< SEARCH[ \t]*\r?\n(.*?)\r?\n^[ \t]*=======[ \t]*\r?\n(.*?)\r?\n^[ \t]*>>>>>>> REPLACE[ \t]*";
    let re = Regex::new(pattern).unwrap();

    let mut blocks = Vec::new();
    for cap in re.captures_iter(text) {
        blocks.push(PatchBlock {
            file_path: cap[1].trim().to_string(),
            search: cap[2].to_string(),
            replace: cap[3].to_string(),
        });
    }
    
    // Fallback: If no blocks found using regex, try state-machine based parsing.
    if blocks.is_empty() {
        blocks = parse_blocks_state_machine(text);
    }

    blocks
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
    
    // Normalize newlines to \n to make parsing easier
    let lines = text.replace("\r\n", "\n");
    let mut lines_iter = lines.split('\n').peekable();
    
    while let Some(line) = lines_iter.next() {
        let trimmed = line.trim();
        match state {
            State::LookingForFile => {
                if trimmed.starts_with("File:") || trimmed.starts_with("file:") || trimmed.starts_with("Path:") {
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
                } else if trimmed.starts_with("File:") || trimmed.starts_with("file:") || trimmed.starts_with("Path:") {
                    // Reset if we see another file marker before search
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
                    
                    // Remove trailing newline from search
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
                    // Remove trailing newline from replace
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

#[derive(Debug, PartialEq)]
pub enum ApplyResult {
    Created(String),
    Updated(String),
    MatchNotFound(String),
    IoError(String, String),
    SecurityError(String),
}

pub fn apply_block(base_dir: &Path, block: &PatchBlock) -> ApplyResult {
    // Prevent path traversal
    if block.file_path.contains("..") {
        return ApplyResult::SecurityError(format!("Path traversal attempt detected: {}", block.file_path));
    }
    
    let file_path = base_dir.join(&block.file_path);
    let file_path_str = block.file_path.clone();

    if !file_path.exists() {
        if let Some(parent) = file_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                return ApplyResult::IoError(file_path_str, e.to_string());
            }
        }
        match fs::write(&file_path, &block.replace) {
            Ok(_) => return ApplyResult::Created(file_path_str),
            Err(e) => return ApplyResult::IoError(file_path_str, e.to_string()),
        }
    }

    let content = match fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => return ApplyResult::IoError(file_path_str, e.to_string()),
    };

    if content.contains(&block.search) {
        let updated = content.replacen(&block.search, &block.replace, 1);
        match fs::write(&file_path, updated) {
            Ok(_) => ApplyResult::Updated(file_path_str),
            Err(e) => ApplyResult::IoError(file_path_str, e.to_string()),
        }
    } else {
        // Try line-ending normalization
        let norm_content = content.replace("\r\n", "\n");
        let norm_search = block.search.replace("\r\n", "\n");
        if norm_content.contains(&norm_search) {
            let norm_replace = block.replace.replace("\r\n", "\n");
            let updated = norm_content.replacen(&norm_search, &norm_replace, 1);
            
            // Re-apply original line endings if needed
            let final_content = if content.contains("\r\n") {
                updated.replace("\n", "\r\n")
            } else {
                updated
            };
            
            match fs::write(&file_path, final_content) {
                Ok(_) => ApplyResult::Updated(file_path_str),
                Err(e) => ApplyResult::IoError(file_path_str, e.to_string()),
            }
        } else {
            // Try whitespace agnostic matching
            if let Some(updated) = apply_fuzzy_match(&content, &block.search, &block.replace) {
                match fs::write(&file_path, updated) {
                    Ok(_) => ApplyResult::Updated(file_path_str),
                    Err(e) => ApplyResult::IoError(file_path_str, e.to_string()),
                }
            } else {
                ApplyResult::MatchNotFound(file_path_str)
            }
        }
    }
}

// Very basic fuzzy match: ignore leading/trailing whitespace on lines
fn apply_fuzzy_match(content: &str, search: &str, replace: &str) -> Option<String> {
    let content_lines: Vec<&str> = content.lines().collect();
    let search_lines: Vec<&str> = search.lines().map(|l| l.trim()).collect();
    
    if search_lines.is_empty() { return None; }
    
    let mut match_idx = None;
    for i in 0..=content_lines.len().saturating_sub(search_lines.len()) {
        let mut matches = true;
        for j in 0..search_lines.len() {
            if content_lines[i + j].trim() != search_lines[j] {
                matches = false;
                break;
            }
        }
        if matches {
            match_idx = Some(i);
            break;
        }
    }
    
    if let Some(idx) = match_idx {
        let mut new_lines = Vec::new();
        new_lines.extend_from_slice(&content_lines[..idx]);
        new_lines.push(replace);
        new_lines.extend_from_slice(&content_lines[idx + search_lines.len()..]);
        // Use standard newline, could be improved to detect CRLF vs LF
        let mut result = new_lines.join("\n");
        if content.ends_with("\r\n") || content.ends_with('\n') {
            result.push('\n');
        }
        Some(result)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_parse_standard_block() {
        let text = "File: test.txt\n<<<<<<< SEARCH\nfoo\n=======\nbar\n>>>>>>> REPLACE";
        let blocks = parse_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].file_path, "test.txt");
        assert_eq!(blocks[0].search, "foo");
        assert_eq!(blocks[0].replace, "bar");
    }

    #[test]
    fn test_parse_windows_newlines() {
        let text = "File: src/main.rs\r\n<<<<<<< SEARCH\r\nfn main() {\r\n}\r\n=======\r\nfn main() {\r\n    println!();\r\n}\r\n>>>>>>> REPLACE";
        let blocks = parse_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].file_path, "src/main.rs");
        // Windows line endings in the search block are retained (or at least consistently parsed)
        assert_eq!(blocks[0].search, "fn main() {\r\n}");
        assert_eq!(blocks[0].replace, "fn main() {\r\n    println!();\r\n}");
    }

    #[test]
    fn test_parse_multiple_blocks() {
        let text = "
File: one.txt
<<<<<<< SEARCH
1
=======
2
>>>>>>> REPLACE

Some chatgpt text here.

file: two.txt
<<<<<<< SEARCH
3
=======
4
>>>>>>> REPLACE
";
        let blocks = parse_blocks(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].file_path, "one.txt");
        assert_eq!(blocks[1].file_path, "two.txt");
    }
    
    #[test]
    fn test_parse_indented_markers() {
        let text = "
  File: a.txt
  <<<<<<< SEARCH
  a
  =======
  b
  >>>>>>> REPLACE
";
        let blocks = parse_blocks(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].file_path, "a.txt");
        assert_eq!(blocks[0].search, "  a");
        assert_eq!(blocks[0].replace, "  b");
    }

    #[test]
    fn test_parse_state_machine_fallback() {
        let text = "Path: weird.txt\nSome noise\n<<<<<<< SEARCH\nabc\n=======\ndef\n>>>>>>> REPLACE";
        let blocks = parse_blocks_state_machine(text);
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].file_path, "weird.txt");
        assert_eq!(blocks[0].search, "abc");
        assert_eq!(blocks[0].replace, "def");
    }

    #[test]
    fn test_path_traversal_prevention() {
        let block = PatchBlock {
            file_path: "../../../etc/passwd".to_string(),
            search: "".to_string(),
            replace: "hacked".to_string(),
        };
        let dir = tempdir().unwrap();
        let res = apply_block(dir.path(), &block);
        assert!(matches!(res, ApplyResult::SecurityError(_)));
    }

    #[test]
    fn test_apply_create_new_file() {
        let dir = tempdir().unwrap();
        let block = PatchBlock {
            file_path: "new/dir/file.rs".to_string(),
            search: "".to_string(),
            replace: "fn main() {}".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        assert_eq!(res, ApplyResult::Created("new/dir/file.rs".to_string()));
        let content = fs::read_to_string(dir.path().join("new/dir/file.rs")).unwrap();
        assert_eq!(content, "fn main() {}");
    }

    #[test]
    fn test_apply_exact_match() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\nline2\nline3\n").unwrap();
        
        let block = PatchBlock {
            file_path: "test.txt".to_string(),
            search: "line2".to_string(),
            replace: "replaced".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        assert_eq!(res, ApplyResult::Updated("test.txt".to_string()));
        
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\nreplaced\nline3\n");
    }

    #[test]
    fn test_apply_crlf_normalization() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line1\r\nline2\r\nline3").unwrap();
        
        let block = PatchBlock {
            file_path: "test.txt".to_string(),
            search: "line2\nline3".to_string(),
            replace: "replaced\r\nline3".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        assert_eq!(res, ApplyResult::Updated("test.txt".to_string()));
    }

    #[test]
    fn test_apply_fuzzy_match() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        // File has different indentation
        fs::write(&file_path, "fn main() {\n    println!();\n}").unwrap();
        
        let block = PatchBlock {
            file_path: "test.txt".to_string(),
            // Search block missing some spaces
            search: "fn main() {\n println!();\n}".to_string(),
            replace: "fn main() {\n    println!(\"fixed\");\n}".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        assert_eq!(res, ApplyResult::Updated("test.txt".to_string()));
        
        let content = fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "fn main() {\n    println!(\"fixed\");\n}");
    }
    
    #[test]
    fn test_no_match() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "foo").unwrap();
        
        let block = PatchBlock {
            file_path: "test.txt".to_string(),
            search: "bar".to_string(),
            replace: "baz".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        assert_eq!(res, ApplyResult::MatchNotFound("test.txt".to_string()));
    }
}
