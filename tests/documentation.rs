use regex::Regex;
use std::fs;
use std::path::{Path, PathBuf};

fn markdown_files(root: &Path) -> Vec<PathBuf> {
    fn visit(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if !matches!(
                    path.file_name().and_then(|name| name.to_str()),
                    Some(".git" | "target")
                ) {
                    visit(&path, files);
                }
            } else if path.extension().is_some_and(|extension| extension == "md") {
                files.push(path);
            }
        }
    }

    let mut files = Vec::new();
    visit(root, &mut files);
    files
}

fn local_target(raw: &str) -> Option<String> {
    let target = raw.trim().trim_matches(['<', '>']);
    if target.is_empty()
        || target.starts_with('#')
        || target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
    {
        return None;
    }
    Some(target.split('#').next().unwrap_or(target).to_string())
}

#[test]
fn local_documentation_links_and_images_resolve() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let markdown_link = Regex::new(r#"!?\[[^\]]*\]\(([^)]+)\)"#).unwrap();
    let html_source = Regex::new(r#"<(?:img|source)[^>]*\bsrc=\"([^\"]+)\""#).unwrap();
    let mut missing = Vec::new();

    for document in markdown_files(root) {
        let content = fs::read_to_string(&document).unwrap();
        let parent = document.parent().unwrap();
        let targets = markdown_link
            .captures_iter(&content)
            .filter_map(|capture| local_target(&capture[1]))
            .chain(
                html_source
                    .captures_iter(&content)
                    .filter_map(|capture| local_target(&capture[1])),
            );

        for target in targets {
            let resolved = parent.join(&target);
            if !resolved.exists() {
                missing.push(format!(
                    "{} -> {}",
                    document.strip_prefix(root).unwrap().display(),
                    target
                ));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "missing local documentation targets:\n{}",
        missing.join("\n")
    );
}
