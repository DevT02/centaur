use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ExportMode {
    Full,
    Changed,
    Staged,
    Compact,
}

pub fn is_git_repo(root: &Path) -> bool {
    Command::new("git")
        .arg("rev-parse")
        .arg("--is-inside-work-tree")
        .current_dir(root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn has_worktree_changes(root: &Path) -> bool {
    Command::new("git")
        .args([
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ])
        .current_dir(root)
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

fn contained_file(root: &Path, candidate: PathBuf) -> Option<PathBuf> {
    let canonical = candidate.canonicalize().ok()?;
    (canonical.is_file() && canonical.starts_with(root)).then_some(candidate)
}

fn repository_root(root: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(root)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!path.is_empty()).then(|| PathBuf::from(path))
}

pub fn get_changed_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Some(canonical_root) = root.canonicalize().ok() else {
        return files;
    };
    let repository_root = repository_root(root).unwrap_or_else(|| root.to_path_buf());

    // -z avoids git's quoting/escaping of unusual paths, and --untracked-files=all
    // expands new directories into their individual files. With the default output,
    // a brand-new folder appeared as a single "dir/" entry that failed the is_file
    // check, so none of its contents were ever exported.
    if let Ok(output) = Command::new("git")
        .args([
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=all",
            "--",
            ".",
        ])
        .current_dir(root)
        .output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut entries = stdout.split('\0');
        while let Some(entry) = entries.next() {
            let (status, path_str) = match (entry.get(..2), entry.get(3..)) {
                (Some(status), Some(path)) if !path.is_empty() => (status, path),
                _ => continue,
            };

            // Renames and copies carry the source path as a second NUL-separated
            // field; consume it so it is not parsed as its own entry.
            if status.starts_with('R') || status.starts_with('C') {
                entries.next();
            }

            // A deleted path has nothing left to export.
            if status.contains('D') {
                continue;
            }

            // Porcelain `-z` reports paths relative to the repository root, even
            // when Git runs from a nested workspace.
            let Some(p) = contained_file(&canonical_root, repository_root.join(path_str)) else {
                continue;
            };
            if !files.contains(&p) {
                files.push(p);
            }
        }
    }

    // Always include project manifests if they exist
    let manifests = [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "README.md",
        "Makefile",
    ];
    for m in manifests {
        let Some(p) = contained_file(&canonical_root, root.join(m)) else {
            continue;
        };
        if !files.contains(&p) {
            files.push(p);
        }
    }

    files
}

pub fn get_staged_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let Some(canonical_root) = root.canonicalize().ok() else {
        return files;
    };
    let repository_root = repository_root(root).unwrap_or_else(|| root.to_path_buf());

    if let Ok(output) = Command::new("git")
        .args(["diff", "--cached", "--name-only", "-z", "--", "."])
        .current_dir(root)
        .output()
        && output.status.success()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for path in stdout.split('\0').filter(|path| !path.is_empty()) {
            let Some(p) = contained_file(&canonical_root, repository_root.join(path)) else {
                continue;
            };
            if !files.contains(&p) {
                files.push(p);
            }
        }
    }

    let manifests = ["Cargo.toml", "package.json", "pyproject.toml", "README.md"];
    for m in manifests {
        let Some(p) = contained_file(&canonical_root, root.join(m)) else {
            continue;
        };
        if !files.contains(&p) {
            files.push(p);
        }
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_change_check_distinguishes_clean_and_untracked() {
        let dir = tempfile::tempdir().unwrap();
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
        assert!(!has_worktree_changes(dir.path()));

        std::fs::write(dir.path().join("new.txt"), "new").unwrap();
        assert!(has_worktree_changes(dir.path()));
    }
}
