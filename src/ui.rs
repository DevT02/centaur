use arboard::Clipboard;
use colored::*;
use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet};
use inquire::{Confirm, Select, Text};
use std::env;
use std::fs;
use std::path::PathBuf;
use crate::config::CentaurConfig;
use crate::export;
use crate::git::{get_changed_files, is_git_repo, ExportMode};
use crate::history::PatchSessionRecord;
use crate::pack::PackOptions;
use crate::patch::{apply_blocks_transactional, ApplyResult};
use crate::prompt::handle_prompt_edit;
use crate::secrets::scan_file_for_secrets;
use crate::{parse_blocks, summarize_patch_blocks, PatchBlock};

pub fn configure_visual_theme() -> RenderConfig<'static> {
    let mut config = RenderConfig::default_colored();
    config.prompt_prefix = inquire::ui::Styled::new(" ❯ ").with_fg(Color::LightCyan).with_attr(Attributes::BOLD);
    config.selected_option = Some(StyleSheet::new().with_fg(Color::LightYellow).with_attr(Attributes::BOLD));
    config.highlighted_option_prefix = inquire::ui::Styled::new(" ➜ ").with_fg(Color::LightYellow).with_attr(Attributes::BOLD);
    config.scroll_up_prefix = inquire::ui::Styled::new(" ▲ ").with_fg(Color::LightCyan);
    config.scroll_down_prefix = inquire::ui::Styled::new(" ▼ ").with_fg(Color::LightCyan);
    config
}

pub fn render_workspace_dashboard() {
    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root_path_str = root_dir.to_string_lossy();

    println!();
    println!("{}", "╭─────────────────────────────────────────────────────────────────────────────╮".bright_cyan());
    println!(
        "{} {}   {}",
        "│".bright_cyan(),
        "✦ THE CLIPBOARD CENTAUR ✦".bright_yellow().bold(),
        "AI Pair-Programming & Patch Engine │".bright_cyan().bold()
    );
    println!("{}", "├─────────────────────────────────────────────────────────────────────────────┤".bright_cyan());

    // Project Root Line
    println!(
        "{}  {} {} {}",
        "│".bright_cyan(),
        "📁 Workspace :".bright_white().bold(),
        root_path_str.bright_cyan(),
        " ".repeat(71usize.saturating_sub(17 + root_path_str.len())) + "│".bright_cyan().to_string().as_str()
    );

    // Git Branch Line
    if is_git_repo(&root_dir) {
        let branch = get_git_branch(&root_dir);
        let changed = get_changed_files(&root_dir);
        let changed_count = changed.len();
        let git_status = if changed_count > 0 {
            format!("{} ({} file(s) modified)", branch.bright_cyan(), changed_count.to_string().bright_yellow().bold())
        } else {
            format!("{} (clean)", branch.bright_cyan())
        };
        let git_line = format!("🌿 Git Status: {}", git_status);
        let display_len = 15 + branch.len() + if changed_count > 0 { 21 + changed_count.to_string().len() } else { 8 };
        println!(
            "{}  {} {}",
            "│".bright_cyan(),
            git_line,
            " ".repeat(73usize.saturating_sub(display_len)) + "│".bright_cyan().to_string().as_str()
        );
    }

    // Clipboard Status Line
    let clip_text = Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()).unwrap_or_default();
    let clip_blocks = parse_blocks(&clip_text);

    if !clip_blocks.is_empty() {
        let targets: Vec<String> = clip_blocks.iter().map(|b| b.file_path.clone()).collect();
        let targets_str = targets.join(", ");
        let clip_msg = format!("⚡ Edits Ready: {} block(s) for ({})", clip_blocks.len(), targets_str);
        let display_len = 17 + clip_blocks.len().to_string().len() + targets_str.len();
        println!(
            "{}  {} {}",
            "│".bright_cyan(),
            clip_msg.bright_green().bold(),
            " ".repeat(73usize.saturating_sub(display_len)) + "│".bright_cyan().to_string().as_str()
        );
    } else {
        let clip_msg = "📋 Clipboard  : Clear (No Search/Replace blocks in clipboard)";
        println!(
            "{}  {} {}",
            "│".bright_cyan(),
            clip_msg.dimmed(),
            " ".repeat(73usize.saturating_sub(60)) + "│".bright_cyan().to_string().as_str()
        );
    }

    println!("{}", "╰─────────────────────────────────────────────────────────────────────────────╯".bright_cyan());
    println!();
}

