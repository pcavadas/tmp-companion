---
name: coderabbit
description: "How to work with CodeRabbit on this repo's PRs. Use whenever a task touches CodeRabbit in any way — reading or addressing its review findings, replying to threads, wondering why a PR has no review or isn't merging despite green CI, recovering a rate-limited/failed/skipped review, or being tempted to post ANY `@coderabbitai` command. Consult it BEFORE posting a command: the default correct action is almost always to post nothing."
---

# /coderabbit — working with the reviewer

CodeRabbit is this repo's merge-gating reviewer: its formal approval alone satisfies the "protect
main" ruleset's required review (write-access rule; bots can't be CODEOWNERS), so a PR stuck at zero
reviews stays unmergeable no matter how green CI is.

**Scope.** It governs one thing: what to post, fix, or wait for on a CodeRabbit review here — not
general agent policy, and never overriding system instructions or an explicit user request. "Wait"
always means "wait rather than post a bot command", never "stop working".

Within that scope it is a **decision procedure**: observe state with §2, look it up in §3, take the
one action named. A situation not in the table means **wait** — the table is deliberately closed so
an unrecognised state can't be improvised into a command.

**Progressive review is automatic.** On a reviewed PR, pushing fix commits and replying to threads is
enough — the incremental review picks up the delta and re-approves. A command there burns quota for
nothing.

## 1. Hard rules (settled by the repo owner; no self-granted exceptions)

Standing owner decisions — don't re-litigate them per-PR. They bind the agent, not the user: an
explicit user instruction supersedes any row.

| #   | Never                                                                                                            | Because                                                                                                                                                                                                                                                                                                                                                                                                                 |
| --- | ---------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| N1  | `@coderabbitai full review`                                                                                      | Standing instruction. Not as escalation, not for a no-op, not in a quiet window.                                                                                                                                                                                                                                                                                                                                        |
| N2  | `@coderabbitai resolve` **on the agent's own judgment**                                                          | Resolves ALL threads at once; resolution is CodeRabbit's acknowledgment, so triggering it uninvited forges it. Like `approve`, it is user-authorizable: when CodeRabbit's own resolve fails platform-side, the §4.2 recovery ends with the USER choosing to post it (#161, 2026-08-30 — it resolved the confirmed-fixed threads and the verdict flipped to APPROVED seconds later).                                     |
| N3  | Resolve a thread by hand — GitHub's "Resolve conversation", the `resolveReviewThread` mutation, `gh` equivalents | Same as N2, with NO user-authorizable variant and one extra teeth: the APPROVED flip reads CodeRabbit's OWN thread state, so a GitHub-side hand-resolve leaves CodeRabbit still seeing the thread open and the verdict stuck — even when CodeRabbit's reply says "resolve it manually" (that invitation is addressed to the human). Already done it? `unresolveReviewThread` immediately, then §4.2 (#161, 2026-08-30). |
| N4  | `@coderabbitai approve` **on the agent's own judgment**                                                          | Resolves all threads AND submits a REAL approval (`request_changes_workflow: true`, `.coderabbit.yaml:13`) — self-approving the merge this repo gates on. ONE exception, and it is the OWNER's, not yours: row SV names it as the remedy the user can authorize for a stale verdict; posted only on their explicit instruction (exercised on #160, 2026-08-30).                                                         |
| N5  | `@coderabbitai autofix`                                                                                          | Pushes bot-authored commits, bypassing the local gate stack (`scripts/gates.sh` stamp, /simplify, HW).                                                                                                                                                                                                                                                                                                                  |
| N6  | Post any command on ambiguous silence                                                                            | Silence is not a documented state; the command spends a quota unit for nothing. See §3 row S1.                                                                                                                                                                                                                                                                                                                          |
| N7  | Push a commit only to nudge a review                                                                             | Every push to a main-targeted PR spends a quota unit.                                                                                                                                                                                                                                                                                                                                                                   |

Only TWO commands are ever postable on the agent's own judgment: **`@coderabbitai review`** (§3 row
S3) and **`@coderabbitai resume`** (§3 row SP). Two more — **`@coderabbitai approve`** (row SV) and
**`@coderabbitai resolve`** (§4.2) — exist solely as user-authorized remedies, posted only after the
user explicitly says to.

