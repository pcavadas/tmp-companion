@AGENTS.md

# Claude Code specifics

[`AGENTS.md`](AGENTS.md) is the canonical guide for every agent; this file adds only what Claude Code loads on its own.

## Where the rules live

Path-scoped rules load only once Claude reads a file their `paths:` glob matches, so anything that must hold while running a command lives in `AGENTS.md` instead.

- [`.claude/rules/danger.md`](.claude/rules/danger.md) loads always and is re-injected after compaction: the device danger rules, each linked to its `notes/device/` section.
- [`device-session.md`](.claude/rules/device-session.md): `session/`, `hid`, the monitor and the device gate → `notes/device/session.md`.
- [`device-presets.md`](.claude/rules/device-presets.md): copy, in-place replace, slot and backup code → `notes/device/presets.md`, `notes/copy.md`.
- [`device-scenes.md`](.claude/rules/device-scenes.md): the leveller, scene jobs and footswitches → `notes/device/scenes.md`, `notes/device/reamp-audio.md`, `notes/leveling.md`.
- [`leveling-dsp.md`](.claude/rules/leveling-dsp.md): measurement, leveling and Doctor code.
- [`rust-backend.md`](.claude/rules/rust-backend.md): any Rust file.
- [`frontend.md`](.claude/rules/frontend.md), [`theme-ds.md`](.claude/rules/theme-ds.md), [`models-catalog.md`](.claude/rules/models-catalog.md): `src/`, the token layer and UI primitives, the catalog and block art.
- [`e2e.md`](.claude/rules/e2e.md): the Playwright harness, its specs and `scripts/e2e.sh`.
- [`shell-scripts.md`](.claude/rules/shell-scripts.md): any `*.sh`.
- [`docs-writing.md`](.claude/rules/docs-writing.md): any markdown file.
- [`docs-site.md`](.claude/rules/docs-site.md): the GitHub Pages site under `docs/`.
- [`app-icon.md`](.claude/rules/app-icon.md): `src-tauri/icons/`.

## Skills

Skills under `.claude/skills/` load on a task match; reach for them by task.

- `verify`: before declaring any change done (the per-change-class runbook).
- `tmp-companion-protocol`: wire encoding, framing and the `.preset` codec.
- `tmp-companion-data-model`: what a preset, scene, block or footswitch means in product terms, from the Owner's Manual.
- `tmp-companion-catalog`: the model catalog, block art and the CPU-cost table under `src/models/`.
- `tmp-companion-frontend`: building or changing a view, a DS component or a frontend test.
- `coderabbit`: anything touching a CodeRabbit review; read it before posting any `@coderabbitai` command.
- `drive-native-window`: only when a task needs the literal native window; use the headless e2e harness first.

## Hooks

`.claude/settings.json` registers two `PreToolUse(Bash)` hooks from `scripts/claude-hooks/`.

- `block-bypass.sh` refuses a `git commit` or `git push` that disables the git hooks (`--no-verify`, `HUSKY=0`, a `core.hooksPath` override).
- `gate-pr.sh` refuses to open or merge a PR without a green `scripts/gates.sh --check` stamp for the worktree.
  It matches the command TEXT and its `gh` is unanchored, so merely quoting the words `pr create` inside an `echo` or heredoc trips it; the block is not evidence the hook is broken.
