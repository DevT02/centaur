use arboard::Clipboard;
use clap::{Parser, Subcommand};
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use sysinfo::System;
use the_clipboard_centaur::config::CentaurConfig;
use the_clipboard_centaur::export;
use the_clipboard_centaur::git::{is_git_repo, ExportMode};
use the_clipboard_centaur::pack::PackOptions;
use the_clipboard_centaur::patch::{apply_blocks_transactional, ApplyResult};
use the_clipboard_centaur::prompt::{
    handle_prompt_copy, handle_prompt_edit, handle_prompt_reset, handle_prompt_show,
};
use the_clipboard_centaur::parse_blocks;

#[derive(Parser, Debug)]
#[command(
    name = "The Clipboard Centaur",
    version,
    about = "Applies LLM-generated search/replace blocks directly to local files.",
    long_about = "A fast, deterministic, and consumer-friendly CLI that parses search/replace blocks and exports context with session batching."
)]
struct Args {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Read the patch text directly from the OS clipboard.
    #[arg(short, long)]
    clipboard: bool,

    /// Read the patch text from a specific file.
    #[arg(short, long)]
    file: Option<String>,

    /// Fallback to a local LLM via Ollama if the deterministic patch fails. Use 'auto' for hardware recommendation.
    #[arg(short, long)]
    llm: Option<String>,

    /// Export files/directories into single string/batches and copy prompt to clipboard.
    #[arg(short, long, num_args = 0..)]
    export: Option<Vec<String>>,

    /// Context export mode: full, changed, staged, compact
    #[arg(long, value_enum, default_value = "full")]
    mode: ExportMode,

    /// Feature task description to automatically insert into the workflow prompt
    #[arg(long)]
    task: Option<String>,

    /// Maximum attachments generated for one upload message (default: 20)
    #[arg(long)]
    max_parts: Option<usize>,

    /// Preferred maximum attachment size in characters (default: 5000000)
    #[arg(long)]
    max_part_chars: Option<usize>,

    /// Model context token budget for warning alerts (default: 200000)
    #[arg(long)]
    context_tokens: Option<usize>,

    /// Preview patch changes without modifying files
    #[arg(long)]
    dry_run: bool,

    /// Run initial setup: environment verification & local model download
    #[arg(long)]
    setup: bool,

    /// Bypass safety size limits for exports
    #[arg(long)]
    force: bool,

    /// Strip detected credentials from the exported copy (the workspace is untouched)
    #[arg(long)]
    redact: bool,

    /// Deprecated alias for --max-part-chars
    #[arg(long)]
    chunk_size: Option<usize>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Manage the Centaur workflow prompt template
    Prompt {
        #[command(subcommand)]
        action: PromptAction,
    },
    /// Manage the Centaur config file
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Revert previous patch session
    Undo {
        /// Session ID to revert (default: latest)
        #[arg(default_value = "latest")]
        session_id: String,
    },
    /// View patch history timeline
    History,
    /// Launch the interactive terminal workspace hub
    Ui,
    /// Audit workspace for leaked credentials & API keys
    Audit,
    /// Auto-update Centaur binary to the latest version on PATH
    Update,
    /// Serve apply/undo tools to an MCP client over stdio
    Mcp {
        /// Workspace the client may patch. Fixed here so the model cannot choose it.
        #[arg(long)]
        workspace: PathBuf,
    },
}

#[derive(Subcommand, Debug)]
enum PromptAction {
    /// Display the current prompt templates
    Show,
    /// Copy the prompt template to clipboard
    Copy,
    /// Open a prompt template in your default editor ($VISUAL/$EDITOR, else Notepad/nano)
    Edit {
        /// Edit the single-upload template instead of the multi-batch one
        #[arg(long)]
        single: bool,
    },
    /// Reset both prompt templates to their defaults
    Reset,
}

#[derive(Subcommand, Debug)]
enum ConfigAction {
    /// Write a config file containing the current defaults
    Init,
    /// Print the config file location
    Path,
}

