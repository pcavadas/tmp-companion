<!-- Title must be a Conventional Commit: feat: / fix: / docs: / chore: / refactor: … (commitlint gates this). -->

## What & why

<!-- What changed and the reason. Link the issue: Closes #123 -->

## How it was tested

<!-- Delete rows that don't apply. Hardware/leveling changes need a real-device note. -->

- [ ] `bun run lint` / `bunx tsc --noEmit` / `bun run test`
- [ ] `cargo test --lib` / `cargo clippy --all-targets`
- [ ] Offline e2e (`bun run e2e`)
- [ ] Verified on a real Tone Master Pro (firmware: )

## Checklist

- [ ] Conventional-commit title
- [ ] Only touched-file formatting (no repo-wide `cargo fmt` / prettier reflow)
- [ ] No lint escape hatches in `src/` (`any` / `@ts-ignore` / non-null `!` / `eslint-disable`)
- [ ] No new dependency, or it clears the health + 7-day cooldown bars (see CONTRIBUTING.md)
