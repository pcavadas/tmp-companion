# REVIEW.md

Guidance for automated reviewers.
CodeRabbit reads this path by default; Claude's `/code-review` can be pointed at it too.

Written for a **reviewer**, not an implementer.
`AGENTS.md` is the agent guide and the danger summary, and is deliberately not summarised here.
Mechanical invariants live in `.coderabbit.yaml` `path_instructions`, are injected per-file, and are not restated below — this file covers what that per-hunk channel structurally cannot see.

## 1. The bug shape here is OMISSION, not a bad added line

Behavioural regressions here are usually a guard present in a sibling path and absent from the changed one:

- a Summary stage without the backdrop-click protection Run has;
- Doctor's run screen without Leveling `RunPage`'s stop-requested / stopped / done states;
- Escape-to-close surviving in an app that is click-only by design;
- a bare device write/measure with no preceding `loadScene`, missing the base recall a sibling path has, so it lands in whatever scene the connection currently holds;
- a new footswitch assignment hardcoding 5 switch-owned fields instead of inheriting them from a sibling function on the same switch;
- an empty-list guard added to `bypass-nodes` while its twin `bypass_all` fall-through, 40 lines away in the SAME function, keeps the omission.

Reading a hunk in isolation cannot catch any of these.
So when a diff touches one stage, branch, or case, name its siblings and compare.
The sets that keep drifting:

- `src/views/level/{SetupPage,RunPage,SummaryPage}` — the Level flow's stages
- `src/views/doctor/DoctorRun` vs `src/views/level/RunPage` — two run screens, one contract
- any `useXxxFlow` state machine — a flag added to one case must be consumed by all
- `leveller/`'s device write/measure entry points (`measure.rs::capture_full_at`, `knob.rs::set_knob`/`set_knobs`, `footswitch.rs::write_footswitch_values`) vs `commands/doctor.rs::apply_ops_under_scene` — each must hoist an explicit scene recall before its first write; the omission is silent (no `presetError`, just the wrong scene)
- any code that constructs a NEW footswitch function assignment — it must inherit `colorA`/`colorB`/`customLabel`/`linkGroup`/`isActive` from a sibling function on the same switch, never hardcode them

Report the gap citing both paths, and say which sibling has it.
If the siblings agree, report nothing; an empty result is correct.

## 2. Destructive and in-flight actions

The unit holds irreplaceable user presets and a save is permanent.
`.coderabbit.yaml`'s `path_instructions` enforce the destructive-slot-mapping guard (scoped to `session/*.rs`) and the in-flight-abort guard (scoped to `src/**/*.{ts,tsx}`); `.claude/rules/danger.md` and `notes/device/presets.md` carry the full rule.

## 3. What cannot be verified from a diff — flag it, don't guess

No reviewer here runs the app, the device, or the e2e harness.
These are settled by hardware and by `notes/`, never by reading the change:

- USB/HID timing, settle constants, re-amp engage sequencing
- leveling and LUFS math; capture-window durations
- whether a given firmware behaviour actually holds

If a change touches these, ask for the hardware note or point at `notes/leveling.md` / `notes/device/`.
A confidently-worded finding invented about device timing is worse than no finding — flag the missing evidence instead of asserting a cause.
