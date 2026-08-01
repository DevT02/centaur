use arboard::Clipboard;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

// Used when codebase fits in a single upload message — no batching handshake needed
pub const DEFAULT_PROMPT_TEMPLATE_SINGLE: &str = r#"I am attaching my codebase exported by The Clipboard Centaur ({{TOTAL_PARTS}} file(s), session {{SESSION_ID}}).

Please read all attached files and then complete this task:
{{USER_TASK}}

OUTPUT FORMAT FOR CODE MODIFICATIONS
When proposing code modifications or creating files, output Search/Replace blocks in this EXACT format:

File: src/path/to/file.rs
<<<<<<< SEARCH
exact original lines of code from the file
=======
new replacement lines of code
>>>>>>> REPLACE

CRITICAL PATCH RULES:
1. Do NOT wrap Search/Replace blocks in backticks (```).
2. Use relative file paths exactly as shown in the context upload (e.g., src/main.rs). Do NOT add ./ or placeholders.
3. Every SEARCH section must contain EXACT matching lines from the current file content. Include 2-3 surrounding lines of context to ensure a unique match.
4. To create a NEW file, leave the SEARCH section empty:
File: src/new_file.rs
<<<<<<< SEARCH
=======
// new file content
>>>>>>> REPLACE
5. Keep explanations AFTER all Search/Replace blocks.
"#;

// Used when the codebase is too large and must be sent across multiple upload messages
pub const DEFAULT_PROMPT_TEMPLATE: &str = r#"You are assisting with a codebase exported by The Clipboard Centaur.

UPLOAD PROTOCOL
I will provide this codebase across {{TOTAL_BATCHES}} upload messages containing {{TOTAL_PARTS}} numbered context files.
Session ID: {{SESSION_ID}}

Until you receive the final part:
- Do not analyze the project.
- Do not propose changes or summarize files.
- Reply ONLY with: RECEIVED BATCH <number>.
- Retain all context from previously supplied parts.

When all parts are provided, perform this task:
{{USER_TASK}}

OUTPUT FORMAT FOR CODE MODIFICATIONS
When proposing code modifications or creating files, output Search/Replace blocks in this EXACT format:

File: src/path/to/file.rs
<<<<<<< SEARCH
exact original lines of code from the file
=======
new replacement lines of code
>>>>>>> REPLACE

CRITICAL PATCH RULES:
1. Do NOT wrap Search/Replace blocks in backticks (```).
2. Use relative file paths exactly as shown in the context upload (e.g., src/main.rs). Do NOT add ./ or placeholders.
3. Every SEARCH section must contain EXACT matching lines from the current file content. Include 2-3 surrounding lines of context to ensure a unique match.
4. To create a NEW file, leave the SEARCH section empty:
File: src/new_file.rs
<<<<<<< SEARCH
=======
// new file content
>>>>>>> REPLACE
5. Keep explanations AFTER all Search/Replace blocks.
"#;

pub fn prompt_template_path() -> PathBuf {
    crate::config::centaur_home().join("prompt_template.txt")
}

pub fn get_prompt_template() -> String {
    let path = prompt_template_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if !content.trim().is_empty() {
                return content;
            }
        }
    }
    DEFAULT_PROMPT_TEMPLATE.to_string()
}

pub fn single_prompt_template_path() -> PathBuf {
    prompt_template_path().with_file_name("prompt_template_single.txt")
}

pub fn get_single_batch_prompt_template() -> String {
    // Check if user has a custom single-batch template saved
    let path = single_prompt_template_path();
    if path.exists() {
        if let Ok(content) = fs::read_to_string(&path) {
            if !content.trim().is_empty() {
                return content;
            }
        }
    }
    DEFAULT_PROMPT_TEMPLATE_SINGLE.to_string()
}

