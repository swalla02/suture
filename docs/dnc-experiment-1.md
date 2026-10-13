# Experiment DNC-1: a self-organizing dependency network

**Question** (directive §8, §13): can a small, shared decision law, operating only on bounded local graph state, help produce globally useful structure on configurations it has never seen? Can it also reorganize that structure after disturbances, without a global coordinator?

All numbers come from `cargo run --release --example dnc_experiment` (total 308 s) and `--example planner_cost`, run on 2026-10-13. The semantics are in `docs/dnc.md`.

## Bottom line

1. **Local decisions matter, but only under contention.**
   - On the contended, large and deep suites, a good decision law satisfies **8–15 points more roots** than a fixed list-order rule or random choice. Example: 63.5% vs 55.0% on worlds three times larger than training.
   - On the scarcest suite the margin is +3 over list order and +11 over random.
   - Where resources are plentiful, the difference is about 2 points.
   - Replacing the law with a random one (the directive's ablation) costs 10 points in the training regime.
2. **The shared law transfers.**
   - It was trained only on small two-layer worlds.
   - It ran unchanged on worlds with 3× the roots, an extra region, more types, an extra dependency layer, or tighter capacity.
   - It kept its advantage over the fixed and random rules in every suite.
3. **Learning did not beat a handwritten law.**
   - The learned laws match the handcrafted greedy law within noise. The best case is +1.7 ± 1.0 points on deeper worlds.
   - The learned weights mostly rediscover greedy's signs.
   - In this world, *decision quality* contributes; *learning* contributed nothing a human hadn't already encoded in four lines.
4. **Reorganization works without a coordinator.**
   - Disturbance: remove 20% of providers, cut capacity on another 10%, add two new roots.
   - The same local law, still running, recovers **85–90% of what a full global re-plan achieves**.
   - The fixed and random rules recover 68–76%.
5. **A conventional algorithm wins outright here.**
   - A centralized planner with global knowledge satisfies more roots in every suite.
   - The local laws reach 89–99% of its score.
   - It is far cheaper: 0.1–0.8 ms per world against 5–43 ms, and about 230 lines against about 1,750 (program and decision layer, excluding the kernel).

   For static assembly with a global view available, the decision-native machine is an expensive orchestrator. What it offers instead are properties this experiment does not reward:
   - no global view needed;
   - incremental local repair;
   - every decision journaled and replayable;
   - hard constraints enforced on every enactment.

## Setup

### World

- **Capability types** are arranged in layers. Each type has several providers, each with:
  - a **region**;
  - a **capacity** (how many consumers it can serve);
  - one or more alternative **recipes**, each listing the capability types it depends on, drawn from the next layer down.
- **Leaves** have no dependencies. In the contended regimes most leaves have a single slot.
- **Roots** need one to three types and accept one or two regions.
- **Region inheritance:** a provider's own dependencies must lie in its region, which couples choices made at different depths.
- **Root success:** a root counts as satisfied only if every need is bound to a provider that is *functional*: active, with every dependency bound to a functional provider, recursively.

### Machine

The machine is the program in `src/nets/depnet`, on the unchanged kernel.

- **Needs travel to markets.** A market is a list of provider cells. A walk gathers up to 6 admissible candidates: alive, with spare capacity, in an allowed region.
- **Two decision laws** open sites:

  | Law | Choice | Features per alternative |
  | --- | --- | --- |
  | `bind` | which candidate to bind | active, activation cost, load, free slots, local failure count, lost before, recipe count |
  | `recipe` | which recipe to expand a newly activated provider into | dependency count, overlap with types that failed before |

- **Bindings are channels in the graph.**
- **Failures and losses travel back up as messages.** Owners retry (3 attempts per dependency), switch recipe, or die, and each step can open new decision sites.

No component reads the graph. The evaluator does, at quiescence, for measurement only, and it also checks structural validity:
- capacity;
- region and type of every binding;
- that every owner's beliefs match the structural channels;
- that dead providers have no consumers.

### Policies

| Policy | What it is |
| --- | --- |
| list order (fixed) | always the first admissible alternative: the "deterministic local rewrite" baseline, and the fallback |
| random | uniformly random admissible alternative |
| greedy (handcrafted) | linear law, hand-set weights: reuse active providers, avoid activation cost, load, failure history and lost providers; prefer recipes with few, untroubled dependencies |
| learned (scratch) | the same linear family, weights trained by the cross-entropy method from zero |
| learned (refined) | the same, training started from the greedy weights |
| planner | centralized depth-first search with full knowledge and the machine's semantics (`planner.rs`): backtracking over each root's goals, greedy across roots, 8 restarts. A reference, not a local policy |

**Training** is offline, then the weights are frozen.
- Data: 32 small contended worlds.
- Search: 25 iterations × 40 candidates, with decaying exploration noise; 32,000 machine runs per training.
- Objective: the mean fraction of satisfied roots.

At inference, the law sees only its site's features.

## Results

### Training (objective: satisfied fraction on the training worlds)

| law | training score |
| --- | --- |
| list order (fixed) | 58.6% |
| random | 60.2% |
| greedy (handcrafted) | 65.2% |
| learned from scratch | 66.4% |
| learned, refined from greedy | 66.0% |

| feature | greedy | learned from scratch | refined from greedy |
| --- | --- | --- | --- |
| bind.active | +3.00 | +0.79 | +3.30 |
| bind.activation_cost | -1.00 | -0.41 | -0.30 |
| bind.load | -1.00 | +0.65 | -0.08 |
| bind.free | +0.50 | +1.28 | +0.67 |
| bind.fails | -1.00 | +0.22 | -0.19 |
| bind.lost_before | -2.00 | +0.52 | -0.83 |
| bind.recipes | +0.00 | -0.62 | +0.17 |
| recipe.deps | -1.00 | -1.44 | -1.60 |
| recipe.failed_type_overlap | -2.00 | +0.28 | -1.65 |

The first training run used plain CEM. It collapsed onto a plateau after five iterations and learned degenerate weights, scoring 64.6% on training and 4 points below greedy on held-out worlds. Adding decaying exploration noise fixed that. Both trained laws agree with greedy on the decisive signs: prefer active providers, fewer recipe dependencies, and spare capacity. The from-scratch law reaches a similar score with partly different weights, which suggests the objective is flat across this region.

### Assembly on held-out and unfamiliar worlds

Mean fraction of roots satisfied, ± standard error. The last column is paired per world.

| suite | planner | list order | random | greedy | learned (scratch) | learned (refined) | refined − greedy |
| --- | --- | --- | --- | --- | --- | --- | --- |
| contended, held-out (40 worlds) | 70.6% | 58.1% ± 3.7 | 58.8% ± 3.4 | 68.4% ± 2.9 | 68.4% ± 3.1 | 68.1% ± 2.8 | −0.3 ± 1.1 (3 better / 4 worse) |
| contended-large: 3× roots, 3 regions, more types (30) | 68.2% | 55.0% ± 2.1 | 55.3% ± 2.3 | 63.5% ± 2.4 | 63.3% ± 2.4 | 63.3% ± 2.2 | −0.1 ± 1.1 (9 / 10) |
| contended-deep: one more layer (30) | 73.3% | 60.4% ± 4.8 | 56.2% ± 4.0 | 70.0% ± 4.2 | 71.7% ± 3.9 | 71.7% ± 4.2 | **+1.7 ± 1.0** (3 / 0) |
| contended-scarce: fewer leaves, more roots (30) | 55.7% | 46.3% ± 2.8 | 38.3% ± 3.2 | 48.7% ± 3.6 | 49.7% ± 3.5 | 48.3% ± 3.6 | −0.3 ± 1.1 (3 / 2) |
| loose: plentiful leaves (30) | 86.2% | 83.3% ± 3.8 | 82.5% ± 3.4 | 84.6% ± 3.4 | 84.6% ± 3.2 | 85.0% ± 3.4 | +0.4 ± 0.4 (1 / 0) |

Every run, for every policy, ended with **zero structural or policy violations** and settled within budget.

### Disturbances: reorganization while running

| suite | policy | before | after | planner re-plan | share of re-plan recovered |
| --- | --- | --- | --- | --- | --- |
| contended | list order | 58.1% | 33.0% | 48.7% | 67.7% |
| contended | random | 58.8% | 33.2% | 48.7% | 68.2% |
| contended | greedy | 68.4% | 41.5% | 48.7% | 85.1% |
| contended | learned (scratch) | 68.4% | 42.2% | 48.7% | 86.7% |
| contended | learned (refined) | 68.1% | 42.0% | 48.7% | 86.2% |
| contended-large | list order | 55.0% | 37.7% | 50.1% | 75.2% |
| contended-large | random | 55.3% | 38.1% | 50.1% | 76.0% |
| contended-large | greedy | 63.5% | 45.0% | 50.1% | 89.8% |
| contended-large | learned (scratch) | 63.3% | 44.6% | 50.1% | 89.0% |
| contended-large | learned (refined) | 63.3% | 44.4% | 50.1% | 88.5% |

How recovery happens:
1. A killed provider's consumers receive `Lost` over their channels.
2. Each one re-needs, with the dead provider recorded in its history.
3. That opens new bind sites. Cascades follow when a provider can no longer build its own dependencies.
4. New roots enter as ordinary admissions.

No recovery procedure exists anywhere in the program.

### Sensitivity to execution order

Setup: 12 held-out worlds, 8 driver seeds each, asynchronous transport (half of the pending messages per round, shuffled).

| policy | mean satisfied | mean per-world std across orders |
| --- | --- | --- |
| list order | 61.7% | 8.5 pts |
| random | 59.0% | 9.1 pts |
| greedy | 70.6% | 6.2 pts |
| learned (scratch) | 68.1% | 5.2 pts |
| learned (refined) | 70.6% | 6.7 pts |

### Uncertain or contradictory decisions

The refined law, with each decision replaced by a random admissible one with probability ε:

| ε | 0 | 0.1 | 0.3 | 0.5 | 1 |
| --- | --- | --- | --- | --- | --- |
| satisfied | 68.1% | 66.9% | 65.9% | 60.6% | 57.5% |

Degradation is graceful. Up to ε = 0.3 the failure-driven repair absorbs most bad decisions.

### Cost

**Machine** (refined law):

| suite | decisions / run | batched calls / run | mean batch | single-alternative sites skipped / run | reductions / run | ms / run |
| --- | --- | --- | --- | --- | --- | --- |
| contended | 24.3 | 7.8 | 3.1 | 8.1 | 1,119 | 8.5 |
| contended-large | 88.6 | 12.0 | 7.4 | 22.6 | 5,091 | 43.2 |
| contended-deep | 33.6 | 12.7 | 2.6 | 9.2 | 1,391 | 12.7 |
| contended-scarce | 28.3 | 8.5 | 3.3 | 11.7 | 1,618 | 14.8 |
| loose | 20.7 | 5.4 | 3.8 | 3.0 | 624 | 5.3 |

**Planner:**

| suite | search nodes / world | ms / world | budget hit |
| --- | --- | --- | --- |
| contended | 303 | 0.1 | 0 / 40 |
| contended-large | 4,544 | 0.8 | 0 / 30 |
| contended-deep | 1,144 | 0.2 | 0 / 30 |
| contended-scarce | 394 | 0.1 | 0 / 30 |
| loose | 141 | 0.0 | 0 / 30 |

**What a remote model would cost.**
- Sites are resolved once per round, in one batch, so the number of sequential model calls equals the number of rounds that have a site: 5–13 per assembly.
- Single-alternative sites (13–29% of all sites, depending on the suite) never reach the model.
- With a remote decision service at 50 ms per call, decision latency alone would be 0.3–0.6 s per assembly, which dwarfs the machine's own 5–43 ms.

Batching across sites already happens. Making a remote model practical would also require batching across concurrent assemblies, or running the law locally, which this one can since it is 9 weights.

## Answers to the directive's questions (§10)

**A. Is this a distinct abstraction?**
DNC-1 is precisely a graph-rewriting system extended with a **resolver**. Four properties go beyond probabilistic graph rewriting:
1. Choice points are explicit typed sites, whose admissible alternatives are enumerated by deterministic rules from local data.
2. One shared, parametric, objective-trained law resolves every site of its kind.
3. Decisions are journaled boundary inputs, so the machine is replayable without the model.
4. Enactment passes the same validation as any rewrite, so decisions cannot break hard constraints.

Whether this is a *useful* abstraction rests on evidence. Here, decision *quality* clearly mattered. A *learned* law added nothing beyond a handwritten one, and a centralized algorithm was better and cheaper. That supports the decision-site machinery, not yet the learning claim.

**B. Is bounded selection expressive enough?**
For this task family, yes. Six structural transformations, two of them decided, assembled and repaired dependency networks with up to about 90 decisions per run. Patch synthesis was never needed. The limit was **information**, not vocabulary: a site cannot see downstream scarcity, which only reaches it later as failures.

**C. What becomes of confluence?**
- The interior keeps strong confluence (T1). Kernel scheduler changes never change the outcome, which is tested.
- The full machine is not confluent. Under asynchronous transport, the same law yields per-world outcomes that vary by 5–9 points across execution orders.
- Given the journal, the state is exact (T2), and replay never calls the model again (tested).

**D. What does learning generalize?**
The law transferred across size, depth, region count and scarcity without retraining, because its features are size-invariant ratios and local counts. But what it generalizes is essentially greedy's preferences. The deep suite is the only place it measurably beat greedy, and only slightly.

**E. How do objectives propagate?**
Offline only: the training objective is global, while inference sees local features. During execution, outcome information reaches decision sites through interactions alone:
- `DepFailed` and `DepLost` increment a provider's failure count;
- they extend a need's history of lost providers.

The from-scratch law gave these signals small weights. They help repair, but they arrive too late to guide first choices.

**F. Can decision cost stay bounded?**
- Per assembly: 5–13 batched calls, and 13–29% of sites never reach the model.
- Bounded, but latency-dominated with a remote model (0.3–0.6 s at 50 ms per call). See Cost above.

## Failure cases found

1. **A real concurrency bug, found by the structural validator.** The transport can reorder a binding's `DepLost` ahead of its `DepBound`. The owner discarded the loss as stale, then believed in a binding that no longer existed. The first full run crashed on that assertion. Fixed: a loss for a pending dependency with the matching nonce is a loss. There is a regression test, plus a 40-world asynchrony and disturbance stress test.
2. **Premature convergence of plain CEM** produced a law 4 points worse than greedy. Decaying exploration noise fixed it. This is a reminder that "learned" can easily mean "worse".
3. **Scarce regime:** local laws reach only 89% of the planner. Random choice collapses to 38%, against 56% for the planner. Under heavy scarcity, a wrong early activation is costly, and local laws cannot foresee it.
4. **Order sensitivity** of 5–9 points per world. The outcome depends on message timing, so any single run is a sample, not the answer.
5. **Permanent death is harsh.** A provider that exhausts its recipes dies even if capacity frees up later. This keeps termination simple but wastes capacity after transient contention.

## Research claim vs product claim

**Research claim: partially supported.**
- Shown: a shared, bounded, local decision law produces globally useful structure, transfers to unfamiliar configurations, and reorganizes after disturbances without any coordinator or recovery procedure.
- Not shown: that *learned* decisions contribute beyond a simple handwritten law, or that local decision dynamics beat a centralized algorithm.

**Product claim: not addressed, and this experiment argues caution.** If a global view is available, a 230-line planner is simpler, faster and better at static assembly. A product case would have to rest on settings where a global view is unavailable, stale, or too costly, or on the operational value of journaled, replayable local decisions. DNC-1 does not price those.

## Recommendation for the next experiment

Test the hypothesis where it could actually win: **give learning something a heuristic cannot easily encode, and price the global view.**

1. **Propagated context.** Add interaction-borne signals, such as scarcity "pressure" that pools push upstream through the graph, or provider reliability observed over time. Then test whether a learned law, possibly non-linear, exploits combinations a handwritten rule does not. Keep the planner, but give it the same information delay: it re-plans from a snapshot that is *k* rounds stale.
2. **Non-stationary disturbances.** Run continuous churn instead of one disturbance: failures, capacity drift, arriving roots. Measure satisfied-root-time and re-plan cost. A centralized planner that must re-plan continuously pays repeatedly; local repair does not.
3. **A real decision model, zero-shot.** Give a Jev-like service the same sites, rendered as text. The question is whether a general decision model reaches greedy-level quality *without task-specific training*. That would be the strongest argument for decision-native execution. It needs credentials, and the backend interface is already model-independent.
4. **A second domain on the same decision layer** (readme P7), for example an incremental dataflow or constraint network. The test: are decision sites, tokens and the resolver reusable as they are?

## Reproduction

```bash
cargo test                                         # includes the DNC scenario, safety, replay and stress tests
cargo run --release --example dnc_experiment       # all tables above except planner cost (~5 min, 8 threads)
cargo run --release --example planner_cost         # the planner cost table
```

Everything is deterministic in its seeds. A second full run reproduced every non-timing number above exactly. Wall-clock figures (ms per run, training time) varied by up to 2.5× with machine load.
