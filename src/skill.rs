//! Installs Centaur as a client-side skill or slash command.
//!
//! Slash commands are not part of MCP: every client that has them reads a markdown
//! file from its own directory instead. The body is identical everywhere, so this
//! is the same file written to a different place per client.

use std::fs;
use std::path::{Path, PathBuf};

/// Where one client looks for user-authored commands.
struct SkillTarget {
    id: &'static str,
    label: &'static str,
    path: Option<PathBuf>,
    /// Skill directories require YAML frontmatter to load; plain command files
    /// render it as body text.
    frontmatter: bool,
    /// How the user reaches it once installed.
    invocation: &'static str,
}

fn known_targets(workspace: &Path, name: &str) -> Vec<SkillTarget> {
    let home = dirs::home_dir();
    let in_home = |parts: &[&str]| {
        home.as_ref()
            .map(|h| parts.iter().fold(h.clone(), |path, part| path.join(part)))
    };

    vec![
        SkillTarget {
            id: "antigravity",
            label: "Antigravity (Skill & GUI Slash Commands)",
            path: in_home(&[".gemini", "config", "skills", name, "SKILL.md"]),
            frontmatter: true,
            invocation: "/{name} or ask for it by name in chat GUI",
        },
        SkillTarget {
            id: "claude",
            label: "Claude Desktop & Claude Code (Skill & Commands)",
            path: in_home(&[".claude", "skills", name, "SKILL.md"]),
            frontmatter: true,
            invocation: "/{name} or /{name} export in chat GUI",
        },
        SkillTarget {
            id: "chatgpt",
            label: "ChatGPT Desktop & Codex CLI (Prompts & Custom Instructions)",
            path: in_home(&[".codex", "prompts", &format!("{}.md", name)]),
            frontmatter: false,
            invocation: "/{name} or paste into Custom Instructions",
        },
        SkillTarget {
            id: "agents",
            label: "Cross-client (~/.agents/skills)",
            path: in_home(&[".agents", "skills", name, "SKILL.md"]),
            frontmatter: true,
            invocation: "any client that reads the shared skills directory",
        },
        SkillTarget {
            id: "cursor",
            label: "Cursor (this project)",
            path: Some(
                workspace
                    .join(".cursor")
                    .join("commands")
                    .join(format!("{}.md", name)),
            ),
            frontmatter: false,
            invocation: "/{name}",
        },
        SkillTarget {
            id: "windsurf",
            label: "Windsurf (this project)",
            path: Some(
                workspace
                    .join(".windsurf")
                    .join("workflows")
                    .join(format!("{}.md", name)),
            ),
            frontmatter: false,
            invocation: "/{name}",
        },
    ]
}

fn listing(workspace: &Path, name: &str) -> String {
    let mut out = String::from("Clients that read a markdown command/skill file:\n\n");
    for target in known_targets(workspace, name) {
        let location = match &target.path {
            Some(path) if path.exists() => format!("{}  (overwrite)", path.display()),
            Some(path) => path.display().to_string(),
            None => "could not resolve a home directory".to_string(),
        };
        out.push_str(&format!("  {:<12} {}\n", target.id, location));
    }
    out.push_str("\nInstall for all clients:  centaur skill install --client all\n");
    out.push_str("Install for one client:  centaur skill install --client <id>\n");
    out.push_str("Any other client:        centaur skill install --output <path to a .md file>\n");
    out
}

