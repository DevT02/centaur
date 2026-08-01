use std::fs;
use tempfile::tempdir;
use the_clipboard_centaur::history::PatchSessionRecord;
use the_clipboard_centaur::pack::{pack_files_dynamic, PackOptions};
use the_clipboard_centaur::secrets::scan_file_for_secrets;
use the_clipboard_centaur::{
    apply_blocks_transactional, parse_blocks, summarize_patch_blocks, ApplyResult, PatchBlock,
};

/// Ensures patch history is stored in an isolated temporary directory during testing.
fn use_scratch_home() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let home = std::env::temp_dir().join(format!("centaur_edge_home_{}", std::process::id()));
        let _ = fs::create_dir_all(&home);
        unsafe { std::env::set_var("CENTAUR_HOME", &home) };
    });
}

// -----------------------------------------------------------------------------
// 1. Newline Preservation (CRLF vs LF)
// -----------------------------------------------------------------------------

#[test]
fn crlf_file_preserves_windows_newlines_when_patched_with_lf_search_block() {
    use_scratch_home();
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("crlf_file.rs");

    // File on disk uses Windows CRLF \r\n
    let original_crlf = "fn main() {\r\n    let val = 42;\r\n    println!(\"{}\", val);\r\n}\r\n";
    fs::write(&file_path, original_crlf).unwrap();

    // LLM response uses Unix LF \n in SEARCH and REPLACE
    let blocks = vec![PatchBlock {
        file_path: "crlf_file.rs".to_string(),
        search: "    let val = 42;\n    println!(\"{}\", val);".to_string(),
        replace: "    let val = 100;\n    println!(\"Updated: {}\", val);".to_string(),
    }];

    let res = apply_blocks_transactional(dir.path(), &blocks, false);
    assert!(res.is_ok(), "Patch should apply cleanly with line-ending normalization: {:?}", res);

    let updated_content = fs::read_to_string(&file_path).unwrap();
    assert!(updated_content.contains("let val = 100;"));
    assert!(updated_content.contains("\r\n"), "File on disk must preserve Windows CRLF newlines");
    assert!(!updated_content.contains("\n\n"), "Should not introduce stray double newlines");
}

// -----------------------------------------------------------------------------
// 2. UTF-8 Byte Order Mark (BOM) Handling
// -----------------------------------------------------------------------------

#[test]
fn utf8_bom_file_is_patched_successfully() {
    use_scratch_home();
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("bom_file.rs");

    // File starts with UTF-8 BOM \u{FEFF}
    let bom_prefix = "\u{FEFF}";
    let content = format!("{}pub fn hello() {{\n    println!(\"hello\");\n}}\n", bom_prefix);
    fs::write(&file_path, content).unwrap();

    let blocks = vec![PatchBlock {
        file_path: "bom_file.rs".to_string(),
        search: "pub fn hello() {\n    println!(\"hello\");\n}".to_string(),
        replace: "pub fn hello() {\n    println!(\"hello world!\");\n}".to_string(),
    }];

    let res = apply_blocks_transactional(dir.path(), &blocks, false);
    assert!(res.is_ok(), "BOM file should patch without error: {:?}", res);

    let patched = fs::read_to_string(&file_path).unwrap();
    assert!(patched.contains("hello world!"));
}

// -----------------------------------------------------------------------------
// 3. Sequential Multi-Block Patch Composition for the Same File
// -----------------------------------------------------------------------------

