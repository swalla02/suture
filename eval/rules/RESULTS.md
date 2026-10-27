# Rules: Suture against prompting, on RuleArena's airline bag fees

Stage 1 of the rule-decisions experiment (`docs/superpowers/specs/2026-10-26-rule-decisions-experiment.md`). RuleArena's airline task: from the policy text and a passenger's trip and items, compute the ticket plus every checked-bag, overweight and oversize fee. 300 cases at three levels (5, 8 or 11 items), from github.com/skyriver-2000/RuleArena at commit 59f60f6b (MIT; `airline/data/SOURCE.md`).

The setups:
- **R**, the reference: RuleArena's `compute_answer.py`, read and rewritten in Rust (`harness/src/reference.rs`), never run. Its answers are the ground truth.
- **S1**, Suture: `airline/suture/airline.st` and its fee rows (`charges.csv`), written by us from the policy text. The numbers below are of this first version, frozen at tag `rules-airline-frozen`; version 2, rewritten after slice 64, gives the same answers (see the end).
- **Extraction**, for R and S1: `typesafe/jev-1.14` through OpenRouter's Decisions API answers typed questions (class, region, direction, ticket, number of items, and each item's three dimensions and weight), each a choice among fixed options. A number is a choice among the numbers in the case's text. The model never writes free text.
- **P1**, prompting: `openai/gpt-oss-120b` with RuleArena's own zero-shot prompt, reasoning effort low, temperature 0.
- **P3**, code generation: the same model writes a Python program for each case. We run it in a container with no network, a read-only file system, no privileges and a 10-second limit.

The split, a keyed hash of the case id, was committed before any model was asked: 56 dev cases, 244 test. Everything was tuned on dev and frozen at tag `rules-airline-frozen`, and test was then run once. Every model answer is recorded (`~/rules-runs/airline`, outside the repo), so `cargo run -p rules-airline -- score test` gives these numbers again without asking anything. Per-case answers: `airline/results/{dev,test}.csv`.

## Numbers (test, 244 cases)

| setup | right | accuracy | right on all 3 runs | same answer on all 3 runs | $ per case | s per case |
|---|---|---|---|---|---|---|
| R, true facts | 244 | 100% | — | — | 0 | 0.00 |
| S1, true facts | 244 | 100% | — | — | 0 | 0.10 |
| R, extracted facts | 244 | 100% | 57 of 57 | 57 of 57 | 0.00064 | 0.90 |
| **S1, extracted facts** | **244** | **100%** | **57 of 57** | **57 of 57** | 0.00064 | 1.00 |
| P1, prompting | 52 | 21.3% | 2 of 57 | 3 of 57 | 0.00035 | 30.2 |
| P3, code per case | 68 | 27.9% | 7 of 57 | 12 of 57 | 0.00029 | 27.1 |

Paired on the same cases (run 1):

| comparison | only first right | only second right | difference | 95% bootstrap interval | exact McNemar p |
|---|---|---|---|---|---|
| S1 extracted − P1 | 192 | 0 | +78.7 pts | [+73.4, +83.6] | 3e-58 |
| S1 extracted − P3 | 176 | 0 | +72.1 pts | [+66.4, +77.5] | 2e-53 |
| S1 extracted − R extracted | 0 | 0 | 0 | [0, 0] | 1 |
| P3 − P1 | 38 | 22 | +6.6 pts | [+0.4, +12.7] | 0.05 |

By level (items per case 5 / 8 / 11):

| setup | level 0 | level 1 | level 2 |
|---|---|---|---|
| S1, extracted | 82 of 82 | 86 of 86 | 76 of 76 |
| P1 | 26 of 82 | 18 of 86 | 8 of 76 |
| P3 | 26 of 82 | 21 of 86 | 21 of 76 |

Dev (56 cases, all three runs on every case) gave the same picture: S1 extracted 100%, the same answer on all 56; P1 12.5%, the same answer on 6; P3 26.8%, the same answer on 9.

An answer that was expected and never recorded (an API failure, the budget stop) would count as wrong. None happened: every setup has every case.

Extraction misread the direction of 2 test trips (and 1 dev trip), all to or from regions whose fees are the same both ways, so no answer changed. It misread nothing else: no class, region, ticket, count, dimension or weight. P3's programs ran in 242 of 244 cases. One printed no number, and one raised an `IndexError`.

The whole of Stage 1 cost $0.70 at OpenRouter: dev $0.23 and test $0.47.

## The hypotheses

