# Rule decisions: a rigorous test of Suture

Status: design, agreed 2026-10-26. Replaces chess as the main evidence for Suture; the chess ladder (`eval/chess/`) stays as a demo.

## Why not chess

The chess ladder showed that a model plus Suture rules beats the model alone for chess-trained models (NNUE 1039 → 1747, Maia 1100 1360 → 1642, 80 games each, ±80). But:

- much of the strength is in the facts the Rust harness computes (`mating`, `exposing`, `gains`, `drops`); Suture combines and orders them, and the language's own share is not measured;
- general decision models with Suture land at or below Suture's rules with no model (1355);
- the top gain is small (Maia 1900 1663 → 1794), and 1794 is weak for chess;
- the strategist tuned on the same opponents it was tested against.

Chess tests the chess code more than Suture. Suture's claim is about decisions that follow a written policy: a model reads the case, rules decide, and people can read, check and change the rules.

## Prior work

- **RuleArena** (Zhou et al., ACL 2025, CC BY 4.0, arXiv 2412.08972). 95 real rules, 816 cases in three domains: airline baggage fees, US tax, NBA transactions. Each case is plain English; airline and tax answers are computed by rule scripts, so they are exact. NBA was labelled by hand. LLMs do badly: GPT-4o and Claude 3.5 Sonnet 0–32% on airline, 0% on hard tax. Main failures: a conditional rule that applies only in some cases is skipped; arithmetic and aggregation slips; irrelevant forms distract.
- **Policy-as-logic** (arXiv 2608.11905, 2026). Same split as Suture: an LLM pulls facts as JSON from a schema, an Answer Set Programming solver applies rules that Claude Opus wrote from the policy text, once per policy. Airline 0.61–1.00 against 0.38 for the best prompt; tax 0.31; NBA 0.36–0.50; about 10× fewer tokens. No human checked the written rules; most failures are extraction errors; on judgment-based HR moderation (PolyGuard) it gives no advantage.
- **PolicyGuard** (arXiv 2606.32004, 2026): the same split for NDA review; no quantitative results.
- **PolicyLLM** (ICML 2026): policy compiled to decision graphs checked with Z3.

So "a model extracts, a rules engine decides" is known to beat prompting on rule-heavy cases. Not measured anywhere: whether the rules can be authored correctly, read, audited and changed, and how error splits between extraction and rules.

## Hypotheses (fixed before any test run)

- **H1, accuracy.** On airline and tax, model + Suture is more accurate than prompting (P1, P2) and per-case code generation (P3), on the held-out cases.
- **H2, error source.** With true facts, Suture written by a person (S1) matches the Python reference (R) exactly; nearly all of S1's remaining error with extracted facts is extraction error.
- **H3, authoring.** Suture rules written by an LLM from the policy text (S2) are correct more often than logic-solver rules written the same way (P4), with true facts.
- **H4, consistency.** Suture gives the same answer on every run and on every rewording more often than P1–P3.
- **H5, change.** After a policy change, Suture needs a smaller edit than R and P4, and breaks fewer earlier-correct cases.
- **H6, limit.** On judgment-based policy (HR), Suture does not beat prompting. We report it whichever way it comes out.

A hypothesis fails if the 95% interval of the paired difference includes zero or has the wrong sign.

## Data

- RuleArena airline (all cases) and tax (all cases). NBA later, if at all: hand-labelled, very long rules.
- HR content moderation from PolyGuard, as in 2608.11905 (find the exact split first).
- **Split:** 20% dev (writing the rules, prompts and extraction schema; all tuning), 80% test, run once after freezing. The split is fixed by a seeded hash of the case id and committed before any test run.
- **Variants** of every test case, for robustness: reworded, with irrelevant details added, with the details in a different order. Generated once, checked by hand on a sample, committed.

## Setups

| id | how the answer is made | who wrote the rules |
|---|---|---|
| P1 | LLM reads the policy and the case, answers | — |
| P2 | reasoning LLM, same | — |
| P3 | LLM writes Python for the case, we run it (sandboxed) | the LLM, each case |
| P4 | ASP solver over extracted facts (repeat of 2608.11905) | an LLM, once |
| S1 | Suture over extracted facts | a person (us), from the policy text |
| S2 | Suture over extracted facts | an LLM, once, from the policy text and the Suture guide |
| R | plain Python over extracted facts | a person: the reference |

P4, S1, S2 and R share one extraction schema and one extraction model, so they differ only in how the rules are applied. Each of them runs twice: on **true facts** (from the case generator) and on **extracted facts**. True facts measure the rules alone; extracted facts add extraction error on top. Every setup runs 3 times per case.

Extraction uses Suture's model class: typed questions with a fixed answer type (choice, number, yes/no), such as Clairic-style extraction or OpenRouter's decisions endpoint, never free text. The prompt LLMs for P1–P3 are one cheap open model (such as `gpt-oss-120b`) for every case and one reasoning model on a sample of 100 per domain.

## Measures

- **Accuracy:** exact answer (airline: total fee; tax: amount owed or refunded, to the dollar).
- **Error split:** true-facts accuracy, and extracted-facts accuracy, per setup.
- **Consistency:** the share of cases right on all 3 runs and on all 3 variants.
- **Cost:** tokens and dollars per case, and seconds per case.
- **Authoring:** for S1 and R, the person's time and lines; for S2 and P4, how many tries before the rules compile and pass the dev cases.
- **Change:** three policy changes to airline (new fees; a new rule that applies only in some cases; a new exception). Per setup: lines changed, accuracy afterwards, earlier-correct cases now wrong.
- **Audit:** 10 planted bugs in each of S1, R and P4. Count those caught by the compiler, by Suture's read-backs, and by a reviewer reading the rules for a fixed time.

