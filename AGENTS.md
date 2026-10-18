# tmp-companion agent guide

TMP Companion is a Tauri 2 desktop app (Rust backend, React/TypeScript frontend) that drives a Fender Tone Master Pro over USB in re-amp mode, with no guitar plugged in.
It plays a synthetic sample through a preset's DSP chain, measures the USB-Out loudness and saves the `presetLevel` that hits a LUFS target.
It ships on macOS, with an alpha Linux `.deb`/`.rpm` and an unsigned Windows `setup.exe` ([`notes/releasing.md`](notes/releasing.md)).

**UX north-star:** fewest possible clicks per action, for users who are often not comfortable with computers; every feature ships whole.
The app is **click-only**: no keyboard shortcuts, no command palette, no dialog Escape-to-close.
The one carve-out is Enter/Escape inside a focused text input, committing or cancelling that field's own edit; in the inline renames that commit on blur, Escape is the only cancel path.

**Where to read:** [`notes/README.md`](notes/README.md) is the doc index, [`notes/architecture.md`](notes/architecture.md) the architecture map, and [Read by path](#read-by-path) routes the rules and skills.
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

**Definition of done:** [`verify`](.claude/skills/verify/SKILL.md) maps each change class to its checks; `scripts/gates.sh` and the git hooks enforce them.
**Every user-reported bug becomes a gate:** a non-regression spec plus fixture per bug class, landing with or before the fix; never characterize or expected-fail a product bug to keep CI green.

## Traps that fire when you run something

- **`git push` runs the gates synchronously.**
  The pre-push hook is `gates.sh --check || gates.sh`; the one green stamp is keyed to a content hash of the worktree (gitignored paths excluded), so any non-prose edit orphans it.
  Run `bash scripts/gates.sh` first, in the background (it outruns a 10-minute timeout).
  A tool timeout (exit 143) on `git push` is the gate being slow: check `git ls-remote origin <branch>` before a retry.
- **PRs run the offline gates only.**
  A device-facing PR also waits on the required `remote-lane` status (10–45 min on a remote unit); the full online suite runs at release (`scripts/pre-release.sh`).
- **The online stamp uses the same worktree hash**, so land every non-prose edit before an online run.
  Prose (`*.md`, `docs/`) is outside the key, except `e2e/fixtures/COVERAGE.md`, which `fixture_gates` parses.
  Stamps live in `$(git rev-parse --git-dir)/tmp-gates`, per worktree: removing a worktree destroys them.
  `scripts/gates.sh --required-online [--since <ref>]` names the specs a diff needs; rebase onto a peer's non-prose commits before a run, never after.
- **A stale e2e server fakes green or fakes online.**
  `reuseExistingServer` adopts an orphaned `e2e_server`, so an "online" run can pass against a leftover SimDevice.
  Confirm `seeded snapshot from the real device` in the server log (`$SERVER_LOG`), never in the runner's stdout, and let `scripts/e2e.sh` clear the ports.
  Never SIGKILL an online lane; SIGINT runs the recovery trap.
- **`online` is a mode subcommand** (`scripts/e2e.sh online [spec…]`), not an env var: `TMP_E2E_ONLINE=1` alone seeds the unit and then finds no tests.
  A pipeline's exit status is its last stage, so `… | tail` reports tail's success, not the run's.
- **A leveling run must end with re-amp OFF.**
  A dropped OFF strands the unit input-muted; recover with `cargo run --bin probe -- --reamp-off` ([stuck re-amp](notes/device/reamp-audio.md#stuck-re-amp)).
- **A preset with no blocks re-amps as clean passthrough**, not a wedged device: check it with `probe --slot-json <1-based slot>` ([→](notes/device/reamp-audio.md#presets-with-no-blocks)).
- **A probe binary must run from `<worktree>/src-tauri`**: `probe_stimulus_path` resolves `resources/samples/<id>.wav` against the current directory.
- **commitlint rejects any capitalized first word** after `type(scope):`, acronyms included; check with `echo "<subject>" | bunx commitlint`.
- **Fresh clone or worktree:** run `bun install` before `bunx tsc --noEmit` or `bun run test`.
- **Worktrees share one stash stack.**
  Never run a bare `git stash`; use `git stash push -u -m "<tag>"` and apply by SHA.
  lint-staged pushes and drops a transient stash on every commit: benign, so don't mis-blame it.

## Device danger summary

Read [`.claude/rules/danger.md`](.claude/rules/danger.md) before touching the device; the facts behind it are in [`notes/device/`](notes/device/).
It covers saves (permanent: the Pro Control backup is the only way back), slot addressing (`userSlot` = list index + 1), the same-slot load, bodies rejected at load, the insert abort, `presetLevel` on recall, scene context, the 750 ms session lapse, silent captures and the guaranteed re-amp OFF.
Serialize every device operation through `device_gate::DEVICE_OP_LOCK`; the HID seize is exclusive, so Pro Control must be closed.

## Invariants no single module can state

- **`blockcaps.rs` is the sole enforcement of the five block rules** (the CPU budget and four count caps): the device enforces none of the count caps, so any new apply path must call it.
- **`SCRATCH_SLOTS` (`slot.rs`) is the one declaration of the scratch zone**, not a universal guard: `probe_import_file` and the in-place replace path guard an empty target slot instead, so an import can land at any empty user slot.
- **Block art is original SVG through the BlockArt engine**, never an `<img>` of a vendor PNG (Fender IP).
- **`ui/primitives.tsx` is deliberately not split**: `Modal` renders `Button`, so a split reintroduces a circular import; `ui/Dialog.tsx` stays button-free.
- **`blockArt.ts` must not import `catalog.ts`**: that closes a module-init cycle (a TDZ crash); `eslint.config.js` enforces it.
- **Connection is fully automatic** (no Connect or Disconnect buttons), and editing `index.html` reloads the whole webview in `tauri dev`, re-running connect-on-mount.

## Leveling model

`presetLevel` is a linear amplitude control: `captured_LUFS = 20·log10(presetLevel) + C`.
`leveller::level_preset` measures once, solves `C` (the preset's maximum reachable loudness) and sets the exact value; a louder target clamps ([`notes/leveling.md`](notes/leveling.md)).

## Pull requests and review

- **CodeRabbit reviews progressively on its own: post no command on a reviewed PR.**
  Pushing fix commits or replying to threads is enough; never resolve a thread by hand.
  Auto-merge, the `main` ruleset and stacked PRs: the [`coderabbit` skill](.claude/skills/coderabbit/SKILL.md).
- Releases start by hand (`release.yml` is `workflow_dispatch` only); a merge never starts one ([`notes/releasing.md`](notes/releasing.md)).

## Read by path

Claude Code loads these on its own; other agents read the file a path or task routes to.
Paths are under `src-tauri/src/` unless they name another root.

- Before touching the device → [`danger.md`](.claude/rules/danger.md)
- `src-tauri/**/*.rs` → [`rust-backend.md`](.claude/rules/rust-backend.md)
- `session/**`, `hid.rs`, `hid/**`, `monitor.rs`, `device_gate.rs` → [`device-session.md`](.claude/rules/device-session.md)
- `commands/{copy_apply,bulk_replace,presets}.rs`, `replace_inplace.rs`, `slot*.rs`, `backup*.rs`, `blockcaps.rs`, `audiograph.rs`, `bulkrun.rs`, `preset_io.rs`, `library.rs`, `paramedit.rs` → [`device-presets.md`](.claude/rules/device-presets.md)
- `leveller/**`, `scene_jobs*.rs`, `footswitch.rs`, `commands/level_*.rs`, `session/reamp.rs` → [`device-scenes.md`](.claude/rules/device-scenes.md)
- `leveller/**`, `lufs.rs`, `audio.rs`, `psd.rs`, `doctor*.rs`, `commands/level_*.rs`, `commands/doctor*.rs` → [`leveling-dsp.md`](.claude/rules/leveling-dsp.md)
- `commands/{songs,setlists}.rs`, `song_io.rs`, `setlist_io.rs`, `session/songs.rs`, `probe_api/{songs,setlists}.rs`, `src/views/songs/**` → [`songs.md`](.claude/rules/songs.md)
- `bin/probe.rs`, `probe_api/**`, `sim_device.rs` → [`device-probe.md`](.claude/rules/device-probe.md)
- `src/**/*.{ts,tsx}` → [`frontend.md`](.claude/rules/frontend.md); `src/theme/**`, `src/ui/**` → [`theme-ds.md`](.claude/rules/theme-ds.md); `src/models/**`, `src/ui/blockart/**` → [`models-catalog.md`](.claude/rules/models-catalog.md)
- `e2e/**`, `scripts/e2e.sh`, `scripts/hw-e2e.sh` → [`e2e.md`](.claude/rules/e2e.md); `**/*.sh` → [`shell-scripts.md`](.claude/rules/shell-scripts.md)
- `notes/**`, `**/*.md` → [`docs-writing.md`](.claude/rules/docs-writing.md); `docs/**` → [`docs-site.md`](.claude/rules/docs-site.md); `src-tauri/icons/**` → [`app-icon.md`](.claude/rules/app-icon.md)

Skills, by task:

- Before declaring a change done → [`verify`](.claude/skills/verify/SKILL.md)
- Wire encoding, framing, the `.preset` codec → [`tmp-companion-protocol`](.claude/skills/tmp-companion-protocol/SKILL.md)
- What a preset, scene, block or footswitch means to the user → [`tmp-companion-data-model`](.claude/skills/tmp-companion-data-model/SKILL.md)
- `src/models/`, block art, the CPU-cost table → [`tmp-companion-catalog`](.claude/skills/tmp-companion-catalog/SKILL.md)
- A view, DS component or frontend test → [`tmp-companion-frontend`](.claude/skills/tmp-companion-frontend/SKILL.md)
- A CodeRabbit review, before any `@coderabbitai` command → [`coderabbit`](.claude/skills/coderabbit/SKILL.md)
- The literal native window, only after the headless e2e harness → [`drive-native-window`](.claude/skills/drive-native-window/SKILL.md)
