# Suture kernel v0: design

- **Date:** 2026-10-12
- **Source:** `readme.md` (Learned Interaction Machine design notes)
- **Request:** "May god be with you : readme.md". I read this as a handoff: execute the notes.
- **Approval:** the brainstorming gates were self-approved under that handoff. The assumptions in §2 are the points most likely to need correction.

## 1. Intent

The readme names the smallest next step that can settle its foundational questions (§10):

> a short **formal IR + memory-semantics specification** (including three worked reductions) followed by the lease and typed-session example programs.

Its first 12-week phase (weeks 1–3) is:

> Typed port-graph reducer in Rust; active pairs, rule table, local invariants, deterministic traces.

This design covers exactly that, plus the minimum needed to answer P0–P3 with evidence: durability semantics (journal, replay, crash recovery) and a second class of computation on the same kernel.

### Success criteria

1. **Kernel:** a Rust library implementing typed port graphs, active-pair detection, a rule table, `INTERACT` as the only internal primitive, validated atomic patches, host `INJECT`, effects, and deterministic traces.
2. **Formal spec:** `docs/ir-spec.md` defines a valid net, port types, active pair, rule and patch legality, host operations, memory semantics, identity, journal and replay, and quiescence. It includes three worked reductions. It states which formal properties are kept and which are lost.
3. **Experiment A (scarce linear resources):** 10 concurrent requests compete for 3 leases with region eligibility. The run must show no duplication, no double consumption, no tenant or residency violation, and no leaked lease, across randomized boundary orders, randomized internal schedules, and crashes. Matching uses only `INTERACT` plus boundary admission, with no `BIND`. Matching cost and overhead are measured.
4. **Experiment B (typed protocol):** two logical services establish a session, exchange typed messages, react to host-injected timeouts, and close or abort. The run must show legal sequencing, no invalid message accepted, exactly-once in-order delivery despite drops, duplicates and reordering, and explainable quiescence. Variant **B0** shows statically typed in-graph session channels.
5. **Generality:** the same kernel runs Peano arithmetic (typed, with explicit `Dup`/`Era` sharing) and symmetric interaction combinators (untyped).
6. **Findings:** `docs/experiments.md` reports measurements and answers to P0–P3, applying the readme's decision rule.

### Out of scope

PostgreSQL (the journal is a trait with in-memory and file backends; Postgres is weeks 4–6), the REST API, real cloud adapters, Helm or containers, learned rules (only the *mechanism* for recording opaque rule output is built), distributed graph partitions, a standalone DSL (P5 says IR first), and multi-threaded reduction beyond what is cheap.

## 2. Assumptions (correct me here)

- **A1.** "Implement the readme" means its stated next step, not the 12-week product.
- **A2.** The crate is named `suture` after the repository. The readme keeps its role as design notes and gets only a short status pointer.
- **A3.** Rules are written in Rust (P5 direction: Rust-based definitions for v1), as functions that build a patch through a builder. There is no text DSL.
- **A4.** Determinism matters more than raw speed in v1. Correctness, replay, and explanation come first, and performance is only measured.

## 3. Key decisions

**D1. The interior is deterministic; nondeterminism enters only at the boundary.** Rules are deterministic functions of the active pair's kinds and payloads. Active pairs are disjoint, so Lafont's one-step strong confluence holds for payload-dependent rules. The order of host `INJECT`s, and the output of opaque (future learned) rules, are the only nondeterministic inputs, and both are journaled. Theorem to state and test: *the graph is a function of the boundary event log*, independent of reduction order or of how reductions interleave with injections.

**D2. `INTERACT` is the only internal computation, and there is no `BIND`.**
- Host operations: `INJECT` (admit a net and bind its free ports to existing holes or to new, optionally labeled holes), effect emission, `COMMIT`, and `OBSERVE`.
- *Discovery and matching happen inside the net.* Many-to-one merges are arbitrated at the boundary by **labeled mailbox holes**: a stream whose tail is a hole the host appends to. Examples are a lease pool receiving demands and returns, or an endpoint receiving messages and timeouts.
- Reason: deterministic interaction nets cannot express a fair merge. This is a known expressiveness limit, since a single principal port means an agent waits on one thing. A merge needs either an in-kernel nondeterministic primitive (`amb`) or arbitration at the boundary. We choose the boundary, keep D1, and measure the cost (host round-trips per lease cycle). That cost is the evidence for P1.