fn resolve_auto_llm() -> String {
    let mut sys = System::new_all();
    sys.refresh_all();
    let total_ram_gb = sys.total_memory() as f64 / 1_073_741_824.0;
    let available_ram_gb = sys.available_memory() as f64 / 1_073_741_824.0;

    println!("🔍 Analyzing system specs: {:.1} GB Total RAM ({:.1} GB Available right now)", total_ram_gb, available_ram_gb);

    let (model, size, reason) = if available_ram_gb >= 20.0 {
        ("deepseek-coder:33b", "~19GB", "Massive available RAM. DeepSeek provides near GPT-4 level coding capabilities.")
    } else if available_ram_gb >= 6.0 {
        ("qwen2.5-coder:7b", "~4.5GB", "Great balance of intelligence and performance for your current available memory.")
    } else {
        ("qwen2.5-coder:1.5b", "~1GB", "Low available memory detected. Ultra-lightweight model chosen to avoid swapping.")
    };

    println!("💡 Auto-Recommendation: Using '{}' (Download: {}) - {}", model, size, reason);
    model.to_string()
}

fn apply_with_llm(model: &str, file_path_str: &str, search: &str, replace: &str) -> bool {
    let file_path = Path::new(file_path_str);
    let content = fs::read_to_string(file_path).unwrap_or_default();

    let prompt = format!(
        "You are a local file-writing agent. Execute the diff and output the final complete updated file.\n\nThe Diffs:\n<<<<<<< SEARCH\n{}\n=======\n{}\n>>>>>>> REPLACE\n\nCurrent File Content:\n{}\n\nOutput the completely updated file now:",
        search, replace, content
    );

    println!("🤖 Asking Ollama ({}) to resolve patch for {}...", model, file_path_str);

    let mut child = match Command::new("ollama")
        .arg("run")
        .arg(model)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            println!("❌ ERROR: Failed to execute Ollama ({})", e);
            return false;
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(prompt.as_bytes()) {
            println!("❌ ERROR writing to Ollama stdin: {}", e);
            return false;
        }
    }

    match child.wait_with_output() {
        Ok(out) => {
            if out.status.success() {
                let mut result_text = String::from_utf8_lossy(&out.stdout).to_string();
                if result_text.starts_with("```") {
                    let mut lines: Vec<&str> = result_text.lines().collect();
                    if !lines.is_empty() && lines[0].starts_with("```") {
                        lines.remove(0);
                    }
                    if !lines.is_empty() && lines.last().map(|l| l.starts_with("```")).unwrap_or(false) {
                        lines.pop();
                    }
                    result_text = lines.join("\n");
                }
                if let Err(e) = fs::write(file_path, result_text) {
                    println!("❌ ERROR writing LLM output to {}: {}", file_path_str, e);
                    false
                } else {
                    println!("✅ Successfully updated via Local LLM: {}", file_path_str);
                    true
                }
            } else {
                println!("❌ ERROR: Ollama failed: {}", String::from_utf8_lossy(&out.stderr));
                false
            }
        }
        Err(e) => {
            println!("❌ ERROR reading Ollama output: {}", e);
            false
        }
    }
}

fn handle_auto_update() -> ExitCode {
    println!("🔄 Checking for Centaur updates...");
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    if is_git_repo(&current_dir) {
        println!("📥 Pulling latest source code from git repository...");
        let git_pull = Command::new("git")
            .arg("pull")
            .current_dir(&current_dir)
            .status();

        if let Ok(status) = git_pull {
            if status.success() {
                println!("✅ Git repository updated successfully.");
            }
        }
    }

    println!("🔨 Re-building and installing Centaur binary to PATH...");
    let install_status = Command::new("cargo")
        .args(["install", "--path", ".", "--force"])
        .current_dir(&current_dir)
        .status();

    match install_status {
        Ok(status) if status.success() => {
            println!("✅ Centaur successfully updated to the latest version on PATH!");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("❌ Auto-update failed during binary installation. Try running 'cargo install --path . --force' manually.");
            ExitCode::FAILURE
        }
    }
}

