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

One documented wrinkle: `auto_pause_after_reviewed_commits` (default **5**) silently pauses
incremental review once a PR has accumulated that many reviewed pushes — a long-lived PR can stop
getting reviews with no error anywhere. It looks exactly like ambiguous silence and it does NOT
license a command; a PR that has genuinely stalled this way goes to a human.

## The review commands

There is exactly ONE command you may ever post on this repo:

| Command                     | Verdict                                                                                                                                                                                      |
| --------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `@coderabbitai review`      | Post **only once a review limit has been lifted** — CodeRabbit said "Review limit reached … next review available in N minutes" and that window has since elapsed. Nothing else licenses it. |
| `@coderabbitai full review` | **Never post it.** Standing instruction. Not as an escalation, not for a proven no-op, not in a quiet window — never.                                                                        |

Ambiguous silence is NOT a lifted limit: with no explicit limit message there is nothing to lift,
so the action is to keep watching, not to post. A limit message whose window has not yet elapsed
is not lifted either — waiting is the action.

Known trap (worth recognizing, no longer actionable): a rate-limited attempt can mark the head
commits as _processed_, so a later plain `review` "finishes" in seconds having reviewed nothing.
With `full review` off the table, a proven no-op escalates to a **human**, never to a second
command.

## Verify a review actually ran

Never trust ack timing or the walkthrough saying "finished" — CodeRabbit edits its ONE
walkthrough comment in place, so only the latest attempt's outcome is visible, and an ack has
been observed claiming completion on a silent no-op. The reliable test:

```bash
gh pr view <n> --json reviews,reviewThreads   # formal reviews + threads on the CURRENT head
```

0 formal reviews + 0 threads after a "finished" ack = the review never ran.

Corollary for watchers: after a fix push, `reviewDecision` STAYS `CHANGES_REQUESTED` — a push
dismisses approvals (`dismiss_stale_reviews_on_push`) but NOT a request-changes review, which
stands until a new review supersedes it. Key "has the re-review happened?" on a NEW review with
`submittedAt` after the push, never on the decision flipping.

## Recovery ladder (for a main-targeted, non-draft, same-repo PR with no review on its head)

1. No explicit limit message → **post nothing, keep watching.** Quiet is not a state you fix.
2. Walkthrough says "Review limit reached … next review available in N minutes" → wait until
   (last edit + N min). Before that, any command is wasted.
3. Once that window has elapsed — the limit is now _lifted_ — post ONE `@coderabbitai review`.
4. If it provably no-ops (0 reviews / 0 threads on the head): **stop and flag for a human.** Do
   not post a second command, and never `full review`.

## Addressing findings

The whole loop, in one line: **fix the valid ones, explain the invalid ones, resolve nothing.**
Both a fix and a reasoned rebuttal are things CodeRabbit evaluates and acknowledges on its own —
it marks the thread resolved itself, and that resolution is the receipt that it agreed. Closing a
thread by hand forges that receipt.

- Verify each finding against **current** code first — reviews can lag pushes and rebases.
- **`.coderabbit.yaml` does not enumerate CodeRabbit's static-analysis integrations.** This repo sets
  no `tools:` section, so every one of them defaults `true` and can appear in a `🧰 Tools` block that
  the repo config never mentions — including **SkillSpector** (NVIDIA's agent-skill security scanner,
  v2.3.11) on a `SKILL.md` diff. Identify an unfamiliar tool from CodeRabbit's tools reference; a grep
  of the repo config cannot tell you whether it is real.
- Fix root causes; batch ALL of a PR's fixes into ONE commit + ONE push (each push to a
  main-targeted PR spends a review attempt — never push cosmetically).
- A finding that is wrong or deliberately not applicable gets ONE factual reply on its thread
  citing file:line, WITH the reason spelled out — a reply that doesn't explain why the finding
  isn't being fixed doesn't give CodeRabbit anything to evaluate, and it won't resolve the thread
  on its own. Never a fake-fix to appease the bot, and no further argument on that thread.
  When replying after a fix push, match threads by the stable thread id — never by `(path, line)`,
  since line numbers shift with the push and the miss is silent. Comment BODY is a fallback only
  after confirming it's unique among the PR's threads (duplicate findings can share near-identical
  bodies, which would reply to the wrong thread).
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
- **`@coderabbitai resolve` — never post it.** Resolving is CodeRabbit's signal that it accepted
  a fix or a rebuttal; resolving on its behalf destroys the only evidence that it agreed. Same
  reason you never click GitHub's own "Resolve conversation" or call the
  `resolveReviewThread` mutation. Fix or explain, then leave the thread alone.
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
activity) — and the quoted "next review available in N minutes" window itself inflates with
same-day spend, so never assume attempt N's window applies to attempt N+1 (a four-PR cascade
saw 42/33/27/59-minute windows).

Free: **draft PRs are skipped entirely** (`auto_review.drafts` default) — iterate in draft, mark
ready when settled; pushes to stacked descendants whose base is NOT main (auto-review fires only
for default-branch-targeted PRs unless `base_branches` extends it — so only the main-targeted
front of a stack is reviewed; each child meets CodeRabbit for the first time when it retargets to
main after its parent merges, so budget one review per cascade step).
