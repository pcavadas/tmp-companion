---
name: coderabbit
description: "How to work with CodeRabbit on this repo's PRs. Use whenever a task touches CodeRabbit in any way — reading or addressing its review findings, replying to threads, wondering why a PR has no review or isn't merging despite green CI, recovering a rate-limited/failed/skipped review, or being tempted to post ANY `@coderabbitai` command. Consult it BEFORE posting a command: the default correct action is almost always to post nothing."
---

# /coderabbit — working with the reviewer

CodeRabbit is this repo's merge-gating reviewer: its formal approval alone satisfies the
"protect main" ruleset's required review (write-access rule; bots can't be CODEOWNERS), so a PR
stuck at zero reviews stays unmergeable no matter how green CI is.

This skill is a **decision procedure**, not advice. Observe state with the commands in §2, look the
state up in §3, take the one action it names. If a situation is not in the table, the action is
**wait**.

**Progressive review is automatic.** On a reviewed PR, pushing fix commits and replying to threads
is enough — the incremental review picks up the delta and the replies on its own, and re-approves
once its concerns are addressed. A command in that flow is a wasted quota unit (it does not, measurably, push
out an open rate-limit window — see §2 — but it spends one).

## 1. Hard rules (no exceptions, no judgment)

| #   | Never                                                                                                            | Because                                                                                                 |
| --- | ---------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| N1  | `@coderabbitai full review`                                                                                      | Standing instruction. Not as escalation, not for a no-op, not in a quiet window.                        |
| N2  | `@coderabbitai resolve`                                                                                          | Resolves ALL threads at once; resolution is CodeRabbit's acknowledgment, so doing it by hand forges it. |
| N3  | Resolve a thread by hand — GitHub's "Resolve conversation", the `resolveReviewThread` mutation, `gh` equivalents | Same as N2. CodeRabbit resolves its own threads once it accepts a fix or a rebuttal.                    |
| N4  | `@coderabbitai approve`                                                                                          | Resolves all threads AND submits the approval that is this repo's merge gate — self-approving a merge.  |
| N5  | `@coderabbitai autofix`                                                                                          | Pushes bot-authored commits, bypassing the local gate stack (`scripts/gates.sh` stamp, /simplify, HW).  |
| N6  | Post any command on ambiguous silence                                                                            | Silence is not a documented state; the command spends a quota unit for nothing. See §3 row S1.          |
| N7  | Push a commit only to nudge a review                                                                             | Every push to a main-targeted PR spends a quota unit.                                                   |

Only ONE command is ever postable on this repo: **`@coderabbitai review`**, and only in §3 row S3.

## 2. Observe state (run these; do not infer)

```bash
gh pr view <n> --json state,isDraft,reviewDecision,mergeStateStatus,headRefOid,reviews
gh pr view <n> --json reviewThreads                      # thread count on the CURRENT head
gh api repos/<owner>/<repo>/issues/<n>/comments --jq \
  '.[] | select(.user.login=="coderabbitai[bot]") | {updated_at, body: .body[0:400]}' | tail -3
date -u +%H:%M:%SZ                                       # for window arithmetic
```

There is **no `merged` field** on `gh pr view --json` — asking for one is a hard error, so a poll
loop that does `--json merged --jq .merged || echo false` reads as "not merged" forever and never
fires. Detect a merge with `state == "MERGED"` (or a non-null `mergedAt`).

Three observations decide everything:

- **`REVIEWED`** — a formal review exists whose `submittedAt` is after the current head was pushed.
- **`LIMITED(t, n)`** — a CodeRabbit **comment** says "Review limit reached … next review available
  in **n** minutes", last edited at **t**. The limit is **lifted** once `now ≥ t + n`.
- **`OPEN_THREADS`** — unresolved threads from `reviewThreads`.

**Read `LIMITED` from the COMMENT's own text and edit timestamp, never from the check run.** The
PR's CodeRabbit check-run label (e.g. "Review rate limited") is a stamp from the attempt that raised
it and does NOT clear when the window passes; trusting the label instead of the quoted window can
turn a 3-minute wait into hours of idle babysitting.

