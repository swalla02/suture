# Can a coding agent write Suture? (2026-10-22)

Most code is now written by agents, and no agent has seen Suture in training. This eval measures how much a fresh agent has to do to write a passing module from a plain-English spec, with and without the agent kit (`suture help`, `suture check`, `suture init`; spec appendix AO).

## Setup

- The agent is a fresh Claude Sonnet subagent with no context, given the task, a copy of the repository without the spec and evals, and the `suture` binary. It may not build, run cargo or use the network. It reports what it ran, what failed and what it read.
- **Today's main** is commit ee9a996: the guide (22 chapters, about 55k tokens), the reference, `build` and `run`.
- **Cards** were the first kit: eleven `suture help` cards (about 6.5k tokens), `suture check`, and `suture init`'s AGENTS.md.
- **The book** is the kit as merged: one tested file, docs/agents/suture.md (`suture help all`, about 4.5k tokens), with `check` and `init`.
- [TASK-A.md](TASK-A.md), library loans, was run on today's main only, to find what agents get stuck on. Its lessons went into the kit, so it would not be a fair test of it.
- [TASK-B.md](TASK-B.md), talk submissions, is in neither the cards nor the book. It was run once on each.

## Results

| | A, today's main | B, today's main | B, cards | B, the book |
|---|---|---|---|---|
| scenarios passing at the end | 11 of 11 | 9 of 9 | 8 of 8 | 7 of 7 |
| requirements fully met | 6 of 7 | 6 of 7 | 6 of 7 | 6 of 7 |
| lines of documentation read | ~1,900 | ~1,600 | ~550 | ~340 (the book only) |
| runs of the binary | 6 | 14 | 16 | 11 |
| runs with compile errors / failing scenarios | 2 / 2 | 6 / 3 | 5 / 2 | 2 / 3 |
| agent tokens | 83k | 108k | 71k | 61k |
| tool calls | 15 | 28 | 20 | 13 |
| wall time | 121 s | 375 s | 168 s | 106 s |

One run per cell, so the numbers are indicative, not statistics. In each run of B the unmet requirement is the same: a reviewer reviews a talk at most once.

## What it shows

- **One book read once beats searching.** With the book, the agent read `suture help all` and nothing else, never opened the repository, and used 43% fewer tokens, 54% fewer tool calls and 72% less time than with the guide. The cards, which it read a few at a time, sat in between.
- **Compile errors fell from 6 to 2.** With the book, both came from trying to write the one thing the language cannot say. The failing scenarios were the agent's own arithmetic in test data.
- **What is left is the language, not the docs:**
  1. **Uniqueness of a pair** ("a reviewer reviews a talk at most once"). No run could write it. Inside a `that` filter every name means the counted entity, so a filter cannot compare with the outer one or the caller; the book now says so. This is the next language slice the eval points at.
  2. **A value from a fixed set of numbers, an average, and a `Count` below 0.** Each run worked these out differently; they are now recipes and limits in the book, and the book run used them at once.
  3. **Rules on the system's own work wait instead of refusing.** Now in the book's semantics.
  4. **Long traces.** A failing scenario's whole trace is long; the book run filtered it with grep.
- **Task A's lessons** were all idioms: becoming overdue on its own, deriving one entity's state from what references it, and a part that is not withdrawn once its request has gone out. All three are in the book.

## Next

- A language slice for uniqueness, e.g. `rule once: no two Reviews have the same talk and reviewer`, or a filter that can name the outer entity.
- A time-driven state (`when a Loan's due passes: …`), so "overdue on its own" needs no reminder part.
- `suture check` showing the steps around a failure first, not only the whole trace.
- More tasks and repeated runs before claiming a trend.
