//! Single implementation of the export pipeline.
//!
//! `main.rs` and `ui.rs` previously each had their own copy, which had already
//! drifted apart three ways: redaction existed only in the TUI, the two used
//! different skip filters, and they named the export directory differently.

use crate::git::{ExportMode, get_changed_files, get_staged_files, is_git_repo};
use crate::pack::{PackOptions, PackResult, pack_files_dynamic, walk_workspace};
use crate::prompt::render_prompt;
use crate::secrets::{SecretWarning, redact_secrets, scan_file_for_secrets};
use arboard::Clipboard;
use std::fs;
use std::path::{Path, PathBuf};

/// Written only when the workflow prompt could not be placed on the clipboard.
pub const PROMPT_FALLBACK_FILENAME: &str = "COPY_THIS_PROMPT.txt";

/// Files Centaur itself generates. Feeding them back into an export compounds
/// context on every run.
fn is_centaur_artifact(path: &Path) -> bool {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    name.starts_with("centaur_context_part")
        || name == PROMPT_FALLBACK_FILENAME
        || name == "manifest.json"
        || name == ".metadata"
        || path.to_string_lossy().contains("centaur_export")
}

/// Resolve the set of files an export should cover.
pub fn collect_files(root: &Path, mode: ExportMode, paths: &[String]) -> Vec<PathBuf> {
    let git_files = match mode {
        ExportMode::Changed if is_git_repo(root) => Some(get_changed_files(root)),
        ExportMode::Staged if is_git_repo(root) => Some(get_staged_files(root)),
        _ => None,
    };
    if let Some(files) = git_files {
        return files;
    }

    let roots: Vec<PathBuf> = if paths.is_empty() {
        vec![root.to_path_buf()]
    } else {
        paths.iter().map(PathBuf::from).collect()
    };

    let mut files = Vec::new();
    for start in roots {
        for entry in walk_workspace(&start).flatten() {
            let p = entry.path();
            if !p.is_file() || is_centaur_artifact(p) {
                continue;
            }
            // Absolute paths keep strip_prefix reliable for both packing and redaction.
            files.push(if p.is_absolute() {
                p.to_path_buf()
            } else {
                root.join(p)
            });
        }
    }
    files
}

pub fn scan_files(files: &[PathBuf]) -> Vec<SecretWarning> {
    files
        .iter()
        .filter_map(|p| {
            fs::read_to_string(p)
                .ok()
                .map(|c| scan_file_for_secrets(p, &c))
        })
        .flatten()
        .collect()
}

pub struct ExportRequest {
    pub root: PathBuf,
    pub task: String,
    /// Rewrite secrets out of a throwaway copy before packing.
    pub redact: bool,
    pub copy_prompt: bool,
    pub pack: PackOptions,
}

pub struct ExportOutcome {
    pub result: PackResult,
    pub prompt: String,
    pub export_dir: PathBuf,
    pub prompt_copied: bool,
    /// Files that could not be written. Previously every write was `let _ =`, so
    /// the user could be told "Export complete" over an empty directory.
    pub write_errors: Vec<String>,
}

/// Copy `files` into `dest`, stripping secrets. Returns the rewritten paths.
fn redact_into(files: &[PathBuf], root: &Path, dest: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for source in files {
        let relative = source
            .strip_prefix(root)
            .map_err(|_| format!("Export path is outside the workspace: {}", source.display()))?;
        let target = dest.join(relative);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Could not create export folder: {}", e))?;
        }

        let written = match fs::read_to_string(source) {
            Ok(content) => {
                let name = source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let cleaned = if name == ".env" || name.starts_with(".env.") {
                    redact_secrets(&redact_env_values(&content))
                } else {
                    redact_secrets(&content)
                };
                fs::write(&target, cleaned)
            }
            // Binary files carry no scannable secrets; copy them through untouched.
            Err(_) => fs::copy(source, &target).map(|_| ()),
        };
        written.map_err(|e| format!("Could not create redacted export copy: {}", e))?;
        out.push(target);
    }
    Ok(out)
}

/// Blank every value in a dotenv file, keeping keys and comments readable.
fn redact_env_values(content: &str) -> String {
    let mut out = content
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                line.to_string()
            } else if let Some((key, _)) = line.split_once('=') {
                format!("{}=[REDACTED_ENV_VALUE_BY_CENTAUR]", key)
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if content.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Purges centaur_export_* folders in temp_dir that are older than max_age_secs (default 24h).
pub fn cleanup_stale_exports(max_age_secs: u64) -> usize {
    let temp = std::env::temp_dir();
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(&temp) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("centaur_export_"))
                && let Ok(metadata) = entry.metadata()
                && let Ok(modified) = metadata.modified()
                && let Ok(elapsed) = modified.elapsed()
                && elapsed.as_secs() > max_age_secs
            {
                let _ = std::fs::remove_dir_all(&path);
                count += 1;
            }
        }
    }
    count
}

