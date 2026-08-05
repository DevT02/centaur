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

pub fn get_changed_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    // -z avoids git's quoting/escaping of unusual paths, and --untracked-files=all
    // expands new directories into their individual files. With the default output,
    // a brand-new folder appeared as a single "dir/" entry that failed the is_file
    // check, so none of its contents were ever exported.
    if let Ok(output) = Command::new("git")
        .args(["status", "--porcelain", "-z", "--untracked-files=all"])
        .current_dir(root)
        .output()
    {
        if output.status.success() {
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

                let p = root.join(path_str);
                if p.is_file() && !files.contains(&p) {
                    files.push(p);
                }
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
        let p = root.join(m);
        if p.exists() && p.is_file() && !files.contains(&p) {
            files.push(p);
        }
    }

    files
}

pub fn get_staged_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Ok(output) = Command::new("git")
        .args(["diff", "--cached", "--name-only"])
        .current_dir(root)
        .output()
    {
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() {
                    let p = root.join(trimmed);
                    if p.exists() && p.is_file() {
                        files.push(p);
                    }
                }
            }
        }
    }

    let manifests = ["Cargo.toml", "package.json", "pyproject.toml", "README.md"];
    for m in manifests {
        let p = root.join(m);
        if p.exists() && p.is_file() && !files.contains(&p) {
            files.push(p);
        }
    }

    files
}
