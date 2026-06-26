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

#[derive(Debug, PartialEq)]
pub enum ApplyResult {
    Created(String),
    Updated(String),
    MatchNotFound(String),
    AmbiguousMatch(String),
    IoError(String, String),
    SecurityError(String),
}

pub fn apply_block(base_dir: &Path, block: &PatchBlock) -> ApplyResult {
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

    let mut content = match fs::read_to_string(&file_path) {
        Ok(c) => c,
        Err(e) => return ApplyResult::IoError(file_path_str, e.to_string()),
    };

    // Strip BOM if present
    if content.starts_with('\u{FEFF}') {
        content = content[3..].to_string(); 
    }

    // Deeper check 1: Exact match
    let match_count = content.matches(&block.search).count();
    if match_count > 1 {
        return ApplyResult::AmbiguousMatch(file_path_str);
    }

    if match_count == 1 {
        let updated = content.replacen(&block.search, &block.replace, 1);
        match fs::write(&file_path, updated) {
            Ok(_) => return ApplyResult::Updated(file_path_str),
            Err(e) => return ApplyResult::IoError(file_path_str, e.to_string()),
        }
    }

    // Deeper check 2: Line-ending normalization match
    let norm_content = content.replace("\r\n", "\n");
    let norm_search = block.search.replace("\r\n", "\n");
    
    let norm_match_count = norm_content.matches(&norm_search).count();
    if norm_match_count > 1 {
        return ApplyResult::AmbiguousMatch(file_path_str);
    }
    
    if norm_match_count == 1 {
        let norm_replace = block.replace.replace("\r\n", "\n");
        let updated = norm_content.replacen(&norm_search, &norm_replace, 1);
        
        let final_content = if content.contains("\r\n") {
            updated.replace("\n", "\r\n")
        } else {
            updated
        };
        
        match fs::write(&file_path, final_content) {
            Ok(_) => return ApplyResult::Updated(file_path_str),
            Err(e) => return ApplyResult::IoError(file_path_str, e.to_string()),
        }
    }

    // Deeper check 3: Whitespace-agnostic fuzzy matching
    if let Some(fuzzy_result) = apply_fuzzy_match(&content, &block.search, &block.replace) {
        match fuzzy_result {
            Ok(updated) => {
                match fs::write(&file_path, updated) {
                    Ok(_) => return ApplyResult::Updated(file_path_str),
                    Err(e) => return ApplyResult::IoError(file_path_str, e.to_string()),
                }
            },
            Err(is_ambiguous) => {
                if is_ambiguous {
                    return ApplyResult::AmbiguousMatch(file_path_str);
                }
            }
        }
    }

    ApplyResult::MatchNotFound(file_path_str)
}

