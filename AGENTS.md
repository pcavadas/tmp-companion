# tmp-companion agent guide

TMP Companion is a Tauri 2 desktop app (Rust backend, React/TypeScript frontend) that drives a Fender Tone Master Pro over USB in re-amp mode, with no guitar plugged in.
It plays a synthetic sample through a preset's DSP chain, measures the USB-Out loudness and saves the `presetLevel` that hits a LUFS target.
It ships on macOS, with an alpha Linux `.deb`/`.rpm` and an unsigned Windows `setup.exe` ([`notes/releasing.md`](notes/releasing.md)).

**UX north-star:** fewest possible clicks per action, for users who are often not comfortable with computers; every feature ships whole.
The app is **click-only**: no keyboard shortcuts, no command palette, no dialog Escape-to-close.
The one carve-out is Enter/Escape inside a focused text input, committing or cancelling that field's own edit; in the inline renames that commit on blur, Escape is the only cancel path.

**Where to read:** [`notes/README.md`](notes/README.md) is the doc index; [`notes/architecture.md`](notes/architecture.md) is the architecture map.
Module headers (`//!` in Rust, `//` in TypeScript) are the authority for their module.

## Commands

```bash
bun install  # first-time deps
bun run tauri dev  # launch the app
bun run test  # frontend Vitest
bunx tsc --noEmit  # frontend typecheck
bun run lint  # strict eslint (--max-warnings 0)
bun run format  # prettier --write
bun run build  # vite production bundle
bun run e2e  # offline Playwright against the SimDevice
bash scripts/gates.sh  # everything the pre-push hook runs

cd src-tauri && cargo test --lib  # Rust unit tests
cargo clippy --all-targets  # lint
cargo fmt  # nothing else formats Rust
cargo run --bin probe  # headless device checks (unit plugged in, Pro Control closed)
```

**Definition of done:** [`.claude/skills/verify/SKILL.md`](.claude/skills/verify/SKILL.md) maps each change class to its checks; `scripts/gates.sh` and the git hooks enforce them.
**Every user-reported bug becomes a gate:** each bug class gets a non-regression spec plus fixture, landing with or before the fix.
Never characterize or expected-fail a product bug to keep CI green.

## Traps that fire when you run something

- **`git push` runs the gates synchronously.**
  The pre-push hook is `gates.sh --check || gates.sh`; the one green stamp is keyed to a content hash of the whole worktree (gitignored paths excluded).
  Any non-prose edit orphans it, so run `bash scripts/gates.sh` first, in the background (it outruns a 10-minute tool timeout).
  A tool timeout (exit 143) on `git push` is the gate being slow, not a failed push: check `git ls-remote origin <branch>` before retrying.
- **PRs run the offline gates only.**
  A device-facing PR also waits on the required `remote-lane` status (`.github/workflows/remote-lane.yml`, its online specs on a remote unit, 10–45 min); other PRs get "not needed" at once.
  The full online suite runs at release, unit plugged in by hand: `scripts/pre-release.sh` runs `scripts/e2e.sh online release`.
- **The online stamp uses the same worktree hash**, so land every non-prose edit before an online run.
  Prose (`*.md`, `docs/`) is excluded from the key, except `e2e/fixtures/COVERAGE.md`, which `fixture_gates` parses.
  Stamps live under `$(git rev-parse --git-dir)/tmp-gates`: a sibling worktree's never counts, and removing a worktree destroys them.
  A passing online run merges its specs into the online stamp; `scripts/gates.sh --required-online [--since <ref>]` names the specs a diff needs.
  Rebasing onto a peer's non-prose commits orphans both stamps: rebase first, then run.
- **A stale e2e server fakes green or fakes online.**
  Playwright's `reuseExistingServer` adopts an orphaned `e2e_server`, so an "online" run can pass against a leftover SimDevice.
  Confirm `seeded snapshot from the real device` in the server log (`$SERVER_LOG`), never in the runner's stdout.
  Let `scripts/e2e.sh` clear ports (`kill_port_range`, band layout in its header).
  Never SIGKILL an online lane; SIGINT runs the recovery trap.
- **`online` is a mode subcommand** (`scripts/e2e.sh online [spec…]`), not an env var: `TMP_E2E_ONLINE=1` alone seeds the unit and then finds no tests.
  A pipeline's exit status is its last stage, so `… | tail` reports tail's success, not the run's.
