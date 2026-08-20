//! The binary used to exit 0 unconditionally, so nothing could script or gate on it.

use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::tempdir;

fn centaur() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_centaur"));
    // Keep the spawned binary's config and patch history out of the real user profile.
    let home = std::env::temp_dir().join(format!("centaur_exit_home_{}", std::process::id()));
    let _ = fs::create_dir_all(&home);
    cmd.env("CENTAUR_HOME", &home);
    cmd
}

#[test]
fn unparseable_input_exits_nonzero() {
    let dir = tempdir().unwrap();
    let patch = dir.path().join("reply.txt");
    fs::write(
        &patch,
        "The AI replied in prose and produced no blocks at all.",
    )
    .unwrap();

    let out = centaur()
        .arg("--file")
        .arg(&patch)
        .arg("--yes")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success(), "no-blocks input should fail");
}

#[test]
fn missing_patch_file_exits_nonzero() {
    let dir = tempdir().unwrap();
    let out = centaur()
        .arg("--file")
        .arg(dir.path().join("does_not_exist.txt"))
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success(), "unreadable patch file should fail");
}

#[test]
fn unmatchable_patch_exits_nonzero() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("code.rs"), "fn main() {}\n").unwrap();
    let patch = dir.path().join("reply.txt");
    fs::write(
        &patch,
        "File: code.rs\n<<<<<<< SEARCH\nfn nothing_like_this() {}\n=======\nfn replaced() {}\n>>>>>>> REPLACE\n",
    )
    .unwrap();

    let out = centaur()
        .arg("--file")
        .arg(&patch)
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(!out.status.success(), "unmatched block should fail");
    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn successful_patch_exits_zero() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("code.rs"), "fn main() {}\n").unwrap();
    let patch = dir.path().join("reply.txt");
    fs::write(
        &patch,
        "File: code.rs\n<<<<<<< SEARCH\nfn main() {}\n=======\nfn main() { run(); }\n>>>>>>> REPLACE\n",
    )
    .unwrap();

    let out = centaur()
        .arg("--file")
        .arg(&patch)
        .arg("--yes")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "applied patch should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "fn main() { run(); }\n"
    );
}

#[test]
fn dry_run_prints_the_exact_diff_without_writing() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("code.rs"), "fn main() {}\n").unwrap();
    let patch = dir.path().join("reply.txt");
    fs::write(
        &patch,
        "File: code.rs\n<<<<<<< SEARCH\nfn main() {}\n=======\nfn main() { run(); }\n>>>>>>> REPLACE\n",
    )
    .unwrap();

    let out = centaur()
        .args(["--dry-run", "--file"])
        .arg(&patch)
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);

    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("-fn main() {}\n+fn main() { run(); }"),
        "{stdout}"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn explicit_no_changes_response_exits_zero() {
    let dir = tempdir().unwrap();
    let patch = dir.path().join("reply.txt");
    fs::write(&patch, "NO_CHANGES\n").unwrap();

    let out = centaur()
        .arg("--file")
        .arg(&patch)
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "an explicit no-op is a successful result"
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("workspace was left untouched"));
}

#[test]
fn malformed_block_aborts_the_complete_payload() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("a.txt"), "old a\n").unwrap();
    fs::write(dir.path().join("b.txt"), "old b\n").unwrap();
    let patch = dir.path().join("reply.txt");
    fs::write(
        &patch,
        "File: a.txt\n<<<<<<< SEARCH\nold a\n=======\nnew a\n>>>>>>> REPLACE\n\n\
         File: b.txt\n<<<<<< SEARCH\nold b\n=======\nnew b\n>>>>>> REPLACE\n",
    )
    .unwrap();

    let out = centaur()
        .arg("--file")
        .arg(&patch)
        .arg("--yes")
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("parsed 1 of 2"));
    assert_eq!(
        fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "old a\n"
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "old b\n"
    );
}

#[test]
fn piped_patch_requires_explicit_approval() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("code.rs"), "fn main() {}\n").unwrap();
    let payload = "File: code.rs\n<<<<<<< SEARCH\nfn main() {}\n=======\nfn main() { run(); }\n>>>>>>> REPLACE\n";

    let mut child = centaur()
        .arg("--stdin")
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();

    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--yes"));
    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn piped_patch_with_approval_applies() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("code.rs"), "fn main() {}\n").unwrap();
    let payload = "File: code.rs\n<<<<<<< SEARCH\nfn main() {}\n=======\nfn main() { run(); }\n>>>>>>> REPLACE\n";

    let mut child = centaur()
        .args(["--stdin", "--yes"])
        .current_dir(dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();

    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("code.rs")).unwrap(),
        "fn main() { run(); }\n"
    );
}

#[test]
fn audit_exits_nonzero_when_it_finds_a_secret() {
    let clean = tempdir().unwrap();
    fs::write(clean.path().join("ok.rs"), "fn main() {}\n").unwrap();
    let out = centaur()
        .arg("audit")
        .current_dir(clean.path())
        .output()
        .unwrap();
    assert!(out.status.success(), "clean workspace should pass");

    let leaky = tempdir().unwrap();
    // A dotfile: invisible to the old walker, which is the point.
    fs::write(leaky.path().join(".env"), "AWS=AKIAIOSFODNN7EXAMPLE\n").unwrap();
    let out = centaur()
        .arg("audit")
        .current_dir(leaky.path())
        .output()
        .unwrap();
    assert!(
        !out.status.success(),
        "leaked credential should fail the audit"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains(".env"),
        "audit must report the .env file: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}

#[test]
fn invalid_export_mode_exits_nonzero() {
    let dir = tempdir().unwrap();
    let out = centaur()
        .args(["--export", "--mode", "change"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "unknown modes must not fall back to a full export"
    );
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid value"));
}
