use std::fs;
use tempfile::tempdir;
use the_clipboard_centaur::history::{BackupFileRecord, PatchSessionRecord};
use the_clipboard_centaur::{
    apply_blocks_transactional, pack_files_dynamic, PackOptions, PatchBlock,
};

/// Keep patch history out of the developer's real config directory.
fn use_scratch_home() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let home = std::env::temp_dir().join(format!("centaur_it_home_{}", std::process::id()));
        let _ = fs::create_dir_all(&home);
        // Safety: set once, before any test thread reads CENTAUR_HOME.
        unsafe { std::env::set_var("CENTAUR_HOME", &home) };
    });
}

/// A SEARCH block longer than the target file used to index past the end of the
/// line window and panic (abort, in release).
#[test]
fn search_block_longer_than_file_does_not_panic() {
    use_scratch_home();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("small.rs"), "aaa\nbbb\n").unwrap();

    let blocks = vec![PatchBlock {
        file_path: "small.rs".to_string(),
        // Leading lines match, so the fuzzy pass runs and walks off the end.
        search: "  aaa\n  bbb\n  ccc\n  ddd".to_string(),
        replace: "zzz".to_string(),
    }];

    let res = apply_blocks_transactional(dir.path(), &blocks, true);
    assert!(res.is_err(), "unmatchable block should fail, not apply");
    assert_eq!(fs::read_to_string(dir.path().join("small.rs")).unwrap(), "aaa\nbbb\n");
}

/// History is stored globally. Reverting from a different workspace used to
/// restore this project's content over the other project's files.
#[test]
fn undo_does_not_reach_into_another_workspace() {
    use_scratch_home();
    let proj_a = tempdir().unwrap();
    let proj_b = tempdir().unwrap();
    fs::write(proj_a.path().join("shared.txt"), "PROJECT A\n").unwrap();
    fs::write(proj_b.path().join("shared.txt"), "PROJECT B\n").unwrap();

    let blocks = vec![PatchBlock {
        file_path: "shared.txt".to_string(),
        search: "PROJECT A".to_string(),
        replace: "PATCHED".to_string(),
    }];
    apply_blocks_transactional(proj_a.path(), &blocks, false).unwrap();

    // "latest" from inside project B must not see project A's session.
    let res = PatchSessionRecord::revert_session(proj_b.path(), "latest");
    assert!(res.is_err(), "cross-workspace revert should be refused: {:?}", res);
    assert_eq!(fs::read_to_string(proj_b.path().join("shared.txt")).unwrap(), "PROJECT B\n");

    // ...but reverting inside project A still works.
    PatchSessionRecord::revert_session(proj_a.path(), "latest").unwrap();
    assert_eq!(fs::read_to_string(proj_a.path().join("shared.txt")).unwrap(), "PROJECT A\n");
}

/// Session records are on-disk input; a traversing path must not be written.
#[test]
fn revert_refuses_paths_outside_the_workspace() {
    use_scratch_home();
    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("victim.txt"), "ORIGINAL\n").unwrap();

    let escape = format!("../{}/victim.txt", outside.path().file_name().unwrap().to_string_lossy());
    let id = PatchSessionRecord::record_patch_session(
        workspace.path(),
        vec![BackupFileRecord {
            relative_path: escape,
            original_content: Some("OVERWRITTEN\n".to_string()),
            is_new_file: false,
        }],
    )
    .unwrap();

    let res = PatchSessionRecord::revert_session(workspace.path(), &id);
    assert!(res.is_err(), "traversing revert should be refused: {:?}", res);
    assert_eq!(fs::read_to_string(outside.path().join("victim.txt")).unwrap(), "ORIGINAL\n");
}

/// Applying a patch must never leave the workspace changed with no way back.
#[test]
fn applying_a_patch_records_an_undo_snapshot_for_this_workspace() {
    use_scratch_home();
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("f.txt"), "before\n").unwrap();

    apply_blocks_transactional(
        dir.path(),
        &[PatchBlock {
            file_path: "f.txt".to_string(),
            search: "before".to_string(),
            replace: "after".to_string(),
        }],
        false,
    )
    .unwrap();

    let sessions = PatchSessionRecord::list_sessions_for(dir.path());
    assert_eq!(sessions.len(), 1, "expected exactly one session for this workspace");
    assert_eq!(sessions[0].files[0].original_content.as_deref(), Some("before\n"));
}

#[test]
fn multiple_blocks_for_one_file_are_combined() {
    use_scratch_home();
    let dir = tempdir().unwrap();
    let target = dir.path().join("combined.txt");
    fs::write(&target, "alpha\nbeta\n").unwrap();

    let blocks = vec![
        PatchBlock {
            file_path: "combined.txt".to_string(),
            search: "alpha".to_string(),
            replace: "one".to_string(),
        },
        PatchBlock {
            file_path: "combined.txt".to_string(),
            search: "beta".to_string(),
            replace: "two".to_string(),
        },
    ];

    let results = apply_blocks_transactional(dir.path(), &blocks, false).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(fs::read_to_string(target).unwrap(), "one\ntwo\n");
}

#[test]
fn zero_export_limits_are_clamped() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("small.txt");
    fs::write(&source, "content").unwrap();

    let options = PackOptions {
        max_attachment_chars: 0,
        max_attachments_per_message: 0,
        ..PackOptions::default()
    };

    let result = pack_files_dynamic(vec![source], dir.path(), options);
    assert_eq!(result.summary.total_parts, 1);
    assert_eq!(result.summary.total_batches, 1);
}

#[test]
fn duplicate_export_paths_are_packed_once() {
    let dir = tempdir().unwrap();
    let source = dir.path().join("src.rs");
    fs::write(&source, "pub fn example() {}").unwrap();

    let result = pack_files_dynamic(
        vec![source.clone(), source],
        dir.path(),
        PackOptions::default(),
    );

    assert_eq!(result.summary.total_files, 1);
    assert_eq!(result.chunks[0].content.matches("File: `src.rs`").count(), 1);
}

#[cfg(unix)]
#[test]
fn new_file_cannot_escape_through_symlinked_parent() {
    use_scratch_home();
    use std::os::unix::fs::symlink;

    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    symlink(outside.path(), workspace.path().join("outside-link")).unwrap();

    let blocks = vec![PatchBlock {
        file_path: "outside-link/new.txt".to_string(),
        search: String::new(),
        replace: "should not be written".to_string(),
    }];

    let result = apply_blocks_transactional(workspace.path(), &blocks, false);
    assert!(result.is_err());
    assert!(!outside.path().join("new.txt").exists());
}