pub fn install(
    client: Option<&str>,
    output: Option<&Path>,
    workspace: &Path,
    name: &str,
) -> Result<String, String> {
    let workspace = std::path::absolute(workspace).map_err(|e| {
        format!(
            "Could not resolve workspace '{}': {}",
            workspace.display(),
            e
        )
    })?;
    if !workspace.is_dir() {
        return Err(format!(
            "Workspace is not a directory: {}",
            workspace.display()
        ));
    }

    if client == Some("all") {
        let clients = vec![
            "antigravity",
            "claude",
            "chatgpt",
            "agents",
            "cursor",
            "windsurf",
        ];
        let mut reports = Vec::new();
        for c in clients {
            if let Ok(rep) = install(Some(c), None, &workspace, name) {
                reports.push(format!("[{}]\n{}", c, rep));
            }
        }
        return Ok(format!(
            "Installed Centaur skills & GUI slash commands for all clients:\n\n{}",
            reports.join("\n\n")
        ));
    }

    let normalized_client = match client {
        Some("claude-desktop") | Some("claude-code") | Some("claude") => Some("claude"),
        Some("codex") | Some("chatgpt") => Some("chatgpt"),
        Some("antigravity-command") | Some("antigravity") => Some("antigravity"),
        other => other,
    };

    let (path, frontmatter, invocation) = match (normalized_client, output) {
        (Some(id), _) => {
            let target = known_targets(&workspace, name)
                .into_iter()
                .find(|t| t.id == id)
                .ok_or_else(|| {
                    format!("Unknown client '{}'.\n\n{}", id, listing(&workspace, name))
                })?;
            let path = target
                .path
                .ok_or_else(|| format!("Could not resolve the skill path for {}.", target.label))?;
            (path, target.frontmatter, target.invocation)
        }
        // A bare .md path is assumed to be a command file; skill directories are
        // named, and this cannot tell which convention an unknown client wants.
        (None, Some(path)) => (path.to_path_buf(), false, "/{name}"),
        (None, None) => return Ok(listing(&workspace, name)),
    };

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create {}: {}", parent.display(), e))?;
    }
    let existed = path.exists();
    fs::write(&path, render(name, frontmatter))
        .map_err(|e| format!("Could not write {}: {}", path.display(), e))?;

    let mut report = format!(
        "{} {}\n\nInvoke it with: {}\nRestart the client if it does not appear.",
        if existed { "Replaced" } else { "Wrote" },
        path.display(),
        invocation.replace("{name}", name)
    );

    // Write client-specific GUI slash commands and extra prompt locations
    if let Some(home) = dirs::home_dir() {
        match normalized_client {
            Some("antigravity") => {
                let legacy_skills = home.join(".gemini").join("skills").join(name);
                if fs::create_dir_all(&legacy_skills).is_ok() {
                    let _ = fs::write(legacy_skills.join("SKILL.md"), render(name, true));
                }
                let ws_skills = workspace.join(".agents").join("skills").join(name);
                if fs::create_dir_all(&ws_skills).is_ok() {
                    let _ = fs::write(ws_skills.join("SKILL.md"), render(name, true));
                }
                let commands_dir = home.join(".gemini").join("commands");
                if fs::create_dir_all(&commands_dir).is_ok() {
                    let _ = fs::write(
                        commands_dir.join(format!("{}.md", name)),
                        render(name, false),
                    );
                    if name == "centaur" {
                        let _ = fs::write(commands_dir.join("export.md"), render("export", false));
                    }
                    report.push_str(&format!(
                        "\nAlso installed GUI slash command(s) in {} and {}",
                        commands_dir.display(),
                        ws_skills.display()
                    ));
                }
            }
            Some("claude") => {
                let commands_dir = home.join(".claude").join("commands");
                if fs::create_dir_all(&commands_dir).is_ok() {
                    let _ = fs::write(
                        commands_dir.join(format!("{}.md", name)),
                        render(name, false),
                    );
                    if name == "centaur" {
                        let _ = fs::write(commands_dir.join("export.md"), render("export", false));
                    }
                    report.push_str(&format!(
                        "\nAlso installed Claude GUI slash command(s) in {}",
                        commands_dir.display()
                    ));
                }
            }
            Some("chatgpt") => {
                let prompts_dir = home.join(".codex").join("prompts");
                if fs::create_dir_all(&prompts_dir).is_ok() {
                    let _ = fs::write(
                        prompts_dir.join(format!("{}.md", name)),
                        render(name, false),
                    );
                    if name == "centaur" {
                        let _ = fs::write(prompts_dir.join("export.md"), render("export", false));
                    }
                    report.push_str(&format!(
                        "\nAlso installed ChatGPT/Codex prompt files in {}\n\nFor ChatGPT Desktop: Add to Settings -> Personalization -> Custom Instructions:\n\"When I type /centaur or /export, export local codebase context and format patches as Search/Replace blocks.\"",
                        prompts_dir.display()
                    ));
                }
            }
            _ => {}
        }
    }

    Ok(report)
}