pub fn run(req: &ExportRequest, files: Vec<PathBuf>) -> Result<ExportOutcome, String> {
    // Redaction packs from a throwaway copy so the real workspace is never touched.
    let redact_dir = req
        .redact
        .then(|| std::env::temp_dir().join(format!("centaur_redacted_{}", req.pack.session_id)));

    let (files, pack_root) = match &redact_dir {
        Some(dir) => {
            fs::create_dir_all(dir)
                .map_err(|e| format!("Could not prepare safe export copy: {}", e))?;
            match redact_into(&files, &req.root, dir) {
                Ok(copies) => (copies, dir.clone()),
                Err(e) => {
                    let _ = fs::remove_dir_all(dir);
                    return Err(e);
                }
            }
        }
        None => (files, req.root.clone()),
    };

    let result = pack_files_dynamic(files, &pack_root, req.pack.clone());
    if let Some(dir) = &redact_dir {
        let _ = fs::remove_dir_all(dir);
    }

    let prompt = render_prompt(
        &req.task,
        &req.pack.session_id,
        result.summary.total_parts,
        result.summary.total_batches,
    );

    // Auto-clean any temporary export directories older than 24 hours
    cleanup_stale_exports(86400);

    // Named by session id: unique per run, so no guessable path to clobber and no
    // need to recursively delete whatever already sits there.
    let export_dir = std::env::temp_dir().join(format!("centaur_export_{}", req.pack.session_id));
    fs::create_dir_all(&export_dir)
        .map_err(|e| format!("Could not create export directory: {}", e))?;

    fn write(errors: &mut Vec<String>, path: PathBuf, data: &str) {
        if let Err(e) = fs::write(&path, data) {
            errors.push(format!("{}: {}", path.display(), e));
        }
    }

    let mut write_errors = Vec::new();

    let single_batch = result.summary.total_batches == 1;
    for chunk in &result.chunks {
        let dir = if single_batch {
            export_dir.clone()
        } else {
            let batch = export_dir.join(format!("batch_{:02}", chunk.batch_number));
            if let Err(e) = fs::create_dir_all(&batch) {
                write_errors.push(format!("{}: {}", batch.display(), e));
                continue;
            }
            batch
        };
        write(
            &mut write_errors,
            dir.join(format!("centaur_context_part{:03}.txt", chunk.part_number)),
            &chunk.content,
        );
    }

    let prompt_copied = req.copy_prompt
        && Clipboard::new()
            .and_then(|mut cb| cb.set_text(prompt.clone()))
            .is_ok();

    // Keep the normal export folder limited to upload files. A prompt file appears
    // only when clipboard delivery was disabled or failed, so the user is never
    // stranded without the workflow instructions.
    if !prompt_copied {
        write(
            &mut write_errors,
            export_dir.join(PROMPT_FALLBACK_FILENAME),
            &prompt,
        );
    }

    Ok(ExportOutcome {
        result,
        prompt,
        export_dir,
        prompt_copied,
        write_errors,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn opts(dir: &Path) -> PackOptions {
        PackOptions {
            session_id: format!("test{}", dir.file_name().unwrap().to_string_lossy()),
            ..PackOptions::default()
        }
    }

    #[test]
    fn collect_sees_dotfiles_but_not_the_git_dir() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".env"), "TOKEN=abc").unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::create_dir_all(dir.path().join(".git")).unwrap();
        fs::write(dir.path().join(".git").join("config"), "[core]").unwrap();

        let names: Vec<String> = collect_files(dir.path(), ExportMode::Full, &[])
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert!(
            names.contains(&".env".to_string()),
            "dotfiles must be visible: {:?}",
            names
        );
        assert!(names.contains(&"main.rs".to_string()));
        assert!(
            !names.contains(&"config".to_string()),
            ".git must stay excluded: {:?}",
            names
        );
    }

    #[test]
    fn redaction_keeps_secrets_out_of_the_export() {
        let dir = tempdir().unwrap();
        fs::write(
            dir.path().join(".env"),
            "AWS_KEY=AKIAIOSFODNN7EXAMPLE\n# note\n",
        )
        .unwrap();

        let files = collect_files(dir.path(), ExportMode::Full, &[]);
        let req = ExportRequest {
            root: dir.path().to_path_buf(),
            task: "review".to_string(),
            redact: true,
            copy_prompt: false,
            pack: opts(dir.path()),
        };
        let out = run(&req, files).unwrap();
        let packed: String = out
            .result
            .chunks
            .iter()
            .map(|c| c.content.clone())
            .collect();

        assert!(
            !packed.contains("AKIAIOSFODNN7EXAMPLE"),
            "secret leaked into export"
        );
        assert!(packed.contains("[REDACTED_ENV_VALUE_BY_CENTAUR]"));
        assert!(
            packed.contains("# note"),
            "comments should survive redaction"
        );
        assert!(out.write_errors.is_empty());
        // The workspace copy must be untouched.
        assert!(
            fs::read_to_string(dir.path().join(".env"))
                .unwrap()
                .contains("AKIAIOSFODNN7EXAMPLE")
        );
    }

    #[test]
    fn export_artifacts_are_not_re_exported() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::write(dir.path().join("COPY_THIS_PROMPT.txt"), "prompt").unwrap();
        fs::write(dir.path().join("centaur_context_part001.txt"), "chunk").unwrap();

        let names: Vec<String> = collect_files(dir.path(), ExportMode::Full, &[])
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();

        assert_eq!(names, vec!["main.rs".to_string()], "got {:?}", names);
    }

    #[test]
    fn prompt_file_is_written_when_clipboard_copy_is_disabled() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}").unwrap();

        let files = collect_files(dir.path(), ExportMode::Full, &[]);
        let req = ExportRequest {
            root: dir.path().to_path_buf(),
            task: "Improve the export experience".to_string(),
            redact: false,
            copy_prompt: false,
            pack: opts(dir.path()),
        };

        let out = run(&req, files).unwrap();
        let fallback = out.export_dir.join(PROMPT_FALLBACK_FILENAME);

        assert!(!out.prompt_copied);
        assert!(out.write_errors.is_empty());
        assert_eq!(fs::read_to_string(fallback).unwrap(), out.prompt);
    }

    #[test]
    fn test_cleanup_stale_exports_runs_without_panicking() {
        let _ = cleanup_stale_exports(86400);
    }
}