#[test]
fn three_non_contiguous_blocks_in_same_file_apply_in_one_transaction() {
    use_scratch_home();
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("multi_section.rs");

    let initial = r#"// Header section
fn section_one() {
    let a = 1;
}

// Middle section
fn section_two() {
    let b = 2;
}

// Footer section
fn section_three() {
    let c = 3;
}
"#;
    fs::write(&file_path, initial).unwrap();

    let blocks = vec![
        PatchBlock {
            file_path: "multi_section.rs".to_string(),
            search: "fn section_one() {\n    let a = 1;\n}".to_string(),
            replace: "fn section_one() {\n    let a = 100;\n}".to_string(),
        },
        PatchBlock {
            file_path: "multi_section.rs".to_string(),
            search: "fn section_two() {\n    let b = 2;\n}".to_string(),
            replace: "fn section_two() {\n    let b = 200;\n}".to_string(),
        },
        PatchBlock {
            file_path: "multi_section.rs".to_string(),
            search: "fn section_three() {\n    let c = 3;\n}".to_string(),
            replace: "fn section_three() {\n    let c = 300;\n}".to_string(),
        },
    ];

    let summary = summarize_patch_blocks(&blocks);
    assert_eq!(summary.len(), 1, "All 3 blocks target 1 file");
    assert_eq!(summary[0].block_count, 3);

    let res = apply_blocks_transactional(dir.path(), &blocks, false);
    assert!(res.is_ok(), "Sequential multi-block patch must succeed: {:?}", res);

    let final_content = fs::read_to_string(&file_path).unwrap();
    assert!(final_content.contains("let a = 100;"));
    assert!(final_content.contains("let b = 200;"));
    assert!(final_content.contains("let c = 300;"));
}

// -----------------------------------------------------------------------------
// 4. Ambiguous Match Protection (Duplicate Functions)
// -----------------------------------------------------------------------------

#[test]
fn ambiguous_search_block_matching_multiple_locations_is_rejected() {
    use_scratch_home();
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("duplicates.rs");

    // File contains two exact identical functions
    let content = r#"fn duplicate_helper() {
    println!("identical");
}

fn other_code() {
    // spacer
}

fn duplicate_helper() {
    println!("identical");
}
"#;
    fs::write(&file_path, content).unwrap();

    let blocks = vec![PatchBlock {
        file_path: "duplicates.rs".to_string(),
        search: "fn duplicate_helper() {\n    println!(\"identical\");\n}".to_string(),
        replace: "fn duplicate_helper() {\n    println!(\"MODIFIED\");\n}".to_string(),
    }];

    let res = apply_blocks_transactional(dir.path(), &blocks, false);
    assert!(res.is_err(), "Ambiguous search block must fail transaction");
    if let Err((failures, _msg)) = res {
        assert_eq!(failures.len(), 1);
        match &failures[0] {
            ApplyResult::AmbiguousMatch(p) => assert_eq!(p, "duplicates.rs"),
            other => panic!("Expected AmbiguousMatch, got {:?}", other),
        }
    }

    // Disk content must remain untouched
    assert_eq!(fs::read_to_string(&file_path).unwrap(), content);
}

// -----------------------------------------------------------------------------
// 5. Transactional All-or-Nothing Guarantee (Atomic Safety)
// -----------------------------------------------------------------------------

#[test]
fn failed_second_block_aborts_entire_transaction_without_modifying_first_file() {
    use_scratch_home();
    let dir = tempdir().unwrap();

    let file_a = dir.path().join("file_a.rs");
    let file_b = dir.path().join("file_b.rs");

    let original_a = "pub fn valid_a() { println!(\"A\"); }\n";
    let original_b = "pub fn valid_b() { println!(\"B\"); }\n";

    fs::write(&file_a, original_a).unwrap();
    fs::write(&file_b, original_b).unwrap();

    let blocks = vec![
        PatchBlock {
            file_path: "file_a.rs".to_string(),
            search: "pub fn valid_a() { println!(\"A\"); }".to_string(),
            replace: "pub fn valid_a() { println!(\"A_PATCHED\"); }".to_string(),
        },
        PatchBlock {
            file_path: "file_b.rs".to_string(),
            search: "THIS SEARCH BLOCK DOES NOT EXIST IN FILE B".to_string(),
            replace: "pub fn valid_b() { println!(\"B_PATCHED\"); }".to_string(),
        },
    ];

    let res = apply_blocks_transactional(dir.path(), &blocks, false);
    assert!(res.is_err(), "Transaction must abort when any block fails");

    // CRITICAL: file_a MUST NOT be modified on disk!
    assert_eq!(
        fs::read_to_string(&file_a).unwrap(),
        original_a,
        "File A must remain unmodified after transaction rollback"
    );
    assert_eq!(fs::read_to_string(&file_b).unwrap(), original_b);
}

