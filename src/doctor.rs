use arboard::Clipboard;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub struct DoctorItem {
    pub category: &'static str,
    pub name: String,
    pub required: bool,
    pub status_ok: bool,
    pub details: String,
    pub redacted_details: String,
}

#[derive(Debug)]
pub struct DoctorReport {
    pub items: Vec<DoctorItem>,
    pub summary_ok: bool,
}

impl DoctorReport {
    pub fn render_human(&self) -> String {
        self.render(false)
    }

    pub fn render_human_redacted(&self) -> String {
        self.render(true)
    }

    fn render(&self, redact_paths: bool) -> String {
        let mut out = String::new();
        out.push_str("Centaur doctor\n\n");

        let mut current_cat = "";
        for item in &self.items {
            if item.category != current_cat {
                current_cat = item.category;
                out.push_str(&format!("[{}]\n", current_cat));
            }
            let status = match (item.status_ok, item.required) {
                (true, _) => "OK",
                (false, true) => "FAIL",
                (false, false) => "OPTIONAL",
            };
            let details = if redact_paths {
                &item.redacted_details
            } else {
                &item.details
            };
            out.push_str(&format!("  [{status}] {}: {details}\n", item.name));
        }

        out.push('\n');
        if self.summary_ok {
            out.push_str("Core checks passed.\n");
        } else {
            out.push_str("One or more core checks failed. Fix the FAIL items above and run `centaur doctor` again.\n");
        }
        if self
            .items
            .iter()
            .any(|item| item.category == "MCP Integrations" && !item.status_ok)
        {
            out.push_str(
                "Optional integrations are not required. Run `centaur install --client <id>` to configure one.\n",
            );
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
        name: "Workspace root".into(),
        required: true,
        status_ok: ws_exists,
        details: workspace.display().to_string(),
        redacted_details: "<workspace>".into(),
    });
    if !ws_exists {
        all_ok = false;
    }

    let git_dir = workspace.join(".git");
    let is_git = git_dir.exists();
    items.push(DoctorItem {
        category: "Workspace",
        name: "Git repository".into(),
        required: false,
        status_ok: is_git,
        details: if is_git {
            "Detected .git repository".into()
        } else {
            "Not a Git repository (Full & Compact export modes still work)".into()
        },
        redacted_details: if is_git {
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
        name: "Centaur home".into(),
        required: true,
        status_ok: home_ok,
        details: home.display().to_string(),
        redacted_details: "<centaur-home>".into(),
    });
    if !home_ok {
        all_ok = false;
    }

    let history_dir = crate::history::PatchSessionRecord::history_dir();
    let hist_ok = history_dir.exists() || std::fs::create_dir_all(&history_dir).is_ok();
    items.push(DoctorItem {
        category: "Storage",
        name: "Undo history".into(),
        required: true,
        status_ok: hist_ok,
        details: history_dir.display().to_string(),
        redacted_details: "<centaur-home>/history".into(),
    });
    if !hist_ok {
        all_ok = false;
    }

    // 3. System Clipboard
    let clipboard_ok = Clipboard::new().is_ok();
    items.push(DoctorItem {
        category: "Environment",
        name: "OS clipboard".into(),
        required: false,
        status_ok: clipboard_ok,
        details: if clipboard_ok {
            "System clipboard is accessible".into()
        } else {
            "Clipboard inaccessible; use --file <path> fallback".into()
        },
        redacted_details: if clipboard_ok {
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

        let recovery = format!("Run `centaur install --client {}` to configure", target.id);
        items.push(DoctorItem {
            category: "MCP Integrations",
            name: format!("{} ({})", target.label, target.id),
            required: false,
            status_ok: exists,
            details: if exists {
                format!("Configured at {}", path_str)
            } else {
                format!("Not configured at {}. {}", path_str, recovery)
            },
            redacted_details: if exists {
                "Configured".into()
            } else {
                format!("Not configured. {}", recovery)
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
        assert!(rendered.contains("Centaur doctor"));
    }

    #[test]
    fn missing_optional_integration_does_not_look_like_a_failed_core_check() {
        let report = DoctorReport {
            items: vec![DoctorItem {
                category: "MCP Integrations",
                name: "Example client".into(),
                required: false,
                status_ok: false,
                details: "Not configured".into(),
                redacted_details: "Not configured".into(),
            }],
            summary_ok: true,
        };

        let rendered = report.render_human();
        assert!(rendered.contains("[OPTIONAL] Example client"), "{rendered}");
        assert!(rendered.contains("Core checks passed."), "{rendered}");
        assert!(!rendered.contains("[FAIL]"), "{rendered}");
    }

    #[test]
    fn redacted_output_uses_shareable_path_placeholders() {
        let report = DoctorReport {
            items: vec![DoctorItem {
                category: "Workspace",
                name: "Workspace root".into(),
                required: true,
                status_ok: true,
                details: r"C:\Users\example\private-project".into(),
                redacted_details: "<workspace>".into(),
            }],
            summary_ok: true,
        };

        let rendered = report.render_human_redacted();
        assert!(rendered.contains("<workspace>"), "{rendered}");
        assert!(!rendered.contains("example"), "{rendered}");
    }
}
