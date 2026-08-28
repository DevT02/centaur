# Visual capture guide

The README visuals should show real product behavior, stay readable at GitHub's content width, and avoid personal data.

## Workflow overview

`workflow.html` is the source for `../screenshots/workflow_overview.png`.

It uses the project palette and carries a copy of the centaur from `centaur_logo.svg`
inline, so the page needs no network and no build step. If the logo art changes, that
copy has to be updated by hand.

1. Open `workflow.html` in Chromium.
2. Set the viewport width to 1160 at device scale 2, so the capture survives being
   displayed at the 900 pixel width the README uses.
3. Capture the full page as `docs/screenshots/workflow_overview.png`.
4. Confirm no label overlaps the trust boundary or a connector, and that every
   label is readable at 900 pixels wide.

The source has no network dependencies, so a contributor can reproduce the image offline.

## CLI screenshots

Use a synthetic Git repository with no private code. Build the current checkout, put `target/debug` on the temporary shell's `PATH`, and set `CENTAUR_HOME`, `TEMP`, and `TMP` to disposable public paths.

Capture these states in a 120-column terminal:

- `centaur doctor --redact-paths` after core checks pass and optional clients are absent
- `centaur --export --mode changed --task "Add input validation"`
- `centaur --dry-run --file response.txt` with one small, valid patch

Before keeping a capture:

- Verify the command was run against the current build.
- Remove usernames, tokens, private repository names, and unrelated windows.
- Keep the command, result, and recovery step visible.
- Do not paste or reconstruct terminal output in an image editor.
- Run `centaur audit` against the fixture.

The final files belong in `docs/screenshots/`. Temporary fixtures and browser artifacts do not belong in Git.
