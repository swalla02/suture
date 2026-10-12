# Experiments A and B: findings

This report applies the readme's §7 decision rule to the two minimal programs it asked for, both built on one kernel and one IR (`docs/ir-spec.md`). Every number below comes from a run on 2026-10-13: the test suites (`cargo test`) and the examples (`cargo run --release --example lease_pool | session | arith`). Rerun the examples to reproduce the measurements.

## Bottom line

- **P0 (graph and memory model): keep the hybrid.**
  - A machine-checked IR exists and every patch is validated against it.
  - One-shot capabilities held across 450 randomized runs with 792 crashes.
  - No data structure needed global mutation.
- **P1 (`INTERACT` vs `BIND`): do not promote `BIND`.** Matching, eligibility, FIFO fairness and race-free acquisition are all expressed as local rewrites. The precise deficiency the experiment found is narrower than `BIND`: deterministic nets cannot **merge** independent event sources. v0 routes every merge through the boundary, at a measured cost of about 1.2 host injects per lease cycle.
- **P2 (protocols from interactions): composable; enrich types, not the kernel.**
  - Sessions, retransmission, timeouts, duplicate suppression, close and abort all run as nets with no hidden executor.
  - Static coverage shows that no (state, message) pair lacks a rule.
  - In-graph channels get static sequencing from port types but cannot time out (B0).
- **P3 (guarantees under extensions): strong confluence and "state is a function of the boundary log" both hold, and are tested by digest equality** across four schedulers in every program. Each lost property is listed in `docs/ir-spec.md` §14.

## Setup

| Item | Value |
| --- | --- |
| Kernel | `src/*.rs`, about 2,600 non-blank lines of Rust, 4 crate dependencies |
| Tests | 101 tests (unit and integration) |
| Programs on the kernel | Peano arithmetic, symmetric interaction combinators, Experiment A, Experiment B, Experiment B0 |
| Rule coverage | complete for all five programs (`coverage().missing` is empty) |
| Throughput | 60 × 60 in Peano arithmetic: 110,042 reductions in 0.56 s, about 196 k reductions/s. Release build, single thread, every patch validated and journaled in memory. |

## Experiment A: scarce linear resources

**Scenario** (`SimConfig::standard`): ten requests from four tenants with region constraints compete for three leases (eu-west-1, eu-west-1, us-east-1). Each job fails with probability 0.25 and is retried up to three attempts.

**Randomized per seed:**
- the order of arrivals, completions, failures and routing;
- the internal scheduler (FIFO, LIFO or random);
- the budget of partial reduction between host actions.

**Mechanism.** The pool consumes a mailbox stream, and matching is entirely `INTERACT`:
- a demand runs `Seek` over the free list;
- a returned lease runs `Match` over the wait queue;
- consecutive scans **pipeline**: a later scan waits on the earlier scan's output wire.

The lease lifecycle is a chain of conserved kinds (`FreeLease → Granted → RunJob → Returned → FreeLease`). It is the readme's `Available → Reserved → Active → Released` state machine, compiled into active-pair reductions.

### Invariants checked

Checked after **every** host action in every run:

| Invariant | Checked by | Result |
| --- | --- | --- |
| No duplication, double consumption or leaked lease | lease census equals {1, 2, 3} after every action, and T3 / L7 on every patch | held in 300 runs, plus 150 runs with 792 crashes |
| At most 3 active reservations | implied by the census | held |
| Provider never sees a double allocation or a residency violation | independent provider ledger, idempotent per effect id | 0 violations |
| Residency and tenancy | hard policies on every patch; a buggy rule that ignores regions is **stopped** (fault, graph unchanged) | held |
| Correct matching | the grants equal those of a 32-line sequential reference model fed the same mailbox order | equal in all 450 runs |
| Completion | every request ends `Done` or `Failed`; at the end, all leases are free and `explain()` shows only the pool waiting on its mailbox | held |
| Crash consistency | crash at random points with commit probability 0.5; recover from the journal (in-memory and JSON-lines file); clients re-send unacknowledged injects; duplicate deliveries are ignored by idempotency key | 792 crashes recovered, 775 duplicates ignored |
| Schedule independence (T2) | replay each run's boundary log under LIFO, random and rounds scheduling, with random partial reduction | identical digests |

### Measurements

Over 300 runs: 3,933 lease cycles (grants), and 53 requests failed after three attempts.

| Metric | Per lease cycle |
| --- | --- |
| Reductions | **12.9** |
| … matching (`pool_msg`, `dispatch_*`, `seek_*`, `enq_*`, `match_*`) | 8.9 |
| … workspace (`granted_await`, `job_*`) | 2.0 |
| … transport (`route_send`) | 1.24 |
| … status acknowledgements | 0.76 |
| Host injects | **3.84** |
| … provider replies | 1.00 |
| … routes (lease returns 1.00 + re-demands after failure 0.24) | 1.24 |
| … admissions / status acks / pool init | 0.76 / 0.76 / 0.08 |
| Peak graph size, any run | 39 agents |

