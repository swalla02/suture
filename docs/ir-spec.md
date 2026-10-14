# Suture IR and memory semantics, v0

This is the formal specification the readme names as its next artifact (§10): "a short formal IR + memory-semantics specification (including three worked reductions)". It describes what the `suture` crate implements and enforces. Section numbers in the code comments refer to this document.

**Status:** implemented and tested; not yet proven. The theorems in §13 are given with proof sketches and are exercised by the property tests listed there. There is no machine-checked proof.

Notation:
- `A >< B` is an active pair of agents A and B.
- `x ~ y` is a wire between two endpoints.
- `T+` and `T−` are the two polarities of a polarized type `T`.
- `dual(T±) = T∓`, and for a self-dual type `dual(T) = T`.

---

## 1. Scope

The IR is a typed port graph that is rewritten by exactly one internal operation, `INTERACT`. The host touches it only through these operations:

| Level | Operation | Defined in |
| --- | --- | --- |
| Pure compute | `INTERACT(A, B)` replaces an active pair by a validated patch | §7 |
| Admission and construction | `INJECT(net, binds)` admits a net at the boundary | §8.1 |
| Side effects | an effect agent emitted by a rule; the host answers at its hole | §8.2 |
| Persistence | `COMMIT` makes the journal records so far durable | §11 |
| Observation | read-only views: graph, outbox, explanation, digest, DOT | §8.3, §12 |

There is no `BIND`. Discovery and matching are expressed as nets (readme P1; the evidence is in `docs/experiments.md`).

## 2. Signature

A signature Σ is versioned (`name`, `version`) and declares types and kinds.

**Types.** A type τ has a mode:
- *Polarized*: ports have type `τ+` (producer) or `τ−` (consumer).
- *Self-dual*: there is a single port type `τ`.

Two port types are **compatible** iff they name the same type and either the type is self-dual or the polarities differ:

```
compatible(τp, σq) ⇔ τ = σ ∧ (selfdual(τ) ∨ p ≠ q)
```

Types are monomorphic atoms in v0. A protocol state is a distinct atom (§15 and B0).

**Kinds.** A kind κ declares:
- a principal port type `π(κ)`;
- auxiliary port types `α₁(κ)…αₙ(κ)`, where `n = ar(κ)`, each with a name;
- a payload schema `ν(κ)` (§3);
- `effect(κ)`, a flag: the principal port of an effect agent faces the host (§8.2);
- `conserved(κ) = (c, i)`, optional: agents of κ are linear capabilities of resource class `c`, identified by tuple field `i` of the payload (§9, T3).

Port 0 of an agent is its principal port. Ports `1..n` are its auxiliary ports.

## 3. Values

A payload is an immutable value:

```
v ::= () | bool | int64 | string | (v, …, v) | [v, …, v]
```

A schema `ν ::= Any | Unit | Bool | Int | Str | Tuple(ν…) | List(ν)` admits values structurally. Values have no identity, and equality is structural. Every value has a canonical byte encoding (tagged and length-prefixed) that is used for digests, so distinct values encode distinctly.

## 4. Nets

A **graph** G consists of:
- a finite set of agents, `a ↦ (κ(a), v(a), ports(a), cause(a))`;
- a finite set of holes, `h ↦ (peer(h), label(h)?, origin(h))`.

An **endpoint** is either `a.p`, port `p` of agent `a`, or a hole `h`. `ports(a)[p]` and `peer(h)` give the endpoint each one is wired to.

**Well-formedness.** `Graph::check` verifies W1–W6, and the test suites call it throughout.

- **W1 Arity.** `|ports(a)| = 1 + ar(κ(a))`.
- **W2 Payload.** `ν(κ(a))` admits `v(a)`.
- **W3 Symmetry.** Every endpoint is wired to exactly one other endpoint and back: `peer(peer(e)) = e`, and `peer(e) ≠ e`. A wire between two auxiliary ports of the same agent, or between an agent's principal and its own auxiliary port, is allowed (the latter is a one-agent deadlock, §12).
- **W4 Typing.** For every wire `a.p ~ b.q` between two agent ports, `compatible(type(a.p), type(b.q))`, where `type(a.0) = π(κ(a))` and `type(a.i) = αᵢ(κ(a))`. A wire with a hole at one end is unconstrained on the hole side: a hole adopts the dual of its peer.
- **W5 Labels.** Labels are unique among live holes.
- **W6 Ready queue.** The set of queued pairs equals the set of active, non-faulted pairs. This is a machine invariant checked by `Machine::check_invariants`.