fn generate_session_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:08x}", (nanos & 0xffff_ffff))
}

fn main() -> ExitCode {
    let args = Args::parse();
    let config = CentaurConfig::load();
    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    if let Some(cmd) = args.command {
        return match cmd {
            Commands::Prompt { action } => {
                let outcome = match action {
                    PromptAction::Show => {
                        handle_prompt_show();
                        Ok(())
                    }
                    PromptAction::Copy => handle_prompt_copy(),
                    PromptAction::Edit { single } => handle_prompt_edit(single),
                    PromptAction::Reset => handle_prompt_reset(),
                };
                match outcome {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("❌ {}", e);
                        ExitCode::FAILURE
                    }
                }
            }
            Commands::Config { action } => match action {
                ConfigAction::Path => {
                    println!("{}", CentaurConfig::config_path().display());
                    ExitCode::SUCCESS
                }
                ConfigAction::Init => {
                    let path = CentaurConfig::config_path();
                    if path.exists() {
                        eprintln!("❌ Config already exists: {}", path.display());
                        ExitCode::FAILURE
                    } else {
                        match config.save() {
                            Ok(()) => {
                                println!("✅ Wrote config: {}", path.display());
                                ExitCode::SUCCESS
                            }
                            Err(e) => {
                                eprintln!("❌ Could not write config: {}", e);
                                ExitCode::FAILURE
                            }
                        }
                    }
                }
            },
            Commands::Undo { session_id } => {
                match the_clipboard_centaur::history::PatchSessionRecord::revert_session(&current_dir, &session_id) {
                    Ok(restored) => {
                        println!("✅ Successfully reverted patch session:");
                        for r in restored {
                            println!("  - {}", r);
                        }
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("❌ Revert failed: {}", e);
                        ExitCode::FAILURE
                    }
                }
            }
            Commands::History => {
                the_clipboard_centaur::ui::run_history_interactive();
                ExitCode::SUCCESS
            }
            Commands::Ui => {
                the_clipboard_centaur::ui::run_interactive_hub();
                ExitCode::SUCCESS
            }
            Commands::Audit => {
                // Leaked credentials are a finding, not a crash — but scripts and CI
                // need a non-zero code to gate on.
                if the_clipboard_centaur::ui::run_security_audit() > 0 {
                    ExitCode::FAILURE
                } else {
                    ExitCode::SUCCESS
                }
            }
            Commands::Update => handle_auto_update(),
            Commands::Mcp { workspace } => match the_clipboard_centaur::mcp::serve(&workspace) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("❌ MCP server failed: {}", e);
                    ExitCode::FAILURE
                }
            },
        };
    }

    if args.export.is_none() && !args.clipboard && args.file.is_none() && !args.setup && !args.dry_run {
        // Zero-flag CLI invocation launch Interactive TUI Workspace Hub
        the_clipboard_centaur::ui::run_interactive_hub();
        return ExitCode::SUCCESS;
    }

    if args.setup {
        println!("\n🔧 Running The Clipboard Centaur Setup...\n");
        println!("Checking for Ollama installation...");
        let ollama_check = Command::new("ollama").arg("--version").output();
        match ollama_check {
            Ok(out) if out.status.success() => println!("✅ Ollama is installed: {}", String::from_utf8_lossy(&out.stdout).trim()),
            _ => {
                eprintln!("❌ Ollama is NOT installed or not found in PATH.");
                return ExitCode::FAILURE;
            }
        }
        let model = resolve_auto_llm();
        println!("\n📥 Pre-downloading model '{}'...", model);
        let pulled = Command::new("ollama")
            .arg("pull")
            .arg(&model)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if pulled {
            println!("\n✅ Setup complete! Model cached.");
            return ExitCode::SUCCESS;
        }
        eprintln!("\n❌ Failed to download model '{}'.", model);
        return ExitCode::FAILURE;
    }

    if let Some(mut paths) = args.export {
        if paths.is_empty() {
            paths.push(".".to_string());
        }

        let mode = args.mode;
        let root_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let root_name = root_dir.file_name().unwrap_or_default().to_string_lossy().to_string();

        let files_to_pack = export::collect_files(&root_dir, mode, &paths);
        let secret_warnings = export::scan_files(&files_to_pack);

        if !secret_warnings.is_empty() {
            println!("\n⚠️  SECURITY WARNING: Sensitive data detected in export target!");
            for w in &secret_warnings {
                println!("  - {}: {}", w.file_path, w.pattern_name);
            }
            if args.redact {
                println!("These will be stripped from the exported copy (--redact).\n");
            } else {
                println!("Re-run with --redact to strip them, or review before uploading.\n");
            }
        }

        let max_parts = args.max_parts.unwrap_or(config.export.max_attachments_per_message);
        // The current flag wins; --chunk-size is only a fallback for old scripts.
        let max_part_chars = args
            .max_part_chars
            .or(args.chunk_size)
            .unwrap_or(config.export.max_attachment_chars);
        let context_tokens = args.context_tokens.unwrap_or(config.export.context_token_budget);
        let session_id = generate_session_id();

        let request = export::ExportRequest {
            root: root_dir.clone(),
            task: args.task.unwrap_or_default(),
            redact: args.redact,
            copy_prompt: config.export.copy_prompt
                && (config.export.prompt_mode == "first" || config.export.prompt_mode == "every-batch"),
            pack: PackOptions {
                max_attachment_chars: max_part_chars,
                max_attachments_per_message: max_parts,
                context_token_budget: context_tokens,
                is_compact_mode: mode == ExportMode::Compact,
                force: args.force,
                project_root_name: root_name.clone(),
                session_id: session_id.clone(),
            },
        };

        let outcome = match export::run(&request, files_to_pack) {
            Ok(outcome) => outcome,
            Err(e) => {
                eprintln!("❌ Export failed: {}", e);
                return ExitCode::FAILURE;
            }
        };
        let result = &outcome.result;
        let export_dir = &outcome.export_dir;

        println!("✅ Export prepared\n");
        println!("Files:                {:>8}", result.summary.total_files);
        println!("Characters:           {:>8}", result.summary.total_chars);
        println!("Estimated tokens:     {:>8}", format!("~{}", result.summary.estimated_tokens));
        println!("Attachments:          {:>8}", result.summary.total_parts);
        println!("Upload messages:      {:>8}", result.summary.total_batches);

        if let Some(warn) = &result.summary.token_warning {
            println!("\n{}", warn);
        }

        if !result.summary.skipped_files.is_empty() {
            println!("\nSkipped files:");
            for sf in &result.summary.skipped_files {
                println!("  - {}: {}", sf.path, sf.reason);
            }
        }

        if !outcome.write_errors.is_empty() {
            eprintln!("\n❌ Some export files could not be written:");
            for e in &outcome.write_errors {
                eprintln!("  - {}", e);
            }
            return ExitCode::FAILURE;
        }

        if outcome.prompt_copied {
            println!("\n✅ Workflow prompt copied to clipboard.");
        }

        if result.summary.total_batches == 1 {
            println!("\n--- HOW TO USE ---");
            println!("1. Open the export folder: {}", export_dir.display());
            println!("2. Copy the text in COPY_THIS_PROMPT.txt and paste it into ChatGPT or Claude as your first message.");
            println!("3. Attach all centaur_context_part*.txt files to the same message.");
            println!("4. Send the message and wait for the AI to reply with code edits.");
            println!("5. Run 'centaur' again — it will detect the AI reply in your clipboard and apply it.");

            if config.export.open_export_directory {
                the_clipboard_centaur::ui::open_directory(export_dir);
            }
        } else {
            println!("\n--- HOW TO USE ({} upload messages required) ---", result.summary.total_batches);
            println!("1. Copy COPY_THIS_PROMPT.txt and paste it into ChatGPT/Claude as your first message.");
            println!("2. Attach files from batch_01/ to the same message and send.");
            for b in 2..=result.summary.total_batches {
                if b == result.summary.total_batches {
                    println!("{}. Attach files from batch_{:02}/ — this is the FINAL batch. The AI will begin once received.", b + 1, b);
                } else {
                    println!("{}. After the AI acknowledges batch {}, attach files from batch_{:02}/ and send.", b + 1, b - 1, b);
                }
            }
            if config.export.open_export_directory {
                the_clipboard_centaur::ui::open_directory(export_dir);
            }
        }
        return ExitCode::SUCCESS;
    }

    // Default patch application flow
    let input = if args.clipboard {
        println!("Reading patch instructions from the clipboard...");
        match Clipboard::new() {
            Ok(mut cb) => cb.get_text().unwrap_or_default(),
            Err(e) => {
                eprintln!("Failed to initialize clipboard: {}", e);
                return ExitCode::FAILURE;
            }
        }
    } else if let Some(path) = args.file {
        println!("Reading patch instructions from {}...", path);
        match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) => {
                // Previously unwrap_or_default(), which turned an unreadable file
                // into "no blocks found" and a success exit.
                eprintln!("❌ Could not read {}: {}", path, e);
                return ExitCode::FAILURE;
            }
        }
    } else {
        println!("Paste the output from ChatGPT below, then press Ctrl+D (Unix) or Ctrl+Z then Enter (Windows) to execute:\n");
        let mut text = String::new();
        let _ = io::stdin().read_to_string(&mut text);
        text
    };

    let blocks = parse_blocks(&input);
    if blocks.is_empty() {
        eprintln!("No valid Search/Replace blocks found in the input.");
        return ExitCode::FAILURE;
    }

    let markers = the_clipboard_centaur::count_search_markers(&input);
    if markers > blocks.len() {
        eprintln!(
            "⚠️  {} of {} Search/Replace blocks could not be parsed and will be skipped.",
            markers - blocks.len(),
            markers
        );
        eprintln!("   Check the delimiter lines (<<<<<<< SEARCH / ======= / >>>>>>> REPLACE).");
    }

    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    if args.dry_run {
        println!("🔍 Running in DRY-RUN mode. Simulating patch application...\n");
    }

    let tx_res = apply_blocks_transactional(&current_dir, &blocks, args.dry_run);
    match tx_res {
        Ok(results) => {
            let mut io_failures = 0;
            for res in results {
                match res {
                    ApplyResult::Created(path) => println!("✅ Created new file: {}", path),
                    ApplyResult::Updated(path) => println!("✅ Successfully updated: {}", path),
                    ApplyResult::DryRunSimulated(path) => println!("🔍 [DRY-RUN] Valid patch match for file: {}", path),
                    ApplyResult::IoError(path, err) => {
                        io_failures += 1;
                        eprintln!("❌ Could not write {}: {}", path, err);
                    }
                    _ => {}
                }
            }
            if io_failures > 0 {
                eprintln!("Run 'centaur undo latest' to revert any successful writes.");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        Err((failures, msg)) => {
            eprintln!("❌ ERROR: {}", msg);
            let resolved_llm = args.llm.map(|m| if m.eq_ignore_ascii_case("auto") { resolve_auto_llm() } else { m });
            let mut recovered_all = resolved_llm.is_some();

            for fail in failures {
                match fail {
                    ApplyResult::AmbiguousMatch(path) | ApplyResult::MatchNotFound(path) => {
                        eprintln!("  - Could not match cleanly in {}", path);
                        match (&resolved_llm, blocks.iter().find(|b| b.file_path == path)) {
                            (Some(model), Some(b)) => {
                                recovered_all &= apply_with_llm(model, &path, &b.search, &b.replace);
                            }
                            _ => recovered_all = false,
                        }
                    }
                    ApplyResult::SecurityError(err) => {
                        eprintln!("  - Security violation: {}", err);
                        recovered_all = false;
                    }
                    ApplyResult::IoError(path, err) => {
                        eprintln!("  - IO Error on {}: {}", path, err);
                        recovered_all = false;
                    }
                    _ => {}
                }
            }

            if recovered_all {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
    }
}
