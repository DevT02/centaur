use crate::config::CentaurConfig;
use crate::export;
use crate::git::{ExportMode, get_changed_files, is_git_repo};
use crate::history::PatchSessionRecord;
use crate::pack::PackOptions;
use crate::patch::{
    ApplyResult, apply_blocks_transactional, apply_planned_transactional, plan_blocks_transactional,
};
use crate::prompt::handle_prompt_edit;
use crate::secrets::scan_file_for_secrets;
use crate::{
    PatchBlock, PatchPayload, parse_patch_payload, render_patch_plan, summarize_patch_blocks,
};
use arboard::Clipboard;
use colored::*;
use inquire::ui::{Attributes, Color, RenderConfig, StyleSheet};
use inquire::{Confirm, Select, Text};
use std::env;
use std::fs;
use std::path::PathBuf;

pub fn configure_visual_theme() -> RenderConfig<'static> {
    let mut config = RenderConfig::default_colored();
    config.prompt_prefix = inquire::ui::Styled::new(" ❯ ")
        .with_fg(Color::LightCyan)
        .with_attr(Attributes::BOLD);
    config.selected_option = Some(
        StyleSheet::new()
            .with_fg(Color::LightYellow)
            .with_attr(Attributes::BOLD),
    );
    config.highlighted_option_prefix = inquire::ui::Styled::new("➜ ")
        .with_fg(Color::LightYellow)
        .with_attr(Attributes::BOLD);
    config.scroll_up_prefix = inquire::ui::Styled::new("");
    config.scroll_down_prefix = inquire::ui::Styled::new("");
    config
}

pub fn render_workspace_dashboard() {
    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root_path_str = root_dir.to_string_lossy();

    println!();
    println!(
        "{}",
        "╭─────────────────────────────────────────────────────────────────────────────╮"
            .bright_cyan()
    );
    println!(
        "{}  {}   {}",
        "│".bright_cyan(),
        "✦ THE CLIPBOARD CENTAUR ✦".bright_yellow().bold(),
        "AI Pair-Programming & Patch Engine".bright_cyan().bold()
    );
    println!(
        "{}",
        "├─────────────────────────────────────────────────────────────────────────────┤"
            .bright_cyan()
    );

    // Project Root Line
    println!(
        "{}  📁 {} {}",
        "│".bright_cyan(),
        "Workspace  :".bright_white().bold(),
        root_path_str.bright_cyan()
    );

    // Git Branch Line
    if is_git_repo(&root_dir) {
        let branch = get_git_branch(&root_dir);
        let changed = get_changed_files(&root_dir);
        let changed_count = changed.len();
        let git_status = if changed_count > 0 {
            format!(
                "{} ({} file(s) modified)",
                branch.bright_cyan(),
                changed_count.to_string().bright_yellow().bold()
            )
        } else {
            format!("{} (clean)", branch.bright_cyan())
        };
        println!(
            "{}  🌿 {} {}",
            "│".bright_cyan(),
            "Git Status :".bright_white().bold(),
            git_status
        );
    }

    // Clipboard Status Line
    let clip_text = Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.get_text().ok())
        .unwrap_or_default();
    let search_markers = crate::count_search_markers(&clip_text);

    if let Ok(PatchPayload::Blocks(clip_blocks)) = parse_patch_payload(&clip_text) {
        let targets: Vec<String> = clip_blocks.iter().map(|b| b.file_path.clone()).collect();
        let clip_msg = format!(
            "⚡ Edits Ready: {} block(s) for ({})",
            clip_blocks.len(),
            targets.join(", ")
        );
        println!("{}  {}", "│".bright_cyan(), clip_msg.bright_green().bold());
    } else if clip_text.trim() == "NO_CHANGES" {
        let clip_msg = "📋 Clipboard  : NO_CHANGES — nothing to apply";
        println!("{}  {}", "│".bright_cyan(), clip_msg.bright_green().bold());
    } else if search_markers > 0 {
        // Has SEARCH markers but failed to parse — malformed delimiters
        let clip_msg = format!(
            "📋 Clipboard  : {} patch marker(s) found but could not be parsed — check delimiters",
            search_markers
        );
        println!("{}  {}", "│".bright_cyan(), clip_msg.bright_yellow().bold());
    } else if clip_text.trim().is_empty() {
        let clip_msg = "📋 Clipboard  : Empty — copy the AI's complete response first";
        println!("{}  {}", "│".bright_cyan(), clip_msg.dimmed());
    } else {
        let clip_msg =
            "📋 Clipboard  : Text copied, but no patch found — copy the complete AI response";
        println!("{}  {}", "│".bright_cyan(), clip_msg.dimmed());
    }

    println!(
        "{}",
        "╰─────────────────────────────────────────────────────────────────────────────╯"
            .bright_cyan()
    );
    println!();
}