fn get_git_branch(root: &PathBuf) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output();
    match output {
        Ok(out) if out.status.success() => {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        }
        _ => "unknown".to_string(),
    }
}

/// Warn when the reply contained blocks the parser could not read, so a malformed
/// delimiter does not quietly drop an edit the user believes was applied.
fn warn_about_dropped_blocks(source: &str, parsed: usize) {
    let markers = crate::count_search_markers(source);
    if markers > parsed {
        println!(
            "   {}",
            format!(
                "⚠️  {} of {} block(s) could not be parsed and will be skipped — check the delimiter lines.",
                markers - parsed,
                markers
            )
            .bright_yellow()
            .bold()
        );
    }
}

fn print_patch_plan(blocks: &[PatchBlock]) {
    println!("\n{}", "🧾 --- PATCH REVIEW ---".bright_cyan().bold());
    for summary in summarize_patch_blocks(blocks) {
        let action = if summary.creates_file { "CREATE" } else { "UPDATE" };
        println!(
            "   {} {}  ({} block(s), -{} +{} lines)",
            action.bright_yellow().bold(),
            summary.file_path.as_str().bright_white(),
            summary.block_count,
            summary.removed_lines,
            summary.added_lines
        );
    }
}

fn review_and_apply_blocks(blocks: &[PatchBlock]) -> bool {
    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    match apply_blocks_transactional(&root_dir, blocks, true) {
        Ok(_) => {}
        Err((failures, msg)) => {
            println!("\n{}", format!("❌ Patch validation failed: {}", msg).bright_red().bold());
            for failure in failures {
                println!("   - {}", format!("{:?}", failure).bright_red());
            }
            return true;
        }
    }

    print_patch_plan(blocks);

    let apply_now = Confirm::new("Apply this validated patch plan?")
        .with_default(false)
        .with_render_config(configure_visual_theme())
        .prompt()
        .unwrap_or(false);

    if !apply_now {
        println!("{}", "Patch was not applied.".bright_yellow());
        return false;
    }

    match apply_blocks_transactional(&root_dir, blocks, false) {
        Ok(results) => {
            let mut had_write_error = false;
            println!("\n{}", "✨ Patch results:".bright_green().bold());

            for result in results {
                match result {
                    ApplyResult::Created(path) => {
                        println!("   - {}", format!("Created {}", path).bright_green())
                    }
                    ApplyResult::Updated(path) => {
                        println!("   - {}", format!("Updated {}", path).bright_green())
                    }
                    ApplyResult::IoError(path, error) => {
                        had_write_error = true;
                        println!(
                            "   - {}",
                            format!("Could not write {}: {}", path, error).bright_red()
                        );
                    }
                    other => println!("   - {}", format!("{:?}", other).bright_yellow()),
                }
            }

            if had_write_error {
                println!(
                    "\n{}",
                    "Some files could not be written. Run 'centaur undo latest' to revert any successful writes."
                        .bright_red()
                        .bold()
                );
            } else {
                println!(
                    "\n{}",
                    "Undo this session at any time with: centaur undo latest"
                        .bright_cyan()
                        .bold()
                );
            }
        }
        Err((failures, msg)) => {
            println!("\n{}", format!("❌ Failed to apply patch: {}", msg).bright_red().bold());
            for failure in failures {
                println!("   - {}", format!("{:?}", failure).bright_red());
            }
        }
    }

    true
}