The **boundary** of G is its set of holes. Holes are the only places where the host can attach structure (§8.1) and where effects wait (§8.2).

## 5. Active pairs and rules

Agents `a ≠ b` form an **active pair** iff `ports(a)[0] = b.0`. Each agent has one principal port, so **active pairs are disjoint**: an agent belongs to at most one.

A **rule** for the unordered kind pair `{κₗ, κᵣ}` is a function

```
R : (κₗ, v(l), κᵣ, v(r)) → Patch | Error
```

It is identified by a name and version and has a mode: `Deterministic`, or `Recorded` (§11). It also carries an effect authority, the set of effect kinds it may emit. Each unordered pair of kinds has at most one rule. For a pair of the same kind, the left agent is the one with the smaller identity (§10), which makes orientation deterministic.

A rule reads only the pair's kinds and payloads. It cannot see the neighbourhood, the graph, the clock, or the host. In the implementation, rules are Rust closures that build a patch through `RuleCx`. A closure that panics, returns an error, or builds a malformed patch causes a fault (§7.3).

**Coverage.** For a signature and rule table, `coverage()` lists:
- every pair of kinds whose principal types are compatible that has neither a rule nor an explicit `forbid`;
- every rule whose principal types can never meet.

A program with empty `missing` cannot get stuck for lack of a rule. All five example programs have complete coverage.

## 6. Patches

A **patch** for `l >< r` is a net over wire variables:

```
P = (vars, agents, links)
agent  = (κ, v, slots),  slot ::= var | Emit
link   = (var, var)
```

The first `ar(κₗ) + ar(κᵣ)` variables are **interface variables**. Variable `i` stands for auxiliary port `i` of the pair, the left agent's ports first. Each interface variable has one **implicit use**: the context, meaning whatever was wired to that auxiliary port.

**Legality.** `check_patch` enforces L1–L8 against the patch alone, before the graph is touched.

- **L1 Arity.** Each agent has `1 + ar(κ)` slots.
- **L2 Payload.** `ν(κ)` admits each payload.
- **L3 Linearity.** Every variable has exactly two uses, counting slots, link ends and implicit uses. A closed loop of links that never touches the context is an error.
- **L4 Interface preservation.** The endpoint that ends up attached to interface variable `i` has the type of the replaced auxiliary port, `iface[i]`. Equivalently, it is compatible with `dual(iface[i])`, the type of whatever was outside. A pass-through link between interface variables `i` and `j` requires `compatible(iface[i], iface[j])`.
- **L5 Wire typing.** Every wire inside the patch, after following links, connects compatible types.
- **L6 Effects.** `Emit` appears only in the principal slot of an effect kind. Effect kinds appear only with `Emit`. Each emitted kind is in the rule's effect authority.
- **L7 Conservation.** For each conserved class, the multiset of keys of the consumed pair equals the multiset of keys of the created agents.
- **L8 Bounded work.** The patch has at most `Limits::max_patch_agents` agents (256 by default).

Then:

- **L9 Policies.** Every registered `Policy` accepts the patch. A policy sees the consumed pair and the created agents, and may reject. Hard constraints are enforced, never scored.

## 7. `INTERACT`

### 7.1 Splicing

Let `l >< r` be active and `P = R(l, r)` a legal patch. Let `out(i)` be the endpoint outside auxiliary port `i`, that is, `ports(l)[i+1]` or `ports(r)[i+1-ar(κₗ)]`.

