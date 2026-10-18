# Observing CodeRabbit review state

Reference for `../SKILL.md` §2. This is the full command toolkit plus every trap in reading
its output, and the long-form definitions of the eight observations §2 summarizes. Read `SKILL.md` §1-§3
first — this file is supporting detail, not the decision procedure.

## Command toolkit

```bash
gh pr view <n> --json state,isDraft,reviewDecision,mergeStateStatus,headRefOid,reviews
gh api graphql --paginate -f query='query($endCursor:String){repository(owner:"<owner>",name:"<repo>"){
  pullRequest(number:<n>){ reviewThreads(first:100, after:$endCursor){
    pageInfo{hasNextPage endCursor}
    nodes{id isResolved isOutdated path
      comments(first:100){pageInfo{hasNextPage endCursor} nodes{author{login} createdAt body}}}}}}}'
# --paginate advances ONLY the outer reviewThreads connection. Any thread reporting
# comments.pageInfo.hasNextPage:true is UNCLASSIFIABLE until you fetch its remaining
# comments explicitly (re-query that thread id with comments(after:<endCursor>)).
# `body` is required: "has CodeRabbit asked for something" cannot be read from authors alone.
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
hundred characters. The `[0:400]` slice above is for timestamps only.

**Two `gh pr view --json` fields that do not exist — both fail as hard errors, so a `|| fallback`
in a poll loop turns each into a silent "no" that never fires:**

- **`merged`** — detect a merge with `state == "MERGED"` (or a non-null `mergedAt`) instead.
- **`reviewThreads`** — there is no REST/`--json` accessor; threads come only from the GraphQL query
  above.

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

## The eight observations, in full

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
  replied to or resolved per-thread. A triage loop keyed on threads is structurally blind to them, and
  the thread view reads "no actionable threads" while they go unfixed. Sweep EVERY review
  body (they accumulate; a finding raised in review 2 is not repeated in review 5), and treat each
  as a finding of record.
- **`ACTIONABLE_OUTSIDE_DIFF`** — the `OUTSIDE_DIFF` subset you have not yet dealt with. One becomes
  **addressed** when the fix, or a written refusal reason, lands in a commit message on this PR
  (§4 lane 5b). There is no thread to reply on, so that commit is the ONLY record — an unexplained
  refusal is indistinguishable from a finding you never read.
- **`OPEN_THREADS`** — unresolved threads from `reviewThreads`.
- **`ACTIONABLE_THREADS`** — the subset of `OPEN_THREADS` that still needs something from you: either you
  have never replied in the thread, or CodeRabbit's latest comment asks a question or requests a
  change. An open thread where you have replied and CodeRabbit's answer asks for nothing is
  **settled** and is NOT actionable, however long it stays open. `ACTIONABLE_THREADS` drives lane 5a of §4;
  keying the loop on `OPEN_THREADS` instead makes a deliberately-deferred thread re-enter §4
  forever. Some acks carry a `<!-- <review_comment_addressed> -->` or `<!-- <review_comment_withdrawn> -->` marker,
  which is a useful hint — but it is applied inconsistently, so it
  confirms settledness and never establishes actionability.

**`UNTRIAGED_RESOLVED` — a resolved thread is not necessarily an addressed one.** A thread can resolve because the diff
moved under it (`isOutdated`), not because anyone answered it — so a finding you never triaged can
leave `OPEN_THREADS` on its own and never reappear. Once per review round, list the RESOLVED threads
too and check each for a finding that got no fix and no reply from you; judge those on the merits
regardless of thread state. Do NOT reopen such a thread (N3) — fix it and say so in the
next round's commit.

## LIMITED timestamp arithmetic

**Read `LIMITED` from the COMMENT's own text and edit timestamp, never from the check run.** The
PR's CodeRabbit check-run label (e.g. "Review rate limited") is a stamp from the attempt that raised
it and does NOT clear when the window passes; trusting the label instead of the quoted window can
turn a 3-minute wait into hours of idle babysitting.

**The deadline is absolute; `n` is remaining time recomputed at each render.** An attempt made
while the window is open costs a quota unit but does not push the window out; a NEW window is armed
only by an attempt made after the previous one lapsed (observed once, for a push — still wait, but do
not treat an accidental attempt as having reset the clock).

**Record `LIMITED(t, n)` the moment you see it — it is not durable.** CodeRabbit reuses ONE
walkthrough comment, so the limit notice gets EDITED AWAY when that comment is next regenerated. Its later absence proves nothing in either direction: do not
re-derive the state from the comment you can still see, and do not treat the disappearance as the
limit lifting. Compute `t + n` from the observation you captured.

**Thread replies are served even when review quota is exhausted.** CodeRabbit answered three thread
replies within 13 seconds while no review had run on the head. So "it responds to replies but no
review appears" is the LIMITED signature, not a dead integration — and conversely, a fast reply is
NOT evidence that a review will run. Never infer `REVIEWED` from reply liveness.

## Two more traps that make observation lie

- **Never key on `reviewDecision` flipping.** A push dismisses approvals
  (`dismiss_stale_reviews_on_push`) but NOT a request-changes review, which stands until a new
  review supersedes it. `CHANGES_REQUESTED` after your fix push is expected, not a signal.
- **Never key on an ack or the walkthrough saying "finished".** CodeRabbit edits its ONE
  walkthrough comment in place, and an ack has been observed claiming completion on a silent no-op.
  `REVIEWED` is defined by `submittedAt`, nothing else.
