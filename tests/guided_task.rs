//! The guided task command should choose useful context without making users
//! understand Centaur's export modes.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

fn git(dir: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {:?} failed: {}",
        args,
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "--quiet"]);
    git(dir, &["config", "user.email", "centaur@example.com"]);
    git(dir, &["config", "user.name", "Centaur Test"]);
    git(dir, &["config", "commit.gpgsign", "false"]);
}

fn commit_all(dir: &Path) {
    git(dir, &["add", "."]);
    git(dir, &["commit", "--quiet", "-m", "fixture"]);
}

fn run_task(workspace: &Path, description: &str) -> Output {
    let home = tempdir().unwrap();
    fs::write(
        home.path().join("config.toml"),
        "[export]\ncopy_prompt = false\nopen_export_directory = false\n",
    )
    .unwrap();

    Command::new(env!("CARGO_BIN_EXE_centaur"))
        .arg("task")
        .args(description.split_whitespace())
        .current_dir(workspace)
        .env("CENTAUR_HOME", home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap()
}

fn unique_task(label: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("guided task {label} {} {nanos}", std::process::id())
}

fn export_for_task(task: &str) -> PathBuf {
    fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("centaur_export_")
        })
        .find_map(|entry| {
            let prompt = entry.path().join("COPY_THIS_PROMPT.txt");
            fs::read_to_string(prompt)
                .ok()
                .filter(|contents| contents.contains(task))
                .map(|_| entry.path())
        })
        .unwrap_or_else(|| panic!("no export found for task {task}"))
}

fn exported_context(dir: &Path) -> String {
    fn collect(dir: &Path, out: &mut String) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                collect(&path, out);
            } else if path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("centaur_context_part"))
            {
                out.push_str(&fs::read_to_string(path).unwrap());
            }
        }
    }

    let mut context = String::new();
    collect(dir, &mut context);
    context
}

#[test]
fn clean_git_project_uses_full_context_even_when_a_manifest_exists() {
    let workspace = tempdir().unwrap();
    init_repo(workspace.path());
    fs::write(workspace.path().join("README.md"), "# Fixture\n").unwrap();
    fs::write(
        workspace.path().join("untouched.txt"),
        "CLEAN_PROJECT_FULL_CONTEXT_SENTINEL\n",
    )
    .unwrap();
    commit_all(workspace.path());

    let task = unique_task("clean");
    let out = run_task(workspace.path(), &task);
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Context: full project"),
        "the guided command should disclose its context choice"
    );
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(workspace.path())
        .output()
        .unwrap();
    assert!(
        status.stdout.is_empty(),
        "starting a task wrote into the clean workspace: {}",
        String::from_utf8_lossy(&status.stdout)
    );

    let export = export_for_task(&task);
    let context = exported_context(&export);
    assert!(
        context.contains("CLEAN_PROJECT_FULL_CONTEXT_SENTINEL"),
        "a clean Git project should export the full project, not only manifests"
    );
    let _ = fs::remove_dir_all(export);
}

#[test]
fn dirty_git_project_uses_changed_context() {
    let workspace = tempdir().unwrap();
    init_repo(workspace.path());
    fs::write(workspace.path().join("README.md"), "# Fixture\n").unwrap();
    fs::write(workspace.path().join("dirty.txt"), "before\n").unwrap();
    fs::write(
        workspace.path().join("untouched.txt"),
        "DIRTY_PROJECT_UNTOUCHED_SENTINEL\n",
    )
    .unwrap();
    commit_all(workspace.path());
    fs::write(
        workspace.path().join("dirty.txt"),
        "DIRTY_PROJECT_CHANGED_SENTINEL\n",
    )
    .unwrap();
    let status_before = Command::new("git")
        .args(["status", "--porcelain", "-z", "--untracked-files=all"])
        .current_dir(workspace.path())
        .output()
        .unwrap()
        .stdout;

    let task = unique_task("dirty");
    let out = run_task(workspace.path(), &task);
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Context: changed files"),
        "the guided command should disclose its context choice"
    );
    let status_after = Command::new("git")
        .args(["status", "--porcelain", "-z", "--untracked-files=all"])
        .current_dir(workspace.path())
        .output()
        .unwrap()
        .stdout;
    assert_eq!(
        status_after, status_before,
        "starting a task changed the dirty workspace"
    );

    let export = export_for_task(&task);
    let context = exported_context(&export);
    assert!(
        context.contains("DIRTY_PROJECT_CHANGED_SENTINEL"),
        "the modified file was not exported"
    );
    assert!(
        !context.contains("DIRTY_PROJECT_UNTOUCHED_SENTINEL"),
        "a dirty Git project should not export unrelated tracked files"
    );
    let _ = fs::remove_dir_all(export);
}

