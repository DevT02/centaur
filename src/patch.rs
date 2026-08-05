use crate::PatchBlock;
use crate::history::{BackupFileRecord, PatchSessionRecord};
use std::fs;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, PartialEq, Clone)]
pub enum ApplyResult {
    Created(String),
    Updated(String),
    MatchNotFound(String),
    AmbiguousMatch(String),
    IoError(String, String),
    SecurityError(String),
    DryRunSimulated(String),
}

pub fn is_safe_path(base_dir: &Path, rel_path_str: &str) -> Result<PathBuf, String> {
    if rel_path_str.contains("..") {
        return Err(format!("Path traversal attempt detected: {}", rel_path_str));
    }
    let p = Path::new(rel_path_str);
    // is_absolute() alone misses root-relative ("\foo") and Windows drive-relative
    // ("C:foo") paths, both of which discard the base when joined.
    if p.is_absolute()
        || p.has_root()
        || p.components()
            .next()
            .is_some_and(|c| matches!(c, Component::Prefix(_)))
    {
        return Err(format!("Absolute paths are forbidden: {}", rel_path_str));
    }
    let combined = base_dir.join(p);
    if combined.exists() {
        if let (Ok(canon_base), Ok(canon_combined)) =
            (base_dir.canonicalize(), combined.canonicalize())
        {
            if !canon_combined.starts_with(&canon_base) {
                return Err(format!(
                    "Security Violation: Symlink points outside root ({})",
                    rel_path_str
                ));
            }
        }
    }
    Ok(combined)
}

pub struct MemoryPatchPlan {
    pub block: PatchBlock,
    pub target_path: PathBuf,
    pub new_content: String,
    pub is_new_file: bool,
}

pub fn evaluate_patch_block(
    base_dir: &Path,
    block: &PatchBlock,
) -> Result<MemoryPatchPlan, ApplyResult> {
    let target_path = match is_safe_path(base_dir, &block.file_path) {
        Ok(p) => p,
        Err(err) => return Err(ApplyResult::SecurityError(err)),
    };

    let file_path_str = block.file_path.clone();

    if !target_path.exists() {
        if block.search.is_empty() {
            return Ok(MemoryPatchPlan {
                block: block.clone(),
                target_path,
                new_content: block.replace.clone(),
                is_new_file: true,
            });
        } else {
            return Err(ApplyResult::MatchNotFound(file_path_str));
        }
    }

    let mut content = match fs::read_to_string(&target_path) {
        Ok(c) => c,
        Err(e) => return Err(ApplyResult::IoError(file_path_str, e.to_string())),
    };

    if content.starts_with('\u{FEFF}') {
        content = content[3..].to_string();
    }

    // Check exact match
    let match_count = content.matches(&block.search).count();
    if match_count > 1 {
        return Err(ApplyResult::AmbiguousMatch(file_path_str));
    }

    if match_count == 1 {
        let updated = content.replacen(&block.search, &block.replace, 1);
        return Ok(MemoryPatchPlan {
            block: block.clone(),
            target_path,
            new_content: updated,
            is_new_file: false,
        });
    }

    // Check line-ending normalization
    let norm_content = content.replace("\r\n", "\n");
    let norm_search = block.search.replace("\r\n", "\n");

    let norm_match_count = norm_content.matches(&norm_search).count();
    if norm_match_count > 1 {
        return Err(ApplyResult::AmbiguousMatch(file_path_str));
    }

    if norm_match_count == 1 {
        let norm_replace = block.replace.replace("\r\n", "\n");
        let updated = norm_content.replacen(&norm_search, &norm_replace, 1);
        let final_content = if content.contains("\r\n") {
            updated.replace('\n', "\r\n")
        } else {
            updated
        };
        return Ok(MemoryPatchPlan {
            block: block.clone(),
            target_path,
            new_content: final_content,
            is_new_file: false,
        });
    }

    // Check fuzzy match
    if let Some(fuzzy_result) = apply_fuzzy_match(&content, &block.search, &block.replace) {
        match fuzzy_result {
            Ok(updated) => {
                return Ok(MemoryPatchPlan {
                    block: block.clone(),
                    target_path,
                    new_content: updated,
                    is_new_file: false,
                });
            }
            Err(is_ambiguous) => {
                if is_ambiguous {
                    return Err(ApplyResult::AmbiguousMatch(file_path_str));
                }
            }
        }
    }

    Err(ApplyResult::MatchNotFound(file_path_str))
}

