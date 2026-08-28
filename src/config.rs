use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportConfig {
    #[serde(default = "default_copy_prompt")]
    pub copy_prompt: bool,
    #[serde(default = "default_open_export_directory")]
    pub open_export_directory: bool,
    #[serde(default = "default_prompt_mode")]
    pub prompt_mode: String,
    #[serde(default = "default_max_attachments_per_message")]
    pub max_attachments_per_message: usize,
    #[serde(default = "default_max_attachment_chars")]
    pub max_attachment_chars: usize,
    #[serde(default = "default_context_token_budget")]
    pub context_token_budget: usize,
}

fn default_copy_prompt() -> bool {
    true
}
fn default_open_export_directory() -> bool {
    true
}
fn default_prompt_mode() -> String {
    "first".to_string()
}
fn default_max_attachments_per_message() -> usize {
    20
}
fn default_max_attachment_chars() -> usize {
    5_000_000
}
fn default_context_token_budget() -> usize {
    200_000
}

impl Default for ExportConfig {
    fn default() -> Self {
        Self {
            copy_prompt: default_copy_prompt(),
            open_export_directory: default_open_export_directory(),
            prompt_mode: default_prompt_mode(),
            max_attachments_per_message: default_max_attachments_per_message(),
            max_attachment_chars: default_max_attachment_chars(),
            context_token_budget: default_context_token_budget(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CentaurConfig {
    #[serde(default)]
    pub export: ExportConfig,
}

/// Directory holding config, prompt templates and patch history.
///
/// `CENTAUR_HOME` overrides the default. Tests rely on this to avoid writing into
/// the developer's real config directory, and it makes portable installs possible.
pub fn centaur_home() -> PathBuf {
    if let Some(dir) = std::env::var_os("CENTAUR_HOME") {
        return PathBuf::from(dir);
    }
    match dirs::config_dir() {
        Some(dir) => dir.join("centaur"),
        None => PathBuf::from("centaur"),
    }
}

impl CentaurConfig {
    pub fn config_path() -> PathBuf {
        centaur_home().join("config.toml")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists()
            && let Ok(content) = fs::read_to_string(&path)
            && let Ok(config) = toml::from_str(&content)
        {
            return config;
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), String> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let content = toml::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(&path, content).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_keys_from_older_versions_do_not_break_loading() {
        // Fields that were removed as dead must not turn an existing config file
        // into a parse error that silently falls back to defaults.
        let old = "[export]\nchunk_size = 300000\nfiles_per_batch = 5\ncopy_prompt = false\n\n[prompt]\nuse_custom_prompt = false\n";
        let parsed: CentaurConfig = toml::from_str(old).expect("legacy config should still parse");
        assert!(!parsed.export.copy_prompt, "live settings must survive");
    }

    #[test]
    fn defaults_round_trip_through_toml() {
        let text = toml::to_string_pretty(&CentaurConfig::default()).unwrap();
        let back: CentaurConfig = toml::from_str(&text).unwrap();
        assert_eq!(
            back.export.max_attachment_chars,
            default_max_attachment_chars()
        );
    }
}
