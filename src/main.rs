use arboard::Clipboard;
use clap::{Parser, Subcommand};
use inquire::Confirm;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use sysinfo::System;
use the_clipboard_centaur::config::CentaurConfig;
use the_clipboard_centaur::export;
use the_clipboard_centaur::git::ExportMode;
use the_clipboard_centaur::pack::PackOptions;
use the_clipboard_centaur::patch::{
    ApplyResult, apply_blocks_transactional, apply_planned_transactional, plan_blocks_transactional,
};
use the_clipboard_centaur::prompt::{
    handle_prompt_copy, handle_prompt_edit, handle_prompt_reset, handle_prompt_show,
};
use the_clipboard_centaur::{
    PatchPayload, parse_patch_payload, render_patch_plan, summarize_patch_blocks,
};

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
    #[arg(short, long, conflicts_with_all = ["file", "stdin"])]
    clipboard: bool,

    /// Read the patch text from a specific file.
    #[arg(short, long, conflicts_with_all = ["clipboard", "stdin"])]
    file: Option<String>,

    /// Read the patch payload from standard input.
    #[arg(long, conflicts_with_all = ["clipboard", "file"])]
    stdin: bool,

    /// Apply a validated patch without an interactive confirmation.
    #[arg(long)]
    yes: bool,

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
    /// Connect Centaur to a GUI client that speaks MCP
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Install the /centaur slash command in a GUI client
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Unified 1-step setup: Install MCP servers and slash commands across GUI clients
    Install {
        /// Client id (default: all). Pass 'all' or specific client like antigravity, claude, cursor
        #[arg(long, default_value = "all")]
        client: String,
        /// Project workspace directory (default: current directory)
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Name for the entry and slash command (default: centaur)
        #[arg(long, default_value = "centaur")]
        name: String,
    },
    /// System diagnostics, environment health checks, and MCP client status
    Doctor,
}