#[test]
fn non_git_project_exports_the_complete_project() {
    let workspace = tempdir().unwrap();
    fs::write(
        workspace.path().join("first.txt"),
        "NON_GIT_FIRST_SENTINEL\n",
    )
    .unwrap();
    fs::write(
        workspace.path().join("second.txt"),
        "NON_GIT_SECOND_SENTINEL\n",
    )
    .unwrap();

    let task = unique_task("non-git");
    let out = run_task(workspace.path(), &task);
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Context: full project"),
        "non-Git projects should disclose that full context was selected"
    );

    let export = export_for_task(&task);
    let context = exported_context(&export);
    assert!(context.contains("NON_GIT_FIRST_SENTINEL"));
    assert!(context.contains("NON_GIT_SECOND_SENTINEL"));
    let _ = fs::remove_dir_all(export);
}

#[test]
fn full_override_includes_untouched_files_from_a_dirty_project() {
    let workspace = tempdir().unwrap();
    init_repo(workspace.path());
    fs::write(workspace.path().join("dirty.txt"), "before\n").unwrap();
    fs::write(
        workspace.path().join("untouched.txt"),
        "FULL_OVERRIDE_UNTOUCHED_SENTINEL\n",
    )
    .unwrap();
    commit_all(workspace.path());
    fs::write(workspace.path().join("dirty.txt"), "after\n").unwrap();

    let task = unique_task("full override");
    let home = tempdir().unwrap();
    fs::write(
        home.path().join("config.toml"),
        "[export]\ncopy_prompt = false\nopen_export_directory = false\n",
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_centaur"))
        .arg("task")
        .arg("--full")
        .args(task.split_whitespace())
        .current_dir(workspace.path())
        .env("CENTAUR_HOME", home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "stdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Context: full project"),
        "--full should override automatic changed-file context"
    );

    let export = export_for_task(&task);
    let context = exported_context(&export);
    assert!(context.contains("FULL_OVERRIDE_UNTOUCHED_SENTINEL"));
    let _ = fs::remove_dir_all(export);
}

#[test]
fn empty_task_fails_clearly_when_no_terminal_can_prompt() {
    let workspace = tempdir().unwrap();
    fs::write(workspace.path().join("project.txt"), "content\n").unwrap();
    let home = tempdir().unwrap();
    fs::write(
        home.path().join("config.toml"),
        "[export]\ncopy_prompt = false\nopen_export_directory = false\n",
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_centaur"))
        .arg("task")
        .current_dir(workspace.path())
        .env("CENTAUR_HOME", home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();

    assert!(
        !out.status.success(),
        "an empty non-interactive task must fail"
    );
    assert!(
        message.contains("task description"),
        "failure should explain what is missing: {message}"
    );
}

#[test]
fn task_without_exportable_files_fails_clearly() {
    let workspace = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        home.path().join("config.toml"),
        "[export]\ncopy_prompt = false\nopen_export_directory = false\n",
    )
    .unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_centaur"))
        .args(["task", "add", "a", "homepage"])
        .current_dir(workspace.path())
        .env("CENTAUR_HOME", home.path())
        .env("NO_COLOR", "1")
        .output()
        .unwrap();
    let message = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    )
    .to_lowercase();

    assert!(!out.status.success(), "a context-free task cannot proceed");
    assert!(
        message.contains("no") && message.contains("file"),
        "failure should explain that no project files were found: {message}"
    );
}

#[test]
fn task_returns_failure_when_the_export_cannot_be_written() {
    let workspace = tempdir().unwrap();
    fs::write(workspace.path().join("project.txt"), "content\n").unwrap();
    let home = tempdir().unwrap();
    fs::write(
        home.path().join("config.toml"),
        "[export]\ncopy_prompt = false\nopen_export_directory = false\n",
    )
    .unwrap();
    let blocked_temp = home.path().join("not-a-directory");
    fs::write(&blocked_temp, "file blocks directory creation").unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_centaur"))
        .args(["task", "update", "the", "project"])
        .current_dir(workspace.path())
        .env("CENTAUR_HOME", home.path())
        .env("TEMP", &blocked_temp)
        .env("TMP", &blocked_temp)
        .env("NO_COLOR", "1")
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "an unwritable export destination must not report success"
    );
}