fn get_git_branch(root: &PathBuf) -> String {
    let output = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(root)
        .output();
    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

fn print_patch_plan(blocks: &[PatchBlock]) {
    println!("\n{}", "🧾 --- PATCH REVIEW ---".bright_cyan().bold());
    for summary in summarize_patch_blocks(blocks) {
        let action = if summary.creates_file {
            "CREATE"
        } else {
            "UPDATE"
        };
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

    let plans = match plan_blocks_transactional(&root_dir, blocks) {
        Ok(plans) => plans,
        Err((failures, msg)) => {
            println!(
                "\n{}",
                format!("❌ Patch validation failed: {}", msg)
                    .bright_red()
                    .bold()
            );
            for failure in failures {
                println!("   - {}", format!("{:?}", failure).bright_red());
            }
            return true;
        }
    };

    print_patch_plan(blocks);
    println!("\n{}", render_patch_plan(&plans));

    let apply_now = Confirm::new("Apply exactly this patch?")
        .with_default(false)
        .with_render_config(configure_visual_theme())
        .prompt()
        .unwrap_or(false);

    if !apply_now {
        println!("{}", "Patch was not applied.".bright_yellow());
        return false;
    }

    match apply_planned_transactional(&root_dir, &plans) {
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
            println!(
                "\n{}",
                format!("❌ Failed to apply patch: {}", msg)
                    .bright_red()
                    .bold()
            );
            for failure in failures {
                println!("   - {}", format!("{:?}", failure).bright_red());
            }
        }
    }

    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HubAction {
    Export,
    Apply,
    Undo,
    MoreTools,
    Exit,
}

const PRIMARY_MENU_ITEMS: [(HubAction, &str); 5] = [
    (
        HubAction::Export,
        "📦 START AI TASK     — Export code and a ready-to-paste prompt",
    ),
    (
        HubAction::Apply,
        "⚡ APPLY AI PATCH   — Copy the complete AI response first, then apply",
    ),
    (
        HubAction::Undo,
        "↺  UNDO LAST PATCH  — Restore the latest workspace snapshot",
    ),
    (
        HubAction::MoreTools,
        "🧰 MORE TOOLS       — Preview, audit, customize prompts, or update",
    ),
    (HubAction::Exit, "🚪 EXIT             — Close Centaur"),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MoreToolsAction {
    Preview,
    Audit,
    EditPrompt,
    Update,
}

const MORE_TOOLS_MENU_ITEMS: [(MoreToolsAction, &str); 4] = [
    (
        MoreToolsAction::Preview,
        "🔍 PREVIEW PATCH   — Validate clipboard edits without changing files",
    ),
    (
        MoreToolsAction::Audit,
        "🛡️ SECURITY AUDIT  — Scan the workspace for exposed credentials",
    ),
    (
        MoreToolsAction::EditPrompt,
        "📝 EDIT PROMPT     — Customize the prompt included with exports",
    ),
    (
        MoreToolsAction::Update,
        "🚀 UPDATE CENTAUR  — Install latest from the official repository",
    ),
];

fn run_more_tools_menu() {
    let options: Vec<&str> = MORE_TOOLS_MENU_ITEMS
        .iter()
        .map(|(_, label)| *label)
        .collect();
    let choice = Select::new("Choose a tool:", options)
        .with_render_config(configure_visual_theme())
        .with_page_size(MORE_TOOLS_MENU_ITEMS.len())
        .raw_prompt();

    let Ok(choice) = choice else {
        println!("{}", "No tool selected.".bright_yellow());
        return;
    };

    match MORE_TOOLS_MENU_ITEMS[choice.index].0 {
        MoreToolsAction::Preview => run_dry_run_interactive(),
        MoreToolsAction::Audit => {
            run_security_audit();
        }
        MoreToolsAction::EditPrompt => run_prompt_edit_interactive(),
        MoreToolsAction::Update => run_auto_update_interactive(),
    }
}

pub fn run_interactive_hub() {
    render_workspace_dashboard();
    let render_config = configure_visual_theme();

    // Surface clipboard prompt immediately if AI edits detected
    let clip_text = Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.get_text().ok())
        .unwrap_or_default();
    match parse_patch_payload(&clip_text) {
        Ok(PatchPayload::Blocks(clip_blocks)) => {
            println!(
                "   {}",
                format!(
                    "⚡ AI code edits detected: {} block(s). Review the validated plan below.",
                    clip_blocks.len()
                )
                .bright_green()
                .bold()
            );

            if review_and_apply_blocks(&clip_blocks) {
                return;
            }
        }
        Ok(PatchPayload::NoChanges) => println!(
            "   {}",
            "The AI reported NO_CHANGES; the workspace is untouched."
                .bright_green()
                .bold()
        ),
        Err(error) if crate::count_search_markers(&clip_text) > 0 => println!(
            "   {}",
            format!("Clipboard patch rejected: {}", error)
                .bright_red()
                .bold()
        ),
        Err(_) => {}
    }

    println!("{}", "  Workflow: Export → AI → Review → Apply".dimmed());
    println!("{}", "  What do you want to do?".bright_white().bold());

    let options: Vec<&str> = PRIMARY_MENU_ITEMS.iter().map(|(_, label)| *label).collect();
    let choice = Select::new("", options)
        .with_render_config(render_config)
        .with_page_size(PRIMARY_MENU_ITEMS.len())
        .raw_prompt();

    let action = choice
        .ok()
        .and_then(|choice| PRIMARY_MENU_ITEMS.get(choice.index))
        .map(|(action, _)| *action)
        .unwrap_or(HubAction::Exit);

    match action {
        HubAction::Export => run_export_wizard(),
        HubAction::Apply => run_apply_interactive(),
        HubAction::Undo => run_history_interactive(),
        HubAction::MoreTools => run_more_tools_menu(),
        HubAction::Exit => println!("{}", "\n👋 Goodbye from Centaur!".bright_cyan().bold()),
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
    println!(
        "\n{}",
        "🚀 --- CENTAUR AUTO-UPDATE ---".bright_cyan().bold()
    );
    println!(
        "{}",
        format!("📥 Installing from {}...", crate::update::UPDATE_REPOSITORY).bright_yellow()
    );

    match crate::update::install_latest() {
        Ok(()) => {
            println!(
                "{}",
                "✨ Centaur CLI update complete!".bright_green().bold()
            );
        }
        Err(error) => println!("{}", format!("❌ Update failed: {}", error).bright_red()),
    }
}

pub fn run_apply_interactive() {
    println!(
        "\n{}",
        "⚡ --- REVIEW & APPLY AI CODE EDITS ---"
            .bright_cyan()
            .bold()
    );
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
    match parse_patch_payload(&text) {
        Ok(PatchPayload::Blocks(blocks)) => {
            let _ = review_and_apply_blocks(&blocks);
        }
        Ok(PatchPayload::NoChanges) => println!(
            "{}",
            "The AI reported NO_CHANGES; the workspace is untouched."
                .bright_green()
                .bold()
        ),
        Err(error) => {
            println!("{}", format!("⚠️ {}", error).bright_yellow());
            println!(
                "{}",
                "Copy the AI's complete response and try again, or save it to a file and run: centaur --file <path>"
                    .dimmed()
            );
        }
    }
}

const EXPORT_TASK_GUIDANCE: &str = "Describe the desired outcome, important constraints, and how success should be verified. Leave blank for a conservative correctness and security review.";
const EXPORT_TASK_LABEL: &str = "What should the AI change or review?";
const EXPORT_TASK_HELP: &str = "Example: Simplify the export instructions, preserve existing CLI behavior, and add a focused regression test.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportScopeChoice {
    Changed,
    Full,
    Staged,
    Compact,
}

impl ExportScopeChoice {
    fn mode(self) -> ExportMode {
        match self {
            Self::Changed => ExportMode::Changed,
            Self::Full => ExportMode::Full,
            Self::Staged => ExportMode::Staged,
            Self::Compact => ExportMode::Compact,
        }
    }
}

const GIT_EXPORT_SCOPES: [(ExportScopeChoice, &str); 4] = [
    (
        ExportScopeChoice::Changed,
        "🌿 Changed Files Only (Recommended — modified and untracked Git files)",
    ),
    (
        ExportScopeChoice::Full,
        "📂 Entire Project (All eligible files in this workspace)",
    ),
    (
        ExportScopeChoice::Staged,
        "📌 Staged Files Only (Git staged files only)",
    ),
    (
        ExportScopeChoice::Compact,
        "⚡ Compact Project (Entire project without repetitive fixtures and data)",
    ),
];

const NON_GIT_EXPORT_SCOPES: [(ExportScopeChoice, &str); 2] = [
    (
        ExportScopeChoice::Full,
        "📂 Entire Project (Recommended — all eligible files in this folder)",
    ),
    (
        ExportScopeChoice::Compact,
        "⚡ Compact Project (Omit repetitive fixtures and data)",
    ),
];

fn export_scope_options(is_git: bool) -> &'static [(ExportScopeChoice, &'static str)] {
    if is_git {
        &GIT_EXPORT_SCOPES
    } else {
        &NON_GIT_EXPORT_SCOPES
    }
}