pub fn render_prompt(
    task: &str,
    session_id: &str,
    total_parts: usize,
    total_batches: usize,
) -> String {
    let task_text = if task.trim().is_empty() {
        "Review the uploaded code for high-confidence correctness, security, and reliability issues. Implement only justified fixes, preserve existing behavior where possible, and add or update focused tests for each behavior change."
    } else {
        task.trim()
    };

    let task_with_rules = format!(
        "{}\n\nWORKING RULES\n- Treat the task above as the source of truth. Do not invent unrelated features.\n- Inspect all supplied files before proposing changes.\n- Make the smallest complete change that solves the task and preserves existing behavior.\n- Fix additional correctness or security issues only when they are high-confidence and explain them separately.\n- Add or update focused tests whenever behavior changes.\n- Do not claim that tests or commands were run unless they were actually run.\n- If no code changes are needed, say so clearly and do not invent Search/Replace blocks.",
        task_text
    );

    // Single-batch: use the simpler, friendlier prompt with no batching handshake
    let template = if total_batches == 1 {
        get_single_batch_prompt_template()
    } else {
        get_prompt_template()
    };

    template
        .replace("{{SESSION_ID}}", session_id)
        .replace("{{TOTAL_PARTS}}", &total_parts.to_string())
        .replace("{{TOTAL_BATCHES}}", &total_batches.to_string())
        .replace("{{USER_TASK}}", &task_with_rules)
}

/// Both templates are live: `render_prompt` picks the single-batch one whenever the
/// export fits in one upload message, which is the common case for small projects.
pub fn handle_prompt_show() {
    println!("--- SINGLE-UPLOAD TEMPLATE (used when the export fits one message) ---");
    println!("Location: {}\n", single_prompt_template_path().display());
    println!("{}", get_single_batch_prompt_template());

    println!("\n--- MULTI-UPLOAD TEMPLATE (used when the export is split into batches) ---");
    println!("Location: {}\n", prompt_template_path().display());
    println!("{}", get_prompt_template());
}

pub fn handle_prompt_copy() -> Result<(), String> {
    let template = get_prompt_template();
    let mut cb = Clipboard::new().map_err(|e| format!("Clipboard error: {}", e))?;
    cb.set_text(template).map_err(|e| format!("Clipboard error: {}", e))?;
    println!("✅ Prompt template copied to clipboard.");
    Ok(())
}

pub fn handle_prompt_edit(single: bool) -> Result<(), String> {
    let (path, default) = if single {
        (single_prompt_template_path(), DEFAULT_PROMPT_TEMPLATE_SINGLE)
    } else {
        (prompt_template_path(), DEFAULT_PROMPT_TEMPLATE)
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Could not create {}: {}", parent.display(), e))?;
    }
    if !path.exists() {
        fs::write(&path, default).map_err(|e| format!("Could not write {}: {}", path.display(), e))?;
    }

    println!("Opening prompt template for editing: {}", path.display());

    // $VISUAL then $EDITOR on every platform — the help text always promised both.
    let editor = env::var("VISUAL").or_else(|_| env::var("EDITOR")).unwrap_or_else(|_| {
        if cfg!(target_os = "windows") { "notepad.exe".to_string() } else { "nano".to_string() }
    });
    Command::new(&editor)
        .arg(&path)
        .status()
        .map_err(|e| format!("Could not launch editor '{}': {}", editor, e))?;

    println!("✅ Prompt template editing complete.");
    Ok(())
}

pub fn handle_prompt_reset() -> Result<(), String> {
    // Both files, or "reset to default" leaves a customised template in place and
    // silently keeps overriding the default it claims to have restored.
    for path in [prompt_template_path(), single_prompt_template_path()] {
        if path.exists() {
            fs::remove_file(&path)
                .map_err(|e| format!("Could not reset {}: {}", path.display(), e))?;
        }
    }
    println!("✅ Prompt templates reset to defaults.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_prompt_placeholders() {
        let rendered = render_prompt("Fix bug in main", "sess123", 5, 2);
        assert!(rendered.contains("Session ID: sess123"));
        assert!(rendered.contains("Fix bug in main"));
        assert!(rendered.contains("5 numbered context files"));
        assert!(rendered.contains("across 2 upload messages"));
    }
}
