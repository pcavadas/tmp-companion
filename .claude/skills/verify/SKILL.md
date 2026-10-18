---
name: verify
description: "The definition-of-done runbook for tmp-companion. Use before declaring ANY change done — it maps the change class (docs / frontend / backend / device-facing / leveling-math) to the checks that must be green, names the traps that produce a false-green result, and states the standing rules for shipping an invariant, deferring a fix, or closing a user-reported bug. Advisory: `scripts/gates.sh` plus the pre-push and PreToolUse hooks are what actually block a red push or PR — this skill is the checklist a zero-context session follows to get there."
---

# /verify — definition of done

This is a runbook, not the enforcement.
**`scripts/gates.sh` + `.husky/pre-push` + `.claude/settings.json`'s `PreToolUse` hooks (`scripts/claude-hooks/{block-bypass,gate-pr}.sh`) are what actually block a red push or a `gh pr create|merge`** — a red tree can't leave the machine.
This skill exists so any session (fresh context, no memory of this repo's traps) runs the right checks in the right order and reports real evidence, not a bare "looks done."

## 1. Pick the change class, run its gates

**Fastest path: just run `/bin/bash scripts/gates.sh`.**
It detects the scope from the diff (vs the `origin/main` merge-base + untracked files) — every mode runs the docs link gate (`scripts/check-docs.sh`, also run by the pre-commit hook and CI), and docs-only adds nothing more; `src/`/`e2e/` → lint + typecheck + test + format; `src-tauri/` → clippy + fmt + `cargo test --lib`; anything device/e2e-relevant → + offline `bun run e2e` — and writes a tree-hash green stamp on a full pass so a repeat check (e.g. after a docs-only follow-up commit) is instant.
Don't re-derive that scope table here; read `scripts/gates.sh`'s own header comment if you need the exact mapping — this skill would drift from it.

What `gates.sh` **cannot** do for you — attended, hardware-gated, layered on top of a green `gates.sh` (each row is cumulative):

- **Leveling-math or device-behaviour change** (solver, capture model, clamp or idempotency logic): on the PR, the required `remote-lane` status (`.github/workflows/remote-lane.yml`) runs the online specs the diff maps to against a remote device (10–45 min) and posts "not needed" at once for any other PR.
  The full online suite runs at release on the unit through `scripts/pre-release.sh` (`scripts/e2e.sh online release` over every device-facing change since the last release tag).
  To check a risky change on the unit before then, `scripts/e2e.sh online auto` runs exactly the specs `scripts/gates.sh --required-online` names for this diff; a passing run merges them into the tree's online stamp, which nothing on a PR checks.
- **Release-risk change to the solve, save or idempotency path:** add `scripts/e2e.sh soak <N>` (N ≥ 5), the attended repeat lane for drift, engage drops and stochastic device-state bugs a single online run won't catch.

## 2. Enforcement reality (why "looks green" isn't optional to prove)

- `scripts/gates.sh --check` gates `.husky/pre-push` — a stale/missing stamp re-runs the gates; a red gate aborts the push with the failing gate's own output.
- `scripts/claude-hooks/gate-pr.sh` (a `PreToolUse(Bash)` hook) blocks `gh pr create`/`gh pr merge` on a stale/missing green stamp.
  It requires no online stamp: PRs run the offline gates, a device-facing PR also waits on the required `remote-lane` status, and the full online suite runs at release (`scripts/pre-release.sh`; the class map is `scripts/gate-map.tsv`, the per-class specs `scripts/gate-policy.tsv`, and a Rust gate fails on any unmapped backend file).
- `scripts/claude-hooks/block-bypass.sh` blocks `--no-verify`/`HUSKY=0`/`core.hooksPath` on any `git commit`/`git push` — there is no sanctioned bypass; fix the red gate instead.
- CI (`ci.yml`) stays the remote authority; these are the local/agent layer that keeps a red tree from ever reaching CI in the first place.

## 3. Traps that produce a false result

- **Stale bridge server = false-green OR false-online** — run through `scripts/e2e.sh`, which sweeps the worktree's port range; never a direct `bunx playwright test` (`AGENTS.md`, "Traps that fire when you run something").
- **Fresh worktree needs deps before checks** — `bun install` before any frontend check (`AGENTS.md`, "Fresh clone or worktree"); `build.rs` stubs `dist/` for bare `cargo` checks.
- **Online false-green tell:** confirm the server log prints `seeded snapshot from the real device` (or `/health` reports `online: true` with `remote` false — a remote server reports `online: true` too) before trusting a pass.
  A stale offline server reused under `TMP_E2E_ONLINE=1` looks identical until you check.
  A green spec run is not evidence by itself: a Playwright expectation is self-referential, so one retargeted to the observed number passes on the regression it should catch.
  The independent read is the ffmpeg `level-validate` pass, and its receipt is the runner logging `external validation PASSED` followed by the stamp.
  Read the runner's own exit status, never a pipeline's — `| tail` reports tail's status and buffers a long run into silence.
- **A restored file keeping its original mtime does not rebuild.**
  `mv`-ing a source version back to A/B two builds leaves cargo's freshness check satisfied, so the "proof" run reuses the previous binary.
  Confirm a compile line appeared, or `touch` the file after any restore.
- **A Channel-streaming command called over raw HTTP hides its per-row outcomes** — they travel only the `"__CHANNEL__:N"` stream (the page gets them through the UI bridge; a raw invoke has no Channel), so `ok: true` can come back with rows silently missing; and `level_scenes_apply_batched` yields `trade: null` unless `baseAnchor` is passed.
- **Never `list_my_presets_strict` in a seed/sweep/write-path list read** — see `.claude/rules/e2e.md`'s "Seeding and list reads" for why (strict is snapshot/monitor-only).
- **A soak/online run needs the unit plugged in and Pro Control closed** — same preconditions as any online `e2e.sh` invocation; a handshake failure reports the "close Pro Control" hint.
- **A docs change runs the doc gates, not the code gates.**
  `bash scripts/check-docs.sh` checks the markdown and doc-comment rules (links and anchors included); run it on any `*.md` or doc-comment change.
  Nothing checks prose for a stray non-ASCII character, so grep a docs diff for CJK or Cyrillic before calling it done.

## 4. Standing rules

1. **An explicitly stated invariant ships WITH its executable gate in the same PR.**
   "The app must do X" without a spec/test asserting X is not done — an unasserted requirement breaks quietly.
   Example: leveling idempotency ("2 consecutive runs produce the same result") is gated by the idempotency tests in `e2e/specs/level.spec.ts` (offline) and `e2e/specs/level.online.spec.ts` (online).
2. **A deferred fix ships WITH a tracking marker + an expected-fail note**, naming the limit inline at the skip site (e.g. `test.skip(..., "harness limit: needs a field-8 read model")`).
   Expected-fail annotations are reserved for harness-internal infrastructure limits — **never** for a product bug (a product bug is a hard, currently-red assert that blocks merge until fixed).
3. **Every user-reported bug gets a spec or test before or with its fix** — not necessarily reproducing the user's exact preset or steps, but every identified bug class gets a non-regression gate.
4. **Evidence over assertion.**
   A completion report pastes the actual check output (the gate name + pass/fail, the online seeded-marker line, the soak ledger) — never a bare "tests pass." The same bar applies to claims about EXTERNAL state (a PR "merged", a review-history pattern): re-query the live source immediately before asserting it, never from memory of an earlier check.
5. **A fix is not done until the module is swept for the same defect shape, and every part of a multi-part finding has landed.**
   A guard added at the cited line usually has an un-cited twin nearby — the same missing check in a sibling branch or fall-through.
   And a multi-ask finding can be half-applied and still reported fixed.
   Grep the module for the shape, and re-read the finding's full body, before calling it done.
   This is the author-side counterpart of `.coderabbit.yaml`'s `Behavioral parity` pre-merge check.
6. **A check's data must be able to exercise the failure it verifies.**
   A pagination check run against fewer rows than the page size, an edge-case guard tested only on the happy path, or a race check run single-threaded all pass for reasons unrelated to correctness.
   Confirm the fixture crosses the threshold that would trip the bug before trusting green.
   For a leveling repro that means the stimulus production actually resolves — the profile's captured DI, not the bundled synthetic sample: reachable ceilings differ by more than 10 dB between them, so a clamp cannot occur under the bundled one.

7. **A verifier covers only what it actually reads — name the address space AND the fields, then prove it can fail.**
   A read-back of the base graph passes while the write landed in a scene overlay; a drift-lock over a typed struct stays green on the fields the struct omits.
   Before trusting any read-back or drift-lock, state the address space it reads (base vs scene overlay, list-index vs 1-based device slot), the data representation it reads (typed struct or raw JSON), and the fields it covers — then check that a deliberate mismatch actually turns it red.