- **A leveling run must end with re-amp OFF.**
  A dropped OFF strands the unit input-muted; recover with `cargo run --bin probe -- --reamp-off` ([stuck re-amp](notes/device/reamp-audio.md#stuck-re-amp)).
- **A preset with no blocks re-amps as clean passthrough**, which is not a wedged device.
  Count the nodes carrying `dspUnitParameters` with `probe --slot-json <1-based slot>` first ([→](notes/device/reamp-audio.md#presets-with-no-blocks)).
- **A probe binary must run from `<worktree>/src-tauri`**: `probe_stimulus_path` resolves `resources/samples/<id>.wav` against the current directory.
- **commitlint rejects any capitalized first word** after `type(scope):`, acronyms included; check with `echo "<subject>" | bunx commitlint`.
- **Fresh clone or worktree:** run `bun install` before `bunx tsc --noEmit` or `bun run test`, or every import of `react` fails.
- **Worktrees share one stash stack.**
  Never run a bare `git stash`; use `git stash push -u -m "<tag>"` and apply by SHA.

## Device danger summary

Read [`.claude/rules/danger.md`](.claude/rules/danger.md) before touching the device; the facts behind it are in [`notes/device/`](notes/device/).

- A save is permanent and a leveling run saves automatically; the Pro Control backup is the only way back ([saving](notes/device/presets.md#saving)).
- Device `userSlot` = list index + 1; confirm a mapping with a read and guard in the mutation's own address space ([slot addressing](notes/device/presets.md#slot-addressing)).
- A same-slot `loadPreset` over a clean working copy keeps it; discard edits only with `leveller::restore_saved_preset`, and never load before `save_deferred_scene_writes` ([→](notes/device/presets.md#same-slot-load-short-circuit)).
- A body rejected at load becomes a silent empty preset that keeps its name; prove it with `replace_inplace::load_and_prove` before any save ([rejected at load](notes/device/presets.md#rejected-at-load)).
- An `insertNode` with a bad anchor or over the CPU budget aborts the device server; run `blockcaps::check_op` first and never re-send blind ([insert abort](notes/device/presets.md#insert-abort)).
- After a `presetLevel` save with base active, call `leveller::reload_after_level_save`, or the next base recall plays the old level ([presetLevel on recall](notes/device/scenes.md#presetlevel-on-recall)).
- A write with no preceding `loadScene` lands in whatever scene is current ([scene context](notes/device/scenes.md#scene-context)).
- 750 ms of host OUT silence lapses the session and drops what follows; keep idle gaps ≤300 ms ([session lapse](notes/device/session.md#session-lapse)).
- A failed engage reads as the stationary output floor, never as a valid capture ([silent captures](notes/device/reamp-audio.md#silent-captures)).
- Every re-amp run ends with a guaranteed OFF on a fresh connection (`leveller::ReampRunGuard`).
- Serialize every device operation (`device_gate::DEVICE_OP_LOCK`); the HID seize is exclusive, so Pro Control must be closed ([exclusive seize](notes/device/session.md#exclusive-seize)).

## Invariants no single module can state

- **`blockcaps.rs` is the sole enforcement of the five block rules** (the CPU budget and four count caps).
  The device enforces none of the count caps, so any new apply path must call it.
- **`SCRATCH_SLOTS` (`slot.rs`) is the one declaration of the scratch zone.**
  It is not a universal guard: `probe_import_file` and the in-place replace path guard an empty target slot instead, so an import can land at any empty user slot.
- **Block art is original SVG through the BlockArt engine**, never an `<img>` of a vendor PNG (Fender IP).
- **`ui/primitives.tsx` is deliberately not split**: `Modal` renders `Button`, so a split reintroduces a circular import; `ui/Dialog.tsx` stays button-free.
- **`blockArt.ts` must not import `catalog.ts`**: that closes a module-init cycle (a TDZ crash), and `no-restricted-imports` in `eslint.config.js` enforces it.
- **Shared seams:** `backup::xor_jld` + `library::decode_preset_bytes` (`.preset` codec), `replace_inplace::replace_inplace_core`, `audiograph::for_each_node` / `for_each_node_mut` + `node_id`.
- **Connection is fully automatic**: there are no Connect or Disconnect buttons.
- **Editing `index.html` reloads the whole webview** in `tauri dev` (not HMR), re-running connect-on-mount.

## Leveling model

`presetLevel` is a linear amplitude control: `captured_LUFS = 20·log10(presetLevel) + C`.
`leveller::level_preset` measures once at a reference level, solves `C` (the preset's maximum reachable loudness) and sets the exact value; a louder target clamps ([`notes/leveling.md`](notes/leveling.md)).

## Pull requests and review

- **CodeRabbit reviews progressively on its own: post no command on a reviewed PR.**
  Pushing fix commits or replying to threads is enough; never resolve a thread by hand (details: [`.claude/skills/coderabbit/SKILL.md`](.claude/skills/coderabbit/SKILL.md)).
- **Auto-merge arms only for `main`-targeted PRs** (`.github/workflows/arm-auto-merge.yml`).
  The "protect main" ruleset (one approval, `require_last_push_approval`, required checks, no bypass actors) covers only `main`, so a stacked PR merges only by hand.
  Never push while an approval is pending or landed (the push dismisses it); batch fix commits before the review lands.
  `delete_branch_on_merge` stays on (GitHub retargets a stacked child when its parent branch is deleted); never delete a stacked parent's branch client-side.
  The workflow arms with a GitHub App token, never `GITHUB_TOKEN`, so the merge push still triggers `ci.yml`.
- Releases start by hand (`release.yml` is `workflow_dispatch` only); a merge never starts one ([`notes/releasing.md`](notes/releasing.md)).
