# tmp-companion

A **macOS-only** Tauri 2 desktop app (Rust backend + React/TypeScript frontend) that auto-levels real Fender Tone Master Pro presets to a LUFS target by driving the device over USB in **re-amp mode** — no guitar plugged in. It plays a synthetic guitar-like sample through a preset's full DSP chain, captures the processed USB-Out, measures loudness, computes the `presetLevel` that hits the target, and (opt-in) saves it back. It renders its **own** UI: this is a USB host controller that draws its own interface, not a plugin.

**UX north-star:** fewest possible clicks per action. The audience is thousands of users, many not comfortable with computers. Every feature ships whole in v1 — no subsetting. The app is **click-only**: no keyboard shortcuts, no command palette.

**Architecture map: [`notes/overview.md`](notes/overview.md).** There is deliberately no module tree here — 88 of 93 backend files carry a `//!` header and 175 of 198 frontend files carry a `//` header, and those are the authority. Feature docs live in [`notes/`](notes/).

## Where the rules live

| File | Loads | Carries |
| --- | --- | --- |
| [`.claude/rules/danger.md`](.claude/rules/danger.md) | always | **Read this before touching the device.** Data loss, device wedging, machine crashes. |
| `.claude/rules/*.md` | when you edit matching files | Path-scoped conventions — frontend, theme/DS, models/catalog, Rust backend, leveling/DSP, e2e, shell |
| [`notes/gotchas.md`](notes/gotchas.md) | on reference | The hardware evidence behind every rule |
| `.claude/skills/` | on task match | Product data model, wire protocol, catalog contract, frontend how-to, `/verify`, CodeRabbit, native-window driving |

Path-scoped rules only load once Claude **reads** a matching file, so anything that must hold while *running* a command lives below instead.

## Traps that fire when you run something

- **`git push` RUNS the gates synchronously.** The pre-push hook is `gates.sh --check || gates.sh`, and the green stamp is keyed to a CONTENT hash of the whole worktree — any edit orphans it, and only ONE stamp is kept. So **every** push after an edit pays the full multi-minute run inside the push. Run `bash scripts/gates.sh` from the repo root yourself first. **A tool-level timeout (143) on `git push` is that gate being slow, NOT a failed push** — check `git ls-remote origin <branch>` before retrying.
- **Stale e2e server = false-green or fake-online.** An orphaned `e2e_server` plus Playwright's `reuseExistingServer` makes a `TMP_E2E_ONLINE=1` run silently REUSE the old server. If that one was offline (SimDevice), the "online" suite passes green **without ever touching the device**. Confirm the log prints `ONLINE — seeded snapshot from the real device`. **Killing one port is no longer enough:** offline runs N servers on `TMP_E2E_PORT + 0..N-1`, so a base-port-only kill leaves the other workers' servers alive for `reuseExistingServer` to adopt. Let `scripts/e2e.sh` do it — its `kill_port_range` sweeps the worktree's whole 8-port stride and kills only `e2e_server` processes, so it can't take out a concurrent run in a sibling worktree. The ports are per-worktree derived, not always 7600.
- **A leveling run must end with re-amp OFF.** A dropped OFF strands the unit input-muted. Recovery: `cargo run --bin probe -- --reamp-off`.
- **commitlint rejects ANY capitalized first word** after `type(scope):` — including an acronym. `docs(device): HW-validated the …` reads correct and is rejected all the same. Lowercase the lead word. Check with `echo "<subject>" | bunx commitlint` instead of burning a commit attempt.
- **Fresh clone or worktree:** `cargo {test,clippy,build}` runs `tauri-build`, whose `generate_context!` panics if `./dist` is absent — which it is in a fresh worktree (dist is gitignored). Run `bun run build` first, or stub `dist/index.html`. Likewise run `bun install` before `bunx tsc --noEmit` / `bun run test`, or you get hundreds of phantom "Cannot find module 'react'" errors.
- **Worktrees share ONE stash stack.** A `git stash apply/pop` can restore an *older* version of a committed branch file, silently reverting it mid-session. Never bare `git stash` in a worktree — use `git stash push -u -m "<tag>"` and `apply` by SHA. NB `lint-staged` pushes and drops a transient stash on every commit; benign, but it rides the same stack, so don't mis-blame it in a pollution audit.

## Invariants no single module can state