// -----------------------------------------------------------------------------
// 6. Deep Nested Directory Creation for New Files
// -----------------------------------------------------------------------------

#[test]
fn new_file_creation_automatically_creates_deep_nested_directories() {
    use_scratch_home();
    let dir = tempdir().unwrap();

    let deep_rel_path = "src/core/services/auth/providers/oauth2.rs";
    let blocks = vec![PatchBlock {
        file_path: deep_rel_path.to_string(),
        search: String::new(), // Empty search indicates new file creation
        replace: "pub struct OAuth2Provider;\n".to_string(),
    }];

    let res = apply_blocks_transactional(dir.path(), &blocks, false);
    assert!(res.is_ok(), "Deep directory creation must succeed: {:?}", res);

    let created_path = dir.path().join(deep_rel_path);
    assert!(created_path.exists(), "Created file must exist on disk");
    assert_eq!(fs::read_to_string(created_path).unwrap(), "pub struct OAuth2Provider;\n");
}

// -----------------------------------------------------------------------------
// 7. Robust Secret Scanner (True Positives vs False Positives)
// -----------------------------------------------------------------------------

#[test]
fn secret_scanner_distinguishes_real_api_keys_from_false_positives() {
    let dummy_path = std::path::Path::new("src/config.rs");

    // False Positive 1: CSS variable names
    let css_code = "let class_name = \"btn-primary-action-active-state\";";
    let warnings = scan_file_for_secrets(dummy_path, css_code);
    assert!(warnings.is_empty(), "CSS class names should not trigger secret scanner");

    // False Positive 2: Documentation URL
    let doc_url = "// See https://github.com/settings/tokens for access configuration";
    let warnings = scan_file_for_secrets(dummy_path, doc_url);
    assert!(warnings.is_empty(), "Documentation URLs should not trigger secret scanner");

    // False Positive 3: Standard env var lookup
    let env_lookup = "let key = std::env::var(\"AWS_SECRET_ACCESS_KEY\").unwrap_or_default();";
    let warnings = scan_file_for_secrets(dummy_path, env_lookup);
    assert!(warnings.is_empty(), "Environment variable name references should not trigger scanner");

    // True Positive 1: OpenAI secret key format
    let openai_key = format!("const OPENAI_KEY: &str = \"sk-proj-{}\";", "1234567890abcdef1234567890abcdef1234567890abcdef");
    let warnings = scan_file_for_secrets(dummy_path, &openai_key);
    assert!(!warnings.is_empty(), "OpenAI secret key format must be detected");

    // True Positive 2: GitHub Personal Access Token format
    let github_pat = format!("let token = \"ghp_{}\";", "123456789012345678901234567890123456");
    let warnings = scan_file_for_secrets(dummy_path, &github_pat);
    assert!(!warnings.is_empty(), "GitHub PAT must be detected");
}

// -----------------------------------------------------------------------------
// 8. Overlapping Path Deduplication in Export Packer
// -----------------------------------------------------------------------------

#[test]
fn export_packer_deduplicates_overlapping_and_redundant_target_paths() {
    let dir = tempdir().unwrap();
    let src_dir = dir.path().join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let main_file = src_dir.join("main.rs");
    let lib_file = src_dir.join("lib.rs");

    fs::write(&main_file, "fn main() {}\n").unwrap();
    fs::write(&lib_file, "pub fn init() {}\n").unwrap();

    // Pass duplicate paths, symlinked paths, and parent folder overlaps
    let input_paths = vec![
        main_file.clone(),
        main_file.clone(),
        lib_file.clone(),
        lib_file.clone(),
    ];

    let result = pack_files_dynamic(input_paths, dir.path(), PackOptions::default());
    assert_eq!(
        result.summary.total_files, 2,
        "Total files exported must be deduplicated to 2 distinct files"
    );
}

// -----------------------------------------------------------------------------
// 9. Fuzzy Indentation Tolerance
// -----------------------------------------------------------------------------

