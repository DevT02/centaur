use arboard::Clipboard;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub struct DoctorItem {
    pub category: &'static str,
    pub name: String,
    pub status_ok: bool,
    pub details: String,
}

#[derive(Debug)]
pub struct DoctorReport {
    pub items: Vec<DoctorItem>,
    pub summary_ok: bool,
}

impl DoctorReport {
    pub fn render_human(&self) -> String {
        let mut out = String::new();
        out.push_str("🏥 --- CENTAUR DIAGNOSTICS & SYSTEM DOCTOR ---\n\n");

        let mut current_cat = "";
        for item in &self.items {
            if item.category != current_cat {
                current_cat = item.category;
                out.push_str(&format!("[{}]\n", current_cat));
            }
            let symbol = if item.status_ok { "  ✅" } else { "  ❌" };
            out.push_str(&format!("{} {}: {}\n", symbol, item.name, item.details));
        }

        out.push('\n');
        if self.summary_ok {
            out.push_str("✨ System health check passed! All components are operating normally.\n");
        } else {
            out.push_str("⚠️ System health check found potential issues. Run `centaur install` to fix missing client setups.\n");
        }
        out
    }
}

pub fn diagnose(workspace: &Path) -> DoctorReport {
    let mut items = Vec::new();
    let mut all_ok = true;

    // 1. Workspace check
    let ws_exists = workspace.exists();
    items.push(DoctorItem {
        category: "Workspace",
        name: "Canonical Workspace Root".into(),
        status_ok: ws_exists,
        details: workspace.display().to_string(),
    });
    if !ws_exists {
        all_ok = false;
    }

    let git_dir = workspace.join(".git");
    let is_git = git_dir.exists();
    items.push(DoctorItem {
        category: "Workspace",
        name: "Git Repository".into(),
        status_ok: is_git,
        details: if is_git {
            "Detected .git repository".into()
        } else {
            "Not a Git repository (Full & Compact export modes still work)".into()
        },
    });

    // 2. Storage & Config check
    let home = crate::config::centaur_home();
    let home_ok = home.exists() || std::fs::create_dir_all(&home).is_ok();
    items.push(DoctorItem {
        category: "Storage",
        name: "Centaur Home Directory".into(),
        status_ok: home_ok,
        details: home.display().to_string(),
    });
    if !home_ok {
        all_ok = false;
    }

    let history_dir = crate::history::PatchSessionRecord::history_dir();
    let hist_ok = history_dir.exists() || std::fs::create_dir_all(&history_dir).is_ok();
    items.push(DoctorItem {
        category: "Storage",
        name: "Undo History Store".into(),
        status_ok: hist_ok,
        details: history_dir.display().to_string(),
    });
    if !hist_ok {
        all_ok = false;
    }

    // 3. System Clipboard
    let clipboard_ok = Clipboard::new().is_ok();
    items.push(DoctorItem {
        category: "Environment",
        name: "OS Clipboard Access".into(),
        status_ok: clipboard_ok,
        details: if clipboard_ok {
            "System clipboard is accessible".into()
        } else {
            "Clipboard inaccessible; use --file <path> fallback".into()
        },
    });

    // 4. GUI Client Integrations
    let mcp_clients = crate::mcp::known_clients(workspace);
    for target in mcp_clients {
        let (path_str, exists) = match &target.path {
            Some(p) => (p.display().to_string(), p.exists()),
            None => ("No standard location".to_string(), false),
        };

        items.push(DoctorItem {
            category: "MCP Integrations",
            name: format!("{} ({})", target.label, target.id),
            status_ok: exists,
            details: if exists {
                format!("Configured at {}", path_str)
            } else {
                format!(
                    "Not configured at {}. Run `centaur install` to setup",
                    path_str
                )
            },
        });
    }

    DoctorReport {
        items,
        summary_ok: all_ok,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_diagnose_runs_without_panicking() {
        crate::test_support::use_scratch_home();
        let scratch = std::env::temp_dir().join("centaur_doctor_test");
        let _ = std::fs::create_dir_all(&scratch);

        let report = diagnose(&scratch);
        assert!(!report.items.is_empty());
        let rendered = report.render_human();
        assert!(rendered.contains("CENTAUR DIAGNOSTICS"));
    }
}