/// One body for every client. It cannot know whether the host has a shell or only
/// the MCP tools, so it tells the agent how to tell the difference and what to do
/// in each case.
fn render(name: &str, frontmatter: bool) -> String {
    let mut out = String::new();
    if frontmatter {
        out.push_str(&format!(
            "---\nname: {}\ndescription: Read a local repository and apply reviewed edits back to \
             it through Centaur, with every change validated before it is written and an undo \
             snapshot recorded. Use when asked to read, patch, or undo edits in a Centaur \
             workspace, or when the host has no filesystem access of its own.\n---\n\n",
            name
        ));
    }

    out.push_str(
        r#"# Centaur

Centaur reads a local repository and writes reviewed edits back to it. Every edit is a
Search/Replace block; all blocks are validated before any of them are written, and a
successful apply records an undo snapshot.

## Installation & Setup

To set up Centaur's MCP server and GUI slash commands across all supported AI clients in one step:

```sh
centaur install
```

For specific clients or custom target folders:
```sh
centaur skill install --client all                     # install skills across all clients
centaur mcp install --client antigravity               # configure MCP for Antigravity
centaur skill install --output path/to/action.md       # custom output file
```

## First Interaction when Invoking /centaur

When the user types `/centaur`, Centaur is already installed. Immediately ask the user what task they would like to accomplish and which context export mode to run:
1. **`changed`** (Recommended for active work): Read modified & untracked Git files.
2. **`full`**: Read the entire workspace.
3. **`compact`**: Read full workspace with repetitive generated content summarized.

If the user already specified a task in their `/centaur` prompt, immediately fetch context (`get_context` or `centaur --export`) and propose Search/Replace patches.

## Pick the path that matches this session

**If the `centaur` MCP tools are available** (`get_context`, `apply_patch`, `undo`), use them.
They are the whole loop and need no terminal:

1. `get_context` — read the workspace. Use `mode: "changed"` when the user is working on
   uncommitted edits, `mode: "full"` otherwise, and `paths` to narrow a large project.
   Large reads come back in numbered parts; keep calling with the next `part` until done.
2. Write the edits as Search/Replace blocks and pass them to `apply_patch`. Use
   `dry_run: true` first when the change is wide or the Search text is not obviously unique.
3. `undo` reverts the last apply if the result is wrong.

The workspace is fixed at install time and is not a tool argument. If `apply_patch` writes
to the wrong project, the fix is `centaur install --workspace <path>` (or `centaur mcp install --client <id> --workspace <path>`) and a client restart.

**If you can run shell commands**, call the CLI directly:

```sh
centaur --export --mode changed --task "<what to do>"   # pack context for a web AI
centaur --clipboard                                     # apply the AI's reply
centaur --dry-run --clipboard                           # validate without writing
centaur undo                                            # revert the last apply
centaur audit                                           # scan for leaked credentials
centaur install                                         # auto-configure MCP & skills
```

Editors that read and write files themselves do not need `--export`. Use `centaur undo`
and `centaur audit`, and edit directly.

## Block format

```
File: src/main.rs
<<<<<<< SEARCH
exact lines from the current file
=======
replacement lines
>>>>>>> REPLACE
```

- Paths are workspace-relative. No `./`, no absolute paths.
- SEARCH must match the file byte for byte. Include 2-3 surrounding lines so it matches once.
- Empty SEARCH creates a new file.
- Never wrap blocks in backticks.

## Failure modes

- `search text not found` — re-read the file with `get_context`; you are patching a stale copy.
- `matches more than once` — add surrounding lines until the match is unique.
- `refused: traversal` — the path leaves the workspace. It will not be written.

If a read reports possible credentials, tell the user, do not echo the values, and offer
`redact: true`.
"#,
    );
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn install_writes_a_command_file_and_frontmatter_only_where_it_belongs() {
        let dir = tempdir().unwrap();

        let plain = dir.path().join("centaur.md");
        install(None, Some(&plain), dir.path(), "centaur").unwrap();
        let body = fs::read_to_string(&plain).unwrap();
        assert!(
            !body.starts_with("---"),
            "command files take no frontmatter"
        );
        assert!(body.contains("get_context"));

        // Cursor's target is inside the workspace, so this exercises the real path
        // construction rather than a caller-supplied file.
        let report = install(Some("cursor"), None, dir.path(), "centaur").unwrap();
        assert!(
            report.contains("Wrote") || report.contains("Replaced"),
            "{}",
            report
        );
        assert!(dir.path().join(".cursor/commands/centaur.md").exists());
    }

    #[test]
    fn install_all_clients_returns_combined_report() {
        let dir = tempdir().unwrap();
        let report = install(Some("all"), None, dir.path(), "centaur").unwrap();
        assert!(report.contains("Installed Centaur skills & GUI slash commands for all clients"));
        assert!(report.contains("[antigravity]"));
        assert!(report.contains("[claude]"));
        assert!(report.contains("[chatgpt]"));
    }
}