**D3. Lineage identities.** A new agent's ID is a 128-bit hash of its causal origin:
- for an agent created by a reduction: `(left id, right id, rule name, index in patch)`;
- for an injected agent: `(inject sequence number, index)`.

IDs are therefore independent of the schedule. The consequences:
- Graphs produced by different schedules are *identical*, not merely isomorphic.
- Effect IDs are stable across crash and replay, so they serve as natural idempotency keys for adapters.
- Concurrent reducers can allocate IDs without coordination.

**D4. Rules are code; patches are data; every patch is validated.** A rule is a Rust function `(RuleCx) -> Result<()>`. It reads the pair's payloads and builds the replacement net with a small builder (vars, agents, links, effects). Before installation, each patch is validated for:
- linearity: every wire variable is used exactly twice;
- arity;
- type duality;
- boundary correspondence: the type of each interface port is preserved;
- conservation of conserved kinds;
- effect authority;
- size bounds;
- registered policies.

Rule modes:
- `Deterministic`: the journal stores the patch digest. Replay re-runs the rule and verifies the digest, which detects rule-code drift.
- `Recorded`: the journal stores the whole patch, and replay applies it without re-running the rule. This is the readme's requirement for learned laws.

**D5. Types.** Each type is either *polarized* (a wire connects `T+` to `T−`) or *self-dual* (any `T` port connects to any `T` port, as interaction combinators need). Port types are monomorphic atoms; there is no polymorphism in v1. Session protocol states are modeled as distinct atoms.

**D6. Memory semantics, the readme's hybrid made concrete.**
- **Payloads:** immutable small values (unit, bool, int, string, tuple, list) with schemas.
- **Wires:** linear. Each port is connected to exactly one other endpoint.
- **Sharing:** explicit only, through duplicator agents, and the same goes for erasure through eraser agents.
- **Capabilities:** *conserved kinds* that declare a resource class and a key in the payload. In every patch, the multiset of conserved keys consumed must equal the multiset created, which rules out duplicating, dropping, or forging a capability inside the interior. Minting requires an explicit inject permission.
- **External state:** reached only through effects. There are no mutable references in the graph.

**D7. Effects are host-facing agents.** An *effect kind* is declared in the signature. When a patch creates an effect agent, its principal port is connected to a fresh hole and the agent appears in the outbox. The host performs the effect, then fills the hole with a reply agent; the reply and the effect form an active pair, and an ordinary rule consumes them.
- Rules must declare which effect kinds they may emit.
- Delivery is at-least-once. Adapters deduplicate by effect ID (see D3).
- Hosts dispatch an effect only after the reduction that created it is committed. `Machine::outbox()` lists pending effects, and the harnesses enforce commit-before-dispatch.

**D8. Journal.** Records:
- `Header` (program name and version)
- `Inject`
- `Reduce` (pair, rule name and version, patch digest, and the patch for recorded rules)
- `Fault`
- `Commit`

The `Journal` trait has `MemJournal` and `FileJournal` (JSON lines, fsync on commit) implementations. Recovery replays everything up to the last commit. A crash loses only uncommitted records, and those are recomputed because they follow deterministically from committed injects. Injects can carry an idempotency key, and duplicate keys are ignored.

**D9. Quiescence is explained structurally.** Every agent's principal port points at exactly one thing, so "what is this waiting on?" forms a functional graph. Following it from any agent ends in one of:
- a hole (an external wait, or a pending effect);
- an active pair (reducible, or *stuck* when there is no rule or the patch was rejected);
- a cycle (a deadlock).

`explain()` groups agents by those terminals. This is the readme's operability requirement ("which unsatisfied demand blocks progress") answered by the model itself.