#[derive(Subcommand, Debug)]
enum SkillAction {
    /// Write the command file for a client. Omit --client to list clients.
    Install {
        /// Client id, for example antigravity, claude-code, cursor
        #[arg(long)]
        client: Option<String>,
        /// Markdown file to write instead, for any client not listed
        #[arg(long)]
        output: Option<PathBuf>,
        /// Project the command applies to, for clients that store it per project
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Name of the command, which is what you type after the slash
        #[arg(long, default_value = "centaur")]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
enum McpAction {
    /// Add Centaur to a client's MCP configuration. Omit --client to list clients.
    Install {
        /// Client id, for example claude-desktop, antigravity, cursor
        #[arg(long)]
        client: Option<String>,
        /// Configuration file to edit instead, for any client not listed
        #[arg(long)]
        config: Option<PathBuf>,
        /// Project the client may patch (default: current directory)
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Name for the entry in the client's configuration
        #[arg(long, default_value = "centaur")]
        name: String,
    },
    /// Run the MCP server over stdio, or Streamable HTTP with --http.
    Serve {
        /// Workspace the client may patch. Fixed here so the model cannot choose it.
        #[arg(long)]
        workspace: PathBuf,
        /// Serve remote MCP on this loopback address, for example 127.0.0.1:3765.
        #[arg(long)]
        http: Option<SocketAddr>,
        /// Allow an exact Origin header in HTTP mode. Repeat for multiple origins.
        #[arg(long, requires = "http")]
        allow_origin: Vec<String>,
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

    println!(
        "🔍 Analyzing system specs: {:.1} GB Total RAM ({:.1} GB Available right now)",
        total_ram_gb, available_ram_gb
    );

    let (model, size, reason) = if available_ram_gb >= 20.0 {
        (
            "deepseek-coder:33b",
            "~19GB",
            "Massive available RAM. DeepSeek provides near GPT-4 level coding capabilities.",
        )
    } else if available_ram_gb >= 6.0 {
        (
            "qwen2.5-coder:7b",
            "~4.5GB",
            "Great balance of intelligence and performance for your current available memory.",
        )
    } else {
        (
            "qwen2.5-coder:1.5b",
            "~1GB",
            "Low available memory detected. Ultra-lightweight model chosen to avoid swapping.",
        )
    };

    println!(
        "💡 Auto-Recommendation: Using '{}' (Download: {}) - {}",
        model, size, reason
    );
    model.to_string()
}

fn repair_with_llm(
    model: &str,
    file_path_str: &str,
    blocks: &[&the_clipboard_centaur::PatchBlock],
) -> Result<String, String> {
    let file_path = Path::new(file_path_str);
    let content = fs::read_to_string(file_path)
        .map_err(|e| format!("Could not read {} for local repair: {}", file_path_str, e))?;
    let diffs = blocks
        .iter()
        .map(|block| {
            format!(
                "<<<<<<< SEARCH\n{}\n=======\n{}\n>>>>>>> REPLACE",
                block.search, block.replace
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");

    let prompt = format!(
        "Apply every supplied diff to the current file in memory. Output only the final complete file, with no Markdown fence or explanation.\n\nDiffs:\n{}\n\nCurrent File Content:\n{}\n\nFinal file:",
        diffs, content
    );

    println!(
        "🤖 Asking Ollama ({}) to resolve patch for {}...",
        model, file_path_str
    );

    let mut child = match Command::new("ollama")
        .arg("run")
        .arg(model)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return Err(format!("Failed to execute Ollama: {}", e));
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        if let Err(e) = stdin.write_all(prompt.as_bytes()) {
            return Err(format!("Could not write to Ollama: {}", e));
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
                    if !lines.is_empty()
                        && lines.last().map(|l| l.starts_with("```")).unwrap_or(false)
                    {
                        lines.pop();
                    }
                    result_text = lines.join("\n");
                }
                Ok(result_text)
            } else {
                Err(format!(
                    "Ollama failed: {}",
                    String::from_utf8_lossy(&out.stderr)
                ))
            }
        }
        Err(e) => Err(format!("Could not read Ollama output: {}", e)),
    }
}

fn handle_auto_update() -> ExitCode {
    println!(
        "🔄 Installing the latest Centaur from {}...",
        the_clipboard_centaur::update::UPDATE_REPOSITORY
    );
    match the_clipboard_centaur::update::install_latest() {
        Ok(()) => {
            println!("✅ Centaur successfully updated on PATH.");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("❌ Auto-update failed: {}", error);
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

fn print_patch_plan(blocks: &[the_clipboard_centaur::PatchBlock]) {
    println!("\nValidated patch plan:");
    for summary in summarize_patch_blocks(blocks) {
        let action = if summary.creates_file {
            "create"
        } else {
            "update"
        };
        println!(
            "  - {} {} ({} block(s), -{} +{} lines)",
            action,
            summary.file_path,
            summary.block_count,
            summary.removed_lines,
            summary.added_lines
        );
    }
}

fn report_apply_results(results: Vec<ApplyResult>) -> ExitCode {
    let mut io_failures = 0;
    for result in results {
        match result {
            ApplyResult::Created(path) => println!("✅ Created new file: {}", path),
            ApplyResult::Updated(path) => println!("✅ Successfully updated: {}", path),
            ApplyResult::DryRunSimulated(path) => {
                println!("🔍 [DRY-RUN] Valid patch match for file: {}", path)
            }
            ApplyResult::IoError(path, error) => {
                io_failures += 1;
                eprintln!("❌ Could not write {}: {}", path, error);
            }
            _ => {}
        }
    }
    if io_failures > 0 {
        eprintln!("Run 'centaur undo latest' to revert any successful writes.");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
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
                match the_clipboard_centaur::history::PatchSessionRecord::revert_session(
                    &current_dir,
                    &session_id,
                ) {
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
            Commands::Mcp { action } => {
                let outcome = match action {
                    McpAction::Install {
                        client,
                        config,
                        workspace,
                        name,
                    } => {
                        let workspace = workspace.unwrap_or_else(|| current_dir.clone());
                        the_clipboard_centaur::mcp::install(
                            client.as_deref(),
                            config.as_deref(),
                            &workspace,
                            &name,
                        )
                        .map(|report| println!("{}", report))
                    }
                    McpAction::Serve {
                        workspace,
                        http,
                        allow_origin,
                    } => match http {
                        Some(bind) => env::var("CENTAUR_MCP_TOKEN")
                            .map_err(|_| {
                                "HTTP mode requires CENTAUR_MCP_TOKEN with at least 32 characters."
                                    .to_string()
                            })
                            .and_then(|token| {
                                the_clipboard_centaur::mcp::serve_http(
                                    &workspace,
                                    bind,
                                    &token,
                                    &allow_origin,
                                )
                            }),
                        None => the_clipboard_centaur::mcp::serve(&workspace),
                    },
                };
                match outcome {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("❌ {}", e);
                        ExitCode::FAILURE
                    }
                }
            }
            Commands::Skill { action } => {
                let SkillAction::Install {
                    client,
                    output,
                    workspace,
                    name,
                } = action;
                let workspace = workspace.unwrap_or_else(|| current_dir.clone());
                match the_clipboard_centaur::skill::install(
                    client.as_deref(),
                    output.as_deref(),
                    &workspace,
                    &name,
                ) {
                    Ok(report) => {
                        println!("{}", report);
                        ExitCode::SUCCESS
                    }
                    Err(e) => {
                        eprintln!("❌ {}", e);
                        ExitCode::FAILURE
                    }
                }
            }
            Commands::Install {
                client,
                workspace,
                name,
            } => {
                let workspace = workspace.unwrap_or_else(|| current_dir.clone());
                println!(
                    "Setting up Centaur for workspace {}...\n",
                    workspace.display()
                );

                let mcp_res =
                    the_clipboard_centaur::mcp::install(Some(&client), None, &workspace, &name);
                let skill_res =
                    the_clipboard_centaur::skill::install(Some(&client), None, &workspace, &name);

                match (mcp_res, skill_res) {
                    (Ok(mcp_rep), Ok(skill_rep)) => {
                        println!("--- MCP Configurations ---\n{}\n", mcp_rep);
                        println!("--- Slash Commands & Skills ---\n{}\n", skill_rep);
                        println!("Centaur setup complete. Restart your AI client(s) to activate.");
                        ExitCode::SUCCESS
                    }
                    (Err(e), _) | (_, Err(e)) => {
                        eprintln!("❌ Setup failed: {}", e);
                        ExitCode::FAILURE
                    }
                }
            }
            Commands::Doctor => {
                let report = the_clipboard_centaur::doctor::diagnose(&current_dir);
                print!("{}", report.render_human());
                if report.summary_ok {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::FAILURE
                }
            }
        };
    }

    if args.export.is_none()
        && !args.clipboard
        && args.file.is_none()
        && !args.stdin
        && !args.setup
        && !args.dry_run
        && io::stdin().is_terminal()
    {
        // Zero-flag CLI invocation launch Interactive TUI Workspace Hub
        the_clipboard_centaur::ui::run_interactive_hub();
        return ExitCode::SUCCESS;
    }

    if args.setup {
        println!("\n🔧 Running The Clipboard Centaur Setup...\n");
        println!("Checking for Ollama installation...");
        let ollama_check = Command::new("ollama").arg("--version").output();
        match ollama_check {
            Ok(out) if out.status.success() => println!(
                "✅ Ollama is installed: {}",
                String::from_utf8_lossy(&out.stdout).trim()
            ),
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
        let root_name = root_dir
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

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

        let max_parts = args
            .max_parts
            .unwrap_or(config.export.max_attachments_per_message);
        // The current flag wins; --chunk-size is only a fallback for old scripts.
        let max_part_chars = args
            .max_part_chars
            .or(args.chunk_size)
            .unwrap_or(config.export.max_attachment_chars);
        let context_tokens = args
            .context_tokens
            .unwrap_or(config.export.context_token_budget);
        let session_id = generate_session_id();

        let request = export::ExportRequest {
            root: root_dir.clone(),
            task: args.task.unwrap_or_default(),
            redact: args.redact,
            copy_prompt: config.export.copy_prompt
                && (config.export.prompt_mode == "first"
                    || config.export.prompt_mode == "every-batch"),
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
        println!(
            "Estimated tokens:     {:>8}",
            format!("~{}", result.summary.estimated_tokens)
        );
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
        } else {
            println!(
                "\n⚠️ Workflow prompt saved to {}.",
                export_dir.join(export::PROMPT_FALLBACK_FILENAME).display()
            );
        }

        if result.summary.total_batches == 1 {
            println!("\n--- NEXT: SEND TO THE AI ---");
            if outcome.prompt_copied {
                println!("1. Paste the copied prompt into ChatGPT or Claude.");
            } else {
                println!(
                    "1. Open {} and copy its text into ChatGPT or Claude.",
                    export::PROMPT_FALLBACK_FILENAME
                );
            }
            println!("2. Attach all centaur_context_part*.txt files to the same message.");
            println!("3. Send the message.");

            println!("\n--- WHEN THE AI REPLIES ---");
            println!("4. Copy the complete AI response (the Centaur patch payload).");
            println!("5. Run 'centaur' again, then review and approve the proposed changes.");

            if config.export.open_export_directory {
                the_clipboard_centaur::ui::open_directory(export_dir);
            }
        } else {
            println!(
                "\n--- NEXT: SEND TO THE AI ({} upload messages) ---",
                result.summary.total_batches
            );
            if outcome.prompt_copied {
                println!("1. Paste the copied prompt into ChatGPT or Claude.");
            } else {
                println!(
                    "1. Open {} and copy its text into ChatGPT or Claude.",
                    export::PROMPT_FALLBACK_FILENAME
                );
            }
            println!("2. Attach files from batch_01/ to the same message and send.");
            for b in 2..=result.summary.total_batches {
                if b == result.summary.total_batches {
                    println!(
                        "{}. Attach files from batch_{:02}/ — this is the final batch.",
                        b + 1,
                        b
                    );
                } else {
                    println!(
                        "{}. After the AI acknowledges batch {}, attach files from batch_{:02}/.",
                        b + 1,
                        b - 1,
                        b
                    );
                }
            }

            println!("\n--- WHEN THE AI REPLIES ---");
            println!(
                "{}. Copy the complete AI response (the Centaur patch payload).",
                result.summary.total_batches + 2
            );
            println!(
                "{}. Run 'centaur' again, then review and approve the proposed changes.",
                result.summary.total_batches + 3
            );

            if config.export.open_export_directory {
                the_clipboard_centaur::ui::open_directory(export_dir);
            }
        }
        return ExitCode::SUCCESS;
    }

    // Default patch application flow
    let input_from_stdin = args.stdin || (!args.clipboard && args.file.is_none());
    let input = if args.clipboard {
        println!("Reading patch instructions from the clipboard...");
        match Clipboard::new() {
            Ok(mut cb) => cb.get_text().unwrap_or_default(),
            Err(e) => {
                eprintln!("Failed to initialize clipboard: {}", e);
                return ExitCode::FAILURE;
            }
        }
    } else if let Some(path) = args.file.as_deref() {
        println!("Reading patch instructions from {}...", path);
        match fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                // Previously unwrap_or_default(), which turned an unreadable file
                // into "no blocks found" and a success exit.
                eprintln!("❌ Could not read {}: {}", path, e);
                return ExitCode::FAILURE;
            }
        }
    } else {
        if io::stdin().is_terminal() {
            println!(
                "Paste the AI patch payload below, then press Ctrl+D (Unix) or Ctrl+Z then Enter (Windows) to validate:\n"
            );
        }
        let mut text = String::new();
        if let Err(e) = io::stdin().read_to_string(&mut text) {
            eprintln!("Could not read patch payload from standard input: {}", e);
            return ExitCode::FAILURE;
        }
        text
    };

    let blocks = match parse_patch_payload(&input) {
        Ok(PatchPayload::NoChanges) => {
            println!("No changes requested; the workspace was left untouched.");
            return ExitCode::SUCCESS;
        }
        Ok(PatchPayload::Blocks(blocks)) => blocks,
        Err(error) => {
            eprintln!("{}", error);
            return ExitCode::FAILURE;
        }
    };

    let current_dir = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

    let tx_res = match plan_blocks_transactional(&current_dir, &blocks) {
        Ok(plans) if args.dry_run => {
            println!("🔍 Running in DRY-RUN mode. No files will be written.\n");
            print_patch_plan(&blocks);
            println!("\n{}", render_patch_plan(&plans));
            apply_blocks_transactional(&current_dir, &blocks, true)
        }
        Ok(plans) => {
            if !args.yes {
                print_patch_plan(&blocks);
                println!("\n{}", render_patch_plan(&plans));
                if input_from_stdin || !io::stdin().is_terminal() {
                    eprintln!(
                        "Refusing to write without approval. Review the plan above, then re-run with --yes; use --dry-run for validation only."
                    );
                    return ExitCode::FAILURE;
                }

                let approved = Confirm::new("Apply exactly this patch?")
                    .with_default(false)
                    .prompt()
                    .unwrap_or(false);
                if !approved {
                    println!("Patch was not applied.");
                    return ExitCode::SUCCESS;
                }
            }
            apply_planned_transactional(&current_dir, &plans)
        }
        Err(error) => Err(error),
    };
    match tx_res {
        Ok(results) => report_apply_results(results),
        Err((failures, msg)) => {
            eprintln!("❌ ERROR: {}", msg);
            let resolved_llm = args
                .llm
                .as_deref()
                .filter(|_| args.yes && !args.dry_run)
                .map(|model| {
                    if model.eq_ignore_ascii_case("auto") {
                        resolve_auto_llm()
                    } else {
                        model.to_string()
                    }
                });
            if args.llm.is_some() && resolved_llm.is_none() && !args.dry_run {
                eprintln!(
                    "  - Local LLM repair was not run because non-interactive repair requires explicit --yes approval."
                );
            }

            let mut repair_paths = Vec::new();
            let mut repairable = true;
            for failure in &failures {
                match failure {
                    ApplyResult::AmbiguousMatch(path) | ApplyResult::MatchNotFound(path) => {
                        eprintln!("  - Could not match cleanly in {}", path);
                        if !repair_paths.contains(path) {
                            repair_paths.push(path.clone());
                        }
                    }
                    ApplyResult::SecurityError(error) => {
                        eprintln!("  - Security violation: {}", error);
                        repairable = false;
                    }
                    ApplyResult::IoError(path, error) => {
                        eprintln!("  - IO Error on {}: {}", path, error);
                        repairable = false;
                    }
                    ApplyResult::SourceChanged(path) => {
                        eprintln!("  - Source changed after review: {}", path);
                        repairable = false;
                    }
                    _ => repairable = false,
                }
            }

            let Some(model) = resolved_llm.filter(|_| repairable) else {
                return ExitCode::FAILURE;
            };
            let mut repaired_blocks: Vec<_> = blocks
                .iter()
                .filter(|block| !repair_paths.contains(&block.file_path))
                .cloned()
                .collect();
            for path in repair_paths {
                let current = match fs::read_to_string(&path) {
                    Ok(content) => content,
                    Err(error) => {
                        eprintln!("  - Could not read {} for local repair: {}", path, error);
                        return ExitCode::FAILURE;
                    }
                };
                let path_blocks: Vec<_> = blocks
                    .iter()
                    .filter(|block| block.file_path == path)
                    .collect();
                let replacement = match repair_with_llm(&model, &path, &path_blocks) {
                    Ok(content) => content,
                    Err(error) => {
                        eprintln!("  - Local repair failed for {}: {}", path, error);
                        return ExitCode::FAILURE;
                    }
                };
                repaired_blocks.push(the_clipboard_centaur::PatchBlock {
                    file_path: path,
                    search: current.trim_start_matches('\u{FEFF}').to_string(),
                    replace: replacement,
                });
            }

            match apply_blocks_transactional(&current_dir, &repaired_blocks, false) {
                Ok(results) => {
                    println!("Local LLM repair validated transactionally; undo snapshot recorded.");
                    report_apply_results(results)
                }
                Err((repair_failures, repair_message)) => {
                    eprintln!("❌ Local repair was not applied: {}", repair_message);
                    for failure in repair_failures {
                        eprintln!("  - {:?}", failure);
                    }
                    ExitCode::FAILURE
                }
            }
        }
    }
}