- **H1, accuracy: holds for airline**, against P1 and P3. The intervals are far from zero. P2, the reasoning model, is not run until Stage 2, so the claim is about a cheap model.
- **H2, error source: holds.** S1 equals R on all 300 cases and on 400 random trips that reach the parts of the policy the cases leave untried (`sutured.rs`): every region and class, bags from 30 to 130 lbs and 45 to 145 in. Where the policy text says "Not accepted" (over 70 lbs to Europe, Israel, Qatar, Australia and New Zealand; over 100 lbs; over 115 in), RuleArena's script charges the highest fee anyway. S1's rows follow the script, since its answers are the ground truth. The generator never makes such a bag, but an extraction error could. With extracted facts S1 is still exact, so the extraction error that remains, 2 misread directions, never changes an answer.
- **H4, consistency: holds across runs, untested across rewordings.** Suture's answer was the same on every run because the extraction answers were. P1 gave three equal answers on 3 of 57 cases, P3 on 12. The reworded variants are not made yet.
- H3, H5, H6: later stages.

## How far to trust this

- **S1 is not held out the way the models are.** We checked it against R on all 300 cases while writing it, test included, because getting the rules exactly right is the point of a hand-written program. What test measures for S1 is extraction. A fair test of *authoring* is S2 (an LLM writes the Suture rules, H3), in Stage 2.
- **The cases are easy to read.** RuleArena generates every case from one template, so the typed questions had little to get wrong. Rewordings, irrelevant details and shuffled order (the spec's variants) are the real test of extraction, and are not done yet.
- **P1 is the cheap arm.** Low reasoning effort, as the spec's P1 is "one cheap open model". A reasoning model (P2) will do better. The gap to beat will be smaller, not gone: level 2 is where prompting collapses (8 of 76).
- **Fewer runs than the spec asks.** Three runs on a seeded sample of 57 test cases (`sample.txt`), one on the rest, to keep the stage under $2. Dev had three runs of everything.
- **R is in Rust, not Python.** It is a rewrite of the generator's own script, checked by hand against the script's logic. The spec asked for plain Python.

## What we learned about Suture

**Where it wins.** Once the rules are right, they stay right: the answer follows from the facts, the same every time. The models' errors were the ones RuleArena reports, made fresh on each case:
- the 3rd and 4th bags charged at the 1st and 2nd bags' fees;
- the 70 lbs allowance for Business forgotten;
- the free allowance given to the first bags rather than to the bags where it saves the most.

P3 writes the same policy anew for every case and gets a different part wrong each time. The typed extraction model costs $0.0006 per case and made no error that mattered.

**Where it loses.** Writing `airline.st` took far more effort than writing R. The policy is a calculation (a table lookup, a max, a sum, an allocation by saving), and Suture v0 has no arithmetic beyond formulas without comparisons. The language gaps the policy hit, with their workarounds, are in `airline/GAPS.md`. The worst three:
- **The answer is not a Suture value.** No sentence can read what a part was granted, so the harness adds up the granted fee rows. The last step of the answer sits outside the language, where no rule can check it and `suture explain` does not show it.
- **Input order carries meaning.** A bag's fee waits on a `weigh` event that the harness sends after the free allowances are given out. Weighed too early, a bag pays the 50 lbs rate.
- **The 4096-byte unit limit forced four redesigns,** each found by a run-time fault, none by the compiler: one row per region, the fee pick on each bag, one unit per free place, and the pool limited to bags that save.

**A rule written by hand that a program would derive.** The free allowance goes to the bags where it saves the most. Suture cannot rank by a computed difference, so we worked out the three tiers by hand (heavy and snug, heavy and close, plump and snug). They hold for these tables. A new fee could reorder them, and nothing would say so.

**What this means for the next stage.** Before tax, which is all arithmetic, Suture needs three things:
- comparisons and conditions in formulas, with `max` and `min`;
- sums over the rows a unit was granted;
- a unit limit that fails at compile time, or no unit limit at all.

Without them, tax would not be a test of the paradigm but a puzzle in working around it.

## Version 2, after slice 64

Slice 64 (spec appendix BO) added the three things above:
- conditionals and `the larger of` in calculations;
- defined values, computed whenever read;
- 64 KiB units.

`airline.st` was rewritten with them:
- The overweight and oversize tables are formulas on Region and Bag, not rows.
- The free allowance is ranked by each bag's saving (`prefer Bag: highest saving`), not by hand-made tiers.
- Each bag's fee is a defined value, which the harness reads (`Host::value`) and adds to the ticket.
- `charges.csv`, the Charge, Grace, Paid and Free kinds, and the `weigh` event are gone: 7 kinds instead of 11, and no input order that carries meaning.

On rescoring from the recorded answers, at no cost:
- Version 2 gives version 1's answer on all 56 dev and 244 test cases, with true and with extracted facts.
- It equals R on the 400 random trips.
- It passes the five scenarios, which now check fees directly (`expect B's fee is 400 USD`).
- It runs at 0.06 s a case with true facts, against 0.10 s for version 1. The numbers in the tables above are unchanged and stay those of version 1.

`airline/GAPS.md` gives each gap's status. Still open:
- the trip's total, a sum of defined values across units, which the harness makes;
- a unit size the compiler can bound;
- the default of 8 candidates in a pool.