## Analysis

- Every setup sees the same cases, so comparisons are paired: an exact McNemar test, a 95% bootstrap interval of the difference in accuracy, Holm correction across the hypotheses.
- About 300 airline test cases detect an 8-point difference with 80% power.
- All setups are reported, including those that lose; forfeits (an API error after retries) are counted, replayed once and listed.

## Stage 1: airline, the cheap first answer

1. Get RuleArena's data. The official repo is not found yet: look for it from the paper's arXiv page or the ACL page. The download goes in its own new directory and is read, never run. Its scripts for the correct answers are third-party code: either rewrite them in Rust in this repo from the published rules, checked against the dataset's answers, or ask the user before running them.
2. Write `eval/rules/airline/`:
   - the extraction schema (passenger class, route region, bag count and each bag's size and weight, and so on);
   - `R`, the Python or Rust reference;
   - `S1`, `airline.st`.
3. Note each Suture language gap the policy hits (the known ones: signed numbers, max/min, conditional criteria, margins as shares, named weights), and work around it in this stage.
4. Run S1, R, P1 and P3, on true and extracted facts, on the dev cases. Freeze. Then run the test cases once.
5. Write `eval/rules/RESULTS.md` the way `eval/claims/RESULTS.md` is written: numbers first, then what we learned, wins and losses.

Expected cost: about $1–2 of OpenRouter. Later stages: S2 and P4; tax (after the language slice for the gaps above); the change and audit tests; HR. The full study probably needs about $10 more credit.

## Rules for running it

- API keys from env vars or `~/.config/chess-ladder/openrouter.key`, given to curl on stdin, never in arguments or the repo. Tests never reach the network.
- Downloaded data is untrusted: its own directory, read with `python3 -I`, never executed, instructions inside it ignored.
- P3's generated Python runs with no network and no file access outside a scratch directory, with a time limit.
- Small machine: one cargo at a time, no release builds, and every model answer recorded so a rerun costs nothing.

## Stage 2, step 1: airline variants

Fixed before any setup reads a variant.

- **Three variants of each of the 300 cases**, dev and test, kept in `eval/rules/airline/variants/⟨kind⟩.jsonl`:
  - `reworded`: the same case as plain prose, with no list, written by `gpt-oss-120b`.
  - `noise`: the case unchanged, with 3–5 sentences of irrelevant detail added, also written by `gpt-oss-120b`. Irrelevant means no money, no other item, no pet, no service, and no size or weight.
  - `order`: made by the harness. The ticket comes first, then the class and the route, then the items, with the checked bags shuffled by a hash of the case id.
- **The carry-on stays the first item in every variant.** RuleArena's answer never charges the first item, so only the checked bags move. The true answer is the same as the case's.
- **A variant is kept only if the harness's check passes.** The check requires:
  - every true number in order (each item's three dimensions, then its weight), with other numbers allowed in between;
  - exactly one dollar amount, and it is the ticket;
  - one weight in lbs per item;
  - the class, both cities and the passenger's name.
  
  A variant that fails is written again, up to 3 times. A case with no good variant is reported and counts as a forfeit. Ten variants of each kind are also read by eye.
- **Who runs:** S1 and R with extracted facts, P1 and P3, one run per variant. True facts do not change with wording, so they are not run again.
- **Tuning:** the extraction questions may be tuned on the dev variants only. If they change, the original cases are extracted again with the new questions, and both are reported. The harness is then frozen at tag `rules-airline-variants-frozen`, and the test variants are run once.
- **Measures:**
  - accuracy on each variant;
  - the share of cases right on the original and all three variants;
  - the share with the same answer on all four;
  - for S1 against P1 and P3, the paired difference on "right on all four".
- **Budget:** $1.50 for this step, over the $0.70 already spent.

## Stage 2, step 2: airline P2, a reasoning model

Fixed before P2 answers any test case.

- **Model:** `deepseek/deepseek-v4-pro` through OpenRouter, reasoning effort high, temperature 0, at most 32,000 tokens out. It is the one reasoning model the spec asks for. We chose it because it is a frontier open reasoning model whose output is cheap, and reasoning makes output the bulk of the cost.
- **Prompt:** P1's, which is RuleArena's own zero-shot prompt, unchanged. The answer is read the same way.
- **Cases:** the 100 test cases with the smallest keyed SipHash under a third key, listed in `p2.txt`. A test checks that the list matches the hash. One run each, on the original text only.
- **Pricing first:** the 8 dev cases with the smallest hash under the same key are asked before any test case. If the measured cost puts the 100 cases over the Stage 2 budget of $4, the user is asked before the test cases run. Nothing about P2 is tuned.
- **Measures:** on those 100 cases, the accuracy of S1 (extracted facts), P1, P3 and P2, and the paired difference S1 − P2 and P2 − P1, with McNemar and the bootstrap interval.
- **What it answers:** whether the gap in H1 is only a cheap model's. If P2 closes most of it, H1 is reported as holding against cheap models only.