## 2. Observe state

Full `gh`/GraphQL/REST toolkit and every trap in reading its output: `references/observing-state.md`.
Eight observations decide everything:

- **`REVIEWED`** — a formal review exists whose `submittedAt` is after the current head was pushed.
- **`LIMITED(t, n)`** — a CodeRabbit **comment** says "Review limit reached … next review available
  in **n** minutes", last edited at **t**. Lifted once `now ≥ t + n`.
- **`PAUSED`** — the walkthrough comment body carries a `> [!NOTE]` block titled **"Reviews
  paused"**; it names its own remedies.
- **`OUTSIDE_DIFF`** — findings existing ONLY in a review's BODY, under a
  `⚠️ Outside diff range comments (N)` heading. **Not threads**: no thread id, so they cannot be
  replied to or resolved per-thread.
- **`ACTIONABLE_OUTSIDE_DIFF`** — the `OUTSIDE_DIFF` subset not yet dealt with. One is **addressed**
  once the fix, or a written refusal reason, lands in a commit message on this PR (step 5b) — with
  no thread, that commit is the only record.
- **`UNTRIAGED_RESOLVED`** — threads resolved or outdated that never got an OUTCOME from you: a
  landed fix, or a written reason for not fixing. A bare acknowledging reply does not clear one.
  A thread can resolve because the diff moved under it, so these left `OPEN_THREADS` on their own.
  Treat each as actionable on the merits; never reopen it (N3).
- **`OPEN_THREADS`** — unresolved threads from `reviewThreads`.
- **`ACTIONABLE_THREADS`** — the `OPEN_THREADS` subset still needing something from you: you have
  never replied, or its latest comment asks a question or requests a change. An open thread where
  you replied and its answer asks for nothing is **settled**, not actionable.

## 3. The decision table

Evaluate top to bottom; take the FIRST matching row and only that action.

| Row | State                                                                                                                                        | Action                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                             |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| S0  | PR is a draft                                                                                                                                | Nothing. Drafts are skipped entirely and spend no quota. Mark ready when settled.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| SP  | `PAUSED` (§2) and no open `LIMITED` window                                                                                                   | Post exactly ONE `@coderabbitai resume`, then S1. ONE per head commit — if `PAUSED` still stands on the same head after it, the lever has failed: go to S7, never a second `resume`.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| S1  | Not `REVIEWED`, no `LIMITED` message                                                                                                         | **Wait.** Re-observe later. Post nothing — this includes an hours-long quiet spell.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                |
| S2  | `LIMITED(t, n)` and `now < t + n`                                                                                                            | **Wait** until `t + n`. Any command before then is wasted.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |
| S3  | `LIMITED(t, n)` and `now ≥ t + n` and not `REVIEWED`                                                                                         | Post exactly ONE `@coderabbitai review`. Then go to S1.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| S4  | `REVIEWED` and (`ACTIONABLE_THREADS` non-empty OR `ACTIONABLE_OUTSIDE_DIFF` non-empty OR `UNTRIAGED_RESOLVED` non-empty)                     | Run §4 on every entry of all three. One commit, one push. Then S1.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| S5  | `REVIEWED`, `ACTIONABLE_THREADS` empty, `ACTIONABLE_OUTSIDE_DIFF` all addressed, `reviewDecision != APPROVED`                                | **Wait ONLY IF `OPEN_THREADS` is empty** — approval follows thread state within seconds (49/49). If ANY thread is still open (even one CodeRabbit deferred), waiting is futile: go to §4.1 and clear it. This wait is BOUNDED, and TIGHT: `CHANGES_REQUESTED` still standing roughly 1-2 min after the last thread cleared means the flip is not coming — go to SV. A 10-hour watch on #160 first proved the wait ends; #165 corrected the bound itself, landing the approve flip plus auto-merge within seconds of the last thread clearing (2026-08-30). The two legitimately slow waits in this table are elsewhere and unrelated: S1's wait for CodeRabbit's first full review pass (~15 min) and S2's `LIMITED` window — neither is this one. |
| SV  | S5's bounded wait expired: `OPEN_THREADS` empty, every finding fixed or withdrawn by CodeRabbit itself, verdict stuck at `CHANGES_REQUESTED` | **Stale verdict — escalate to the user, naming `@coderabbitai approve` as the remedy they can authorize.** It clears ONLY the stale review gate — the merge still goes through S6 (CI, `mergeStateStatus`, armed auto-merge; external-author PRs additionally need the owner's own approval per `arm-auto-merge.yml`), so keep watching and report completion from `MERGED`, never from the approval. It stays user-authorized-only per N4 — wait for their explicit go. Don't offer manual dismiss-and-approve instead: a PR author cannot approve their own PR, so the command is the only lever (#160, 2026-08-30).                                                                                                                             |
| S6  | `APPROVED` + CI green + `mergeStateStatus` clean                                                                                             | **Not done yet** — auto-merge still has to land it. Keep watching; report completion only from `gh pr view --json state` reading `MERGED`, never from an approval.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| S7  | A posted lever provably failed — S3's `review` no-oped (0 reviews, 0 threads), or SP's `resume` left `PAUSED` on the same head               | **Stop. Flag a human.** Do not post again (N1 forbids the old escalation).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                         |

