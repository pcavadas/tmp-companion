---
name: coderabbit
description: "How to work with CodeRabbit on this repo's PRs. Use whenever a task touches CodeRabbit in any way — reading or addressing its review findings, replying to threads, wondering why a PR has no review or isn't merging despite green CI, recovering a rate-limited/failed/skipped review, or being tempted to post ANY `@coderabbitai` command. Consult it BEFORE posting a command: the default correct action is almost always to post nothing."
---

# /coderabbit — working with the reviewer

CodeRabbit is this repo's merge-gating reviewer: its formal approval alone satisfies the
"protect main" ruleset's required review (write-access rule; bots can't be CODEOWNERS), so a PR
stuck at zero reviews stays unmergeable no matter how green CI is.

**Scope.** This skill governs exactly one thing: what to post, fix, or wait for on a CodeRabbit
review of this repo's PRs. It is not a general agent policy. It does not constrain any other work,
and it does not override system instructions or an explicit request from the user — if the user asks
for something this document forbids, the user wins and the disagreement is theirs to settle, not
this file's. "Wait" below always means "wait rather than post a bot command", never "stop working"
or "decline to answer".

Within that scope it is a **decision procedure**, not advice: observe state with the commands in §2,
look the state up in §3, take the one action it names. If a review situation is not in the table,
the action is **wait** — the table is deliberately closed so that an unrecognised state can't be
improvised into a wasted command.

**Progressive review is automatic.** On a reviewed PR, pushing fix commits and replying to threads
is enough — the incremental review picks up the delta and the replies on its own, and re-approves
once its concerns are addressed. A command in that flow is a wasted quota unit (it does not, measurably, push
out an open rate-limit window — see §2 — but it spends one).

## 1. Hard rules (settled by the repo owner; no self-granted exceptions)

These are the owner's standing decisions about bot commands on this repo, so don't re-litigate them
per-PR or talk yourself into an exception — but they bind the agent, not the user: an explicit
instruction from the user supersedes any row here.

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

## 2. Observe state (run these; do not infer)

```bash
gh pr view <n> --json state,isDraft,reviewDecision,mergeStateStatus,headRefOid,reviews
gh api graphql --paginate -f query='query($endCursor:String){repository(owner:"<owner>",name:"<repo>"){
  pullRequest(number:<n>){ reviewThreads(first:100, after:$endCursor){
    pageInfo{hasNextPage endCursor}
    nodes{id isResolved isOutdated path
      comments(first:100){pageInfo{hasNextPage} nodes{author{login} createdAt}}}}}}}'
gh api repos/<owner>/<repo>/issues/<n>/comments --jq \
  '.[] | select(.user.login=="coderabbitai[bot]") | {updated_at, body: .body[0:400]}' | tail -3
gh pr view <n> --comments                                # FULL bodies — see below
# Review BODIES — where "Outside diff range comments" live. NOT in reviewThreads.
gh api repos/<owner>/<repo>/pulls/<n>/reviews --paginate --jq \
  '.[] | select(.user.login=="coderabbitai[bot]") | select(.body|length>0) | .id'
gh api repos/<owner>/<repo>/pulls/<n>/reviews/<id> --jq .body   # once per id above
date -u +%H:%M:%SZ                                       # for window arithmetic
```

**Read the walkthrough comment BODY in full — never grep it for a keyword, never truncate it.**
Its state declarations (rate limit, pause) sit in the body, not in headers, labels, or the first few
hundred characters. On PR #119, grepping it for rate-limit strings and reading ~150 chars hid a
self-declared pause for 7 hours. The `[0:400]` slice above is for timestamps only.

**Two `gh pr view --json` fields that do not exist — both fail as hard errors, so a `|| fallback`
in a poll loop turns each into a silent "no" that never fires:**

- **`merged`** — detect a merge with `state == "MERGED"` (or a non-null `mergedAt`) instead.
- **`reviewThreads`** — there is no REST/`--json` accessor; threads come only from the GraphQL query
  above. This one bit twice on PR #119: once in a watcher, once written into this very file three
  lines under the warning about `merged`.

Before trusting any `gh` field in a loop, run it once bare and look at the output.

**Paginate both connections, and never derive `ACTIONABLE_THREADS` from a truncated read.** A bare
`reviewThreads(last:40)` is a sliding window — on a long-running PR the older threads fall out and
silently stop existing as far as the loop is concerned. Worse, `comments(last:1)` shows only the
newest comment, which cannot tell you whether YOU replied earlier in that thread — and "have I
replied" is exactly what `ACTIONABLE_THREADS` turns on. The cursor variable MUST be named `$endCursor` — `gh api graphql --paginate` looks for that exact
name to inject the next cursor, so calling it `$c` (or anything else) silently returns page one and
stops, which looks identical to "there was only one page". Comments are a NESTED connection and
`--paginate` drives only the outer one, so select `comments(first:100){pageInfo{hasNextPage} ...}`
and treat any thread reporting `hasNextPage: true` as unclassifiable until you fetch its remaining
comments explicitly — do not silently derive `ACTIONABLE_THREADS` from a truncated comment list.

