# REVIEW.md

Guidance for automated reviewers. CodeRabbit reads this path by default; Claude's
`/code-review` can be pointed at it too.

Written for a **reviewer**, not an implementer. `CLAUDE.md` is the build guide and is
deliberately not summarised here. Mechanical invariants live in `.coderabbit.yaml`
`path_instructions`, are injected per-file, and are not restated below — this file
covers what that per-hunk channel structurally cannot see.

## 1. The bug shape here is OMISSION, not a bad added line

Every recent behavioural regression was a guard present in a sibling path and absent
from the changed one:

| PR   | Defect                                                                                                                                                               |
| ---- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| #109 | Summary stage lacked the backdrop-click protection Run already had                                                                                                   |
| #108 | Doctor's run screen lacked `RunBody`'s stop-requested / stopped / done states                                                                                        |
| #110 | Escape-to-close survived in an app that is click-only by design                                                                                                      |
| #112 | A bare device write/measure with no preceding `loadScene` omitted the base recall present in a sibling path, landing in whatever scene the connection currently held |
| #112 | A new footswitch assignment hardcoded 5 switch-owned fields instead of inheriting them from a sibling function on the same switch                                    |
| #119 | A `bypass-nodes` empty-list guard was added while its twin `bypass_all` fall-through, 40 lines away in the SAME function, kept the omission                          |

Reading a hunk in isolation cannot catch any of these. So when a diff touches one
stage, branch, or case, name its siblings and compare. The sets that keep drifting:

- `src/views/overlays/{SetupBody,RunBody,SummaryBody}` — the wizard stages
- `src/views/doctor/DoctorRun` vs `src/views/overlays/RunBody` — two run screens, one contract
- any `useXxxFlow` state machine — a flag added to one case must be consumed by all
- `leveller.rs`'s device write/measure entry points (`capture_full_at`/`set_knob`/`set_knobs`/`write_footswitch_values`) vs `commands/doctor.rs::apply_ops_under_scene` — each must hoist an explicit scene recall before its first write; the omission is silent (no `presetError`, just the wrong scene)
- any code that constructs a NEW footswitch function assignment — it must inherit `colorA`/`colorB`/`customLabel`/`linkGroup`/`isActive` from a sibling function on the same switch, never hardcode them

Report the gap citing both paths, and say which sibling has it. If the siblings agree,
report nothing; an empty result is correct.

## 2. Ungated surfaces — a regression here ships silently

`notes/user-journeys.md` marks every journey FULL / PARTIAL / NONE. A green suite says
nothing about the NONE and PARTIAL rows, so changes touching them deserve
disproportionate scrutiny:

- Stop mid-run, then Continue (Vitest-mocked only)
- Detach mid-run; relaunch or webview reload mid-operation (no gate)
- Copy: Back discards staged edits; partial save failure across N targets (no gate)
- Doctor apply/save → Level sees the post-write graph (no gate)
- Settings target reorder / slider commit (no gate)
- "Even out parallel amps" joint-k rebalance (online-only class, no gate)

Consult that table rather than trusting CI.

## 3. Destructive and in-flight actions

The unit holds irreplaceable user presets and a save is permanent. In any diff:

- an in-flight device run (leveling, Doctor, a Copy save) must not be abortable by a
  backdrop click or an unguarded close
- a stop control must acknowledge immediately, not only after the in-flight capture ends
- a destructive op keyed on a slot mapping needs a non-destructive read confirming that
  mapping in the **same address space** as the mutation

## 4. What cannot be verified from a diff — flag it, don't guess

No reviewer here runs the app, the device, or the e2e harness. These are settled by
hardware and by `notes/`, never by reading the change:

- USB/HID timing, settle constants, re-amp engage sequencing
- leveling and LUFS math; capture-window durations
- whether a given firmware behaviour actually holds

If a change touches these, ask for the hardware note or point at `notes/leveling.md` /
`notes/protocol.md`. A confidently-worded finding invented about device timing is worse
than no finding — flag the missing evidence instead of asserting a cause.