**D10. Faults are local.** An active pair with no rule, or whose patch fails validation or policy, is marked stuck, journaled, and reported. The rest of the graph keeps reducing. Hard policy is never scored; a violation rejects the patch.

## 4. Architecture

One crate, `suture`. Each module has one purpose.

| Module | Purpose | Depends on |
| --- | --- | --- |
| `value` | Immutable payload values, schemas, canonical encoding | — |
| `sig` | Types, agent kinds (ports, payload schema, effect and conserved flags), `Signature` builder | `value` |
| `id` | Lineage IDs (stable SipHash-2-4, 128-bit), digests | — |
| `graph` | Agents, endpoints, holes and labels; connect/disconnect primitives; well-formedness check; digest | `sig`, `id` |
| `net` | Builder for patch and inject nets (vars, agents, links, effects), serializable `NetData` | `sig`, `value` |
| `check` | Static patch legality (linearity, arity, types, boundary, conservation, effect authority, bounds) | `net`, `sig` |
| `rule` | `Rule`, `RuleTable` (lookup by unordered kind pair plus orientation), `RuleCx`, modes, coverage analysis | `net`, `check` |
| `apply` | Splicing a patch into the graph: path resolution through virtual endpoints, vicious circles, new active pairs | `graph`, `net` |
| `policy` | `Policy` trait and the read-only `PatchView` neighborhood | `graph`, `net` |
| `machine` | `Program`, `Machine`: inject, run (FIFO, LIFO, random, rounds), faults, outbox, stats, observe | everything above |
| `journal` | Records, the `Journal` trait, `MemJournal`, `FileJournal`, replay and recovery | `machine` |
| `explain` | Wait-for analysis and rendering | `graph` |
| `dot` | Graphviz export (inspector v0) | `graph` |
| `nets::{arith, combinators, lease, session, session_typed}` | Example programs and host harnesses for the experiments | public API only |

Data flow of one reduction:
1. Pop an active pair.
2. Look up the rule and orient the pair.
3. The rule builds the patch.
4. Run the static checks and the policy checks.
5. Resolve the splice paths and run the dynamic type check on every final wire.
6. Mutate: remove the pair, insert the new agents, rewire the neighbors, allocate effect holes.
7. Enqueue new active pairs.
8. Journal the `Reduce` record.

If any step before 6 fails, the graph is untouched and a `Fault` is recorded.

## 5. Experiments (detailed design)

### A: lease pool, using `INTERACT` only

- **Lifecycle as kinds** (conserved class `lease`, keyed by lease id): `FreeLease → Granted → RunJob (effect) → Returned → FreeLease`. This is the readme's `Available → Reserved → Active → Released` compiled into active-pair reductions.
- **Pool:** a `Pool(free, queue)` agent consumes its mailbox stream (`MCons(msg, rest)`). Each message creates `Dispatch`.
  - A `Demand` starts a `Seek` along the free list: grant the first eligible lease, or enqueue the demand at the tail of the wait `Queue`.
  - A `Returned` lease starts a `Match` along the queue: grant to the first eligible waiting demand, or push the lease onto the free list.
  - Successive messages pipeline through the lists. A later `Seek` waits on the output wire of the earlier one, so acquisition is ordered and race-free *by construction*.
- **Workspaces:** an admission inject appends `Demand` to the pool mailbox and wires its reply port to `AwaitLease(status)`.
  - `Granted >< AwaitLease` emits the effect `RunJob` (the effect agent holds the lease).
  - The host replies `JobDone` or `JobFailed`. The lease returns through a `Send` effect, routed by the host into the pool mailbox (an injected `Route` agent plus the `Route >< Send` rule).
  - On failure, a bounded number of retries re-demand.
  - The status hole ends at `Done` or `Failed`.
- **Policies:** residency (lease region allowed for the demand) and tenant match on every grant. A deliberately buggy rule variant must be rejected.
- **Host harness:** a seeded simulator.
  - It picks enabled actions (admit, route, complete or fail a job, crash and recover) and runs the machine with random budgets, so injections interleave with partial reduction.
  - The fake provider keeps an authoritative ledger and checks idempotency per effect ID.
