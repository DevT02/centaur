use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, ExitStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationCommand {
    pub label: String,
    pub program: String,
    pub args: Vec<String>,
}

impl VerificationCommand {
    pub fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn run(&self, root: &Path) -> std::io::Result<ExitStatus> {
        Command::new(&self.program)
            .args(&self.args)
            .current_dir(root)
            .status()
    }
}

fn command(label: &str, program: &str, args: &[&str]) -> VerificationCommand {
    VerificationCommand {
        label: label.to_string(),
        program: program.to_string(),
        args: args.iter().map(|arg| (*arg).to_string()).collect(),
    }
}

fn node_program(root: &Path) -> Option<&'static str> {
    if root.join("pnpm-lock.yaml").is_file() {
        Some("pnpm")
    } else if root.join("yarn.lock").is_file() {
        Some("yarn")
    } else if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        Some("bun")
    } else if root.join("package.json").is_file() {
        Some(if cfg!(windows) { "npm.cmd" } else { "npm" })
    } else {
        None
    }
}

fn python_program(root: &Path) -> String {
    let local = if cfg!(windows) {
        root.join(".venv").join("Scripts").join("python.exe")
    } else {
        root.join(".venv").join("bin").join("python")
    };
    if local.is_file() {
        local.to_string_lossy().to_string()
    } else {
        "python".to_string()
    }
}

pub fn detect(root: &Path) -> Vec<VerificationCommand> {
    let mut checks = Vec::new();

    if root.join("Cargo.toml").is_file() {
        let mut args = vec!["test", "--all-targets"];
        if root.join("Cargo.lock").is_file() {
            args.push("--locked");
        }
        checks.push(command("Rust tests", "cargo", &args));
    }

    let package_json = root.join("package.json");
    if let (Some(program), Ok(contents)) = (node_program(root), fs::read_to_string(&package_json))
        && let Ok(package) = serde_json::from_str::<Value>(&contents)
        && let Some(scripts) = package.get("scripts").and_then(Value::as_object)
    {
        for (name, label) in [("test", "Node tests"), ("build", "Node build")] {
            let Some(script) = scripts.get(name).and_then(Value::as_str) else {
                continue;
            };
            if name == "test" && script.contains("no test specified") {
                continue;
            }
            checks.push(command(label, program, &["run", name]));
        }
    }

    let pyproject = fs::read_to_string(root.join("pyproject.toml")).unwrap_or_default();
    let has_pytest = root.join("pytest.ini").is_file()
        || root.join("conftest.py").is_file()
        || pyproject.contains("[tool.pytest.ini_options]");
    if has_pytest {
        checks.push(command(
            "Python tests",
            &python_program(root),
            &["-m", "pytest"],
        ));
    }

    checks
}

pub fn describe(checks: &[VerificationCommand]) -> Vec<String> {
    checks
        .iter()
        .map(|check| format!("{}: {}", check.label, check.display()))
        .collect()
}

pub fn run_all(root: &Path, checks: &[VerificationCommand]) -> bool {
    let mut passed = true;
    for check in checks {
        println!("\nRunning {}: {}", check.label, check.display());
        match check.run(root) {
            Ok(status) if status.success() => println!("Passed: {}", check.label),
            Ok(status) => {
                eprintln!("Failed: {} exited with {}", check.label, status);
                passed = false;
            }
            Err(error) => {
                eprintln!("Failed to start {}: {}", check.label, error);
                passed = false;
            }
        }
    }
    passed
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn detects_manifest_backed_checks_without_a_shell() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='fixture'\n").unwrap();
        fs::write(dir.path().join("Cargo.lock"), "").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"test":"vitest","build":"vite build"}}"#,
        )
        .unwrap();
        fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[tool.pytest.ini_options]\n",
        )
        .unwrap();

        let checks = detect(dir.path());
        let displays: Vec<String> = checks.iter().map(VerificationCommand::display).collect();
        let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };

        assert_eq!(
            displays,
            [
                "cargo test --all-targets --locked".to_string(),
                format!("{npm} run test"),
                format!("{npm} run build"),
                format!("{} -m pytest", python_program(dir.path())),
            ]
        );
        assert!(checks.iter().all(|check| !check.program.contains(' ')));
    }

    #[test]
    fn skips_placeholder_node_tests_and_unknown_projects() {
        let dir = tempdir().unwrap();
        assert!(detect(dir.path()).is_empty());

        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"test":"echo Error: no test specified && exit 1"}}"#,
        )
        .unwrap();
        assert!(detect(dir.path()).is_empty());
    }
}