Resolution works on a small graph of **virtual nodes**, where every variable and every link is a node of degree 2:
- an interface variable's implicit use is connected to `out(i)`;
- when `out(i)` is itself auxiliary port `j` of the pair (the pair's auxiliary ports are wired to each other), the implicit uses of `i` and `j` are **glued**.

The terminals are new agents' ports and outside endpoints. Following each chain from terminal to terminal gives the final wires. Chains closed only through glue are Lafont's **vicious circles**: wire loops without agents, which are dropped. `net::resolve` implements this, and it is shared by patch checking, splicing and `INJECT`.

### 7.2 The step

`INTERACT(l, r)`:
1. Validate L1–L9 and resolve the wires.
2. Check W4 on every final wire whose two ends are agent ports. L4 already implies this when G is well typed.
3. Remove `l` and `r`.
4. Insert the patch agents, each with identity `id(l, r, rule, k)` (§10) and cause `Reduce(l, r, rule)`.
5. For each effect agent `e`, create the hole `effect(e)` with `peer = e.0` (§8.2).
6. Link the final wires.
7. Enqueue every new principal–principal wire as an active pair.
8. Append `Reduce(l, r, rule, version, digest(P), P if Recorded)` to the journal.

The step is atomic. Steps 1–2 never mutate, and steps 3–8 cannot fail.

### 7.3 Faults

If there is no rule, the rule errors or panics, or L1–L9 or W4 fail, the pair becomes **stuck**: it is recorded as a `Fault` (journaled), never retried, and the graph is unchanged. Faults are local, so the rest of the graph keeps reducing.

### 7.4 Scheduling

A scheduler chooses which ready pair to reduce next: FIFO, LIFO, seeded random, or rounds. In rounds mode, every pair ready at the start of a round is reduced before the next round, which measures critical-path depth and width. By T1 and T2, the choice never affects the resulting graph.

## 8. Host operations

### 8.1 `INJECT`

`INJECT(N, B, key?, mint)`:
- `N` is a net (§6) with no interface variables.
- `B` binds some of `N`'s variables to the boundary. A binding is either `Hole(h | label)`, which consumes an existing hole, or `New(label?)`, which creates a hole.

Validity:
- **I1.** L1–L3, and L8 against `Limits::max_inject_agents`, hold with bindings counted as uses.
- **I2.** Each existing hole is bound at most once. Each referenced hole and label exists.
- **I3.** New labels are distinct, and each is either free or held by a hole this inject consumes. That case is *label transfer*, the way a stream tail advances.
- **I4.** `N` contains no effect agents. Conserved agents require `mint`.
- **I5.** Every final wire between agent ports is compatible (W4). For a bound hole, the real endpoint is the hole's current peer. Two consumed holes wired to each other are glued (§7.1).

The inject applies only if it is valid, and an invalid inject changes nothing and consumes no sequence number. Applying it:
1. Remove the consumed holes.
2. Create the new holes: `hole(seq, k)`, with origin `Inject(seq)`.
3. Insert agents `id(seq, k)` with cause `Inject(seq)`.
4. Link the wires.
5. Enqueue new active pairs.
6. Journal `Inject` with labels resolved to hole identities.

An idempotency key makes a second inject with the same key a no-op that returns the first sequence number.

Two consequences:
- `INJECT` is the only way new structure, and new nondeterminism, enters the graph (D1).
- It also performs graph construction (`CONNECT`): a variable bound to two existing holes wires their peers together.

### 8.2 Effects and the outbox

A rule creates an effect agent `e` with `Emit`. `e.0` is wired to a fresh hole `effect(e)`, and the pair `(e, effect(e))` is an **outbox entry**. An effect agent's principal faces the host, so only the host can interact with it. The host answers by injecting a reply agent at `effect(e)`. The reply and `e` then form an active pair, and an ordinary rule consumes them. Effects are therefore *holes the world must fill*.

- **Dispatch discipline.** A host dispatches an effect only after `COMMIT`. Delivery is at least once. The effect's hole identity is stable across crash and replay (§10), so adapters deduplicate on it.
- **Authority.** A rule can only emit the effect kinds its authority lists (L6). Adapters must still authorize independently: the provider in Experiment A re-checks residency and keeps its own ledger.
- **Effect queue.** The machine indexes pending effects by hole and queues each new one.
  - `take_new_effects()` returns the effects created since its last call that are still pending, sorted by hole. It examines only those entries, counted in `Stats::effects_examined`.
  - `outbox()` lists every pending effect from the same index, in hole order, without scanning the graph.
  - After recovery, every pending effect is queued again, so a host dispatches it again. Adapters deduplicate on the hole identity.

### 8.3 Observation

Observation never mutates. The read-only views are:
- graph queries;
- `outbox()`: pending effects, sorted;
- `census(class)`: the conserved keys present;
- `hole_peer`;
- `digest()`;
- `explain()` (§12);
- `to_dot()`.

## 9. Memory semantics

This is the readme's hybrid hypothesis (§2.7), made concrete.

- **M1 Immutable values.** Payloads are values (§3). A rewrite that "changes" a payload creates a new agent. There are no mutable references anywhere in the graph.
- **M2 Linear wires.** Every port has exactly one peer (W3). Two agents can only "share" something through explicit structure. The kernel never aliases.
- **M3 Explicit sharing and erasure.** Copying structure requires a duplicator agent and its rules (Peano `Dup`, combinator δ). Discarding structure requires an eraser (Peano `Era`, combinator ε). The kernel never copies or frees structure implicitly.
- **M4 Linear capabilities.** A conserved kind is a capability. By T3 no rule can duplicate, drop or forge one. Its lifecycle is a sequence of conserved kinds, such as `FreeLease → Granted → RunJob → Returned → FreeLease` in Experiment A. A token does not replace the external ledger: adapters still enforce access (§8.2).
- **M5 External state by capability.** Durable external data lives behind effects. The graph holds identifiers inside capability payloads, never ambient references.
- **M6 Read-only sharing.** Read-only data is shared either by value (an immutable payload, internally reference-counted) or by an explicit duplicator tree of a read-only kind.

The experiments use all three representations the readme lists:
- payload-on-agent: lease ids, regions, sequence numbers;
- value-as-network: free lists, queues, mailboxes and inboxes as streams, numerals;
- external-by-capability: the provider ledger behind `RunJob`.

No structure needed global mutation.

## 10. Identity

Every agent and hole identity is a 128-bit SipHash-2-4 digest with fixed keys. Each identity is domain-separated and length-prefixed, and the known-answer test pins the encoding:

```
id(seq, k)          = H("agent/inject", seq, k)             -- k-th agent of inject #seq
id(l, r, rule, k)   = H("agent/reduce", l, r, rule, k)       -- k-th agent of the patch for l >< r
hole(seq, k)        = H("hole/inject",  seq, k)              -- k-th binding of inject #seq
effect(e)           = H("hole/effect",  e)                   -- outbox hole of effect agent e
```

Identities depend on causal origin, never on scheduling. Consequences:
- Graphs reached through different schedules are **identical**, not merely isomorphic. This makes T1 and T2 testable by digest equality.
- Effect identities are stable across crash and replay, which makes them natural idempotency keys.
- Concurrent reducers could allocate identities without coordination.

Each pair reduces at most once, so `(l, r)` is unique and identities are unique up to hash collision (probability about 2⁻⁶⁴ at 2³² agents). `insert_agent` asserts this in debug builds.

## 11. Journal and replay

**Records.**

| Record | Fields |
| --- | --- |
| `Header` | program, version, format |
| `Inject` | seq, key?, mint, net, binds with resolved holes |
| `Reduce` | l, r, rule, version, digest, patch? |
| `Fault` | l, r, rule?, error |
| `Commit` | — |

`append` buffers records. `COMMIT` makes them durable atomically. `FileJournal` appends JSON lines and a commit marker, then calls `fsync`. Opening a journal truncates any torn or uncommitted tail.

**Recovery** replays the committed records in order, using the same code paths with journaling disabled:
- Injects are re-applied with their recorded hole identities.
- A deterministic `Reduce` re-runs its rule. A version mismatch fails with `RuleVersion`, and a digest mismatch fails with `Divergence`. **Rule-code drift is detected, never silently replayed.**
- A recorded `Reduce` applies the journaled patch after re-validating it. The rule is not re-run, so a nondeterministic or learned law reproduces its recorded decision.
- The ready queue is rebuilt from the active pairs that have not faulted.

## 12. Quiescence and explanation

Every agent's principal points at exactly one endpoint, which defines a functional **wait-for** graph:

```
next(a) = a.0's peer:   hole h          → external wait (or pending effect if a is an effect at effect(a))
                        b.0             → active pair: Ready, or Stuck if faulted
                        b.k, k > 0      → a waits on b; follow b
```

Following `next` from any agent ends at one of:
- a hole, which is an external wait;
- an effect hole, which is a pending effect;
- a ready pair;
- a stuck pair;
- a cycle, which is a **deadlock**: no rewrite can break it, since every member waits on an auxiliary port.

`explain()` groups all agents by terminal. This answers the readme's P2 question ("is quiescence a valid completion state, a deadlock, or an external wait?") structurally, without domain knowledge. For example, the final state of an Experiment B session reads:

```
4 agents, 4 holes, 0 ready, 0 faults
- waiting on hole 'app:1' (h:6cf8521d): 1 agent(s) [Delivered×1]        ← result delivered
- waiting on hole 'c:1' (h:75978d9d): 1 agent(s) [CDone×1]              ← client finished, draining its inbox
- waiting on hole 'cstatus:1' (h:b9835616): 1 agent(s) [SessionOk×1]
- waiting on hole 's:1' (h:f2718564): 1 agent(s) [SClosed×1]
```

## 13. Theorems

**T1 (Strong confluence).** Assume every rule is deterministic, that is, a function of the pair's kinds and payloads. If `G →P G₁` and `G →Q G₂` for distinct active pairs P and Q, then `G₁ →Q G'` and `G₂ →P G'` for the **same** `G'`, identities included. The same holds when P or Q faults: the fault set does not depend on order.

*Sketch.*
- Active pairs are disjoint (§5).
- Reducing P removes only P's agents and rewires only the outside endpoints of P's auxiliary ports. Q's agents, payloads and principal wire are untouched, so Q stays active and its patch, legality and identities (§10) are the same in both orders.
- Splicing is local substitution at P's interface. A wire between an auxiliary port of P and one of Q is rewired by each splice at its own end, and both orders compose to the same final wire (the glue case of §7.1).
- Whether a pair faults depends only on the pair, its patch and static checks (L1–L9). Dynamic typing (W4) is implied by L4 in a well-typed graph.

*Corollary.* All maximal reduction sequences from G reduce the same set of pairs, have the same length, and end in the same graph.

*Tests:* `schedulers_agree_on_digest`, `random_expressions_match_rust_under_all_schedulers`, `strong_confluence_on_random_nets` (equal digests **and** equal reduction counts), and `rounds_*`.

**T2 (Boundary determinism).** Let `E = e₁…eₖ` be injects, each valid when applied. Apply E with any interleaving of internal reductions and reduce to quiescence. The result depends only on E.

*Sketch.*
- Injected identities depend only on sequence numbers.
- An inject never destroys an active pair or changes an existing payload.
- Reductions never consume holes: they only change hole peers, and only the host consumes holes.
- An inject bound to hole h therefore attaches to the same final endpoint whether or not the pair currently behind h has reduced. L4 keeps the type check (I5) invariant under reduction.
- T1 then makes internal reductions commute with each other and with injects.

*Precondition.* An inject that answers an effect needs the effect's hole to exist, so it must follow the reduction that creates that hole. Hosts only answer effects they have observed.

*Tests:* `boundary_determinism_inject_interleaving` (Peano) and `experiment_a_schedule_independence`. The latter replays recorded Experiment A boundary logs under LIFO, random and rounds scheduling, with random partial reduction between injects, and requires digests identical to the original run.

**T3 (Conservation).** For every conserved class `c`, the multiset of class-`c` keys in the graph changes only through injects with `mint`.

*Proof.* Rewrites satisfy L7. Injects without `mint` cannot create conserved agents (I4). Agents disappear only as a consumed pair, which L7 accounts for. A fault changes nothing.

*Tests:* the lease census is checked after every action of 450 randomized Experiment A runs, including 792 crashes. `conservation` covers the static check.

**T4 (Replay).** Recovery from a journal yields exactly the committed graph. Every deterministic step is re-verified by digest, and every recorded step reproduces its journaled patch.

*Tests:*
- `replay_*`
- `crash_drops_uncommitted_and_rerun_converges`
- `file_journal_torn_tail`
- `replay_detects_rule_drift`
- `replay_refuses_version_change`
- `recorded_rule_replays_without_rerunning`
- the crash runs of Experiment A

**Not guaranteed.**
- Termination. `run(budget)` bounds work, and non-terminating nets are resumable.
- Deadlock freedom. Deadlocks are detected (§12), not prevented.
- Protocol liveness, which depends on host timers.
- Semantic correctness of rules. Typing establishes legality, not meaning.

## 14. What survives each extension

The readme (§1, P3) warns that typed guards, matching, external state and learned rewrites may sacrifice the properties of strict interaction nets. This table records what v0 keeps:

| Extension over Lafont's nets | Strong confluence (T1) | State = f(boundary log) (T2) | Conservation (T3) | How |
| --- | --- | --- | --- | --- |
| Typed ports (polarized and self-dual) | kept | kept | — | typing only restricts which nets exist |
| Payloads and payload-dependent rules | kept | kept | kept | rules are pure functions of the pair |
| Guards and policies (reject) | kept | kept | kept | rejection is a fault, decided by the pair alone |
| Effects (host-facing principal ports) | kept inside the graph | kept | kept | the reply is an inject |
| Labeled holes and stream tails | — | kept | — | labels resolve at inject time and are journaled as identities |
| Recorded (opaque or learned) rules | **lost** unless the rule is deterministic | kept relative to the journal | kept (L7 still checked) | the output is journaled like an inject |
| `amb` or `BIND` (not implemented) | **lost** | only if every choice is journaled | — | see `docs/experiments.md`, P1 |

## 15. Three worked reductions

Each reduction lists the active pair with the port types of its auxiliary ports, the patch the rule builds, the checks it must pass, and the resulting wiring. Identities are abbreviated.

### 15.1 `S(x) >< Add(y, r)` (Peano addition, `add_s`)

Before (computing `1 + 1` into hole `out`):

```
S#a  : principal Nat+  ~ Add#b.0           ← the active pair
S#a.pred   : Nat-  ~ Z#c.0                   out(0)
Add#b.y    : Nat-  ~ S#d.0  (S#d.pred ~ Z#e.0)   out(1)
Add#b.r    : Nat+  ~ hole "out"              out(2)
```

The interface types are `iface = [Nat−, Nat−, Nat+]`, and the variables are `x=v0, y=v1, r=v2`. The rule builds one fresh variable, `r1 = v3`:

```
Add'(v0; v1, v3)      -- principal at x, y passed through, result into r1
S'(v2; v3)            -- S' at r, its predecessor is r1
```

Checks:
- L1 holds (Add takes two auxiliary ports, S one), and L2 holds.
- L3: each of `v0`–`v3` is used exactly twice.
- L4: `Add'.0` is `Nat−` and `iface[0] = Nat−`; `Add'.y` is `Nat−` and `iface[1] = Nat−`; `S'.0` is `Nat+` and `iface[2] = Nat+`.
- L5: `v3` connects `Add'.r` (`Nat+`) with `S'.pred` (`Nat−`), which is compatible.
- L6–L8 hold trivially.

After resolution:

```
Add'.0 ~ Z#c.0          ← new active pair (add_z will fire)
Add'.y ~ S#d.0
Add'.r ~ S'.pred
S'.0   ~ hole "out"
```

The identities are `Add' = H(a, b, "add_s", 0)` and `S' = H(a, b, "add_s", 1)`. Next, `add_z` links `y ~ r`, wiring `S#d.0` straight to `S'.pred`. The result at `out` is `S'(S#d(Z#e)) = 2`.

### 15.2 `Seek >< FreeLease` (lease grant, `seek_free`)

Before: the pool is serving request `r1`, whose demand is `("r1", "acme", ["eu-west-1"], 1)`. The head of the free list is lease 2 in eu-west-1.

```
Seek#s[D]  .0 FreeList-  ~ FreeLease#f.0       ← active pair
  .reply Grant+   ~ AwaitLease#w.0   (r1's workspace)
  .out   FreeList+ ~ Seek#t.0        (the next message's scan, pipelined)
  .qi    Queue-   ~ QNil#q.0
  .qo    Queue+   ~ Pool#p.queue
FreeLease#f[(2,"eu-west-1")] .next FreeList- ~ FNil#n.0
```

The interface is `[reply: Grant+, out: FreeList+, qi: Queue−, qo: Queue+, next: FreeList−]`. The region is eligible, so the rule builds:

```
Granted[(2, "eu-west-1", "r1", "acme")](v0)   -- at reply
link(v1, v4)                                  -- out ~ next : the rest of the list flows on
link(v3, v2)                                  -- qo ~ qi    : the queue is unchanged
```

Checks:
- L4: `Granted.0` is `Grant+`, the same as `iface[0]`. Both links connect compatible outside types: `FreeList+` with `FreeList−`, and `Queue+` with `Queue−`.
- L6: no effects.
- L7: the consumed multiset `{lease:2}` (from `FreeLease`) equals the created multiset `{lease:2}` (from `Granted`).
- L9 `residency`: the granted region `eu-west-1` is in the demand's regions.

After:

```
Granted.0 ~ AwaitLease#w.0      ← new active pair (granted_await)
Seek#t.0  ~ FNil#n.0            ← the next scan resumes on the rest of the list
Pool#p.queue ~ QNil#q.0
```

*Counterfactual.* With `buggy_ignore_region`, the same rule would grant lease 1 in us-east-1 to this EU demand. `residency` rejects the patch, so the pair faults, the graph is unchanged, the census stays `{1}`, and `explain()` reports `STUCK Seek >< FreeLease: policy residency rejected …` (`buggy_rule_is_stopped_by_residency_policy`).

### 15.3 Effect round trip: `Granted >< AwaitLease` → `RunJob` → `JobDone`

**Step 1: `granted_await`.** The pair is `Granted[(2,"eu-west-1","r1","acme")] >< AwaitLease[D](status: Status+)`, where `status` is wired to hole `op:r1`. The patch is:

```
effect RunJob[(2, "eu-west-1", "r1", "acme", ["eu-west-1"], 1)](v0)    -- status at v0
```

- L6: `RunJob` is an effect kind created with `Emit`, and the rule's authority is `{RunJob}`.
- L7: `{lease:2}` becomes `{lease:2}`.
- The kernel creates the hole `h = effect(RunJob#j)` and wires `RunJob#j.0 ~ h`. The outbox now lists `RunJob#j` at `h`, and `RunJob#j.status ~ hole "op:r1"`.

**Step 2: host.** After `COMMIT`, the provider starts the job, keyed by `h`, which is stable across crashes. When the job ends, the host injects:

```
INJECT( JobDone(v0) , { v0 ↦ Hole(h) } , key "reply:h" )
```

I5 holds: `JobDone.0` (`JobReply+`) is compatible with `RunJob#j.0` (`JobReply−`). The hole `h` is consumed, and `JobDone >< RunJob#j` is now active.

**Step 3: `job_done`.**

```
m = fresh variable
effect Send[("pool")](m)          -- the Send's msg port at m
Returned[(2, "eu-west-1")] @ m    -- the message itself
Done[("r1", 2)] @ status
```

- L7: `{lease:2}` (from `RunJob`) equals `{lease:2}` (from `Returned`).
- The new outbox entry is `Send` at `effect(Send)`.
- Hole `op:r1` now faces `Done.0`, so the request's status is readable as `Done{lease: 2}`.

**Step 4: transport.** The host routes the `Send`:

```
INJECT( Route(p; d, n), { p ↦ Hole(effect(Send)), d ↦ Hole("pool"), n ↦ New("pool") } )
```

`route_send` builds `MCons(msg, next) @ dest`, appending the `Returned` message to the pool mailbox. The label `pool` transfers to the new tail. The many-to-one merge, where several holders return leases to one pool, happens here at the boundary, in journaled order (D2).
