---
name: coderabbit
description: "How to work with CodeRabbit on this repo's PRs. Use whenever a task touches CodeRabbit in any way — reading or addressing its review findings, replying to threads, wondering why a PR has no review or isn't merging despite green CI, recovering a rate-limited/failed/skipped review, or being tempted to post ANY `@coderabbitai` command. Consult it BEFORE posting a command: the default correct action is almost always to post nothing."
---

# /coderabbit — working with the reviewer

CodeRabbit is this repo's merge-gating reviewer: its formal approval alone satisfies the
"protect main" ruleset's required review (write-access rule; bots can't be CODEOWNERS), so a PR
stuck at zero reviews stays unmergeable no matter how green CI is. That makes review health part
of shipping — and makes the review quota a real resource to spend deliberately.

## The one rule

**Progressive review is automatic.** On a PR that has been reviewed, pushing fix commits and
replying to threads is enough — the incremental review picks up the delta and the replies on its
own, and re-approves once its concerns are addressed. Posting a command in that flow is at best a
wasted quota unit and at worst resets a rate-limit countdown. Default action after addressing
findings: push once, reply on the threads, post nothing.

One documented exception to "automatic": `auto_pause_after_reviewed_commits` (default **5**)
silently pauses incremental review once a PR has accumulated that many reviewed pushes — a
long-lived PR can stop getting reviews with no error anywhere. That pause, like a rate-limit
skip, is a legitimate reason to post one plain `review`.

## The review commands (each has a narrow use)

| Command                     | What it does                        | When to post it                                                                                                                                                                                  |
| --------------------------- | ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `@coderabbitai review`      | Incremental review of new changes   | A review was **skipped** (rate limit / failure — skips never auto-retry; wait out the "next review available in N minutes" window first), or auto-review **paused** after many reviewed commits. |
| `@coderabbitai full review` | From-scratch review of the whole PR | You deliberately want everything re-reviewed — or a recovery `review` **provably no-oped** (see below).                                                                                          |

Both consume one review from the quota per execution. Known trap behind the escalation clause: a
rate-limited attempt can mark the head commits as _processed_, so a later plain `review`
"finishes" in seconds having reviewed nothing. That proven no-op is the only case where
`full review` is the recovery, posted ONCE in a quiet window.

## Verify a review actually ran

Never trust ack timing or the walkthrough saying "finished" — CodeRabbit edits its ONE
walkthrough comment in place, so only the latest attempt's outcome is visible, and an ack has
been observed claiming completion on a silent no-op. The reliable test:

```bash
gh pr view <n> --json reviews,reviewThreads   # formal reviews + threads on the CURRENT head
```

0 formal reviews + 0 threads after a "finished" ack = the review never ran.

## Recovery ladder (for a main-targeted, non-draft, same-repo PR with no review on its head)

1. Walkthrough says "Review limit reached … next review available in N minutes" → wait until
   (last edit + N min). Before that, any command is wasted.
2. After the window (or for failure/skip/pause states with no window): post ONE
   `@coderabbitai review`.
3. If that provably no-ops (0 reviews / 0 threads on the head): ONE `@coderabbitai full review`,
   in a quiet window (same-day dependabot/auto-merge PRs drain the same shared quota).
4. Three recovery commands after the current head with no real review → stop; flag for human
   attention instead of posting a fourth.

## Addressing findings

- Verify each finding against **current** code first — reviews can lag pushes and rebases.
- Fix root causes; batch ALL of a PR's fixes into ONE commit + ONE push (each push to a
  main-targeted PR spends a review attempt — never push cosmetically).
- A finding that is wrong or deliberately not applicable gets ONE factual reply on its thread
  citing file:line — never a fake-fix to appease the bot, and no further argument on that thread.
  Include `@coderabbitai` in the reply when you want the bot to actually engage with the rebuttal
  (it answers contextually and can concede); a plain reply is only a note for the next review
  pass and human readers.
- `dismiss_stale_reviews_on_push` is on: an approval is dismissed by any later push, so the
  approval that merges must postdate the final commit.

## Other commands — safety notes for THIS repo

- **`@coderabbitai approve` — never post it.** It resolves all threads AND submits a formal
  approval; since CodeRabbit's approval alone satisfies this repo's merge gate, posting it is
  self-approving the merge. Same class as "never approve/merge your own PR".
- **`@coderabbitai autofix` — don't use here.** It pushes CodeRabbit-authored fixes from its own
  side, which bypasses the local gate stack (`scripts/gates.sh` stamp, /simplify, HW checks).
  Implement findings locally through the gates instead.
- **`@coderabbitai resolve`** — blunt: resolves ALL CodeRabbit threads at once (top-level
  comment only). Prefer per-thread replies; reach for `resolve` only after genuinely addressing
  everything.
- **`@coderabbitai pause` / `resume`** — quota-friendly during a rapid push series on an
  already-ready PR (drafts are the better tool when available; pause doesn't block manual
  commands).
- **`@coderabbitai ignore`** — goes in the PR **description** (not a comment), permanently
  disables auto-review for that PR until removed.
- `configuration` / `help` / `generate docstrings|unit tests|sequence diagram` — informational /
  finishing touches; harmless but each generation is more bot noise on the PR.

## Quota economics (free/OSS tier — small, shared, adaptive)

Spends a review: every push to a main-targeted PR, every retarget-to-main, every manual review
command. Refills slowly (~a few/hour, throttling toward ~1/hour under sustained multi-PR
activity).

Free: **draft PRs are skipped entirely** (`auto_review.drafts` default) — iterate in draft, mark
ready when settled; pushes to stacked descendants whose base is NOT main (auto-review fires only
for default-branch-targeted PRs unless `base_branches` extends it — so only the main-targeted
front of a stack is reviewed; each child meets CodeRabbit for the first time when it retargets to
main after its parent merges, so budget one review per cascade step).