- **Parallelism.** Ten simultaneous admissions reduce in 19 rounds of at most 5 independent pairs: 62 reductions, 3.3 per round on average. The pipelined scans really do overlap.
- **Queue cost.** Appending to the wait queue walks it: `enq_cons` runs 1.82 times per lease cycle, which is O(queue length). A difference-list queue, with its tail exposed as a wire, would make the append O(1).

### What Experiment A says about `BIND` (P1)

- **Discovery and matching need no new primitive.**
  - Eligibility is a payload guard inside one rule.
  - FIFO fairness falls out of stream order.
  - Capacity is the number of conserved tokens.
  - Race-freedom falls out of pipelining: a scan can only see list cells that earlier scans have released.
  - The host never decides who gets a lease, and the agreement with the reference model shows the decision is the net's.
- **The deficiency is the merge.** An agent waits on exactly one principal port. A pool that must accept *both* new demands *and* returns from up to three holders is a many-to-one merge. Deterministic interaction nets cannot express a fair merge; that needs nondeterminism (McCarthy's `amb`, or agents with several principal ports).
- **v0 resolves every merge at the boundary.** A lease leaves its holder as a `Send` effect, and the host splices it into the pool mailbox (`Route >< Send`). The cost is one effect plus one inject per return, about 1.2 per lease cycle including re-demands.

This keeps D1 (a deterministic interior, with state a function of the journaled boundary log), and it is the only place the host participates in "matching".

> **Decision under the readme's rule:** do **not** promote `BIND`. If boundary round-trips become the bottleneck, the smallest primitive that removes them is a *narrowly specified nondeterministic merge* whose choices are journaled like injects. That keeps T2 relative to the journal. `BIND`, meaning host-side discovery, is not needed by anything this experiment does.

## Experiment B: typed protocol with timeouts

**Scenario.** A client sends 4 items to a server with stop-and-wait. Each step retransmits on timeout, up to `max_tries = 6`. The server delivers each sequence number exactly once and re-acknowledges duplicates.

**Mechanism.**
- Each endpoint consumes an inbox stream whose cells *are* the messages.
- Messages and timer firings cross the boundary as `Transmit` and `Timer` effects.
- The simulated network drops, duplicates and reorders messages, and fires timers in random order. Timeouts race replies: a timer fires before a pending delivery with probability 0.15 per step.

**Rules.** The protocol is 5 client states and 3 server states, each paired with all 8 message kinds: 64 explicit rules generated by two functions, plus 3 effect acknowledgements. Every (state, message) pair is handled: as a legal transition, as a tolerated duplicate (a late `Ack` or `Accept`, or a stale timeout), or as an explicit rejection (`Reject` effect, state unchanged). Static coverage confirms that no pair is missing.

### Results

300 sessions per network condition:

| Network | Closed OK | Aborted | Messages per session | Timeouts fired | Invalid messages rejected | Reductions per session |
| --- | --- | --- | --- | --- | --- | --- |
| perfect | 300 | 0 | 14.3 | 7.1 | 0 | 43.8 |
| drop 0.3, dup 0.2 | 259 | 41 (connect 8, data 29, close 4) | 22.5 | 12.3 | 0 | 67.2 |
| … plus an adversary injecting invalid messages (p = 0.3) | 269 | 31 | 23.4 | 12.8 | **3,497** | 95.2 |

These properties held in every run of the test suites (300 lossy and 200 adversarial seeds):
- **Exactly-once, in order:** a session that closes cleanly delivered exactly `[10, 20, 30, 40]`.
- **Safe abort:** an aborted session delivered a prefix, never a duplicate or an out-of-order item.
- **No invalid message accepted:** every invalid message from the adversary was rejected, including a wrong session id, a sequence number far in the future, and a message in the wrong direction. The adversary does not forge *valid* messages, such as a correct `Ack`; those cannot be told apart without authentication, which is out of scope.
- **Explainable quiescence.** The final state is classified structurally with no domain knowledge, as results waiting at their holes and endpoints idle on their inboxes. No run ended stuck or deadlocked.

The "perfect" row shows the cost of racing timers. With no loss at all, 4.7 `Data` transmissions are made for 4 items, because timeouts sometimes beat replies and stale timers are ignored.

### B0: the same protocol as an in-graph, session-typed channel

**Mechanism.**
- The channel is a wire whose port type is the protocol state (`S0`…`S4`).
- A message is an agent that carries the continuation channel.
- Each exchange is exactly one interaction: a 3-item session takes 10 reductions.

**What the types catch:**
- Two deliberately broken rules, one where the client sends `Ack` in the `Data`/`Close` state and one where the server answers `Data` with `Closed`, are **rejected by the patch type check** as "ill-typed interface". The pair faults and nothing is delivered.
- Injecting a `Data` message at a server waiting for `Open` is rejected at the boundary (`InjectError::Type`).

**The measured limitation.** A receiver's single principal port faces the channel, so **nothing else can reach it**, and in-graph channels cannot time out. This is the same merge deficiency as in Experiment A: a race between a reply and a timer is a two-input wait.

> **Decision under the readme's rule:** protocols compose from interactions without kernel changes. Use session-state port types for synchronous in-graph channels, which gives static sequencing. Use boundary inboxes plus rule coverage for asynchronous sessions with timeouts, which gives total and explainable handling. "Enrich types before enlarging the kernel" (P2) holds. The obvious next step is to generate the per-state types and rule skeletons from a session-type description.

## Generality on the same kernel

| Program | Class of computation | Evidence |
| --- | --- | --- |
| Peano arithmetic | typed functional computation with explicit sharing (`Dup`) and erasure (`Era`) | 200 random expressions agree with Rust arithmetic under FIFO, LIFO and random scheduling, with identical digests and reduction counts. `(3+2)*4`: 85 reductions, critical path 28 rounds, max width 6. |
| Symmetric interaction combinators | untyped, universal, self-dual | Of 400 random nets, 338 normalize. Those reach identical digests and *identical reduction counts* under 4 schedulers (strong confluence). δ duplicates trees exactly. Self-wired pairs vanish as vicious circles. Growing nets stay resumable under a budget. |
| Experiments A, B, B0 | service orchestration: resources, protocols, effects | above |

This shows one kernel and one IR serving several *classes* of computation, as the readme requires. It does **not** yet show the second, unrelated *application* (incremental computation, dataflow, or constraint networks) that P7 asks for.

## The readme's falsification criteria

| Kill criterion (readme §7) | Status after v0 |
| --- | --- |
| Most useful logic lives in a giant hidden binding planner | **Not observed.** The whole lease program is 15 small rules, 9 of which do the matching. Host operations are each about 10-line inject builders (admit, route, reply, ack), and none of them chooses an outcome. |
| External side effects cannot be reconciled reliably | **Not observed in simulation.** Effect ids are stable across replay, the provider is idempotent per id, and 792 crashes were recovered. Real providers are untested. |
| Graph transformations are not debuggable or replayable | **Not observed.** Replay is verified by digest, rule drift and version changes are refused, `explain()` classifies every agent, DOT export works, and every agent records its cause. |
| Rewrites reproduce ordinary workflows with worse tooling and no measurable benefit | **Open.** Against the reference model, the lease rules take 141 lines where the sequential pool takes 32. That is about 4.4× the code, in exchange for checked conservation, residency and typing, replay, and pipelining. No comparison with Temporal or Kubernetes controllers yet. |
| Every new problem needs new global algorithms or retraining | **Not observed** across five programs, though all were written by one author. |
| Learned transformations cannot respect hard constraints | **Untested.** Only the mechanism exists: `Recorded` rules are journaled and replayed without being re-run, and they pass through the same validator and policies. |

## Limitations

- **Durability is simulated.** The journal is a trait with in-memory and JSON-lines backends. Crashes are simulated in-process. There is no PostgreSQL backend, and no contention measurement (readme weeks 4–6).
- **Single-threaded reduction.** Parallelism is *measured* (rounds give critical path and width) but not exploited. Patches within a round are independent and could be computed on separate threads.
- **No authentication.** Forged *valid* messages are out of scope, as noted under Experiment B.
- **Server sessions are never reclaimed.** After a client aborts, the server endpoint remains idle on its inbox. Real systems need an idle timeout on the server side.
- **Static guarantees are per-patch.** Rules are closures, so legality is checked on every application rather than proven once per rule. A template-only rule form would allow full static verification.
- **Performance is unoptimized:** about 196 k validated reductions/s.

## Suggested next steps

In order of how much they would change the decisions above:

1. **PostgreSQL `Journal` and outbox table.** Measure commit contention for one graph partition with several effect workers (readme weeks 4–6).
2. **A second, unrelated application** on the same kernel, such as incremental computation or a dataflow network, to test P7's "reusable computer" claim.
3. **Journaled nondeterministic merge**, prototyped behind a feature flag. Compare its host round-trips and replay story against boundary routing, using Experiment A's metrics.
4. **Session-type front end.** Generate per-state port types and the rule skeleton (including rejections) from a session-type declaration. Compare its boilerplate and error quality against hand-written rules (P5).
5. **A learned-law pilot using `Recorded` rules.** A small model proposes patches for one rewrite family (for example, a list-scan family like `Seek`). Every proposal is checked by L1–L9, and the measures are rejection rates and transfer to unseen list lengths (P6).
6. **Difference-list queue** in the lease pool (O(1) enqueue), as an IR-level data-structure exercise.