pub fn run_interactive_hub() {
    render_workspace_dashboard();
    let render_config = configure_visual_theme();

    // Surface clipboard prompt immediately if AI edits detected
    let clip_text = Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok()).unwrap_or_default();
    let clip_blocks = parse_blocks(&clip_text);

    if !clip_blocks.is_empty() {
        println!(
            "   {}",
            format!(
                "⚡ AI code edits detected: {} block(s). Review the validated plan below.",
                clip_blocks.len()
            )
            .bright_green()
            .bold()
        );
        warn_about_dropped_blocks(&clip_text, clip_blocks.len());

        if review_and_apply_blocks(&clip_blocks) {
            return;
        }
    }

    // Main Menu Options with clear color visual hierarchy
    let options = vec![
        "⚡ [1] REVIEW & APPLY       — Validate and review clipboard edits before writing",
        "📦 [2] SEND CODEBASE        — Package context & prompt for ChatGPT / Claude",
        "↺  [3] UNDO LAST CHANGES    — Revert files to pre-patch state",
        "🔍 [4] PREVIEW PATCH        — Dry-run test diff matching without modifying disk",
        "🛡️  [5] SECURITY AUDIT       — Scan project for exposed API keys & passwords",
        "📝 [6] EDIT PROMPT          — Customize upload prompt template",
        "🚀 [7] UPDATE CENTAUR       — Pull repo & rebuild centaur CLI on PATH",
        "🚪 [8] EXIT                 — Close Centaur dashboard",
    ];

    println!("{}", "  Select Action:".bright_white().bold());

    let choice = Select::new("", options)
        .with_render_config(render_config)
        .prompt();

    match choice {
        Ok(sel) if sel.contains("[1]") => run_apply_interactive(),
        Ok(sel) if sel.contains("[2]") => run_export_wizard(),
        Ok(sel) if sel.contains("[3]") => run_history_interactive(),
        Ok(sel) if sel.contains("[4]") => run_dry_run_interactive(),
        Ok(sel) if sel.contains("[5]") => {
            run_security_audit();
        }
        Ok(sel) if sel.contains("[6]") => run_prompt_edit_interactive(),
        Ok(sel) if sel.contains("[7]") => run_auto_update_interactive(),
        _ => println!("{}", "\n👋 Goodbye from Centaur!".bright_cyan().bold()),
    }
}

/// Centaur keeps two templates and picks between them by export size, so the menu
/// has to offer both — the single-upload one is the common case for small projects.
pub fn run_prompt_edit_interactive() {
    let options = vec![
        "📄 Single-upload template  — used when the export fits in one message",
        "📚 Multi-upload template   — used when the export is split into batches",
    ];
    let choice = Select::new("Which prompt template should we edit?", options.clone())
        .with_render_config(configure_visual_theme())
        .prompt()
        .unwrap_or(options[0]);

    if let Err(e) = handle_prompt_edit(choice.contains("Single")) {
        println!("{}", format!("❌ {}", e).bright_red());
    }
}

pub fn run_auto_update_interactive() {
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    println!("\n{}", "🚀 --- CENTAUR AUTO-UPDATE ---".bright_cyan().bold());

    if is_git_repo(&current_dir) {
        println!("{}", "📥 Pulling latest repository updates...".bright_yellow());
        let _ = std::process::Command::new("git").arg("pull").current_dir(&current_dir).status();
    }

    println!("{}", "🔨 Re-building and updating Centaur binary on PATH...".bright_yellow());
    let status = std::process::Command::new("cargo")
        .args(["install", "--path", ".", "--force"])
        .current_dir(&current_dir)
        .status();

    if let Ok(s) = status {
        if s.success() {
            println!("{}", "✨ Centaur CLI update complete!".bright_green().bold());
            return;
        }
    }
    println!("{}", "❌ Failed to update. Try running 'cargo install --path . --force'".bright_red());
}

pub fn run_apply_interactive() {
    println!("\n{}", "⚡ --- REVIEW & APPLY AI CODE EDITS ---".bright_cyan().bold());
    println!(
        "{}",
        "Validates every Search/Replace block, summarizes affected files, then asks before writing."
            .dimmed()
    );
    println!();

    let text = Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.get_text().ok())
        .unwrap_or_default();
    let blocks = parse_blocks(&text);

    if blocks.is_empty() {
        println!("{}", "⚠️ No Search/Replace blocks found in the clipboard.".bright_yellow());
        println!(
            "{}",
            "Copy the AI's complete response and try again, or save it to a file and run: centaur --file <path>"
                .dimmed()
        );
        return;
    }

    warn_about_dropped_blocks(&text, blocks.len());
    let _ = review_and_apply_blocks(&blocks);
}

