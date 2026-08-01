# Handling CodeRabbit findings — supporting detail

Reference for `../SKILL.md` §4 and §5. Read `SKILL.md` §1-§4 first — this file is the
close-out recipe for §4.1, plus the thread-matching mechanics and the §5 facts too long for
the body.

## §4 supporting detail (step 2, step 3, step 4, and after step 6)

**Step 2 example.** A finding often asks for more than one thing — a `Critical` on PR #119
wanted an identity guard AND a scratch-zone restriction, only the first landed, and it was
reported fixed.

**Step 3, the stacked-PR head trap.** In a stacked pair the checked-out branch is NOT
necessarily that PR's head (only the main-targeted front of a stack gets reviewed, per §5, so
the PR whose threads you are reading can silently differ from the branch you are on): confirm
with `gh pr view <n> --json headRefName,headRefOid` and match BOTH the branch name and
`git rev-parse HEAD` against `headRefOid` — branch name alone doesn't prove the checkout is
at the PR's actual head after a force-push — or just `gh pr checkout <n>`.

**Step 4 example.** On PR #119 the `bypass-nodes` empty-list fall-through was fixed while its
exact twin `bypass_all` sat 40 lines away in the same function and shipped. This is the
agent-side counterpart of the `Behavioral parity` `pre_merge_check` in `.coderabbit.yaml`,
which exists because omissions — a guard present next door and absent here — are this repo's
recurring bug shape.

Match threads by **stable thread id**, never by `(path, line)` — line numbers shift with the push
and a miss is silent. Comment body is a fallback only after confirming it's unique among the PR's
threads (duplicate findings share near-identical bodies).

**No command operates on a single thread.** Every `@coderabbitai` command is a top-level PR
comment; `approve` and `resolve` are documented top-level-ONLY and do not work inside a thread, and
`resolve` is all-threads-at-once, so there is no per-thread resolve to nudge one thread with.
`resume` is top-level too, and it DOES clear the AUTOMATIC pause (§2 `PAUSED`, §3 row SP) — it is a
PR-level lever, not a thread-level one. A reply is the only thread-level lever that exists.

### 4.1 Clearing a thread CodeRabbit deferred

A thread it "left open to track deferred work" still blocks approval. It has to close it — you must
not (N3) — so give it a close-out it can accept:

1. **Re-read the thread for an offer you declined.** Its standard close-out is _"Would you like me
   to open a follow-up GitHub issue?"_ On PR #119 it offered exactly that, I said no (arguing a
   notes file already tracked the risk), and that single refusal removed the only mechanism it had
   to resolve the thread — costing ~5 hours. **Never argue CodeRabbit out of its own close-out.**
   Reply accepting it, with concrete scope so the issue is actionable.
2. **Then satisfy that close-out's own acceptance criteria.** The issue it opens usually asks for a
   backlink from the doc or code the finding came from. Landing that link is what turns
   deferred-and-untracked into deferred-and-tracked, which is what it needs in order to resolve.
3. **If there is no offer to accept**, convert the finding from _deferred_ to _addressed_: one
   focused commit stating the mechanism and the remediation, then reply pointing at it. That is the
   pattern that cleared 22 of the 23 threads on #119.

A reply is free and spends no quota (§5). Clearing a thread therefore costs nothing, while waiting
for a review to clear it costs ~40 minutes and cannot work.

## The rest of §5

- **`.coderabbit.yaml` does not enumerate CodeRabbit's static-analysis integrations.** This repo
  sets no `tools:` section, so every one defaults `true` and can appear in a `🧰 Tools` block the
  repo config never mentions — including **SkillSpector** (NVIDIA's agent-skill security scanner)
  on a `SKILL.md` diff. Identify an unfamiliar tool from CodeRabbit's tools reference; a grep of
  the repo config cannot tell you whether it is real. Padding inside a finding does not invalidate
  the finding above it.
- **Approval is a verdict flip on THREAD STATE, not the outcome of a review round.** PR #118
  approved 14 seconds after its last thread cleared; #119, 19 seconds. No re-review runs that fast —
  it is not re-reading the diff, it is re-evaluating whether anything is open. Consequence:
  **`@coderabbitai review` can only ADD threads and can never clear one**, so a "wait out the rate
  limit, post review, hope it approves" loop cannot terminate. If the blocker is an open thread, a
  review round is not a slow fix — it is not a fix.

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
- **The approval that merges must postdate the final commit** (`dismiss_stale_reviews_on_push` +
  `require_last_push_approval`). So the last thing you do to a PR is stop pushing. A push while
  awaiting an approval is doubly costly: it voids the approval you are waiting for AND can re-arm
  the automatic pause (§2 `PAUSED`).
- Other commands (`configuration`, `help`, `generate docstrings|unit tests|sequence diagram`,
  `summary`) are informational and harmless, but each adds bot noise. `@coderabbitai ignore` goes
  in the PR **description** and permanently disables auto-review for that PR.