fn cancel_export() {
    println!(
        "{}",
        "Export cancelled. No files were created.".bright_yellow()
    );
}

pub fn run_export_wizard() {
    let render_config = configure_visual_theme();
    println!(
        "\n{}",
        "📦 --- SEND CODEBASE TO CHATGPT / CLAUDE ---"
            .bright_cyan()
            .bold()
    );
    println!(
        "{}",
        "Packages project context into attachments with automated workflow instructions.".dimmed()
    );
    println!();

    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let root_name = root_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();

    let scopes = export_scope_options(is_git_repo(&root_dir));
    let scope_labels: Vec<&str> = scopes.iter().map(|(_, label)| *label).collect();
    let scope_choice = Select::new(
        "Which files should be included in the export?",
        scope_labels,
    )
    .with_render_config(render_config)
    .with_page_size(scopes.len())
    .raw_prompt();

    let Ok(scope_choice) = scope_choice else {
        cancel_export();
        return;
    };
    let mode = scopes[scope_choice.index].0.mode();

    println!("{}", EXPORT_TASK_GUIDANCE.dimmed());
    let task_prompt = Text::new(EXPORT_TASK_LABEL)
        .with_help_message(EXPORT_TASK_HELP)
        .with_render_config(render_config)
        .prompt();

    let Ok(task_prompt) = task_prompt else {
        cancel_export();
        return;
    };

    let files_to_pack = export::collect_files(&root_dir, mode, &[]);

    // Security Scan
    let secret_warnings = export::scan_files(&files_to_pack);
    let mut auto_redact = false;

    if !secret_warnings.is_empty() {
        println!(
            "\n{}",
            "🛡️  SECURITY WARNING: Sensitive credentials detected!"
                .bright_red()
                .bold()
        );
        for w in &secret_warnings {
            println!(
                "   - {} in {}",
                w.pattern_name.bright_red(),
                w.file_path.bright_yellow()
            );
        }

        let sec_opts = vec![
            "🛡️  Automatically redact secrets in the export copy (Recommended)",
            "⚠️  Export as-is (include credentials in export)",
            "❌ Cancel export",
        ];
        let sec_choice = Select::new("How should secrets be handled?", sec_opts.clone())
            .with_render_config(render_config)
            .prompt();

        let Ok(sec_choice) = sec_choice else {
            cancel_export();
            return;
        };

        if sec_choice.contains("Cancel") {
            cancel_export();
            return;
        }
        if sec_choice.contains("redact") {
            auto_redact = true;
        }
    }

    let config = CentaurConfig::load();
    let session_id = format!(
        "{:08x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );

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
    let (result, export_dir, _prompt_copied) =
        (&outcome.result, &outcome.export_dir, outcome.prompt_copied);

    println!("\n{}", "✨ Export complete!".bright_green().bold());

    let files_label = if result.summary.total_files == 1 {
        "1 project file".to_string()
    } else {
        format!("{} project files", result.summary.total_files)
    };
    let upload_file_label = if result.summary.total_parts == 1 {
        "1 upload file".to_string()
    } else {
        format!("{} upload files", result.summary.total_parts)
    };
    println!(
        "   {}",
        format!("✓ {} packed into {}", files_label, upload_file_label).bright_green()
    );
    if outcome.prompt_copied {
        println!("   {}", "✓ Prompt copied to clipboard".bright_green());
    } else {
        println!(
            "   {}",
            format!(
                "! Prompt not copied — use {}",
                export::PROMPT_FALLBACK_FILENAME
            )
            .bright_yellow()
        );
    }
    println!(
        "   {}",
        format!(
            "{} characters · ~{} tokens",
            result.summary.total_chars, result.summary.estimated_tokens
        )
        .dimmed()
    );

    if let Some(warn) = &result.summary.token_warning {
        println!("\n{}", warn.bright_yellow());
    }

    if !outcome.write_errors.is_empty() {
        println!(
            "\n{}",
            "❌ Some export files could not be written:"
                .bright_red()
                .bold()
        );
        for e in &outcome.write_errors {
            println!("   - {}", e.bright_red());
        }
    }

    println!(
        "\n{}",
        "📋 --- NEXT: SEND TO THE AI ---".bright_cyan().bold()
    );
    if outcome.prompt_copied {
        println!("   1. Paste the prompt");
        println!("      Press Ctrl+V in ChatGPT or Claude.");
    } else {
        println!("   1. Copy the saved prompt");
        println!(
            "      Open {} and copy all its text.",
            export::PROMPT_FALLBACK_FILENAME
        );
    }

    if result.summary.total_batches == 1 {
        if result.summary.total_parts == 1 {
            println!("   2. Upload the selected file");
            println!("      Drag centaur_context_part001.txt into the same message.");
        } else {
            println!("   2. Upload the selected files");
            println!(
                "      Drag all {} centaur_context_part*.txt files into the same message.",
                result.summary.total_parts
            );
        }
        println!("   3. Send your message");

        println!("\n{}", "--- WHEN THE AI REPLIES ---".bright_cyan().bold());
        println!("   4. Copy the complete AI response");
        println!("      Include all Search/Replace code blocks in your copy.");
        println!("   5. Run centaur again");
        println!("      Review the proposed changes, then approve them.");
    } else {
        println!("   2. Upload batch_01/ files into the same message");
        for b in 2..=result.summary.total_batches {
            println!(
                "   {}. Upload batch_{:02}/ files after AI acknowledges batch {}",
                b + 1,
                b,
                b - 1
            );
        }
        println!("\n{}", "--- WHEN THE AI REPLIES ---".bright_cyan().bold());
        println!(
            "   {}. Copy the complete AI response",
            result.summary.total_batches + 2
        );
        println!("      Include all Search/Replace code blocks in your copy.");
        println!("   {}. Run centaur again", result.summary.total_batches + 3);
        println!("      Review the proposed changes, then approve them.");
    }

    if config.export.open_export_directory {
        println!(
            "\n{}",
            "✓ Explorer opened with the upload file selected".bright_green()
        );
        open_directory(export_dir);
    } else {
        println!(
            "\n   Export folder: {}",
            export_dir.display().to_string().bright_yellow()
        );
    }
}

pub fn open_directory(dir: &std::path::Path) {
    let primary_target = dir.join("centaur_context_part001.txt");
    let batch_target = dir.join("batch_01");

    let target_file = if primary_target.exists() {
        Some(primary_target)
    } else if batch_target.exists() {
        Some(batch_target)
    } else {
        None
    };

    if let Some(target) = target_file {
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("explorer")
                .arg(format!("/select,{}", target.display()))
                .spawn();
            return;
        }
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open")
                .arg("-R")
                .arg(&target)
                .spawn();
            return;
        }
    }

    #[cfg(target_os = "windows")]
    let _ = std::process::Command::new("explorer").arg(dir).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(dir).spawn();
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("xdg-open").arg(dir).spawn();
}