// Very basic fuzzy match: ignore leading/trailing whitespace on lines
// Returns Ok(String) if exactly one match found, Err(true) if ambiguous, Err(false) if no match.
fn apply_fuzzy_match(content: &str, search: &str, replace: &str) -> Option<Result<String, bool>> {
    let content_lines: Vec<&str> = content.lines().collect();
    let search_lines: Vec<&str> = search.lines().map(|l| l.trim()).collect();
    
    if search_lines.is_empty() { return None; }
    
    let mut matches_found = vec![];
    for i in 0..=content_lines.len().saturating_sub(search_lines.len()) {
        let mut matches = true;
        for j in 0..search_lines.len() {
            if content_lines[i + j].trim() != search_lines[j] {
                matches = false;
                break;
            }
        }
        if matches {
            matches_found.push(i);
        }
    }
    
    if matches_found.len() > 1 {
        return Some(Err(true));
    }
    
    if let Some(&idx) = matches_found.first() {
        let mut new_lines = Vec::new();
        new_lines.extend_from_slice(&content_lines[..idx]);
        new_lines.push(replace);
        new_lines.extend_from_slice(&content_lines[idx + search_lines.len()..]);
        
        let mut result = new_lines.join("\n");
        if content.ends_with("\r\n") || content.ends_with('\n') {
            result.push('\n');
        }
        Some(Ok(result))
    } else {
        Some(Err(false))
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
        assert_eq!(blocks[0].search, "fn main() {\r\n}");
        assert_eq!(blocks[0].replace, "fn main() {\r\n    println!();\r\n}");
    }

    #[test]
    fn test_parse_multiple_blocks() {
        let text = "\nFile: one.txt\n<<<<<<< SEARCH\n1\n=======\n2\n>>>>>>> REPLACE\n\nSome text\n\nfile: two.txt\n<<<<<<< SEARCH\n3\n=======\n4\n>>>>>>> REPLACE\n";
        let blocks = parse_blocks(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].file_path, "one.txt");
        assert_eq!(blocks[1].file_path, "two.txt");
    }

    #[test]
    fn test_parse_indented_markers() {
        let text = "\n  File: a.txt\n  <<<<<<< SEARCH\n  a\n  =======\n  b\n  >>>>>>> REPLACE\n";
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
        fs::write(&file_path, "fn main() {\n    println!();\n}").unwrap();
        
        let block = PatchBlock {
            file_path: "test.txt".to_string(),
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

    // === NEW DEEPER TESTS ===

    #[test]
    fn test_ambiguous_match() {
        // If the SEARCH block is found multiple times, we should refuse to apply it to prevent corrupting the file
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("ambiguous.txt");
        fs::write(&file_path, "duplicate\nduplicate\n").unwrap();
        
        let block = PatchBlock {
            file_path: "ambiguous.txt".to_string(),
            search: "duplicate".to_string(),
            replace: "replaced".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        assert_eq!(res, ApplyResult::AmbiguousMatch("ambiguous.txt".to_string()));
    }

    #[test]
    fn test_ambiguous_fuzzy_match() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("fuzzy_ambig.txt");
        // Spaced differently but semantically identical lines
        fs::write(&file_path, "  line  \n\tline\t\n").unwrap();
        
        let block = PatchBlock {
            file_path: "fuzzy_ambig.txt".to_string(),
            search: "line".to_string(), // will fuzzily match both
            replace: "replaced".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        assert_eq!(res, ApplyResult::AmbiguousMatch("fuzzy_ambig.txt".to_string()));
    }

    #[test]
    fn test_unicode_support() {
        // Ensuring it handles emojis, cyrillic, or whatever gracefully
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("unicode.txt");
        fs::write(&file_path, "Hello 🌍!\nПривет мир!\n").unwrap();
        
        let block = PatchBlock {
            file_path: "unicode.txt".to_string(),
            search: "Привет мир!".to_string(),
            replace: "Goodbye 🚀".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        assert_eq!(res, ApplyResult::Updated("unicode.txt".to_string()));
        
        let content = fs::read_to_string(&file_path).unwrap();
        assert!(content.contains("Goodbye 🚀"));
    }

    #[test]
    fn test_bom_handling() {
        // If a file has a UTF-8 BOM, we should still match correctly
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("bom.txt");
        let mut with_bom = vec![0xEF, 0xBB, 0xBF];
        with_bom.extend_from_slice(b"content\n");
        fs::write(&file_path, &with_bom).unwrap();
        
        let block = PatchBlock {
            file_path: "bom.txt".to_string(),
            search: "content".to_string(),
            replace: "new_content".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        assert_eq!(res, ApplyResult::Updated("bom.txt".to_string()));
    }

    #[test]
    fn test_empty_search_but_file_exists() {
        // If search is empty but file exists, it should probably be treated carefully.
        // Aider sometimes uses empty search block to mean append to file, 
        // but here it will just match the start of the file or nothing.
        // Our fuzzy matcher will reject empty search lines entirely.
        // And exact match of "" matches everywhere, so it's ambiguous.
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("empty.txt");
        fs::write(&file_path, "existing\n").unwrap();
        
        let block = PatchBlock {
            file_path: "empty.txt".to_string(),
            search: "".to_string(),
            replace: "appended\n".to_string(),
        };
        let res = apply_block(dir.path(), &block);
        // Since "" matches everywhere in the string, matches() returns > 1, so it's ambiguous.
    }
}

pub fn pack_files(files: Vec<std::path::PathBuf>, chunk_limit: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    let mut current_chunk = String::new();

    for path in files {
        if let Ok(content) = std::fs::read_to_string(&path) {
            let normalized_path = path.display().to_string().replace("\\", "/");
            let file_str = format!("File: `{}`\n```\n{}\n```\n\n", normalized_path, content);
            
            if current_chunk.len() + file_str.len() > chunk_limit && !current_chunk.is_empty() {
                chunks.push(current_chunk);
                current_chunk = String::new();
            }
            current_chunk.push_str(&file_str);
        }
    }
    if !current_chunk.is_empty() {
        chunks.push(current_chunk);
    }

    let total_chunks = chunks.len();
    if total_chunks == 0 {
        return chunks;
    }

    for (i, chunk) in chunks.iter_mut().enumerate() {
        let part_num = i + 1;
        let mut final_chunk = format!("(Part {} of {})\n\n", part_num, total_chunks);
        final_chunk.push_str("Here is my codebase context:\n\n");
        final_chunk.push_str(chunk);

        if part_num == total_chunks {
            final_chunk.push_str("\n\n(All parts provided. Please suggest fixes ONLY using the <<<<<<< SEARCH / >>>>>>> REPLACE block format. Do not output the entire file.)");
        } else {
            final_chunk.push_str("\n\n(End of Part {}. Do not analyze yet. Reply ONLY with 'Awaiting next part' until I send the final part.)");
        }

        *chunk = final_chunk;
    }

    chunks
}

#[cfg(test)]
mod pack_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;
    
    #[test]
    fn test_pack_files_single_chunk() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("file1.rs");
        fs::write(&file1, "fn main() {}").unwrap();
        
        let chunks = pack_files(vec![file1.clone()], 100_000);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].contains("File: `"));
        assert!(chunks[0].contains("fn main() {}"));
        assert!(chunks[0].contains("(Part 1 of 1)"));
        assert!(chunks[0].contains("All parts provided."));
    }

    #[test]
    fn test_pack_files_multiple_chunks() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("file1.rs");
        let file2 = dir.path().join("file2.rs");
        fs::write(&file1, "A".repeat(60)).unwrap();
        fs::write(&file2, "B".repeat(60)).unwrap();
        
        let chunks = pack_files(vec![file1.clone(), file2.clone()], 50);
        assert_eq!(chunks.len(), 2);
        
        assert!(chunks[0].contains("(Part 1 of 2)"));
        assert!(chunks[0].contains("Awaiting next part"));
        assert!(chunks[0].contains("A".repeat(60).as_str()));
        
        assert!(chunks[1].contains("(Part 2 of 2)"));
        assert!(chunks[1].contains("All parts provided."));
        assert!(chunks[1].contains("B".repeat(60).as_str()));
    }
}