fn apply_fuzzy_match(content: &str, search: &str, replace: &str) -> Option<Result<String, bool>> {
    let content_lines: Vec<&str> = content.lines().collect();
    let search_lines: Vec<&str> = search.lines().map(|l| l.trim()).collect();

    // A search block longer than the file can never match, and the window loop
    // below would index past the end of content_lines.
    if search_lines.is_empty() || search_lines.len() > content_lines.len() {
        return None;
    }

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

pub fn apply_blocks_transactional(
    base_dir: &Path,
    blocks: &[PatchBlock],
    dry_run: bool,
) -> Result<Vec<ApplyResult>, (Vec<ApplyResult>, String)> {
    let transaction_error = "Transactional application failed: 1 or more patch blocks could not be matched cleanly. Aborting transaction without modifying disk.";
    let canonical_base = match base_dir.canonicalize() {
        Ok(path) => path,
        Err(e) => {
            return Err((
                vec![ApplyResult::SecurityError(format!(
                    "Could not resolve workspace root: {}",
                    e
                ))],
                transaction_error.to_string(),
            ));
        }
    };

    let apply_to_content = |content: &str, block: &PatchBlock| -> Result<String, ApplyResult> {
        let file_path = block.file_path.clone();
        if block.search.is_empty() {
            return Err(ApplyResult::AmbiguousMatch(file_path));
        }

        let exact_matches = content.matches(&block.search).count();
        if exact_matches > 1 {
            return Err(ApplyResult::AmbiguousMatch(file_path));
        }
        if exact_matches == 1 {
            return Ok(content.replacen(&block.search, &block.replace, 1));
        }

        let normalized_content = content.replace("\r\n", "\n");
        let normalized_search = block.search.replace("\r\n", "\n");
        let normalized_matches = normalized_content.matches(&normalized_search).count();
        if normalized_matches > 1 {
            return Err(ApplyResult::AmbiguousMatch(file_path));
        }
        if normalized_matches == 1 {
            let normalized_replace = block.replace.replace("\r\n", "\n");
            let updated = normalized_content.replacen(&normalized_search, &normalized_replace, 1);
            return Ok(if content.contains("\r\n") {
                updated.replace('\n', "\r\n")
            } else {
                updated
            });
        }

        if let Some(fuzzy_result) = apply_fuzzy_match(content, &block.search, &block.replace) {
            return match fuzzy_result {
                Ok(updated) => Ok(updated),
                Err(true) => Err(ApplyResult::AmbiguousMatch(file_path)),
                Err(false) => Err(ApplyResult::MatchNotFound(file_path)),
            };
        }

        Err(ApplyResult::MatchNotFound(file_path))
    };

    let mut plans: Vec<MemoryPatchPlan> = Vec::new();
    let mut failures = Vec::new();

    for block in blocks {
        let target_path = match is_safe_path(base_dir, &block.file_path) {
            Ok(path) => path,
            Err(err) => {
                failures.push(ApplyResult::SecurityError(err));
                continue;
            }
        };

        // Validate the nearest existing ancestor. This closes the new-file case
        // where a symlinked parent could otherwise redirect a write outside root.
        let mut existing_ancestor = target_path.as_path();
        let mut safe_ancestor_found = true;
        while !existing_ancestor.exists() {
            match existing_ancestor.parent() {
                Some(parent) => existing_ancestor = parent,
                None => {
                    failures.push(ApplyResult::SecurityError(format!(
                        "Could not resolve a safe parent for {}",
                        block.file_path
                    )));
                    safe_ancestor_found = false;
                    break;
                }
            }
        }
        if !safe_ancestor_found {
            continue;
        }

        match existing_ancestor.canonicalize() {
            Ok(path) if path.starts_with(&canonical_base) => {}
            Ok(_) => {
                failures.push(ApplyResult::SecurityError(format!(
                    "Path resolves outside workspace: {}",
                    block.file_path
                )));
                continue;
            }
            Err(e) => {
                failures.push(ApplyResult::SecurityError(format!(
                    "Could not resolve path '{}': {}",
                    block.file_path, e
                )));
                continue;
            }
        }

        if let Some(plan_index) = plans
            .iter()
            .position(|plan| plan.target_path == target_path)
        {
            match apply_to_content(&plans[plan_index].new_content, block) {
                Ok(updated) => plans[plan_index].new_content = updated,
                Err(result) => failures.push(result),
            }
            continue;
        }

        match evaluate_patch_block(base_dir, block) {
            Ok(plan) => plans.push(plan),
            Err(res) => failures.push(res),
        }
    }

    if !failures.is_empty() {
        return Err((failures, transaction_error.to_string()));
    }

    let mut results = Vec::new();

    if dry_run {
        for plan in plans {
            results.push(ApplyResult::DryRunSimulated(plan.block.file_path));
        }
        return Ok(results);
    }

    // Record patch backup snapshot for Time Machine
    let backup_records: Vec<BackupFileRecord> = plans
        .iter()
        .map(|p| BackupFileRecord {
            relative_path: p.block.file_path.clone(),
            original_content: fs::read_to_string(&p.target_path).ok(),
            is_new_file: p.is_new_file,
        })
        .collect();
    // No snapshot means no undo. Refuse to touch the workspace rather than modify
    // files the user cannot get back.
    if let Err(e) = PatchSessionRecord::record_patch_session(base_dir, backup_records) {
        return Err((
            vec![ApplyResult::IoError("<undo snapshot>".to_string(), e)],
            "Could not write the undo snapshot. Aborting without modifying disk.".to_string(),
        ));
    }

    // Execute atomic writes
    for plan in plans {
        if let Some(parent) = plan.target_path.parent() {
            if let Err(e) = fs::create_dir_all(parent) {
                results.push(ApplyResult::IoError(
                    plan.block.file_path.clone(),
                    e.to_string(),
                ));
                continue;
            }
        }

        let temp_path = plan.target_path.with_extension("centaur_tmp");
        if let Err(e) = fs::write(&temp_path, &plan.new_content) {
            results.push(ApplyResult::IoError(
                plan.block.file_path.clone(),
                e.to_string(),
            ));
            continue;
        }

        if let Err(_e) = fs::rename(&temp_path, &plan.target_path) {
            // Fallback copy if rename fails across filesystems
            if let Err(e_copy) = fs::copy(&temp_path, &plan.target_path) {
                let _ = fs::remove_file(&temp_path);
                results.push(ApplyResult::IoError(
                    plan.block.file_path.clone(),
                    e_copy.to_string(),
                ));
                continue;
            }
            let _ = fs::remove_file(&temp_path);
        }

        if plan.is_new_file {
            results.push(ApplyResult::Created(plan.block.file_path));
        } else {
            results.push(ApplyResult::Updated(plan.block.file_path));
        }
    }

    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::use_scratch_home;
    use tempfile::tempdir;

    #[test]
    fn test_transactional_success() {
        use_scratch_home();
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("f1.txt");
        fs::write(&file1, "foo\n").unwrap();

        let blocks = vec![
            PatchBlock {
                file_path: "f1.txt".to_string(),
                search: "foo".to_string(),
                replace: "bar".to_string(),
            },
            PatchBlock {
                file_path: "f2.txt".to_string(),
                search: "".to_string(),
                replace: "created".to_string(),
            },
        ];

        let res = apply_blocks_transactional(dir.path(), &blocks, false).unwrap();
        assert_eq!(res.len(), 2);
        assert_eq!(fs::read_to_string(&file1).unwrap(), "bar\n");
        assert_eq!(
            fs::read_to_string(dir.path().join("f2.txt")).unwrap(),
            "created"
        );
    }

    #[test]
    fn test_transactional_rollback_on_failure() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("f1.txt");
        fs::write(&file1, "foo\n").unwrap();

        let blocks = vec![
            PatchBlock {
                file_path: "f1.txt".to_string(),
                search: "foo".to_string(),
                replace: "bar".to_string(),
            },
            PatchBlock {
                file_path: "nonexistent.txt".to_string(),
                search: "missing_content".to_string(),
                replace: "fail".to_string(),
            },
        ];

        let res = apply_blocks_transactional(dir.path(), &blocks, false);
        assert!(res.is_err());
        // Verify f1.txt was NOT modified because transaction was aborted
        assert_eq!(fs::read_to_string(&file1).unwrap(), "foo\n");
    }

    #[test]
    fn test_dry_run_mode() {
        let dir = tempdir().unwrap();
        let file1 = dir.path().join("f1.txt");
        fs::write(&file1, "foo\n").unwrap();

        let blocks = vec![PatchBlock {
            file_path: "f1.txt".to_string(),
            search: "foo".to_string(),
            replace: "bar".to_string(),
        }];

        let res = apply_blocks_transactional(dir.path(), &blocks, true).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0], ApplyResult::DryRunSimulated("f1.txt".to_string()));
        // Verify file was NOT modified during dry run
        assert_eq!(fs::read_to_string(&file1).unwrap(), "foo\n");
    }

    #[test]
    fn test_path_traversal_protection() {
        let dir = tempdir().unwrap();
        let blocks = vec![PatchBlock {
            file_path: "../../../etc/passwd".to_string(),
            search: "".to_string(),
            replace: "hacked".to_string(),
        }];

        let res = apply_blocks_transactional(dir.path(), &blocks, false);
        assert!(res.is_err());
    }
}
