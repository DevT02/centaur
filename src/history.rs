use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFileRecord {
    pub relative_path: String,
    pub original_content: Option<String>,
    pub is_new_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchSessionRecord {
    pub session_id: String,
    pub timestamp_secs: u64,
    /// Canonical workspace the patch was applied to. History is stored globally, so
    /// without this a revert run from another project restores this project's content
    /// over the other project's files. Legacy records default to empty and are never
    /// matched to a workspace.
    #[serde(default)]
    pub workspace_root: String,
    pub files: Vec<BackupFileRecord>,
}

/// Canonical, comparable form of a workspace path.
fn workspace_key(dir: &Path) -> String {
    let raw = dir
        .canonicalize()
        .unwrap_or_else(|_| dir.to_path_buf())
        .to_string_lossy()
        .to_string();
    if let Some(stripped) = raw.strip_prefix(r"\\?\") {
        stripped.to_string()
    } else {
        raw
    }
}

impl PatchSessionRecord {
    pub fn history_dir() -> PathBuf {
        crate::config::centaur_home().join("history")
    }

    pub fn record_patch_session(
        base_dir: &Path,
        files: Vec<BackupFileRecord>,
    ) -> Result<String, String> {
        let dir = Self::history_dir();
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let timestamp_secs = (nanos / 1_000_000_000) as u64;

        let session_id = format!("{:016x}", nanos);

        let record = PatchSessionRecord {
            session_id: session_id.clone(),
            timestamp_secs,
            workspace_root: workspace_key(base_dir),
            files,
        };

        let file_path = dir.join(format!("session_{}.json", session_id));
        let content = serde_json_to_string(&record)?;
        fs::write(&file_path, content).map_err(|e| e.to_string())?;

        Ok(session_id)
    }

    pub fn list_sessions() -> Vec<PatchSessionRecord> {
        let dir = Self::history_dir();
        let mut sessions = Vec::new();
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() && p.extension().map(|e| e == "json").unwrap_or(false) {
                        if let Ok(content) = fs::read_to_string(&p) {
                            if let Ok(record) = serde_json_from_str::<PatchSessionRecord>(&content)
                            {
                                sessions.push(record);
                            }
                        }
                    }
                }
            }
        }
        sessions.sort_by(|a, b| b.timestamp_secs.cmp(&a.timestamp_secs));
        sessions
    }

    /// Sessions recorded against `base_dir`, newest first. Reverting is only ever
    /// safe within the workspace the patch was applied to.
    pub fn list_sessions_for(base_dir: &Path) -> Vec<PatchSessionRecord> {
        let key = workspace_key(base_dir);
        Self::list_sessions()
            .into_iter()
            .filter(|s| s.workspace_root == key)
            .collect()
    }

    pub fn revert_session(base_dir: &Path, session_id: &str) -> Result<Vec<String>, String> {
        let sessions = Self::list_sessions_for(base_dir);
        let session = if session_id == "latest" {
            sessions.into_iter().next()
        } else {
            sessions.into_iter().find(|s| s.session_id == session_id)
        }
        .ok_or_else(|| {
            format!(
                "Session '{}' not found in history for this workspace ({}).",
                session_id,
                base_dir.display()
            )
        })?;

        // Session files are on-disk input. Validate every path before writing, the
        // same way the patch path does — a relative_path is not inherently safe.
        for file in &session.files {
            crate::patch::is_safe_path(base_dir, &file.relative_path)
                .map_err(|e| format!("Refusing to revert unsafe path: {}", e))?;
        }

        let mut restored = Vec::new();
        for file in session.files {
            let target_path = base_dir.join(&file.relative_path);
            if file.is_new_file {
                if target_path.exists() {
                    fs::remove_file(&target_path).map_err(|e| e.to_string())?;
                    restored.push(format!("Deleted created file: {}", file.relative_path));
                }
            } else if let Some(content) = file.original_content {
                if let Some(parent) = target_path.parent() {
                    let _ = fs::create_dir_all(parent);
                }
                fs::write(&target_path, content).map_err(|e| e.to_string())?;
                restored.push(format!("Restored original content: {}", file.relative_path));
            }
        }

        Ok(restored)
    }
}

// Lightweight JSON helpers to avoid adding extra dependency overhead if std serde is present
fn serde_json_to_string<T: Serialize>(val: &T) -> Result<String, String> {
    toml::to_string_pretty(val).map_err(|e| e.to_string())
}

fn serde_json_from_str<T: for<'de> Deserialize<'de>>(s: &str) -> Result<T, String> {
    toml::from_str(s).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    use crate::test_support::use_scratch_home;

    #[test]
    fn test_record_and_revert() {
        use_scratch_home();
        let dir = tempdir().unwrap();
        let target = dir.path().join("modified.txt");
        fs::write(&target, "Original Content").unwrap();

        let backup = BackupFileRecord {
            relative_path: "modified.txt".to_string(),
            original_content: Some("Original Content".to_string()),
            is_new_file: false,
        };

        let session_id =
            PatchSessionRecord::record_patch_session(dir.path(), vec![backup]).unwrap();

        // Mutate target
        fs::write(&target, "Corrupted AI Content").unwrap();
        assert_eq!(fs::read_to_string(&target).unwrap(), "Corrupted AI Content");

        // Revert
        let res = PatchSessionRecord::revert_session(dir.path(), &session_id).unwrap();
        assert_eq!(res.len(), 1);
        assert_eq!(fs::read_to_string(&target).unwrap(), "Original Content");
    }
}
