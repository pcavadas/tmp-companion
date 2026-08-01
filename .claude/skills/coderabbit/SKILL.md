---
name: coderabbit
description: "How to work with CodeRabbit on this repo's PRs. Use whenever a task touches CodeRabbit in any way — reading or addressing its review findings, replying to threads, wondering why a PR has no review or isn't merging despite green CI, recovering a rate-limited/failed/skipped review, or being tempted to post ANY `@coderabbitai` command. Consult it BEFORE posting a command: the default correct action is almost always to post nothing."
---

# /coderabbit — working with the reviewer

CodeRabbit is this repo's merge-gating reviewer: its formal approval alone satisfies the "protect
main" ruleset's required review (write-access rule; bots can't be CODEOWNERS), so a PR stuck at zero
reviews stays unmergeable no matter how green CI is.

**Scope.** This skill governs one thing: what to post, fix, or wait for on a CodeRabbit review of
this repo's PRs. It is not a general agent policy and does not override system instructions or an
explicit user request. "Wait" below always means "wait rather than post a bot command", never "stop
working" or "decline to answer".

Within that scope it is a **decision procedure**, not advice: observe state with §2, look it up in
§3, take the one action named. If a situation is not in the table, the action is **wait** — the table
is deliberately closed so an unrecognised state can't be improvised into a command.

**Progressive review is automatic.** On a reviewed PR, pushing fix commits and replying to threads is
enough — the incremental review picks up the delta and re-approves once its concerns are addressed.
A command in that flow spends a quota unit for nothing.

## 1. Hard rules (settled by the repo owner; no self-granted exceptions)

The owner's standing decisions about bot commands here — don't re-litigate them per-PR or talk
yourself into an exception. They bind the agent, not the user: an explicit user instruction
supersedes any row.

| #   | Never                                                                                                            | Because                                                                                                                                                                                                  |
| --- | ---------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| N1  | `@coderabbitai full review`                                                                                      | Standing instruction. Not as escalation, not for a no-op, not in a quiet window.                                                                                                                         |
| N2  | `@coderabbitai resolve`                                                                                          | Resolves ALL threads at once; resolution is CodeRabbit's acknowledgment, so doing it by hand forges it. Changes NO formal review state — it cannot turn `CHANGES_REQUESTED` into `APPROVED`.             |
| N3  | Resolve a thread by hand — GitHub's "Resolve conversation", the `resolveReviewThread` mutation, `gh` equivalents | Same as N2. CodeRabbit resolves its own threads once it accepts a fix or a rebuttal.                                                                                                                     |
| N4  | `@coderabbitai approve`                                                                                          | Resolves all threads AND submits a REAL approval: `.coderabbit.yaml:13` sets `request_changes_workflow: true`, so it lands as the formal review this repo's merge gate needs — self-approving the merge. |
| N5  | `@coderabbitai autofix`                                                                                          | Pushes bot-authored commits, bypassing the local gate stack (`scripts/gates.sh` stamp, /simplify, HW).                                                                                                   |
| N6  | Post any command on ambiguous silence                                                                            | Silence is not a documented state; the command spends a quota unit for nothing. See §3 row S1.                                                                                                           |
| N7  | Push a commit only to nudge a review                                                                             | Every push to a main-targeted PR spends a quota unit.                                                                                                                                                    |

Only TWO commands are ever postable on this repo: **`@coderabbitai review`** (§3 row S3) and
**`@coderabbitai resume`** (§3 row SP). Nothing else, ever.

## 2. Observe state

Full `gh`/GraphQL/REST toolkit and every trap in reading its output: `references/observing-state.md`.
Six observations decide everything:

- **`REVIEWED`** — a formal review exists whose `submittedAt` is after the current head was pushed.
- **`LIMITED(t, n)`** — a CodeRabbit **comment** says "Review limit reached … next review available
  in **n** minutes", last edited at **t**. Lifted once `now ≥ t + n`.
- **`PAUSED`** — the walkthrough comment body carries a `> [!NOTE]` block titled **"Reviews
  paused"**; it names its own remedies.
- **`OUTSIDE_DIFF`** — findings existing ONLY in a review's BODY, under a
  `⚠️ Outside diff range comments (N)` heading. **Not threads**: no thread id, so they cannot be
  replied to or resolved per-thread.
- **`OPEN_THREADS`** — unresolved threads from `reviewThreads`.
- **`ACTIONABLE_THREADS`** — the `OPEN_THREADS` subset still needing something from you: you have
  never replied, or its latest comment asks a question or requests a change. An open thread where
  you replied and its answer asks for nothing is **settled**, not actionable.

## 3. The decision table

Evaluate top to bottom; take the FIRST matching row and only that action.

