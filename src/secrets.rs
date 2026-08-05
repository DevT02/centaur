use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

// Compiled once per process. These were previously rebuilt on every call, which
// cost ~300us per file scanned — roughly 1000x the cost of the match itself.
static AWS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"AKIA[0-9A-Z]{16}").unwrap());
static GCP_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"AIzaSy[a-zA-Z0-9_-]{33}").unwrap());
static GITHUB_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(ghp_[a-zA-Z0-9]{36}|github_pat_[a-zA-Z0-9_]{82})").unwrap());
// The body must not contain hyphens, or ordinary kebab-case identifiers like
// ".sk-spinner-container-wrapper" register as leaked API keys.
static API_KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\bsk-(?:proj-)?[a-zA-Z0-9]{20,}\b").unwrap());
static PRIVATE_KEY_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?s)-----BEGIN [A-Z ]+ PRIVATE KEY-----.*?-----END [A-Z ]+ PRIVATE KEY-----")
        .unwrap()
});
/// Header only: catches truncated keys that have no matching END line.
static PRIVATE_KEY_HEADER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"-----BEGIN [A-Z ]{0,24}PRIVATE KEY-----").unwrap());

#[derive(Debug, PartialEq, Clone)]
pub struct SecretWarning {
    pub file_path: String,
    pub pattern_name: String,
}

pub fn scan_file_for_secrets(path: &Path, content: &str) -> Vec<SecretWarning> {
    let mut warnings = Vec::new();
    let file_path_str = path.to_string_lossy().to_string();
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();

    if file_name == ".env"
        || file_name.ends_with(".env.local")
        || file_name.ends_with(".env.production")
    {
        warnings.push(SecretWarning {
            file_path: file_path_str.clone(),
            pattern_name: "Environment Secrets File (.env)".to_string(),
        });
    }

    // One header pattern covers PLAIN/RSA/EC/OPENSSH/DSA and anything else, instead of
    // enumerating each literal — which also kept this file from tripping its own scan.
    if PRIVATE_KEY_HEADER_RE.is_match(content) {
        warnings.push(SecretWarning {
            file_path: file_path_str.clone(),
            pattern_name: "PEM/RSA/SSH Private Key".to_string(),
        });
    }

    // AWS's own documentation examples all end in EXAMPLE. Flagging them trains
    // users to ignore the warning, and fails the audit on any repo that quotes the docs.
    if AWS_RE
        .find_iter(content)
        .any(|m| !m.as_str().ends_with("EXAMPLE"))
    {
        warnings.push(SecretWarning {
            file_path: file_path_str.clone(),
            pattern_name: "AWS Access Key ID".to_string(),
        });
    }

    if GCP_RE.is_match(content) {
        warnings.push(SecretWarning {
            file_path: file_path_str.clone(),
            pattern_name: "Google Cloud API Key".to_string(),
        });
    }

    if GITHUB_RE.is_match(content) {
        warnings.push(SecretWarning {
            file_path: file_path_str.clone(),
            pattern_name: "GitHub Personal Access Token".to_string(),
        });
    }

    if API_KEY_RE.is_match(content) {
        warnings.push(SecretWarning {
            file_path: file_path_str,
            pattern_name: "Secret API Key (sk-...)".to_string(),
        });
    }

    warnings
}