- **`blockcaps.rs` is the SOLE enforcement of the 5 block-count caps.** The device audio engine does NOT reject an over-cap edit and cannot return a `presetError` for one — the cap code is client-side only. Any new apply path must call it, because nothing downstream will.
- **`SCRATCH_SLOTS` (`probe_api/mod.rs`) is the ONE declaration** every destructive or working-copy-writing probe guard checks. Widening the scratch zone is one edit, not four.
- **Block art is original SVG through the BlockArt engine — never an `<img>` photo tile.** The copyrighted vendor PNGs are not bundled and must not be reintroduced (Fender IP).
- **`ui/primitives.tsx` is deliberately NOT split** — `Modal` renders `Button`, so splitting reintroduces a circular import.
- **`blockArt.ts` must NOT import `catalog.ts`** — it closes a module-init cycle (a TDZ crash). Enforced by `no-restricted-imports` in `eslint.config.js`.

## Commands

```bash
bun install                                   # first-time deps
bun run tauri dev                             # launch the app
bun run test                                  # frontend Vitest
bunx tsc --noEmit                             # frontend typecheck
bun run lint                                  # strict eslint (--max-warnings 0)
bun run format                                # prettier --write
bun run build                                 # vite production bundle
bun run e2e                                   # offline Playwright (SimDevice, ~1.5 min)
bash scripts/gates.sh                         # everything the pre-push hook runs

cd src-tauri && cargo test --lib              # Rust unit tests
cargo clippy --all-targets                    # lint
cargo fmt                                     # nothing else formats Rust — on you
cargo run --bin probe                         # headless HW re-validation (device plugged in, Pro Control CLOSED)
```

**Definition of done:** the `/verify` skill is the per-change-class runbook; `scripts/gates.sh` plus the pre-push and PreToolUse hooks are what actually enforce it. `notes/user-journeys.md` tracks journey coverage and the bug→gate registry.

**Every user-reported bug becomes a gate.** Each bug class gets a non-regression spec plus fixture, landing with or before the fix. Never characterize or expected-fail a product bug to keep CI green.

## Rules with no other home

- **App icon (level-meter mark):** flat terracotta (`#d97757`) macOS-squircle tile, 3 white bottom-aligned level bars in a 6:11:8 height rhythm. [→ evidence](notes/gotchas.md#app-icon-level-meter-mark)
- **Marketing site** (`docs/index.html` + `.nojekyll` + `assets/`): GitHub Pages branch-deploy from `main` `/docs`. It is a PROJECT repo, so the URL is a `/tmp-companion/` **subpath** — all asset paths must be RELATIVE. [→ evidence](notes/gotchas.md#marketing-site-docsindexhtml--nojekyll--assets)
- **CodeRabbit: progressive review is automatic — post NO command on a reviewed PR.** Pushing fix commits or replying to threads is enough. [→ evidence](notes/gotchas.md#coderabbit-progressive-review-is-automatic--post-no-command-on-a-reviewed-pr)
- **Auto-merge arms ONLY for `main`-targeted PRs.** [→ evidence](notes/gotchas.md#auto-merge-arms-only-for-main-targeted-prs)
- **Connection is fully automatic** — there are no manual Connect/Disconnect buttons. [→ evidence](notes/gotchas.md#fully-automatic-connection)
- **Editing `index.html` triggers a FULL webview reload** (not HMR) in `tauri dev`, re-running connect-on-mount. Reload via the UI instead.
- **Shared seams** — the cross-module reuse contracts: `backup::xor_jld` (the `.preset` codec) + `library::decode_preset_bytes`; `crate::replace_inplace_core` (in-place re-import); `audiograph::for_each_node` / `for_each_node_mut` + `node_id` (the node-walk helpers the scan and edit modules share).

## How leveling works

`presetLevel` is a **linear amplitude** control: `captured_LUFS = 20·log10(presetLevel) + C`. So `leveller::level_preset` measures once at a reference level, solves `C`, and sets the exact value — over **three fresh connections** (load / measure / apply), forced by the re-amp rules in `danger.md`.

`C` is each preset's **max reachable** loudness. A louder target clamps, and the ceiling is preset- and model-specific — there is no hard `−20 LUFS` rule (a maxed 65 Twin reached `−14 LUFS` comfortably). For relative leveling, pick a target below the quietest preset's measured max.

Full model — scene/footswitch recipes, parallel-amp rebalance, Fletcher–Munson playback compensation, the dynamics-spread flag, and the outcome taxonomy: **[`notes/leveling.md`](notes/leveling.md)**.
