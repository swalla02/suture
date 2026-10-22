# Can a coding agent write Suture? (2026-10-22)

Most code is now written by agents, and no agent has seen Suture in training. This eval measures how much a fresh agent has to do to write a passing module from a plain-English spec, with and without the agent kit (`suture help`, `suture check`, `suture init`; spec appendix AO).

## Setup

- The agent is a fresh Claude Sonnet subagent with no context, given the task, a copy of the repository without the spec and evals, and the `suture` binary. It may not build, run cargo or use the network. It reports what it ran, what failed and what it read.
- **Today's main** is commit ee9a996: the guide, the reference, `build` and `run`.
- **The kit** adds the cards (`suture help`), `suture check`, and a directory made by `suture init` (AGENTS.md).
- [TASK-A.md](TASK-A.md), library loans, was run on today's main only, to find what agents get stuck on. The cards then took its lessons (the time card shows its overdue idiom), so it would not be a fair test of them.
- [TASK-B.md](TASK-B.md), talk submissions, is new to both and is in no card. It was run once on each.

## Results

| | A, today's main | B, today's main | B, the kit |
|---|---|---|---|
| scenarios passing at the end | 11 of 11 | 9 of 9 | 8 of 8 |
| lines of documentation read | ~1,900 | ~1,600 | ~550 |
| runs of the binary | 6 | 14 | 16 |
| runs with compile errors / failing scenarios | 2 / 2 | 6 / 3 | 5 / 2 |
| agent tokens | 83k | 108k | 71k |
| tool calls | 15 | 28 | 20 |
| wall time | 121 s | 375 s | 168 s |

One run per cell, so the numbers are indicative, not statistics.

## What it shows

- **The kit cuts what the agent reads by about two thirds,** and with it the tokens (−34%) and the time (−55%) for task B. The agent went to `help start`, `recipes` and `limits`, then to the cards it needed, and grepped the guide only to confirm.
- **It does not cut trial and error yet.** Both runs of B spent their failed compiles on the same things, which are gaps in the language, not in the docs:
  1. **Uniqueness of a pair** ("a reviewer reviews a talk at most once"). Neither agent could write it, and both modelled the pair as an entity, with duplicates not prevented. This is the next language slice the eval points at.
  2. **A value from a fixed set of numbers** (30 or 45). One agent found `define … when its length is 30 or its length is 45` plus `admit … where`; the other built a lookup entity. It is now a recipe.
  3. **An average,** and a `Count` formula below 0 silently not being negative. One agent compared totals (`total score is at least number * 3`), the other used Money. It is now a recipe and a limit.
  4. **Rules on the system's own work wait instead of refusing.** Both agents expected a rule to refuse a request that creates work, and found it waiting. It is now in the limits card.
- **Task A's lessons** were all idioms. Becoming overdue on its own, reading one entity's facts from another, and a part not being withdrawn once its request has gone out are now in the time, recipes and limits cards.

## Next

- A language slice for uniqueness, e.g. `rule once: no two Reviews have the same talk and reviewer`, or a count filtered by the caller.
- A time-driven state (`when a Loan's due passes: …`), so "overdue on its own" needs no reminder part.
- Run B again with the cards as they are now, and a third task, before claiming a trend.