pub fn redact_secrets(content: &str) -> String {
    let mut redacted = content.to_string();

    redacted = AWS_RE
        .replace_all(&redacted, "[REDACTED_AWS_KEY_BY_CENTAUR]")
        .to_string();
    redacted = GCP_RE
        .replace_all(&redacted, "[REDACTED_GCP_KEY_BY_CENTAUR]")
        .to_string();
    redacted = GITHUB_RE
        .replace_all(&redacted, "[REDACTED_GITHUB_TOKEN_BY_CENTAUR]")
        .to_string();
    redacted = API_KEY_RE
        .replace_all(&redacted, "[REDACTED_API_KEY_BY_CENTAUR]")
        .to_string();
    redacted = PRIVATE_KEY_RE
        .replace_all(&redacted, "[REDACTED_PRIVATE_KEY_BY_CENTAUR]")
        .to_string();

    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test vectors are split so no complete key literal appears in this file —
    // otherwise Centaur's own audit flags its test suite forever. They are
    // reassembled at runtime, so the patterns are still exercised in full.
    const AWS_KEY: &str = concat!("AKIA", "QYLPMN5HHHFPZAM2");
    const AWS_DOC_EXAMPLE: &str = concat!("AKIA", "IOSFODNN7EXAMPLE");
    const GH_TOKEN: &str = concat!("ghp_", "0123456789abcdefghijklmnopqrstuvwxyz");
    const OPENAI_KEY: &str = concat!("sk-", "proj-", "abcdefghijklmnopqrstuvwxyz0123456789");
    const PEM_BODY: &str = "MIIBOgIB";
    const PEM_BEGIN: &str = concat!("-----", "BEGIN RSA PRIVATE KEY", "-----");
    const PEM_END: &str = concat!("-----", "END RSA PRIVATE KEY", "-----");

    #[test]
    fn test_detect_env_file() {
        let p = Path::new(".env");
        let w = scan_file_for_secrets(p, "DATABASE_URL=postgres://localhost");
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].pattern_name, "Environment Secrets File (.env)");
    }

    #[test]
    fn test_redact_secrets() {
        let text = format!(
            "aws={}\ngh={}\nkey={}\n{}\n{}\n",
            AWS_KEY, GH_TOKEN, PEM_BEGIN, PEM_BODY, PEM_END
        );
        let redacted = redact_secrets(&text);

        assert!(!redacted.contains(AWS_KEY), "aws key survived");
        assert!(!redacted.contains(GH_TOKEN), "gh token survived");
        assert!(!redacted.contains(PEM_BODY), "private key body survived");
        assert!(redacted.contains("[REDACTED_AWS_KEY_BY_CENTAUR]"));
        assert!(redacted.contains("[REDACTED_GITHUB_TOKEN_BY_CENTAUR]"));
        assert!(redacted.contains("[REDACTED_PRIVATE_KEY_BY_CENTAUR]"));
    }

    #[test]
    fn test_scan_detects_real_keys() {
        let w = scan_file_for_secrets(Path::new("cfg.rs"), &format!("let k = \"{}\";", AWS_KEY));
        assert_eq!(w.len(), 1);
        assert_eq!(w[0].pattern_name, "AWS Access Key ID");
    }

    #[test]
    fn test_documented_aws_example_is_not_a_finding() {
        // AWS's published example key; flagging it fails audits on docs and fixtures.
        let w = scan_file_for_secrets(
            Path::new("README.md"),
            &format!("AWS_KEY={}", AWS_DOC_EXAMPLE),
        );
        assert!(
            w.is_empty(),
            "documented placeholder should not warn: {:?}",
            w
        );
    }

    #[test]
    fn test_kebab_case_identifiers_are_not_api_keys() {
        for text in [
            ".sk-spinner-container-wrapper { color: red; }",
            "import { skeleton } from 'sk-ui-components-library';",
        ] {
            let w = scan_file_for_secrets(Path::new("app.css"), text);
            assert!(w.is_empty(), "false positive on {:?}: {:?}", text, w);
        }
    }

    #[test]
    fn test_real_api_key_still_detected() {
        let w = scan_file_for_secrets(Path::new("cfg.rs"), &format!("OPENAI_KEY={}", OPENAI_KEY));
        assert_eq!(w.len(), 1, "real key must still be caught: {:?}", w);
        assert_eq!(w[0].pattern_name, "Secret API Key (sk-...)");
    }

    /// Centaur must not flag its own source. A scanner that fails its own audit
    /// teaches users to ignore the tool.
    #[test]
    fn centaur_source_is_clean() {
        for file in ["secrets.rs", "export.rs", "lib.rs", "main.rs", "ui.rs"] {
            let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file);
            let content = std::fs::read_to_string(&path).unwrap();
            let w = scan_file_for_secrets(&path, &content);
            assert!(w.is_empty(), "{} trips the scanner: {:?}", file, w);
        }
    }
}