pub fn run_history_interactive() {
    let render_config = configure_visual_theme();
    println!(
        "\n{}",
        "↺  --- UNDO LAST AI CHANGES ---".bright_cyan().bold()
    );
    println!(
        "{}",
        "Reverts files to their exact state prior to the selected patch session.".dimmed()
    );
    println!();

    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let sessions = PatchSessionRecord::list_sessions_for(&root_dir);
    if sessions.is_empty() {
        println!(
            "{}",
            "⚠️ No previous patch sessions found for this workspace.".bright_yellow()
        );
        return;
    }

    let mut options = Vec::new();
    for s in &sessions {
        let files_str: Vec<String> = s.files.iter().map(|f| f.relative_path.clone()).collect();
        options.push(format!(
            "Session {} — {} file(s): {}",
            s.session_id.bright_yellow(),
            s.files.len(),
            files_str.join(", ")
        ));
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
    println!(
        "\n{}",
        "🔍 --- PREVIEW PATCH (DRY RUN) ---".bright_cyan().bold()
    );
    println!(
        "{}",
        "Simulates patch block matching without modifying any files on disk.".dimmed()
    );
    println!();

    let mut text = String::new();
    if let Ok(mut cb) = Clipboard::new() {
        if let Ok(clip_text) = cb.get_text() {
            text = clip_text;
        }
    }
    let blocks = match parse_patch_payload(&text) {
        Ok(PatchPayload::Blocks(blocks)) => blocks,
        Ok(PatchPayload::NoChanges) => {
            println!(
                "{}",
                "The AI reported NO_CHANGES; there is nothing to preview."
                    .bright_green()
                    .bold()
            );
            return;
        }
        Err(error) => {
            println!("{}", format!("⚠️ {}", error).bright_yellow());
            return;
        }
    };

    let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    match plan_blocks_transactional(&root_dir, &blocks) {
        Ok(plans) => {
            print_patch_plan(&blocks);
            println!("\n{}", render_patch_plan(&plans));
        }
        Err((_, msg)) => {
            println!(
                "{}",
                format!("❌ Match Preview Failed: {}", msg).bright_red()
            );
            return;
        }
    }
    match apply_blocks_transactional(&root_dir, &blocks, true) {
        Ok(results) => {
            println!("{}", "✨ Match Preview Successful:".bright_green().bold());
            for r in results {
                println!("   - {}", format!("{:?}", r).bright_green());
            }
        }
        Err((_, msg)) => println!(
            "{}",
            format!("❌ Match Preview Failed: {}", msg).bright_red()
        ),
    }
}

/// Returns the number of potential secrets found, so callers can set an exit code.
pub fn run_security_audit() -> usize {
    println!("\n{}", "🛡️  --- SECURITY AUDIT ---".bright_cyan().bold());
    println!(
        "{}",
        "Scans workspace files for leaked API keys, tokens, and passwords.".dimmed()
    );
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
        println!(
            "{}",
            "✨ Security Audit Passed: No credentials detected in workspace."
                .bright_green()
                .bold()
        );
    } else {
        println!(
            "{}",
            format!("⚠️ Found {} potential secret warning(s):", found.len())
                .bright_red()
                .bold()
        );
        for f in &found {
            println!(
                "   - {} in {}",
                f.pattern_name.bright_red(),
                f.file_path.bright_yellow()
            );
        }
    }
    found.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primary_menu_focuses_on_the_everyday_workflow() {
        let labels: Vec<&str> = PRIMARY_MENU_ITEMS.iter().map(|(_, label)| *label).collect();

        assert_eq!(labels.len(), 5);
        assert!(labels[0].contains("START AI TASK"));
        assert!(labels[1].contains("APPLY AI PATCH"));
        assert!(labels[2].contains("UNDO LAST PATCH"));
        assert!(labels[3].contains("MORE TOOLS"));
        assert!(labels.iter().all(|label| !label.contains("SECURITY AUDIT")));
    }

    #[test]
    fn advanced_actions_remain_available_under_more_tools() {
        let labels: Vec<&str> = MORE_TOOLS_MENU_ITEMS
            .iter()
            .map(|(_, label)| *label)
            .collect();

        assert_eq!(labels.len(), 4);
        for expected in [
            "PREVIEW PATCH",
            "SECURITY AUDIT",
            "EDIT PROMPT",
            "UPDATE CENTAUR",
        ] {
            assert!(
                labels.iter().any(|label| label.contains(expected)),
                "missing advanced action: {}",
                expected
            );
        }
    }

    #[test]
    fn export_task_prompt_encourages_actionable_requests() {
        assert!(EXPORT_TASK_GUIDANCE.contains("desired outcome"));
        assert!(EXPORT_TASK_GUIDANCE.contains("constraints"));
        assert!(EXPORT_TASK_GUIDANCE.contains("verified"));
        assert_eq!(EXPORT_TASK_LABEL, "What should the AI change or review?");
        assert!(EXPORT_TASK_HELP.starts_with("Example:"));
    }

    #[test]
    fn git_export_scope_menu_defaults_to_current_changes() {
        let scopes = export_scope_options(true);

        assert_eq!(scopes.len(), 4);
        assert_eq!(scopes[0].0, ExportScopeChoice::Changed);
        assert_eq!(scopes[0].0.mode(), ExportMode::Changed);
        assert!(
            scopes
                .iter()
                .any(|(scope, _)| *scope == ExportScopeChoice::Staged)
        );
    }

    #[test]
    fn non_git_export_scope_menu_hides_git_only_choices() {
        let scopes = export_scope_options(false);

        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0].0, ExportScopeChoice::Full);
        assert_eq!(scopes[0].0.mode(), ExportMode::Full);
        assert!(scopes.iter().all(|(scope, _)| {
            !matches!(
                *scope,
                ExportScopeChoice::Changed | ExportScopeChoice::Staged
            )
        }));
    }
}