`mergeStateStatus: DIRTY` is not a review state — `main` moved and the branch now conflicts. Merge
`origin/main` in (never rebase + force-push a PR branch), resolve, re-enter at S1. Preserve what the
incoming side added: a conflict in a file both branches edited is two sessions' findings.

## 4. Handling one finding (deterministic)

Findings arrive on two lanes that close differently. Work BOTH; never mix their rules.

| Lane                      | Source                             | How it closes                                                                                                          |
| ------------------------- | ---------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `ACTIONABLE_THREADS`      | `reviewThreads` (§2)               | Fix and/or reply ON the thread (5a). CodeRabbit resolves it itself.                                                    |
| `ACTIONABLE_OUTSIDE_DIFF` | review BODIES, `OUTSIDE_DIFF` (§2) | Fix and/or record the outcome in the COMMIT MESSAGE (5b). No thread exists, so it closes when the next review sees it. |

A settled thread needs nothing more FROM YOU — don't re-reply. It is still an approval blocker while
it stays open (§5), so clear it through §4.1 rather than waiting on it.

**Enumerate before fixing.** List every finding from both sources first — threads and all review
bodies — then work the list.

Per finding, in order:

1. **Re-verify against current code** (`grep`/`sed` the cited `file:line`) — reviews lag pushes and
   rebases, so a finding can already be fixed or have moved.
2. **Classify from the finding's BODY, not its title.** A finding often asks for more than one thing:
   enumerate every requirement, then satisfy or refuse each explicitly. THREE outcomes, no others:
   valid ⇒ 3; already addressed per step 1 ⇒ skip 3 and 4, go to 5 citing the current-code evidence;
   invalid or out of scope ⇒ skip 3 and 4, go to 5 recording the refusal REASON. A refused or
   already-addressed finding never gets a code change.
3. **Fix the root cause**, on the branch the findings were actually posted on. Batch every fix into
   ONE commit + ONE push (N7). Confirm you're on the PR's real head — stacked-PR trap in
   `references/handling-findings.md`.
4. **Sweep for siblings before calling it fixed** — grep the module for the same shape. This is the
   agent-side counterpart of the `Behavioral parity` `pre_merge_check` in `.coderabbit.yaml`.
5. **Close the loop on the finding's own lane.**
   - **5a — `ACTIONABLE_THREADS`:** reply ONCE on the thread, including `@coderabbitai` so it
     engages, citing `file:line`, and **stating the reason** when it isn't being fixed — a reply
     without a reason gives it nothing to evaluate and the thread stays open. No second reply —
     with ONE narrow exception: the §4.2 retry reply, sent only after CodeRabbit has confirmed the
     fix but reported failing to resolve the thread itself.
   - **5b — `ACTIONABLE_OUTSIDE_DIFF`:** do NOT try to reply — there is no thread id and
     `@coderabbitai` commands are top-level-only. Record the outcome in the COMMIT MESSAGE, which is
     what the next review reads; an unexplained refusal is indistinguishable from a missed finding.