pub fn run_export_wizard() {
    let render_config = configure_visual_theme();
    println!("\n{}", "📦 --- SEND CODEBASE TO CHATGPT / CLAUDE ---".bright_cyan().bold());
    println!("{}", "Packages project context into attachments with automated workflow instructions.".dimmed());
    println!();

    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root_name = root_dir.file_name().unwrap_or_default().to_string_lossy().to_string();

    let scopes = vec![
        "🌿 Changed Files Only  (Recommended — modified & untracked git files)",
        "📂 Entire Project      (All tracked source files in repository)",
        "📌 Staged Files Only   (Git staged files only)",
        "⚡ Compact Mode        (Full export omitting test fixtures & data)",
    ];

    let scope_choice = Select::new("Which files should be included in export?", scopes.clone())
        .with_render_config(render_config)
        .prompt()
        .unwrap_or(scopes[0]);

    let mode = if scope_choice.contains("Changed") {
        ExportMode::Changed
    } else if scope_choice.contains("Staged") {
        ExportMode::Staged
    } else if scope_choice.contains("Compact") {
        ExportMode::Compact
    } else {
        ExportMode::Full
    };

    println!(
        "{}",
        "Describe the desired outcome, important constraints, and how success should be verified. Leave blank for a conservative correctness/security review."
            .dimmed()
    );
    let task_prompt = Text::new("What should the AI change or review?")
        .with_help_message("Example: Fix duplicate exports, preserve existing CLI flags, and add a regression test.")
        .with_render_config(render_config)
        .prompt()
        .unwrap_or_default();

    let files_to_pack = export::collect_files(&root_dir, mode, &[]);

    // Security Scan
    let secret_warnings = export::scan_files(&files_to_pack);
    let mut auto_redact = false;

    if !secret_warnings.is_empty() {
        println!("\n{}", "🛡️  SECURITY WARNING: Sensitive credentials detected!".bright_red().bold());
        for w in &secret_warnings {
            println!("   - {} in {}", w.pattern_name.bright_red(), w.file_path.bright_yellow());
        }

        let sec_opts = vec![
            "🛡️  Automatically redact secrets in the export copy (Recommended)",
            "⚠️  Export as-is (include credentials in export)",
            "❌ Cancel export",
        ];
        let sec_choice = Select::new("How should secrets be handled?", sec_opts.clone())
            .with_render_config(render_config)
            .prompt()
            .unwrap_or(sec_opts[0]);

        if sec_choice.contains("Cancel") {
            println!("{}", "Export cancelled.".bright_yellow());
            return;
        }
        if sec_choice.contains("redact") {
            auto_redact = true;
        }
    }

    let config = CentaurConfig::load();
    let session_id = format!("{:08x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs());

    let request = export::ExportRequest {
        root: root_dir.clone(),
        task: task_prompt,
        redact: auto_redact,
        copy_prompt: config.export.copy_prompt,
        pack: PackOptions {
            max_attachment_chars: config.export.max_attachment_chars,
            max_attachments_per_message: config.export.max_attachments_per_message,
            context_token_budget: config.export.context_token_budget,
            is_compact_mode: mode == ExportMode::Compact,
            force: false,
            project_root_name: root_name,
            session_id,
        },
    };

    let outcome = match export::run(&request, files_to_pack) {
        Ok(outcome) => outcome,
        Err(e) => {
            println!("{}", format!("❌ Export failed: {}", e).bright_red().bold());
            return;
        }
    };
    let (result, export_dir, prompt_copied) =
        (&outcome.result, &outcome.export_dir, outcome.prompt_copied);

    println!("\n{}", "📊 --- EXPORT SUMMARY ---".bright_cyan().bold());
    println!("   Files exported      : {}", result.summary.total_files.to_string().bright_yellow());
    println!("   Total characters    : {}", result.summary.total_chars.to_string().bright_yellow());
    println!("   Estimated AI tokens : {}", format!("~{}", result.summary.estimated_tokens).bright_yellow());
    println!("   Attachments created : {}", result.summary.total_parts.to_string().bright_yellow());
    println!("   Upload messages     : {}", result.summary.total_batches.to_string().bright_yellow());

    if let Some(warn) = &result.summary.token_warning {
        println!("\n{}", warn.bright_yellow());
    }

    if !outcome.write_errors.is_empty() {
        println!("\n{}", "❌ Some export files could not be written:".bright_red().bold());
        for e in &outcome.write_errors {
            println!("   - {}", e.bright_red());
        }
    }

    if result.summary.total_batches == 1 {
        println!("\n{}", "✨ Export complete!".bright_green().bold());
        println!("\n{}", "📋 --- NEXT STEPS ---".bright_cyan().bold());
        println!("   1. Export Folder : {}", export_dir.display().to_string().bright_yellow());
        if prompt_copied {
            println!("   2. Prompt        : Already copied — paste it into ChatGPT / Claude");
        } else {
            println!("   2. Prompt        : Copy COPY_THIS_PROMPT.txt into ChatGPT / Claude");
        }
        println!("   3. Attachments   : Upload all centaur_context_part*.txt files");
        println!("   4. Apply Edits   : Copy the AI reply, run 'centaur', review the plan, and approve");
    } else {
        println!("\n{}", format!("✨ Export complete! {} upload messages required.", result.summary.total_batches).bright_green().bold());
        println!("   Export Folder : {}", export_dir.display().to_string().bright_yellow());
        if prompt_copied {
            println!("   1. Paste the prompt already copied to your clipboard");
        } else {
            println!("   1. Paste COPY_THIS_PROMPT.txt into ChatGPT / Claude");
        }
        println!("   2. Upload batch_01/ files and send message");
        for b in 2..=result.summary.total_batches {
            println!("   {}. Upload batch_{:02}/ files after AI acknowledges batch {}", b + 1, b, b - 1);
        }
    }

    if config.export.open_export_directory {
        open_directory(export_dir);
    }
}

pub fn open_directory(dir: &std::path::Path) {
    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

pub fn run_history_interactive() {
    let render_config = configure_visual_theme();
    println!("\n{}", "↺  --- UNDO LAST AI CHANGES ---".bright_cyan().bold());
    println!("{}", "Reverts files to their exact state prior to the selected patch session.".dimmed());
    println!();

    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let sessions = PatchSessionRecord::list_sessions_for(&root_dir);
    if sessions.is_empty() {
        println!("{}", "⚠️ No previous patch sessions found for this workspace.".bright_yellow());
        return;
    }

    let mut options = Vec::new();
    for s in &sessions {
        let files_str: Vec<String> = s.files.iter().map(|f| f.relative_path.clone()).collect();
        options.push(format!("Session {} — {} file(s): {}", s.session_id.bright_yellow(), s.files.len(), files_str.join(", ")));
    }

    // Select by index. The rendered label embeds ANSI colour codes, so parsing the
    // session id back out of the chosen string never matched in a real terminal.
    if let Ok(choice) = Select::new("Select a session to revert:", options)
        .with_render_config(render_config)
        .raw_prompt()
    {
        let sess_id = sessions[choice.index].session_id.as_str();
        match PatchSessionRecord::revert_session(&root_dir, sess_id) {
            Ok(restored) => {
                println!("\n{}", "✨ Revert Complete:".bright_green().bold());
                for r in restored {
                    println!("   - {}", r.bright_green());
                }
            }
            Err(e) => println!("{}", format!("❌ Failed to revert: {}", e).bright_red()),
        }
    }
}

pub fn run_dry_run_interactive() {
    println!("\n{}", "🔍 --- PREVIEW PATCH (DRY RUN) ---".bright_cyan().bold());
    println!("{}", "Simulates patch block matching without modifying any files on disk.".dimmed());
    println!();

    let mut text = String::new();
    if let Ok(mut cb) = Clipboard::new() {
        if let Ok(clip_text) = cb.get_text() {
            text = clip_text;
        }
    }
    let blocks = parse_blocks(&text);
    if blocks.is_empty() {
        println!("{}", "⚠️ No Search/Replace code blocks found in clipboard to preview.".bright_yellow());
        return;
    }

    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match apply_blocks_transactional(&root_dir, &blocks, true) {
        Ok(results) => {
            println!("{}", "✨ Match Preview Successful:".bright_green().bold());
            for r in results {
                println!("   - {}", format!("{:?}", r).bright_green());
            }
        }
        Err((_, msg)) => println!("{}", format!("❌ Match Preview Failed: {}", msg).bright_red()),
    }
}

/// Returns the number of potential secrets found, so callers can set an exit code.
pub fn run_security_audit() -> usize {
    println!("\n{}", "🛡️  --- SECURITY AUDIT ---".bright_cyan().bold());
    println!("{}", "Scans workspace files for leaked API keys, tokens, and passwords.".dimmed());
    println!();

    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut found = Vec::new();

    for entry in crate::pack::walk_workspace(&root_dir).flatten() {
        let p = entry.path();
        if p.is_file() {
            if let Ok(content) = fs::read_to_string(p) {
                found.extend(scan_file_for_secrets(p, &content));
            }
        }
    }

    if found.is_empty() {
        println!("{}", "✨ Security Audit Passed: No credentials detected in workspace.".bright_green().bold());
    } else {
        println!("{}", format!("⚠️ Found {} potential secret warning(s):", found.len()).bright_red().bold());
        for f in &found {
            println!("   - {} in {}", f.pattern_name.bright_red(), f.file_path.bright_yellow());
        }
    }
    found.len()
}
