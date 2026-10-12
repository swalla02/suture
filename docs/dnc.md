# Decision-Native Computing: minimal operational model (v0)

This note gives the smallest extension of the Suture kernel (`docs/ir-spec.md`) under which learned decisions take part in execution. The hypothesis it serves:

> A shared, bounded decision mechanism, embedded into local graph-reduction semantics, can enable reusable computational behavior that emerges from the composition of decisions rather than explicitly programmed global procedures.

The experiment that tests it is in `docs/dnc-experiment-1.md`. This document fixes the semantics that experiment runs on.

## 1. What is reused unchanged

| Kernel facility | Role in DNC |
| --- | --- |
| Typed port graph and `INTERACT` (validated, atomic patches; IR spec §4–§7) | the deterministic physics; decisions never bypass it |
| Effects: agents whose principal port faces a host hole (IR spec §8.2) | **decision sites** are effects of a declared decision law |
| `INJECT` with typed hole binding (IR spec §8.1) | the resolver answers a site by injecting a **decision token** |
| Journal and replay (IR spec §11) | tokens are journaled injects, so replay never re-calls a model |
| Policies, conservation, coverage (IR spec L7, L9, §5) | hard constraints are enforced on every enactment, whatever was decided |
| Labeled mailboxes and routing (Experiment A) | the transport that carries structure between meeting places |

The deterministic reducer gains **no new instruction**.

## 2. Decision laws

A **decision law** `d` declares:
- `site(d)`: an effect kind whose payload is `(observation, alternatives)`;
- `token(d)`: the kind of the decision token;
- an **enacting rule** `token(d) >< site(d)`, which turns a selection into structure;
- an **encoder** `enc_d`, which maps the site payload to one feature vector per alternative, for model-independent backends.

The deterministic rule that creates a site computes both its observation and its admissible alternatives, from local data only (§5). A model therefore never invents an alternative, a capability, or an effect. It only picks an index.

## 3. Machine

A configuration is `(G, J)`: a graph and a journal. Transitions:

| Label | Meaning | Deterministic? | Journaled as |
| --- | --- | --- | --- |
| `INTERACT(a, b)` | reduce an active pair (IR spec §7) | yes | `Reduce` |
| `ROUTE(e)` | deliver a pending `Send` effect into the mailbox it names | order is a choice | `Inject` |
| `RESOLVE(s, i)` | answer decision site `s` of law `d` with alternative `i = π_d(enc_d(s))` | **no** | `Inject` carrying `Decided(i, model, trace, scores)` |
| `ENV(ev)` | an external event: disturbance, effect reply | no | `Inject` |

`RESOLVE` is defined as:

```
RESOLVE(s):  (o, A) := payload(s)              -- built by a deterministic rule
             i      := π_d(o, A)               -- the decision model; 0 ≤ i < |A| or fallback
             INJECT(Decided(i, meta) at hole(s))
then         Decided >< site  →  enacting rule (deterministic, validated)
```

A decision becomes a **typed token in the graph**, and an ordinary rule consumes it. This is the directive's suggestion (§2) taken literally. Learned resolution becomes a first-class machine step, while the kernel stays the small deterministic reducer it was.

**`BIND` is not needed as a primitive.** Connecting a requirement to a capability decomposes into four steps:
1. `ROUTE` the requirement, with its reply wire, into the capability's meeting place (a market).
2. A deterministic walk gathers at most `K` admissible candidates.
3. `RESOLVE` picks one.
4. A deterministic rule creates the binding channel.

The only nondeterminism is the decision and the arrival order, and both are journaled.

## 4. Representation in the executable graph (experiment 1)

- **Markets.** A market is an agent that consumes its mailbox and holds a list of provider cells. A provider cell carries its own state (capacity, load, recipe, dependency status) and a list of consumer entries.
- **Bindings are wires.** A binding is a one-way channel from a provider's consumer entry to the consumer's socket. Losses and releases travel along it as interactions. The assembled dependency network *is* the graph: rendered with DOT, the channels are the edges.
- **Sites.** A bind site is created when a gathering walk ends. A recipe site is created when a provider is activated and has more than one recipe.

| Transformation | Kind of step | Type constraints enforced by the kernel |
| --- | --- | --- |
| **Bind** a need to a provider | decided (`bind` law) | candidate must be alive, have spare capacity, and be in an allowed region; region checked again by a hard policy on the enacting patch |
| **Expand** a provider into one of its dependency motifs (recipes) | decided (`recipe` law) | spawned needs inherit the provider's region; conservation of provider cells |
| **Release** a binding / **collapse** an unused provider | deterministic | consumer entry and socket are removed together over the channel |
| **Re-need** after a loss or failure | deterministic, bounded attempts | history of lost providers travels with the need |
| **Switch recipe** after exhausting attempts | deterministic trigger, decided choice | only untried recipes are admissible |
| **Die** (provider removed or unsatisfiable) | deterministic | consumers receive `Lost` over their channels |

## 5. Locality

A site's observation is built by the rule that creates it. It contains only:
- the interacting agents' payloads;
- bounded history stored in those agents (lost providers, attempts, failure counts);
- what a walk collected from the market cells it actually visited, up to `K` candidates.

The resolver and the model never read the graph. Information from a distant part of the network reaches a site only by interactions, for example a failure that travels up as `DepFailed` and increments a provider's failure count.

A bounded-neighbourhood variant (observing radius-`r` structure) is not implemented. Its cost is that a site's observation would depend on agents outside the active pair, which other reductions may rewrite concurrently. That breaks causal independence, so observations would have to be snapshot-and-validated, the way bind choices already are (§7).

## 6. Properties

| Property | Holds? | Why |
| --- | --- | --- |
| Strong confluence of the deterministic interior (T1) | yes | `INTERACT` is unchanged |
| State is a function of the journal (T2) | yes | decisions, routes and environment events are all journaled injects |
| Decision replay (T4) | yes | replay applies the recorded `Decided` tokens; the model is never re-called and may have changed |
| Decision safety | yes | alternatives are enumerated by deterministic rules; enactment re-checks eligibility and passes L1–L9. No decision sequence can exceed a capacity, violate region policy, or duplicate a provider. Tested with an adversarial backend |
| Confluence of the full machine | **no** | different decisions, or the same decisions in a different order, give different networks |
| Independence from execution order for a fixed policy | **no**, by design | a decision's observation depends on what has already happened; measured as order sensitivity |
| Termination | bounded by construction | attempts per dependency, recipe switches and conflict retries are capped; the driver also has a round budget |

## 7. Failure semantics of the decision layer

| Situation | Behavior |
| --- | --- |
| Backend returns an index out of range | the selection is replaced by the fallback policy's choice and counted as `invalid` |
| Backend error, timeout or unavailability | the batch is answered by the fallback policy and counted as `fallback` |
| Low confidence (optional threshold on the top score margin) | fallback, counted. Confidence is never treated as correctness: enactment is validated either way |
| Stale choice: the chosen candidate filled up or died between decision and enactment | the need is re-queued and decided again (optimistic concurrency, bounded retries), counted as `conflict` |
| Site with exactly one alternative | resolved without a model call, counted as `trivial` (decisions compiled away) |

The deterministic fallback is the list-order policy, which is also the "deterministic local rewrite" baseline.
