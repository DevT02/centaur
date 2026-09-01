# The Clipboard Centaur for VS Code

Use Centaur's guided AI task, reviewed update, and undo workflow without
composing terminal flags.

The `centaur` executable must be installed and available on `PATH`.

## Commands

- **Centaur: Start an AI Task** asks what should change and prepares the right
  project context.
- **Centaur: Apply AI Update** opens Centaur's interactive exact-diff review so
  the approved plan and the applied change stay in one process.
- **Centaur: Check Project** shows detected project commands and requires
  approval before running repository code.
- **Centaur: Undo Last Change** restores the latest Centaur snapshot.
- **Centaur: Export Context for AI** retains the expert changed-file export.

The extension never inserts a task description into a shell command. It passes
arguments directly to the local Centaur executable.

Project documentation: <https://github.com/DevT02/centaur>
