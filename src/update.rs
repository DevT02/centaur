use std::process::Command;

pub const UPDATE_REPOSITORY: &str = "https://github.com/DevT02/centaur.git";

fn update_command() -> Command {
    let mut command = Command::new("cargo");
    command.args(["install", "--git", UPDATE_REPOSITORY, "--locked", "--force"]);
    command
}

pub fn install_latest() -> Result<(), String> {
    let status = update_command()
        .status()
        .map_err(|error| format!("Could not start Cargo: {}", error))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("Cargo install failed with status {}", status))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_uses_the_explicit_locked_repository() {
        let command = update_command();
        let args: Vec<_> = command.get_args().collect();

        assert_eq!(command.get_program(), "cargo");
        assert_eq!(
            args,
            ["install", "--git", UPDATE_REPOSITORY, "--locked", "--force"]
        );
        assert!(
            command.get_current_dir().is_none(),
            "the updater must never target the caller's repository"
        );
    }
}