- **Oracles:**
  - a global conservation census;
  - provider-ledger exclusivity;
  - policy checks;
  - a reference model (a plain Rust FIFO-eligible pool) that must produce the same assignments from the same mailbox order;
  - completion with a clean final graph;
  - an identical digest across schedulers;
  - crash and recovery that matches the run without crashes.
- **Metrics:** reductions per grant, host events per lease cycle, maximum and final graph size, parallel width and depth (rounds scheduler).

### B: session with timeouts, through boundary transport

- Each endpoint has an inbox stream: a labeled hole, where message kinds *are* the stream cells (`Open`, `Accept`, `Data(seq, v)`, `Ack(seq)`, `Close`, `Closed`, `Timeout(tag)`).
- Endpoint state agents consume their inbox:
  - client: `Connecting`, `AwaitAck`, `Closing`, `Done`/`Aborted`;
  - server: `Listen`, `Established(expected, delivered)`, `Closed`.
- Effects:
  - `Transmit(to, msg)`, which the host network simulator delivers, drops, duplicates, or reorders;
  - `Timer(tag)`, which the host fires by appending `Timeout` to the client inbox.
- Stop-and-wait with retransmission and a bounded number of attempts. The server deduplicates by sequence number.
- Every (state, message) pair has an explicit rule, including rejection rules. Static coverage analysis checks that every pair of kinds whose principal port types can meet has a rule or is explicitly declared forbidden.
- **Oracles:**
  - if the client ends in `Closed`, the server delivered exactly the items, in order;
  - if it ends in `Aborted`, the server delivered a prefix with no duplicates;
  - adversarially injected invalid messages are never accepted;
  - `explain()` classifies the final state.

### B0: typed in-graph session channel

The client and server endpoints meet across one wire, and each exchange is one interaction. Protocol states are port types, so a rule that sends an illegal message is rejected by the type check, which gives static sequencing. Expected finding: in-graph channels get static typing but **cannot** express timeouts, because a race needs boundary arbitration (D2).

### Generality

- **Peano arithmetic:** `Z, S, Add, Mul, Dup, Era`. Random expression trees are checked against Rust arithmetic.
- **Symmetric interaction combinators:** `γ, δ, ε`, all of one self-dual type. Random closed and open nets are reduced under random schedules, and the resulting digests must be identical.

## 6. Error handling

- Builder misuse returns a `RuleError` inside a rule (which becomes a fault) or an `InjectError` at the host, never a panic.
- Inject failures leave the graph unchanged, and the API returns an error.
- Replay divergence (digest mismatch, unknown rule, version mismatch on a deterministic rule, a record for a missing pair) is a hard `RecoveryError`.
- Budget exhaustion is a normal outcome (`RunOutcome::Budget`), not an error.

## 7. Testing strategy

- **Unit tests** per module: values, schema, signature validation, builder linearity, the static checks (one failing case per rule), every corner of path resolution (pass-through, a pair's aux ports wired to each other, self-loops, vicious circles), and labels.
- **Property tests** with a local seeded RNG (no proptest dependency; the failing seed is printed):
  - confluence of random combinator nets;
  - Peano evaluation against Rust;
  - the boundary-determinism theorem (interleaving injects with partial runs gives the same digest);
  - replay equality;
  - crash anywhere followed by recovery equals the run without a crash.
- **Experiment suites** as integration tests that run hundreds of seeds within CI time. Example binaries print metrics, traces, and DOT output.
- **TDD:** each behavior gets a failing test first.

## 8. Risks

- **The list-scan cost in the pool is O(n) per message.** Acceptable for 10/3, and reported. A difference-list queue is the known fix.
- **Host round-trips for lease returns.** These are measured, and they are the price of D2.
- **Static guarantees are weaker for closure rules.** Validating every patch at runtime plus coverage analysis mitigates this. A template-only rule form that allows full static verification is noted as future work.
- **Learned-rule mechanism only.** No model is trained, so the P6 claims stay untested.