6. **Resolve nothing** (N2, N3). Stop and re-observe.

### 4.1 Clearing a thread CodeRabbit deferred

A thread it "left open to track deferred work" still blocks approval. It has to close it — you must
not (N3). Recipe: `references/handling-findings.md`.

### 4.2 When CodeRabbit's OWN resolve fails (observed #161, 2026-08-30)

CodeRabbit can confirm a fix in-thread yet fail to resolve the thread on its side, replying
"I couldn't resolve this review thread on the repository platform. Please retry or resolve it
manually." That sentence changes nothing about N3 — hand-resolving still strands the verdict.
The deterministic recovery:

1. **Reply in-thread asking it to retry resolving itself** — the one sanctioned second reply
   (step 5a's exception). Keep the `@coderabbitai` mention and name the failed resolve it reported.
   Free, answered in seconds.
2. It may answer that resolve works only as a **top-level command** ("Post `@coderabbitai resolve`
   or `@coderabbitai approve` as a new top-level PR comment. Approve commands are disabled for
   review-thread replies.").
3. **Escalate to the user with both options named** — `resolve` (CodeRabbit resolves its own
   confirmed-fixed threads; the verdict then flips on thread state) or `approve` (threads + a real
   approval in one shot). Post whichever THEY pick, only on their explicit go (N2, N4).

On #161 the user chose `resolve`: threads cleared as CodeRabbit's own acknowledgment and
`CHANGES_REQUESTED` flipped to `APPROVED` within seconds. The precise `resolve` state model:
`resolve` changes no formal review state ITSELF — it unblocks only by clearing threads so the
§5 thread-state flip fires. So it rescues a verdict blocked on OPEN confirmed-fixed threads
(this recovery), and does nothing for a verdict already stuck with ZERO open threads — that is
row SV's stale-verdict case, where `approve` is the lever.

**A second shape, observed on #165 (2026-08-30):** the sanctioned retry reply (step 1) got
CodeRabbit to resolve all 3 threads — GitHub flipped them to `resolved:true` — but the verdict
stayed `CHANGES_REQUESTED`. A further `@coderabbitai resolve` then came back **inconclusive**
("the GitHub review-thread API does not return a review thread for that comment"): CodeRabbit's
own thread lookup filters on `isResolved == false`, so once GitHub marks a thread resolved it is
invisible to CodeRabbit's own lookup — `resolve` has nothing left to find and structurally cannot
work. **Once threads read `resolved:true` on GitHub but the verdict is still stuck, don't offer
`resolve`** — this is exactly why it no-ops. That state is row SV's stale-verdict shape (zero open
threads, verdict stuck), and `approve` is the only working lever, still posted only on the user's
explicit go per N4. On #165, `approve` flipped the verdict in seconds and auto-merge landed the PR.

## 5. Facts that change how you read a review

- **THE APPROVAL GATE IS ZERO UNRESOLVED THREADS — CodeRabbit's gate, not GitHub's.** The "protect
  main" ruleset sets `required_review_thread_resolution: false`, making an open thread look
  harmless. It is not: **49 of 49 approvals in this repo's history landed with 0 unresolved
  threads.** Reading GitHub's rules and concluding "the open thread is merge-safe" is the single most
  expensive mistake made here. **One open thread = no approval = no merge, full stop.**
- **Thread replies are free; pushes are not.** A reply is answered in ~15-30 s and spends no quota; a
  push trips a fresh rate-limit window, and the windows lengthen under sustained use. When both
  would work, reply. The published limits are hourly review rates per plan on rolling windows,
  tightened adaptively above recent-usage norms. There is no documented lines-changed cap, but there
  IS a per-review FILE limit (reviews over 300 files are unsupported on usage pricing —
  docs.coderabbit.ai/management/plans, read 2026-09-02), so splitting a PR buys iteration headroom
  and, past that file count, eligibility.