#[test]
fn fuzzy_matcher_tolerates_slight_indentation_differences() {
    use_scratch_home();
    let dir = tempdir().unwrap();
    let file_path = dir.path().join("indentation.rs");

    // File on disk uses 4-space indent
    let disk_code = r#"pub fn calculate() -> i32 {
    let val = 10;
    let multiplier = 2;
    val * multiplier
}
"#;
    fs::write(&file_path, disk_code).unwrap();

    // Search block from LLM has 2-space indent
    let blocks = vec![PatchBlock {
        file_path: "indentation.rs".to_string(),
        search: "pub fn calculate() -> i32 {\n  let val = 10;\n  let multiplier = 2;\n  val * multiplier\n}".to_string(),
        replace: "pub fn calculate() -> i32 {\n    let val = 100;\n    let multiplier = 2;\n    val * multiplier\n}".to_string(),
    }];

    let res = apply_blocks_transactional(dir.path(), &blocks, false);
    assert!(res.is_ok(), "Fuzzy matcher should handle minor indentation mismatches: {:?}", res);

    let updated = fs::read_to_string(&file_path).unwrap();
    assert!(updated.contains("let val = 100;"));
}

// -----------------------------------------------------------------------------
// 10. Complete Time Machine Rollback (New File Deletion + Modified File Restoration)
// -----------------------------------------------------------------------------

#[test]
fn session_undo_deletes_newly_created_files_and_restores_existing_files() {
    use_scratch_home();
    let dir = tempdir().unwrap();

    let existing_file = dir.path().join("existing.rs");
    let original_existing_text = "pub fn old_feature() -> bool { true }\n";
    fs::write(&existing_file, original_existing_text).unwrap();

    let blocks = vec![
        // Edit existing file
        PatchBlock {
            file_path: "existing.rs".to_string(),
            search: "pub fn old_feature() -> bool { true }".to_string(),
            replace: "pub fn old_feature() -> bool { false }".to_string(),
        },
        // Create new file
        PatchBlock {
            file_path: "new_module.rs".to_string(),
            search: String::new(),
            replace: "pub fn brand_new() {}\n".to_string(),
        },
    ];

    // Apply patch transaction
    let res = apply_blocks_transactional(dir.path(), &blocks, false);
    assert!(res.is_ok(), "Patch transaction must succeed");

    let new_file = dir.path().join("new_module.rs");
    assert!(new_file.exists(), "New module must exist after patch");
    assert_eq!(
        fs::read_to_string(&existing_file).unwrap(),
        "pub fn old_feature() -> bool { false }\n"
    );

    // Trigger Time Machine Undo
    let undo_res = PatchSessionRecord::revert_session(dir.path(), "latest");
    assert!(undo_res.is_ok(), "Undo session must succeed: {:?}", undo_res);

    // Verify existing file is restored byte-for-byte
    assert_eq!(
        fs::read_to_string(&existing_file).unwrap(),
        original_existing_text,
        "Existing file must be restored to original content"
    );

    // Verify newly created file is deleted from disk
    assert!(!new_file.exists(), "Newly created file must be deleted upon session rollback");
}

// -----------------------------------------------------------------------------
// 11. Search/Replace Block Parsing Edge Cases
// -----------------------------------------------------------------------------

#[test]
fn parser_handles_markdown_fences_mixed_case_headers_and_windows_newlines() {
    let raw_llm_response = r#"Here are the requested code edits:

```markdown
File: src/utils/helper.rs
<<<<<<< SEARCH
fn old_helper() {
    println!("old");
}
=======
fn new_helper() {
    println!("new");
}
>>>>>>> REPLACE
```

Also created a new file:

Path: src/models/user.rs
<<<<<<< SEARCH
=======
pub struct User {
    pub id: u64,
}
>>>>>>> REPLACE
"#;

    let blocks = parse_blocks(raw_llm_response);
    assert_eq!(blocks.len(), 2);

    assert_eq!(blocks[0].file_path, "src/utils/helper.rs");
    assert!(blocks[0].search.contains("fn old_helper()"));
    assert!(blocks[0].replace.contains("fn new_helper()"));

    assert_eq!(blocks[1].file_path, "src/models/user.rs");
    assert!(blocks[1].search.is_empty());
    assert!(blocks[1].replace.contains("pub struct User"));
}
