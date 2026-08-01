//! `git status --porcelain` parsing used to drop whole untracked directories and
//! renamed files, so new work silently never reached the export.

use std::fs;
use std::process::Command;
use tempfile::tempdir;
use the_clipboard_centaur::git::get_changed_files;

fn git(dir: &std::path::Path, args: &[&str]) {
    let out = Command::new("git").args(args).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&out.stderr));
}

fn repo() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.email", "t@example.com"]);
    git(dir.path(), &["config", "user.name", "Test"]);
    dir
}

fn names(dir: &std::path::Path) -> Vec<String> {
    let mut n: Vec<String> = get_changed_files(dir)
        .iter()
        .map(|p| {
            p.strip_prefix(dir)
                .unwrap_or(p)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    n.sort();
    n
}

#[test]
fn files_in_a_new_untracked_directory_are_included() {
    let dir = repo();
    fs::write(dir.path().join("existing.rs"), "fn main() {}").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "init"]);

    // A brand-new folder: previously reported as a single "feature/" entry and dropped.
    fs::create_dir_all(dir.path().join("feature/deep")).unwrap();
    fs::write(dir.path().join("feature/one.rs"), "pub fn one() {}").unwrap();
    fs::write(dir.path().join("feature/deep/two.rs"), "pub fn two() {}").unwrap();

    let found = names(dir.path());
    assert!(found.contains(&"feature/one.rs".to_string()), "got {:?}", found);
    assert!(found.contains(&"feature/deep/two.rs".to_string()), "got {:?}", found);
}

#[test]
fn renamed_files_are_included_and_the_old_path_is_not() {
    let dir = repo();
    fs::write(dir.path().join("before.rs"), "pub fn thing() {}").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "init"]);
    git(dir.path(), &["mv", "before.rs", "after.rs"]);

    let found = names(dir.path());
    assert!(found.contains(&"after.rs".to_string()), "new path missing: {:?}", found);
    assert!(!found.contains(&"before.rs".to_string()), "old path leaked: {:?}", found);
}

#[test]
fn deleted_files_are_not_exported() {
    let dir = repo();
    fs::write(dir.path().join("gone.rs"), "pub fn gone() {}").unwrap();
    fs::write(dir.path().join("kept.rs"), "pub fn kept() {}").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "init"]);
    fs::remove_file(dir.path().join("gone.rs")).unwrap();
    fs::write(dir.path().join("kept.rs"), "pub fn kept() { changed(); }").unwrap();

    let found = names(dir.path());
    assert!(!found.contains(&"gone.rs".to_string()), "deleted file listed: {:?}", found);
    assert!(found.contains(&"kept.rs".to_string()), "modified file missing: {:?}", found);
}

#[test]
fn paths_with_spaces_survive_parsing() {
    let dir = repo();
    fs::write(dir.path().join("initial.rs"), "fn main() {}").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "init"]);
    // Default porcelain output quotes and escapes these; -z does not.
    fs::write(dir.path().join("my notes.md"), "# notes").unwrap();

    let found = names(dir.path());
    assert!(found.contains(&"my notes.md".to_string()), "got {:?}", found);
}
