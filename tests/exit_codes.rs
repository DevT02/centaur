//! The binary used to exit 0 unconditionally, so nothing could script or gate on it.

use std::fs;
use std::process::Command;
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
    fs::write(&patch, "The AI replied in prose and produced no blocks at all.").unwrap();

    let out = centaur().arg("--file").arg(&patch).current_dir(dir.path()).output().unwrap();
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

    let out = centaur().arg("--file").arg(&patch).current_dir(dir.path()).output().unwrap();
    assert!(!out.status.success(), "unmatched block should fail");
    assert_eq!(fs::read_to_string(dir.path().join("code.rs")).unwrap(), "fn main() {}\n");
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

    let out = centaur().arg("--file").arg(&patch).current_dir(dir.path()).output().unwrap();
    assert!(
        out.status.success(),
        "applied patch should succeed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(fs::read_to_string(dir.path().join("code.rs")).unwrap(), "fn main() { run(); }\n");
}

#[test]
fn audit_exits_nonzero_when_it_finds_a_secret() {
    let clean = tempdir().unwrap();
    fs::write(clean.path().join("ok.rs"), "fn main() {}\n").unwrap();
    let out = centaur().arg("audit").current_dir(clean.path()).output().unwrap();
    assert!(out.status.success(), "clean workspace should pass");

    let leaky = tempdir().unwrap();
    // A dotfile: invisible to the old walker, which is the point.
    fs::write(leaky.path().join(".env"), "AWS=AKIAIOSFODNN7EXAMPLE\n").unwrap();
    let out = centaur().arg("audit").current_dir(leaky.path()).output().unwrap();
    assert!(!out.status.success(), "leaked credential should fail the audit");
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

    assert!(!out.status.success(), "unknown modes must not fall back to a full export");
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid value"));
}
