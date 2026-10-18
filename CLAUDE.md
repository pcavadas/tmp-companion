@AGENTS.md

# Claude Code specifics

[`AGENTS.md`](AGENTS.md) is the canonical guide for every agent; this file adds only what Claude Code does on its own.

## Rules and skills

- Path-scoped rules under `.claude/rules/` load only once Claude reads a file their `paths:` glob matches, so anything that must hold while running a command lives in `AGENTS.md` instead.
- [`.claude/rules/danger.md`](.claude/rules/danger.md) has no `paths:`, so it loads always and is re-injected after compaction.
- Skills under `.claude/skills/` trigger on a task match against their `description`.
- The glob → rule and task → skill map is [`AGENTS.md#read-by-path`](AGENTS.md#read-by-path).

## Hooks

`.claude/settings.json` registers two `PreToolUse(Bash)` hooks from `scripts/claude-hooks/`.
Both match the command TEXT, so a command that merely quotes the words they look for (in an `echo` or heredoc) trips them; the block is not evidence the hook is broken.

- `block-bypass.sh` refuses a `git commit` or `git push` that disables the git hooks (`--no-verify`, `HUSKY=0`, a `core.hooksPath` override).
- `gate-pr.sh` refuses to open or merge a PR without a green `scripts/gates.sh --check` stamp for the worktree; its `gh` is unanchored, so quoting `pr create` is enough.