| Row | State                                                                                                         | Action                                                                                                                                                                                                 |
| --- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| S0  | PR is a draft                                                                                                 | Nothing. Drafts are skipped entirely and spend no quota. Mark ready when settled.                                                                                                                      |
| SP  | `PAUSED` (§2) and no open `LIMITED` window                                                                    | Post exactly ONE `@coderabbitai resume`, then S1. ONE per head commit — a pause re-declared after a `resume` on the same head is S7, never a second `resume`.                                          |
| S1  | Not `REVIEWED`, no `LIMITED` message                                                                          | **Wait.** Re-observe later. Post nothing — this includes an hours-long quiet spell.                                                                                                                    |
| S2  | `LIMITED(t, n)` and `now < t + n`                                                                             | **Wait** until `t + n`. Any command before then is wasted.                                                                                                                                             |
| S3  | `LIMITED(t, n)` and `now ≥ t + n` and not `REVIEWED`                                                          | Post exactly ONE `@coderabbitai review`. Then go to S1.                                                                                                                                                |
| S4  | `REVIEWED` and (`ACTIONABLE_THREADS` non-empty OR any unaddressed `ACTIONABLE_OUTSIDE_DIFF`)                  | Run §4 on every `ACTIONABLE_THREADS` entry AND every unaddressed `ACTIONABLE_OUTSIDE_DIFF` finding. One commit, one push. Then S1.                                                                     |
| S5  | `REVIEWED`, `ACTIONABLE_THREADS` empty, `ACTIONABLE_OUTSIDE_DIFF` all addressed, `reviewDecision != APPROVED` | **Wait ONLY IF `OPEN_THREADS` is empty** — approval follows thread state within seconds. If ANY thread is still open (even one CodeRabbit chose to defer), waiting is futile: go to §4.1 and clear it. |
| S6  | `APPROVED` + CI green + `mergeStateStatus` clean                                                              | **Not done yet** — auto-merge still has to land it. Keep watching; report completion only from `state == "MERGED"` (§2), never from an approval.                                                       |
| S7  | S3 was taken and the review provably no-oped (0 reviews, 0 threads)                                           | **Stop. Flag a human.** Do not post again (N1 forbids the old escalation).                                                                                                                             |

`mergeStateStatus: DIRTY` is absent from the table because it is not a review state — it means `main`
moved and the branch now conflicts. Merge `origin/main` in (never rebase + force-push a PR branch),
resolve, re-enter at S1. Preserve what the incoming side added: a conflict in a file both branches
edited is two sessions' findings, not yours versus noise.

## 4. Handling one finding (deterministic)

Findings arrive on two lanes that close differently. Work BOTH; never let one lane's rule leak into
the other.

| Lane                      | Source                             | How it closes                                                                                                          |
| ------------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `ACTIONABLE_THREADS`      | `reviewThreads` (§2)               | Fix and/or reply ON the thread (5a). CodeRabbit resolves it itself.                                                    |
| `ACTIONABLE_OUTSIDE_DIFF` | review BODIES, `OUTSIDE_DIFF` (§2) | Fix and/or record the outcome in the COMMIT MESSAGE (5b). No thread exists, so it closes when the next review sees it. |

A settled thread is terminal even while open — don't re-reply, don't let it hold up S5/S6.

**Enumerate before fixing.** List every finding from both sources first — threads and all review
bodies — then work the list. Anything not on it does not get fixed.

Per finding, in order:

1. **Re-verify against current code** (`grep`/`sed` the cited `file:line`) — reviews lag pushes and
   rebases, so a finding can already be fixed or have moved.
2. **Classify from the finding's BODY, not its title.** A finding often asks for more than one thing:
   enumerate every requirement, then satisfy or refuse each explicitly. Valid ⇒ 3, invalid or out of
   scope ⇒ 4. There is no third branch.
3. **Fix the root cause**, on the branch the findings were actually posted on. Batch every fix into
   ONE commit + ONE push (N7). Confirm you're on the PR's real head — stacked-PR trap in
   `references/handling-findings.md`.
4. **Sweep for siblings before calling it fixed** — grep the module for the same shape. This is the
   agent-side counterpart of the `Behavioral parity` `pre_merge_check` in `.coderabbit.yaml`.
5. **Close the loop on the finding's own lane.**
   - **5a — `ACTIONABLE_THREADS`:** reply ONCE on the thread, including `@coderabbitai` so it
     engages, citing `file:line`, and **stating the reason** when it isn't being fixed — a reply
     without a reason gives it nothing to evaluate and the thread stays open. No second reply.
   - **5b — `ACTIONABLE_OUTSIDE_DIFF`:** do NOT try to reply. There is no thread id and
     `@coderabbitai` commands are top-level-only, so there is no per-finding channel at all. Record
     the outcome in the COMMIT MESSAGE, which is what the next review reads; a refusal with no
     written reason is indistinguishable from having missed the finding.
6. **Resolve nothing** (N2, N3). Stop and re-observe.

Thread-matching mechanics and why no `@coderabbitai` command targets a single thread:
`references/handling-findings.md`.

### 4.1 Clearing a thread CodeRabbit deferred

A thread it "left open to track deferred work" still blocks approval. It has to close it — you must
not (N3). Recipe: `references/handling-findings.md`.

## 5. Facts that change how you read a review

- **THE APPROVAL GATE IS ZERO UNRESOLVED THREADS — CodeRabbit's gate, not GitHub's.** The "protect
  main" ruleset sets `required_review_thread_resolution: false`, making an open thread look
  harmless. It is not: **49 of 49 approvals in this repo's history landed with 0 unresolved
  threads.** Reading GitHub's rules and concluding "the open thread is merge-safe" is the single most
  expensive mistake made here. **One open thread = no approval = no merge, full stop.**
- **Thread replies are free; pushes are not.** A reply is answered in ~15-30 s and spends no quota. A
  push trips a fresh rate-limit window, and the windows lengthen under sustained use (33 → 41 → 42
  minutes on #119). When both would work, reply.

The rest of §5 (tooling notes, quota mechanics, timing anecdotes): `references/handling-findings.md`.

## References

- `references/observing-state.md` — the full command toolkit, every trap in reading its output, and
  the six states' long-form definitions.
- `references/handling-findings.md` — the §4.1 close-out recipe, thread-matching mechanics, the
  stacked-PR head trap, and the rest of §5.
