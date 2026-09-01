use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct PackedFile {
    pub relative_path: String,
    pub content: String,
    pub is_compact_summary: bool,
}

#[derive(Debug, Clone)]
pub struct PackOptions {
    pub max_attachment_chars: usize,
    pub max_attachments_per_message: usize,
    pub context_token_budget: usize,
    pub is_compact_mode: bool,
    pub force: bool,
    pub project_root_name: String,
    pub session_id: String,
}

impl Default for PackOptions {
    fn default() -> Self {
        Self {
            max_attachment_chars: 5_000_000,
            max_attachments_per_message: 20,
            context_token_budget: 200_000,
            is_compact_mode: false,
            force: false,
            project_root_name: "project".to_string(),
            session_id: "session".to_string(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ChunkPart {
    pub part_number: usize,
    pub total_parts: usize,
    pub batch_number: usize,
    pub total_batches: usize,
    pub content: String,
    pub checksum: String,
    pub is_final: bool,
}

#[derive(Debug, Clone)]
pub struct ExportSummary {
    pub total_files: usize,
    pub total_chars: usize,
    pub estimated_tokens: usize,
    pub total_parts: usize,
    pub total_batches: usize,
    pub skipped_files: Vec<SkippedFile>,
    pub token_warning: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PackResult {
    pub summary: ExportSummary,
    pub chunks: Vec<ChunkPart>,
    pub manifest_json: String,
}

/// Walk a workspace the way Centaur should: honour .gitignore, but do NOT skip
/// dotfiles. The default `hidden(true)` made `.env`, `.npmrc` and `.aws/` invisible
/// to both the export scan and the security audit. `.git` is excluded explicitly,
/// since it is only ever skipped today as a side effect of being hidden.
pub fn walk_workspace(root: &Path) -> ignore::Walk {
    ignore::WalkBuilder::new(root)
        .hidden(false)
        .require_git(false)
        // Lets a project exclude fixtures and vendored trees from exports and audits
        // using ordinary gitignore syntax, without inventing a config format.
        .add_custom_ignore_filename(".centaurignore")
        .filter_entry(|entry| entry.file_name() != ".git")
        .build()
}

pub fn sort_files_by_priority(files: &mut [PathBuf]) {
    files.sort_by(|a, b| {
        let p_a = priority_rank(a);
        let p_b = priority_rank(b);
        if p_a != p_b { p_a.cmp(&p_b) } else { a.cmp(b) }
    });
}

fn priority_rank(path: &Path) -> usize {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    let path_str = path.to_string_lossy().to_lowercase();

    // 0: Manifests & core config
    if name == "cargo.toml"
        || name == "package.json"
        || name == "pyproject.toml"
        || name == "makefile"
        || name == "tsconfig.json"
        || name == ".gitignore"
    {
        return 0;
    }

    // 1: Primary source files
    if path_str.contains("/src/")
        || path_str.contains("\\src\\")
        || path_str.contains("src/")
        || path_str.contains("/lib/")
        || path_str.contains("\\lib\\")
        || name.ends_with(".rs")
        || name.ends_with(".ts")
        || name.ends_with(".js")
        || name.ends_with(".py")
        || name.ends_with(".go")
    {
        return 1;
    }

    // 2: Unit / Integration tests
    if path_str.contains("test")
        || name.contains("test")
        || name.ends_with(".spec.ts")
        || name.ends_with(".spec.js")
    {
        return 2;
    }

    // 3: Documentation
    if path_str.contains("doc") || name.ends_with(".md") || name.ends_with(".txt") {
        return 3;
    }

    // 4: Fixtures, assets, others
    4
}

pub fn is_repetitive_or_generated(path: &Path, content: &str) -> bool {
    let file_name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if file_name.starts_with("massive_")
        || file_name.ends_with(".min.js")
        || file_name.ends_with(".map")
    {
        return true;
    }

    // If large and has high line repetition or extremely long single lines
    if content.len() > 100_000 {
        let lines: Vec<&str> = content.lines().collect();
        if !lines.is_empty() {
            let sample = &lines[..lines.len().min(100)];
            let unique_lines: HashSet<_> = sample.iter().collect();
            if unique_lines.len() < 5 {
                return true;
            }
        }
        for line in lines.iter().take(50) {
            if line.len() > 10_000 {
                return true;
            }
        }
    }
    false
}

pub fn calculate_checksum(data: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data.as_bytes());
    format!("{:x}", hasher.finalize())[..16].to_string()
}

pub fn pack_files_dynamic(
    files: Vec<PathBuf>,
    base_root: &Path,
    options: PackOptions,
) -> PackResult {
    let mut sorted_files = files;
    let mut seen_paths = HashSet::new();
    sorted_files.retain(|path| {
        let identity = path.canonicalize().unwrap_or_else(|_| path.clone());
        seen_paths.insert(identity)
    });
    sort_files_by_priority(&mut sorted_files);

    let mut packed_files = Vec::new();
    let mut skipped_files = Vec::new();

    // Directory Structure map header
    let mut tree_str = String::from("Project Directory Structure:\n");
    for path in &sorted_files {
        let rel = path.strip_prefix(base_root).unwrap_or(path);
        let normalized = rel.to_string_lossy().replace('\\', "/");
        tree_str.push_str(&format!("  - {}\n", normalized));
    }
    tree_str.push_str("\n==================================================\n\n");

    for path in &sorted_files {
        let rel = path.strip_prefix(base_root).unwrap_or(path);
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let file_name = path.file_name().unwrap_or_default().to_string_lossy();

        if file_name == "package-lock.json"
            || file_name == "Cargo.lock"
            || file_name == "yarn.lock"
            || file_name == "poetry.lock"
        {
            skipped_files.push(SkippedFile {
                path: rel_str,
                reason: "Lockfile skipped to preserve token budget".to_string(),
            });
            continue;
        }

        if !options.force
            && let Ok(meta) = path.metadata()
            && meta.len() > 2_000_000
        {
            skipped_files.push(SkippedFile {
                path: rel_str,
                reason: format!("File exceeds 2MB limit ({} bytes)", meta.len()),
            });
            continue;
        }

        let raw_content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => {
                skipped_files.push(SkippedFile {
                    path: rel_str,
                    reason: "Binary or non-UTF8 text".to_string(),
                });
                continue;
            }
        };

        // Whitespace collapse
        let mut content = raw_content;
        while content.contains("\n\n\n") {
            content = content.replace("\n\n\n", "\n\n");
        }

        // Truncate CSV / JSONL
        if rel_str.ends_with(".csv") || rel_str.ends_with(".jsonl") {
            let lines: Vec<&str> = content.lines().collect();
            if lines.len() > 50 {
                content = lines[0..50].join("\n");
                content.push_str("\n\n... (Data truncated by Centaur to save AI tokens. Schema and examples preserved.) ...");
            }
        }

        if options.is_compact_mode && is_repetitive_or_generated(path, &content) {
            let checksum = calculate_checksum(&content);
            let preview: String = content.chars().take(100).collect();
            let summary = format!(
                "File omitted from full context:\nPath: {}\nSize: {} bytes\nReason: highly repetitive generated/test content\nSHA-256: {}\nPreview: {}...\n\n",
                rel_str,
                content.len(),
                checksum,
                preview.replace('\n', " ")
            );
            packed_files.push(PackedFile {
                relative_path: rel_str,
                content: summary,
                is_compact_summary: true,
            });
        } else {
            packed_files.push(PackedFile {
                relative_path: rel_str,
                content,
                is_compact_summary: false,
            });
        }
    }

    let mut file_blocks = Vec::new();
    for pf in &packed_files {
        if pf.is_compact_summary {
            file_blocks.push(pf.content.clone());
        } else {
            // Line-aware splitting if a single file is very large (> 1,000,000 chars)
            if pf.content.len() > 1_000_000 {
                let lines: Vec<&str> = pf.content.lines().collect();
                let total_lines = lines.len();
                let chunk_line_count = 2000;
                let segments = total_lines.div_ceil(chunk_line_count);
                for seg_idx in 0..segments {
                    let start_line = seg_idx * chunk_line_count;
                    let end_line = ((seg_idx + 1) * chunk_line_count).min(total_lines);
                    let seg_content = lines[start_line..end_line].join("\n");
                    let block = format!(
                        "File: `{}` (Segment {} of {}, lines {}-{} of {})\n```\n{}\n```\n\n",
                        pf.relative_path,
                        seg_idx + 1,
                        segments,
                        start_line + 1,
                        end_line,
                        total_lines,
                        seg_content
                    );
                    file_blocks.push(block);
                }
            } else {
                let block = format!("File: `{}`\n```\n{}\n```\n\n", pf.relative_path, pf.content);
                file_blocks.push(block);
            }
        }
    }

    let total_chars: usize = file_blocks.iter().map(|b| b.len()).sum::<usize>() + tree_str.len();
    let estimated_tokens = total_chars / 3;
    let max_attachment_chars = options.max_attachment_chars.max(10_000);
    let max_attachments_per_message = options.max_attachments_per_message.max(1);

    // Reserve 2KB margin for headers/footers. This comes off the ceiling before the
    // split is planned: subtracting it afterwards shrank a limit that was already
    // exactly total_chars, so the last block always overflowed by up to 2KB and a
    // content-free part holding only the directory map was emitted ahead of it.
    let usable_chunk_chars = max_attachment_chars.saturating_sub(2048).max(5000);

    let parts_needed_by_size = total_chars.div_ceil(usable_chunk_chars).max(1);
    let parts_for_first_message = parts_needed_by_size.min(max_attachments_per_message);
    let target_chunk_limit = (total_chars.div_ceil(parts_for_first_message))
        .min(usable_chunk_chars)
        .max(5000);

    let mut raw_chunks: Vec<String> = Vec::new();
    let mut current_chunk = String::new();
    current_chunk.push_str(&tree_str);

    // The directory map opens the first chunk. On its own it is not a part worth
    // spending an attachment or a round trip on, so the first block joins it even
    // when that overruns the target.
    let mut chunk_holds_a_file = false;

    for block in file_blocks {
        if current_chunk.len() + block.len() > target_chunk_limit && chunk_holds_a_file {
            raw_chunks.push(std::mem::take(&mut current_chunk));
        }
        current_chunk.push_str(&block);
        chunk_holds_a_file = true;
    }
    if !current_chunk.is_empty() {
        raw_chunks.push(current_chunk);
    }

    let total_parts = raw_chunks.len();
    let total_batches = total_parts.div_ceil(max_attachments_per_message);

    let mut final_chunks = Vec::new();
    for (i, raw_content) in raw_chunks.iter().enumerate() {
        let part_num = i + 1;
        let batch_num = ((i / max_attachments_per_message) + 1).min(total_batches);
        let is_final = part_num == total_parts;
        let checksum = calculate_checksum(raw_content);

        let header = format!(
            "CENTAUR EXPORT\nSession: {}\nPart: {} of {}\nBatch: {} of {}\nProject root: {}\nContent checksum: {}\nFinal part: {}\n\n",
            options.session_id,
            part_num,
            total_parts,
            batch_num,
            total_batches,
            options.project_root_name,
            checksum,
            if is_final { "yes" } else { "no" }
        );

        let footer = if is_final {
            "\n\n(All parts provided. Please suggest fixes ONLY using the <<<<<<< SEARCH / >>>>>>> REPLACE block format. Do not output the entire file.)"
        } else {
            ""
        };

        let chunk_body = format!("{}{}{}", header, raw_content, footer);

        final_chunks.push(ChunkPart {
            part_number: part_num,
            total_parts,
            batch_number: batch_num,
            total_batches,
            content: chunk_body,
            checksum,
            is_final,
        });
    }

    let token_warning = if estimated_tokens > options.context_token_budget {
        Some(format!(
            "⚠️ Export contains approximately {} estimated tokens (Context budget: {} tokens). Attachments generated, but AI model performance may degrade.",
            estimated_tokens, options.context_token_budget
        ))
    } else {
        None
    };

    let summary = ExportSummary {
        total_files: packed_files.len(),
        total_chars,
        estimated_tokens,
        total_parts,
        total_batches,
        skipped_files,
        token_warning,
    };

    let manifest_json = format!(
        "{{\n  \"session_id\": \"{}\",\n  \"total_files\": {},\n  \"total_chars\": {},\n  \"estimated_tokens\": {},\n  \"total_parts\": {},\n  \"total_batches\": {}\n}}",
        options.session_id,
        summary.total_files,
        summary.total_chars,
        summary.estimated_tokens,
        summary.total_parts,
        summary.total_batches
    );

    PackResult {
        summary,
        chunks: final_chunks,
        manifest_json,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_priority_sorting() {
        let mut paths = vec![
            PathBuf::from("src/main.rs"),
            PathBuf::from("Cargo.toml"),
            PathBuf::from("tests/test.rs"),
            PathBuf::from("README.md"),
        ];
        sort_files_by_priority(&mut paths);
        assert_eq!(paths[0], PathBuf::from("Cargo.toml"));
        assert_eq!(paths[1], PathBuf::from("src/main.rs"));
        assert_eq!(paths[2], PathBuf::from("tests/test.rs"));
        assert_eq!(paths[3], PathBuf::from("README.md"));
    }

    /// One file that comfortably fits the ceiling used to be split in two: the
    /// header margin was taken off a limit already equal to the total, leaving a
    /// first part that held nothing but the directory map.
    #[test]
    fn a_file_under_the_ceiling_is_not_split_off_from_the_directory_map() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("main.rs");
        fs::write(&file, "fn main() {}\n".repeat(700)).unwrap();

        let result = pack_files_dynamic(
            vec![file],
            dir.path(),
            PackOptions {
                max_attachment_chars: 200_000,
                max_attachments_per_message: 1,
                ..PackOptions::default()
            },
        );

        assert_eq!(
            result.summary.total_parts, 1,
            "9KB should not need two parts"
        );
        assert!(result.chunks[0].content.contains("fn main() {}"));
    }

    #[test]
    fn test_pack_files_dynamic_single_attachment() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("Cargo.toml");
        fs::write(&file1, "[package]\nname=\"test\"").unwrap();

        let options = PackOptions {
            max_attachment_chars: 1_000_000,
            max_attachments_per_message: 20,
            context_token_budget: 200_000,
            is_compact_mode: false,
            force: false,
            project_root_name: "test_proj".to_string(),
            session_id: "sess123".to_string(),
        };

        let result = pack_files_dynamic(vec![file1], dir.path(), options);
        assert_eq!(result.summary.total_parts, 1);
        assert_eq!(result.summary.total_batches, 1);
        assert!(result.chunks[0].content.contains("CENTAUR EXPORT"));
        assert!(result.chunks[0].content.contains("Session: sess123"));
        assert!(result.chunks[0].content.contains("Part: 1 of 1"));
    }

    #[test]
    fn test_skipped_lockfile() {
        let dir = tempdir().unwrap();
        let lock = dir.path().join("Cargo.lock");
        fs::write(&lock, "[lock]").unwrap();

        let options = PackOptions::default();
        let result = pack_files_dynamic(vec![lock], dir.path(), options);
        assert_eq!(result.summary.skipped_files.len(), 1);
        assert!(
            result.summary.skipped_files[0]
                .reason
                .contains("Lockfile skipped")
        );
    }

    #[test]
    fn test_compact_fixture_omission() {
        let dir = tempdir().unwrap();
        let fixture = dir.path().join("massive_data.txt");
        fs::write(&fixture, "A".repeat(150_000)).unwrap();

        let options = PackOptions {
            is_compact_mode: true,
            ..PackOptions::default()
        };

        let result = pack_files_dynamic(vec![fixture], dir.path(), options);
        assert_eq!(result.chunks.len(), 1);
        assert!(
            result.chunks[0]
                .content
                .contains("File omitted from full context")
        );
        assert!(result.chunks[0].content.contains("massive_data.txt"));
    }
}
