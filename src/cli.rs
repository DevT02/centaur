use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use the_clipboard_centaur::git::ExportMode;

#[derive(Parser, Debug)]
#[command(
    name = "The Clipboard Centaur",
    version,
    about = "Safely exchange repository context and reviewed patches with AI clients.",
    long_about = "Export local repository context, run workspace-scoped MCP tools, and validate complete Search/Replace patch sets before any file is written."
)]
pub(crate) struct Args {
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,

    /// Read the patch text directly from the OS clipboard.
    #[arg(short, long, conflicts_with_all = ["file", "stdin"])]
    pub(crate) clipboard: bool,

    /// Read the patch text from a specific file.
    #[arg(short, long, conflicts_with_all = ["clipboard", "stdin"])]
    pub(crate) file: Option<String>,

    /// Read the patch payload from standard input.
    #[arg(long, conflicts_with_all = ["clipboard", "file"])]
    pub(crate) stdin: bool,

    /// Apply a validated patch without an interactive confirmation.
    #[arg(long)]
    pub(crate) yes: bool,

    /// Fallback to a local LLM via Ollama if the deterministic patch fails. Use 'auto' for hardware recommendation.
    #[arg(short, long)]
    pub(crate) llm: Option<String>,

    /// Export files/directories into upload-ready context parts and copy the prompt.
    #[arg(short, long, num_args = 0..)]
    pub(crate) export: Option<Vec<String>>,

    /// Context export mode: full, changed, staged, or compact.
    #[arg(long, value_enum, default_value = "full")]
    pub(crate) mode: ExportMode,

    /// Task description to insert into the workflow prompt.
    #[arg(long)]
    pub(crate) task: Option<String>,

    /// Maximum attachments generated for one upload message (default: 20).
    #[arg(long)]
    pub(crate) max_parts: Option<usize>,

    /// Preferred maximum attachment size in characters (default: 5000000).
    #[arg(long)]
    pub(crate) max_part_chars: Option<usize>,

    /// Model context token budget for warning alerts (default: 200000).
    #[arg(long)]
    pub(crate) context_tokens: Option<usize>,

    /// Preview patch changes without modifying files.
    #[arg(long)]
    pub(crate) dry_run: bool,

    /// Verify Ollama and download a recommended local repair model.
    #[arg(long)]
    pub(crate) setup: bool,

    /// Bypass safety size limits for exports.
    #[arg(long)]
    pub(crate) force: bool,

    /// Strip detected credentials from the exported copy (the workspace is untouched).
    #[arg(long)]
    pub(crate) redact: bool,

    /// Deprecated alias for --max-part-chars.
    #[arg(long)]
    pub(crate) chunk_size: Option<usize>,
}

#[derive(Subcommand, Debug)]
pub(crate) enum Commands {
    /// Manage the Centaur workflow prompt template.
    Prompt {
        #[command(subcommand)]
        action: PromptAction,
    },
    /// Manage the Centaur config file.
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// Revert a previous patch session.
    Undo {
        /// Session ID to revert (default: latest).
        #[arg(default_value = "latest")]
        session_id: String,
    },
    /// View the patch history for the current workspace.
    History,
    /// Launch the interactive terminal workspace hub.
    Ui,
    /// Audit the workspace for likely leaked credentials and API keys.
    Audit,
    /// Update Centaur from its explicit source repository.
    Update,
    /// Connect Centaur to a GUI client that speaks MCP.
    Mcp {
        #[command(subcommand)]
        action: McpAction,
    },
    /// Install the /centaur command or skill in a GUI client.
    Skill {
        #[command(subcommand)]
        action: SkillAction,
    },
    /// Configure MCP and commands or skills for supported GUI clients.
    Install {
        /// Client ID (default: all), such as antigravity, claude, or cursor.
        #[arg(long, default_value = "all")]
        client: String,
        /// Project workspace directory (default: current directory).
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Name for the entry and command (default: centaur).
        #[arg(long, default_value = "centaur")]
        name: String,
    },
    /// Check the workspace, local storage, clipboard, and optional integrations.
    Doctor {
        /// Hide local filesystem paths so the output is safer to share.
        #[arg(long)]
        redact_paths: bool,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum SkillAction {
    /// Write the command file for a client. Omit --client to list clients.
    Install {
        /// Client ID, for example antigravity, claude-code, or cursor.
        #[arg(long)]
        client: Option<String>,
        /// Markdown file to write instead, for a client not listed.
        #[arg(long)]
        output: Option<PathBuf>,
        /// Project the command applies to, for clients that store it per project.
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Name of the command, which is what you type after the slash.
        #[arg(long, default_value = "centaur")]
        name: String,
    },
}

#[derive(Subcommand, Debug)]
pub(crate) enum McpAction {
    /// Add Centaur to a client's MCP configuration. Omit --client to list clients.
    Install {
        /// Client ID, for example claude-desktop, antigravity, or cursor.
        #[arg(long)]
        client: Option<String>,
        /// Configuration file to edit instead, for a client not listed.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Project the client may patch (default: current directory).
        #[arg(long)]
        workspace: Option<PathBuf>,
        /// Name for the entry in the client's configuration.
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
pub(crate) enum PromptAction {
    /// Display the current prompt templates.
    Show,
    /// Copy the prompt template to the clipboard.
    Copy,
    /// Open a prompt template in your default editor.
    Edit {
        /// Edit the single-upload template instead of the multi-batch template.
        #[arg(long)]
        single: bool,
    },
    /// Reset both prompt templates to their defaults.
    Reset,
}

#[derive(Subcommand, Debug)]
pub(crate) enum ConfigAction {
    /// Write a config file containing the current defaults.
    Init,
    /// Print the config file location.
    Path,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn command_definition_is_internally_consistent() {
        Args::command().debug_assert();
    }

    #[test]
    fn top_level_help_names_each_workflow_entry_point() {
        let help = Args::command().render_long_help().to_string();

        assert!(help.contains("Export local repository context"), "{help}");
        assert!(help.contains("workspace-scoped MCP tools"), "{help}");
        assert!(help.contains("Search/Replace patch sets"), "{help}");
    }
}
