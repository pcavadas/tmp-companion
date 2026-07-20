---
name: verify
description: "The definition-of-done runbook for tmp-companion. Use before declaring ANY change done — it maps the change class (docs / frontend / backend / device-facing / leveling-math) to the checks that must be green, names the traps that produce a false-green result (stale :7600, fresh-worktree deps, online seeding), and states the standing rules for shipping an invariant, deferring a fix, or closing a user-reported bug. Advisory: `scripts/gates.sh` + the pre-push/PreToolUse hooks are what actually block a red push or PR — this skill is the checklist a zero-context session follows to get there without re-deriving it."
---

# /verify — definition of done

This is a runbook, not the enforcement. **`scripts/gates.sh` + `.husky/pre-push` +
`.claude/settings.json`'s `PreToolUse` hooks (`scripts/claude-hooks/{block-bypass,gate-pr}.sh`)
are what actually block a red push or a `gh pr create|merge`** — a red tree can't leave the
machine. This skill exists so any session (fresh context, no memory of this repo's traps) runs
the right checks in the right order and reports real evidence, not a bare "looks done."

## 1. Pick the change class, run its gates

**Fastest path: just run `/bin/bash scripts/gates.sh`.** It detects the scope from the diff
(vs the `origin/main` merge-base + untracked files) — docs-only → no gates; `src/`/`e2e/` →
lint + typecheck + test + format; `src-tauri/` → clippy + fmt + `cargo test --lib`; anything
device/e2e-relevant → + offline `bun run e2e` — and writes a tree-hash green stamp on a full
pass so a repeat check (e.g. after a docs-only follow-up commit) is instant. Don't re-derive
that scope table here; read `scripts/gates.sh`'s own header comment if you need the exact
mapping — this skill would drift from it.

What `gates.sh` **cannot** do for you — attended, hardware-gated, layered on top of a green
`gates.sh` (each row is cumulative):

| Change class                                                                            | Escalate to                                                                                                                                       |
| --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------- |
| Leveling-math / device-behavior change (solver, capture model, clamp/idempotency logic) | attended **online**: `scripts/e2e.sh online`, then `scripts/gates.sh --record-online`                                                             |
| Release-risk change to the solve/save/idempotency path                                  | + `scripts/e2e.sh soak <N>` (N ≥ 5) — the attended repeat lane for drift/engage-drop/stochastic device-state bugs a single online run won't catch |

## 2. Enforcement reality (why "looks green" isn't optional to prove)

- `scripts/gates.sh --check` gates `.husky/pre-push` — a stale/missing stamp re-runs the gates;
  a red gate aborts the push with the failing gate's own output.
- `scripts/claude-hooks/gate-pr.sh` (a `PreToolUse(Bash)` hook) blocks `gh pr create`/`gh pr merge`
  on a stale/missing green stamp, and additionally requires a fresh **online** stamp
  (`--check-online`) when the diff touches a device-facing path (`leveller.rs`/`footswitch.rs`/
  `session.rs`/`audio.rs`/`commands/level_*`/`commands/doctor.rs` — `gate-pr.sh`'s `device_re`
  is the authoritative list; keep this line in sync with it).
- `scripts/claude-hooks/block-bypass.sh` blocks `--no-verify`/`HUSKY=0`/`core.hooksPath` on any
  `git commit`/`git push` — there is no sanctioned bypass; fix the red gate instead.
- CI (`ci.yml`) stays the remote authority; these are the local/agent layer that keeps a red tree
  from ever reaching CI in the first place.

## 3. Traps that produce a false result

- **Stale bridge server = false-green OR false-online** (CLAUDE.md's "stale-fake-online trap").
  `scripts/e2e.sh` kills the port before every run, but a direct `bunx playwright test` invocation
  can hit a leftover WRONG-mode server via `reuseExistingServer: true`. Kill the REAL port first,
  always — in a worktree that's the per-worktree DERIVED port (`TMP_E2E_PORT`, offset off 7600
  since the port-isolation PR), not necessarily 7600; check `$TMP_E2E_PORT` or the e2e.sh log
  line before killing a hardcoded port.
- **Fresh worktree needs deps before checks, not just before dev.** `bun install` (node_modules
  is gitignored) before `bunx tsc --noEmit`/`bun run test`; `bun run build` (or stub
  `dist/index.html`) before any `cargo` gate — `generate_context!` panics without `./dist`.
- **Online false-green tell:** confirm the server log prints `seeded snapshot from the real
device` (or `/health` reports `online: true`) before trusting a pass — a stale offline server
  reused under `TMP_E2E_ONLINE=1` looks identical until you check.
- **Never `list_my_presets_strict` in a seed/sweep/write-path list read** — see CLAUDE.md's HID
  open-lockout note for why (tolerant reads are correct there; strict is snapshot/monitor-only).
- **A soak/online run needs the unit rested and Pro Control closed** — same preconditions as any
  online `e2e.sh` invocation; a handshake failure reports the "close Pro Control" hint.

## 4. Standing rules

1. **An explicitly stated invariant ships WITH its executable gate in the same PR.** "The app
   must do X" without a spec/test asserting X is not done — PR #74 shipped "2 consecutive
   leveling runs must produce the same result" as a requirement with no gate anywhere asserting
   it, and the requirement quietly broke in production before this harness caught it
   (`e2e/specs/level-rerun.spec.ts` is that gate now).
2. **A deferred fix ships WITH a tracking marker + an expected-fail note**, naming the limit
   inline at the skip site (e.g. `test.skip(..., "harness limit: needs a field-8 read model")`).
   Expected-fail annotations are reserved for harness-internal infrastructure limits — **never**
   for a product bug (a product bug is a hard, currently-red assert that blocks merge until fixed).
3. **Every user-reported bug gets a `notes/user-journeys.md` bug→gate registry row + a spec/test
   before or with its fix** — not necessarily reproducing the user's exact preset/steps, but every
   identified bug class gets a non-regression gate.
4. **Evidence over assertion.** A completion report pastes the actual check output (the gate
   name + pass/fail, the online seeded-marker line, the soak ledger) — never a bare "tests pass."

See `notes/user-journeys.md` for the journey-coverage map + the bug→gate registry this rule
enforces, and `notes/e2e-test-plan.md` for the full per-tab scenario inventory.