**The deadline is absolute; `n` is remaining time recomputed at each render.** An attempt made
while the window is open does NOT restart it. Measured on PR #119, same comment id: 22:08:21Z said
"48 minutes" (deadline 22:56:21Z) and 22:17:35Z said "39 minutes" (deadline 22:56:35Z) — a push in
between re-rendered the notice without moving the deadline by more than render time. So a wasted
mid-window attempt costs a quota unit but does not push the window out (contrary to the older
"resets the countdown" belief, which was never vendor-documented). A NEW window is armed only by an
attempt made after the previous one lapsed: the 21:05Z window expired at 21:45Z, and the next
attempt at 22:08Z armed a fresh 48-minute one. This is one measured push, not a general proof for
every command type — still wait, but do not treat an accidental attempt as having reset the clock.

**Record `LIMITED(t, n)` the moment you see it — it is not durable.** CodeRabbit reuses ONE
walkthrough comment, so the limit notice gets EDITED AWAY when that comment is next regenerated (on
PR #119 the 21:05Z "next review available in 40 minutes" text was gone by 21:27Z, same comment id,
leaving only walkthrough content). Its later absence proves nothing in either direction: do not
re-derive the state from the comment you can still see, and do not treat the disappearance as the
limit lifting. Compute `t + n` from the observation you captured.

**Thread replies are served even when review quota is exhausted.** CodeRabbit answered three thread
replies within 13 seconds while no review had run on the head. So "it responds to replies but no
review appears" is the LIMITED signature, not a dead integration — and conversely, a fast reply is
NOT evidence that a review will run. Never infer `REVIEWED` from reply liveness.

Two more traps that make observation lie:

- **Never key on `reviewDecision` flipping.** A push dismisses approvals
  (`dismiss_stale_reviews_on_push`) but NOT a request-changes review, which stands until a new
  review supersedes it. `CHANGES_REQUESTED` after your fix push is expected, not a signal.
- **Never key on an ack or the walkthrough saying "finished".** CodeRabbit edits its ONE
  walkthrough comment in place, and an ack has been observed claiming completion on a silent no-op.
  `REVIEWED` is defined by `submittedAt`, nothing else.

## 3. The decision table

Evaluate top to bottom; take the FIRST matching row and only that action.

| Row | State                                                               | Action                                                                              |
| --- | ------------------------------------------------------------------- | ----------------------------------------------------------------------------------- |
| S0  | PR is a draft                                                       | Nothing. Drafts are skipped entirely and spend no quota. Mark ready when settled.   |
| S1  | Not `REVIEWED`, no `LIMITED` message                                | **Wait.** Re-observe later. Post nothing — this includes an hours-long quiet spell. |
| S2  | `LIMITED(t, n)` and `now < t + n`                                   | **Wait** until `t + n`. Any command before then is wasted.                          |
| S3  | `LIMITED(t, n)` and `now ≥ t + n` and not `REVIEWED`                | Post exactly ONE `@coderabbitai review`. Then go to S1.                             |
| S4  | `REVIEWED` and `OPEN_THREADS` non-empty                             | Run §4 on every open thread. One commit, one push. Then S1.                         |
| S5  | `REVIEWED`, no `OPEN_THREADS`, `reviewDecision != APPROVED`         | **Wait** — it re-approves on its own after accepting the last thread.               |
| S6  | `APPROVED` + CI green + `mergeStateStatus` clean                    | Done. Auto-merge takes it; report the merge.                                        |
| S7  | S3 was taken and the review provably no-oped (0 reviews, 0 threads) | **Stop. Flag a human.** Do not post again (N1 forbids the old escalation).          |

`mergeStateStatus: DIRTY` is not in this table because it is not a review state — it means `main`
moved and the branch now conflicts. Merge `origin/main` in (never rebase + force-push a PR branch),
resolve, and re-enter at S1. Preserve what the incoming side added: a conflict in a file both
branches edited is two sessions' findings, not yours versus noise.

## 4. Handling one thread (deterministic)

Per finding, in order:

1. **Re-verify against current code** (`grep`/`sed` the cited `file:line`) — reviews lag pushes and
   rebases, so a finding can already be fixed or have moved.
2. **Classify.** Valid ⇒ step 3. Invalid or deliberately out of scope ⇒ step 4. There is no third
   branch: every open thread gets a fix or a reasoned reply.
3. **Fix the root cause**, on the branch of the PR the findings were actually posted on. Batch every
   fix into ONE commit + ONE push (N7). In a stacked pair the checked-out branch is NOT necessarily
   that PR's head (only the main-targeted front of a stack gets reviewed, per §5, so the PR whose
   threads you are reading can silently differ from the branch you are on): confirm with
   `gh pr view <n> --json headRefName,headRefOid` and match BOTH the branch name and
   `git rev-parse HEAD` against `headRefOid` — branch name alone doesn't prove the checkout is at
   the PR's actual head after a force-push — or just `gh pr checkout <n>`.
4. **Reply once** on the thread, including `@coderabbitai` so it engages, citing `file:line`, and
   **stating the reason** it isn't being fixed. A reply without a reason gives it nothing to
   evaluate and the thread stays open. No second reply, no argument.
5. **Resolve nothing** (N2, N3). Stop and re-observe.

Match threads by **stable thread id**, never by `(path, line)` — line numbers shift with the push
and a miss is silent. Comment body is a fallback only after confirming it's unique among the PR's
threads (duplicate findings share near-identical bodies).

**No command operates on a single thread.** Every `@coderabbitai` command is a top-level PR
comment; `approve` and `resolve` are documented top-level-ONLY and do not work inside a thread, and
`resolve` is all-threads-at-once, so there is no per-thread resolve to nudge one thread with.
`resume` pairs exclusively with a prior `pause` and no-ops otherwise, so it is never the fix for a
stalled review. A reply is the only thread-level lever that exists.

## 5. Facts that change how you read a review

- **`.coderabbit.yaml` does not enumerate CodeRabbit's static-analysis integrations.** This repo
  sets no `tools:` section, so every one defaults `true` and can appear in a `🧰 Tools` block the
  repo config never mentions — including **SkillSpector** (NVIDIA's agent-skill security scanner)
  on a `SKILL.md` diff. Identify an unfamiliar tool from CodeRabbit's tools reference; a grep of
  the repo config cannot tell you whether it is real. Padding inside a finding does not invalidate
  the finding above it.
- **Processed-commits trap.** A rate-limited attempt can mark head commits _processed_, so a later
  plain `review` "finishes" in seconds having reviewed nothing. That is row S7, not a retry.
- **Quota (free/OSS: small, shared, adaptive).** Spent by every push to a main-targeted PR, every
  retarget-to-main, and every manual command. Refills ~a few/hour, throttling toward ~1/hour under
  sustained activity — and the quoted "next review available in n minutes" varies unpredictably with
  same-day spend, so never assume attempt N's window applies to N+1 (a four-PR cascade saw
  42/33/27/59-minute windows, non-monotonic).
- **Only main-targeted PRs are auto-reviewed** (unless `base_branches` extends it). A stacked
  child meets CodeRabbit for the first time when it retargets to main after its parent merges —
  budget one review per cascade step; pushes to non-main-based descendants are quota-free.
- **An unresolved thread does NOT block merge here.** The "protect main" ruleset has
  `required_review_thread_resolution: false` — what gates the merge is
  `required_approving_review_count: 1` plus `require_last_push_approval: true`. CodeRabbit will
  sometimes deliberately leave a thread open to track deferred work ("I'll leave this finding
  unresolved for the deferred implementation"). That is its choice and it is merge-safe: leave it
  open (N3) and do not chase it.
- **The approval that merges must postdate the final commit** (`dismiss_stale_reviews_on_push` +
  `require_last_push_approval`). So the last thing you do to a PR is stop pushing.
- Other commands (`configuration`, `help`, `generate docstrings|unit tests|sequence diagram`,
  `summary`) are informational and harmless, but each adds bot noise. `@coderabbitai ignore` goes
  in the PR **description** and permanently disables auto-review for that PR.