Six observations decide everything:

- **`REVIEWED`** — a formal review exists whose `submittedAt` is after the current head was pushed.
- **`LIMITED(t, n)`** — a CodeRabbit **comment** says "Review limit reached … next review available
  in **n** minutes", last edited at **t**. The limit is **lifted** once `now ≥ t + n`.
- **`PAUSED`** — the walkthrough comment body carries a `> [!NOTE]` block titled **"Reviews
  paused"**: "CodeRabbit has automatically paused this review. You can configure this behavior by
  changing the `reviews.auto_review.auto_pause_after_reviewed_commits` setting." It names its own
  remedies (`@coderabbitai resume`, `@coderabbitai review`) plus checkbox quick-actions. This state
  is SELF-DECLARED and is therefore NOT ambiguous silence — it is invisible only if you skim the body.
- **`OUTSIDE_DIFF`** — findings that exist ONLY in a review's BODY, under a
  `⚠️ Outside diff range comments (N)` heading, because they sit on lines the diff didn't touch.
  They are NOT threads: they never appear in `reviewThreads`, have no thread id, and cannot be
  replied to or resolved per-thread. A triage loop keyed on threads is structurally blind to them —
  on PR #119 there were 20 across 5 review bodies and 5 went unfixed, including a `Critical`
  destructive-save guard, while the thread view read "no actionable threads". Sweep EVERY review
  body (they accumulate; a finding raised in review 2 is not repeated in review 5), and treat each
  as a finding of record: fix it, or state the reason in the commit message, since there is no
  thread to reply on.
- **`OPEN_THREADS`** — unresolved threads from `reviewThreads`.
- **`ACTIONABLE_THREADS`** — the subset of `OPEN_THREADS` that still needs something from you: either you
  have never replied in the thread, or CodeRabbit's latest comment asks a question or requests a
  change. An open thread where you have replied and CodeRabbit's answer asks for nothing is
  **settled** and is NOT actionable, however long it stays open. `ACTIONABLE_THREADS` drives lane 5a of §4;
  keying the loop on `OPEN_THREADS` instead makes a deliberately-deferred thread re-enter §4
  forever. Some acks carry a `<!-- <review_comment_addressed> -->` or `<!-- <review_comment_withdrawn> -->` marker,
  which is a useful hint — but it is applied inconsistently (3 of 15 acks on PR #119), so it
  confirms settledness and never establishes actionability.

**A resolved thread is not necessarily an addressed one.** A thread can resolve because the diff
moved under it (`isOutdated`), not because anyone answered it — so a finding you never triaged can
leave `OPEN_THREADS` on its own and never reappear. Once per review round, list the RESOLVED threads
too and check each for a finding that got no fix and no reply from you; judge those on the merits
regardless of thread state. On PR #119 a `Major` security finding ("do not make repository content an
unscoped agent-control policy") auto-resolved this way after an edit shifted its lines, and was
found only by reading the resolved list. Do NOT reopen such a thread (N3) — fix it and say so in the
next round's commit.

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

| Row | State                                                                                                         | Action                                                                                                                                                              |
| --- | ------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| S0  | PR is a draft                                                                                                 | Nothing. Drafts are skipped entirely and spend no quota. Mark ready when settled.                                                                                   |
| SP  | `PAUSED` (§2) and no open `LIMITED` window                                                                    | Post exactly ONE `@coderabbitai resume`. Then go to S1. ONE per head commit — a pause re-declared after a `resume` on the same head is S7, never a second `resume`. |
| S1  | Not `REVIEWED`, no `LIMITED` message                                                                          | **Wait.** Re-observe later. Post nothing — this includes an hours-long quiet spell.                                                                                 |
| S2  | `LIMITED(t, n)` and `now < t + n`                                                                             | **Wait** until `t + n`. Any command before then is wasted.                                                                                                          |
| S3  | `LIMITED(t, n)` and `now ≥ t + n` and not `REVIEWED`                                                          | Post exactly ONE `@coderabbitai review`. Then go to S1.                                                                                                             |
| S4  | `REVIEWED` and (`ACTIONABLE_THREADS` non-empty OR any unaddressed `ACTIONABLE_OUTSIDE_DIFF`)                  | Run §4 on every `ACTIONABLE_THREADS` entry AND every unaddressed `ACTIONABLE_OUTSIDE_DIFF` finding. One commit, one push. Then S1.                                  |
| S5  | `REVIEWED`, `ACTIONABLE_THREADS` empty, `ACTIONABLE_OUTSIDE_DIFF` all addressed, `reviewDecision != APPROVED` | **Wait** — it re-approves on its own. Do not reach this row without having swept the review bodies.                                                                 |
| S6  | `APPROVED` + CI green + `mergeStateStatus` clean                                                              | **Not done yet** — auto-merge still has to land it. Keep watching; report completion only from `state == "MERGED"` (§2), never from an approval.                    |
| S7  | S3 was taken and the review provably no-oped (0 reviews, 0 threads)                                           | **Stop. Flag a human.** Do not post again (N1 forbids the old escalation).                                                                                          |

`mergeStateStatus: DIRTY` is not in this table because it is not a review state — it means `main`
moved and the branch now conflicts. Merge `origin/main` in (never rebase + force-push a PR branch),
resolve, and re-enter at S1. Preserve what the incoming side added: a conflict in a file both
branches edited is two sessions' findings, not yours versus noise.

## 4. Handling one finding (deterministic)

Findings arrive on two lanes and they close differently. Work BOTH; never let one lane's rule leak
into the other:

| Lane                      | Source                             | How it closes                                                                                                                                                                             |
| ------------------------- | ---------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `ACTIONABLE_THREADS`      | `reviewThreads` (§2)               | Fix and/or reply ON the thread (step 5a). CodeRabbit resolves it itself.                                                                                                                  |
| `ACTIONABLE_OUTSIDE_DIFF` | review BODIES, `OUTSIDE_DIFF` (§2) | Fix and/or record the outcome in the COMMIT MESSAGE (step 5b). There is no thread, so there is nothing to reply to and nothing to resolve — it closes when the next review sees the code. |

A settled thread is terminal even while open — do not re-reply to it, and do not let it hold up
S5/S6.

**Enumerate before fixing.** List every finding from both sources first — threads and all review
bodies — and work the list. Anything not on the list does not get fixed, and a thread-only list is
how a `Critical` reached "done" unaddressed on PR #119.

Per finding, in order:

1. **Re-verify against current code** (`grep`/`sed` the cited `file:line`) — reviews lag pushes and
   rebases, so a finding can already be fixed or have moved.
2. **Classify, from the finding's BODY not its title.** A finding often asks for more than one
   thing — a `Critical` on PR #119 wanted an identity guard AND a scratch-zone restriction, only
   the first landed, and it was reported fixed. Enumerate every requirement in the finding, then
   satisfy or refuse each explicitly. Valid ⇒ step 3. Invalid or out of scope ⇒ step 4. There is no
   third branch.
3. **Fix the root cause**, on the branch of the PR the findings were actually posted on. Batch every
   fix into ONE commit + ONE push (N7). In a stacked pair the checked-out branch is NOT necessarily
   that PR's head (only the main-targeted front of a stack gets reviewed, per §5, so the PR whose
   threads you are reading can silently differ from the branch you are on): confirm with
   `gh pr view <n> --json headRefName,headRefOid` and match BOTH the branch name and
   `git rev-parse HEAD` against `headRefOid` — branch name alone doesn't prove the checkout is at
   the PR's actual head after a force-push — or just `gh pr checkout <n>`.
4. **Sweep for siblings before calling it fixed.** The defect is rarely unique to the cited line:
   grep the module for the same shape. On PR #119 the `bypass-nodes` empty-list fall-through was
   fixed while its exact twin `bypass_all` sat 40 lines away in the same function and shipped. This
   is the agent-side counterpart of the `Behavioral parity` `pre_merge_check` in `.coderabbit.yaml`,
   which exists because omissions — a guard present next door and absent here — are this repo's
   recurring bug shape.
5. **Close the loop on the finding's own lane.**
   - **5a — `ACTIONABLE_THREADS`:** reply ONCE on the thread, including `@coderabbitai` so it
     engages, citing `file:line`, and **stating the reason** when it isn't being fixed. A reply
     without a reason gives it nothing to evaluate and the thread stays open. No second reply, no
     argument.
   - **5b — `ACTIONABLE_OUTSIDE_DIFF`:** do NOT try to reply — there is no thread id, and
     `@coderabbitai` commands are top-level-only, so there is no per-finding channel at all. Record
     the outcome (fixed, or refused and why) in the COMMIT MESSAGE, which is what the next review
     reads. A refusal with no written reason is indistinguishable from having missed the finding.
6. **Resolve nothing** (N2, N3). Stop and re-observe.

Match threads by **stable thread id**, never by `(path, line)` — line numbers shift with the push
and a miss is silent. Comment body is a fallback only after confirming it's unique among the PR's
threads (duplicate findings share near-identical bodies).

**No command operates on a single thread.** Every `@coderabbitai` command is a top-level PR
comment; `approve` and `resolve` are documented top-level-ONLY and do not work inside a thread, and
`resolve` is all-threads-at-once, so there is no per-thread resolve to nudge one thread with.
`resume` is top-level too, and it DOES clear the AUTOMATIC pause (§2 `PAUSED`, §3 row SP) — it is a
PR-level lever, not a thread-level one. A reply is the only thread-level lever that exists.

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
  `require_last_push_approval`). So the last thing you do to a PR is stop pushing. A push while
  awaiting an approval is doubly costly: it voids the approval you are waiting for AND can re-arm
  the automatic pause (§2 `PAUSED`).
- Other commands (`configuration`, `help`, `generate docstrings|unit tests|sequence diagram`,
  `summary`) are informational and harmless, but each adds bot noise. `@coderabbitai ignore` goes
  in the PR **description** and permanently disables auto-review for that PR.
