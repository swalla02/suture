# Suture Kernel v0 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a deterministic, typed interaction-net kernel in Rust, then use it to run Experiments A (lease contention) and B/B0 (typed sessions) plus two generality demos, and publish the formal IR spec and the findings.

**Architecture:** There is one crate, `suture`. A port graph with lineage IDs is rewritten only by `INTERACT`. Rules are Rust closures that build a patch, and every patch passes static and dynamic validation before it is spliced in atomically. The host changes the graph only through `INJECT` and effect holes. Everything is journaled, so state is a function of the boundary log.

**Tech Stack:** Rust 1.97 (edition 2024), with `serde`/`serde_json` (journal), `siphasher` (stable 128-bit lineage hashing), and `thiserror` (errors). Tests use a local SplitMix64 RNG and no proptest.

**Spec:** `docs/superpowers/specs/2026-10-12-suture-kernel-design.md`. The decisions D1–D10 referenced below are defined there.

## Global Constraints

- The interior is deterministic. The only nondeterministic inputs are the `INJECT` order and the output of `Recorded` rules, and both are journaled (D1).
- No `BIND`. `INTERACT` is the only internal rewrite (D2).
- Agent and hole IDs are lineage hashes (SipHash-2-4, 128-bit, fixed keys) and must never depend on scheduling (D3).
- No patch is installed without validation, and a failed validation leaves the graph untouched (D4, D10).
- No panics escape `Machine` APIs. A panic inside a rule closure becomes a fault.
- No library-level HashMap iteration may influence results. Sort wherever order matters.
- Dependencies are limited to serde, serde_json, siphasher, and thiserror. No other crates.
- Every new behavior is test-first (TDD). Run `cargo test` before every commit, and the commit must be green.

## Review Focus

1. **A rule closure that panics** (for example, a payload index out of bounds) must become a fault on that pair while the machine keeps running. This is pinned in Task 7 (`rule_panic_becomes_fault`).
2. **A malformed host inject** (wrong payload shape, unknown label, a hole bound twice, label collision, type mismatch at a hole, minting without permission) must return `Err`, leave the graph byte-identical (same digest), and consume no sequence number. This is pinned in Task 7 (`inject_errors_leave_graph_unchanged`).
3. **A non-terminating net** must return `RunOutcome::Budget` from `run(Some(n))`, stay consistent and resumable, and replay its partial progress exactly. This is pinned in Task 9 (`non_terminating_net_is_resumable`) and Task 10 (`replay_partial_run`).
4. **A torn journal tail** (a partial last line, or records after the last commit) must be ignored on recovery, and appending must then continue cleanly. This is pinned in Task 10 (`file_journal_torn_tail`).
5. **Rule code changing under an old journal** (same version, different behavior) must be refused with a clear divergence error rather than replayed silently. This is pinned in Task 10 (`replay_detects_rule_drift`).

---

## File Structure

```
Cargo.toml
src/lib.rs            — module wiring + re-exports + `tup!`/`vlist!` macros
src/value.rs          — Value, ValueType, ValueError, canonical encoding, Display
src/rng.rs            — SplitMix64 `Rng` (harnesses + tests)
src/sig.rs            — TypeId, KindId, Polarity, PortType, TypeMode, KindDecl, Signature, SigBuilder, SigError
src/id.rs             — AgentId, HoleId, Digest, DigestBuilder, lineage constructors, IdBuildHasher
src/graph.rs          — Endpoint, Agent, Cause, Hole, HoleOrigin, Graph, GraphError
src/net.rs            — Var, Slot, NetAgent, NetData, NetBuilder, Bind, HoleRef, Injection, resolve()
src/check.rs          — NetError, PatchSpec, check_net_static(), conservation
src/rule.rs           — Rule, RuleMode, RuleCx, RuleError, RuleTable, Coverage
src/policy.rs         — Policy, PatchView
src/machine.rs        — Program, Limits, Machine, SchedulerKind, RunOutcome, Rounds, Fault, InjectReceipt, InjectError, EffectView, Stats
src/journal.rs        — Record, RecBind, Journal, MemJournal, FileJournal, JournalError, RecoveryError
src/explain.rs        — Terminal, WaitGroup, Explanation, render
src/dot.rs            — to_dot()
src/nets/mod.rs       — pub mod arith, combinators, lease, session, session_typed
src/nets/arith.rs     — Peano program, Expr, build/eval/readback
src/nets/combinators.rs — symmetric interaction combinators, random net generator, tree readback
src/nets/lease.rs     — Experiment A program, policies, reference model, simulator, report
src/nets/session.rs   — Experiment B program, network simulator, report
src/nets/session_typed.rs — Experiment B0 typed in-graph session
tests/common/mod.rs   — shared fixtures (mini programs)
tests/kernel.rs       — machine/inject/fault/effect/scheduler tests
tests/arith.rs, tests/combinators.rs, tests/journal.rs, tests/explain.rs, tests/lease.rs, tests/session.rs
examples/arith.rs, examples/lease_pool.rs, examples/session.rs
docs/ir-spec.md, docs/experiments.md
```

---

### Task 1: Crate skeleton, values, RNG

**Files:**
- Create: `Cargo.toml`, `src/lib.rs`, `src/value.rs`, `src/rng.rs`, `.gitignore`

**Interfaces (produces):**
```rust
// value.rs
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Value { Unit, Bool(bool), Int(i64), Str(Arc<str>), Tuple(Arc<[Value]>), List(Arc<[Value]>) }
impl Value {
    pub fn tuple(items: impl IntoIterator<Item = Value>) -> Value;
    pub fn list(items: impl IntoIterator<Item = Value>) -> Value;
    pub fn as_int(&self) -> Option<i64>; pub fn as_str(&self) -> Option<&str>; pub fn as_bool(&self) -> Option<bool>;
    pub fn items(&self) -> Option<&[Value]>;            // tuple or list elements
    pub fn get(&self, i: usize) -> Result<&Value, ValueError>;   // tuple field i
    pub fn int(&self, i: usize) -> Result<i64, ValueError>;
    pub fn str(&self, i: usize) -> Result<&str, ValueError>;
    pub fn list_at(&self, i: usize) -> Result<&[Value], ValueError>;
    pub fn encode(&self, out: &mut Vec<u8>);             // canonical, tagged, length-prefixed
}
impl From<()>, From<bool>, From<i64>, From<i32>, From<u32>, From<usize>, From<&str>, From<String> for Value
impl Display for Value   // (), true, 42, "eu", (1, "eu"), [1, 2]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValueType { Any, Unit, Bool, Int, Str, Tuple(Vec<ValueType>), List(Box<ValueType>) }
impl ValueType { pub fn admits(&self, v: &Value) -> bool; }
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("payload {value}: {reason}")] pub struct ValueError { pub value: String, pub reason: String }
// lib.rs macros
macro_rules! tup { ($($x:expr),* $(,)?) => { $crate::Value::tuple([$($crate::Value::from($x)),*]) } }
macro_rules! vlist { ($($x:expr),* $(,)?) => { $crate::Value::list([$($crate::Value::from($x)),*]) } }
// rng.rs
pub struct Rng(u64);
impl Rng { pub fn new(seed: u64) -> Rng; pub fn next_u64(&mut self) -> u64; pub fn below(&mut self, n: u64) -> u64;
           pub fn chance(&mut self, p: f64) -> bool; pub fn shuffle<T>(&mut self, xs: &mut [T]); pub fn index(&mut self, len: usize) -> usize; }
```
`From<Value> for Value` comes from the blanket impl, so `tup![v.clone()]` works.

- [ ] **Step 1: Write failing unit tests** (in `src/value.rs` and `src/rng.rs` `#[cfg(test)]`)

```rust
#[test] fn schema_admits() {
    let t = ValueType::Tuple(vec![ValueType::Int, ValueType::Str, ValueType::List(Box::new(ValueType::Str))]);
    assert!(t.admits(&tup![1, "acme", vlist!["eu"]]));
    assert!(!t.admits(&tup![1, 2, vlist![]]));
    assert!(!t.admits(&tup![1, "acme"]));
    assert!(ValueType::Any.admits(&Value::Unit));
    assert!(ValueType::List(Box::new(ValueType::Int)).admits(&vlist![1, 2]));
    assert!(!ValueType::List(Box::new(ValueType::Int)).admits(&vlist![1, "x"]));
}
#[test] fn accessors() {
    let v = tup![7, "eu", vlist![1]];
    assert_eq!(v.int(0).unwrap(), 7);
    assert_eq!(v.str(1).unwrap(), "eu");
    assert_eq!(v.list_at(2).unwrap().len(), 1);
    assert!(v.int(1).is_err());
    assert!(v.int(9).is_err());
    assert!(Value::Int(3).int(0).is_err());
}
#[test] fn encoding_is_injective_on_shapes() {
    let enc = |v: &Value| { let mut b = vec![]; v.encode(&mut b); b };
    let vals = [Value::Int(1), Value::from("1"), tup![1], vlist![1], Value::Bool(true), Value::Unit, tup![], vlist![]];
    for i in 0..vals.len() { for j in 0..vals.len() { if i != j { assert_ne!(enc(&vals[i]), enc(&vals[j])); } } }
}
#[test] fn display_is_compact() {
    assert_eq!(tup![1, "eu", vlist![2, 3]].to_string(), r#"(1, "eu", [2, 3])"#);
    assert_eq!(Value::Unit.to_string(), "()");
}
// rng.rs
#[test] fn rng_is_deterministic_and_bounded() {
    let (mut a, mut b) = (Rng::new(42), Rng::new(42));
    for _ in 0..100 { assert_eq!(a.next_u64(), b.next_u64()); }
    let mut r = Rng::new(7);
    for _ in 0..1000 { assert!(r.below(10) < 10); }
    let mut xs: Vec<u32> = (0..50).collect(); r.shuffle(&mut xs); xs.sort(); assert_eq!(xs, (0..50).collect::<Vec<_>>());
}
```

- [ ] **Step 2: Run** `cargo test --lib` and confirm it fails to compile because the types are missing.
- [ ] **Step 3: Implement.** `Cargo.toml`:
```toml
[package]
name = "suture"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
description = "Suture: a typed, deterministic interaction-net kernel (Learned Interaction Machine v0)"
publish = false

[dependencies]
serde = { version = "1", features = ["derive", "rc"] }
serde_json = "1"
siphasher = "1"
thiserror = "2"
```
Encoding tags: `Unit=0, Bool=1, Int=2, Str=3, Tuple=4, List=5`. Lengths are u64 LE, ints are i64 LE, and strings are UTF-8 bytes with a length prefix. SplitMix64: `z = (state += 0x9E3779B97F4A7C15); z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9; z = (z ^ (z >> 27)) * 0x94D049BB133111EB; z ^ (z >> 31)`. `below(n)` uses rejection sampling, and `chance(p)` uses `(next_u64() >> 11) as f64 / 2^53 < p`.
- [ ] **Step 4: Run** `cargo test --lib`. Expected: PASS.
- [ ] **Step 5: Commit** with message `feat: crate skeleton, payload values, deterministic RNG`.

---

### Task 2: Signature

**Files:** Create `src/sig.rs` (unit tests inline).

**Interfaces (produces):**
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)] pub struct TypeId(pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)] pub struct KindId(pub u16);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)] pub enum Polarity { Pos, Neg }
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)] pub struct PortType { pub ty: TypeId, pub pol: Polarity }
impl TypeId { pub fn pos(self) -> PortType; pub fn neg(self) -> PortType; pub fn any(self) -> PortType /* = pos; for self-dual types */ }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum TypeMode { Polarized, SelfDual }
pub struct TypeDecl { pub name: String, pub mode: TypeMode }
pub struct PortDecl { pub name: String, pub ty: PortType }
pub struct Conserved { pub class: String, pub key: usize }      // key = tuple field index of the payload
pub struct KindDecl { pub name: String, pub principal: PortType, pub aux: Vec<PortDecl>, pub payload: ValueType, pub effect: bool, pub conserved: Option<Conserved> }
pub struct Signature { /* name, version, types, kinds, index */ }
impl Signature {
    pub fn name(&self) -> &str; pub fn version(&self) -> u32;
    pub fn kind(&self, k: KindId) -> &KindDecl; pub fn kind_id(&self, name: &str) -> Option<KindId>;
    pub fn kinds(&self) -> impl Iterator<Item = (KindId, &KindDecl)>;
    pub fn type_decl(&self, t: TypeId) -> &TypeDecl;
    pub fn arity(&self, k: KindId) -> usize;                 // aux count
    pub fn port_type(&self, k: KindId, port: usize) -> PortType;   // 0 = principal
    pub fn compatible(&self, a: PortType, b: PortType) -> bool;    // same type && (self-dual || opposite polarity)
    pub fn fmt_port_type(&self, p: PortType) -> String;            // "Nat+", "Nat-", "C"
    pub fn conserved_key(&self, k: KindId, payload: &Value) -> Option<(String, Value)>;
}
pub struct SigBuilder { .. }
impl SigBuilder {
    pub fn new(name: &str, version: u32) -> SigBuilder;
    pub fn polarized(&mut self, name: &str) -> TypeId; pub fn self_dual(&mut self, name: &str) -> TypeId;
    pub fn kind(&mut self, name: &str, principal: PortType) -> KindBuilder<'_>;
    pub fn build(self) -> Result<Signature, SigError>;
}
impl KindBuilder<'_> { pub fn aux(self, name: &str, ty: PortType) -> Self; pub fn payload(self, t: ValueType) -> Self;
                       pub fn effect(self) -> Self; pub fn conserved(self, class: &str, key_field: usize) -> Self; pub fn done(self) -> KindId; }
#[derive(Debug, thiserror::Error)] pub enum SigError { DuplicateType(String), DuplicateKind(String), TooMany, ConservedKeyOutOfSchema(String) }
```
The default payload is `ValueType::Unit`. `conserved()` requires the payload schema to be a `Tuple` with the key index present; `build()` checks this.

- [ ] **Step 1: Failing tests**
```rust
#[test] fn compatibility() {
    let mut b = SigBuilder::new("t", 1);
    let nat = b.polarized("Nat"); let c = b.self_dual("C");
    let sig = b.build().unwrap();
    assert!(sig.compatible(nat.pos(), nat.neg()));
    assert!(!sig.compatible(nat.pos(), nat.pos()));
    assert!(!sig.compatible(nat.neg(), nat.neg()));
    assert!(sig.compatible(c.any(), c.any()));
    assert!(!sig.compatible(nat.pos(), c.any()));
    assert_eq!(sig.fmt_port_type(nat.neg()), "Nat-");
    assert_eq!(sig.fmt_port_type(c.any()), "C");
}
#[test] fn kinds() {
    let mut b = SigBuilder::new("t", 1);
    let nat = b.polarized("Nat");
    let s = b.kind("S", nat.pos()).aux("pred", nat.neg()).done();
    let lease = b.kind("Lease", nat.pos()).payload(ValueType::Tuple(vec![ValueType::Int])).conserved("lease", 0).done();
    let ask = b.kind("Ask", nat.neg()).effect().done();
    let sig = b.build().unwrap();
    assert_eq!(sig.arity(s), 1);
    assert_eq!(sig.port_type(s, 0), nat.pos());
    assert_eq!(sig.port_type(s, 1), nat.neg());
    assert_eq!(sig.kind_id("S"), Some(s));
    assert!(sig.kind(ask).effect);
    assert_eq!(sig.conserved_key(lease, &tup![9]), Some(("lease".to_string(), Value::Int(9))));
    assert_eq!(sig.conserved_key(s, &Value::Unit), None);
}
#[test] fn rejects_bad_signatures() {
    let mut b = SigBuilder::new("t", 1); let nat = b.polarized("Nat");
    b.kind("S", nat.pos()).done(); b.kind("S", nat.neg()).done();
    assert!(matches!(b.build(), Err(SigError::DuplicateKind(_))));
    let mut b = SigBuilder::new("t", 1); let nat = b.polarized("Nat");
    b.kind("L", nat.pos()).conserved("lease", 0).done();   // Unit payload has no field 0
    assert!(matches!(b.build(), Err(SigError::ConservedKeyOutOfSchema(_))));
}
```
- [ ] **Step 2:** `cargo test --lib sig` fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** `cargo test --lib` passes.
- [ ] **Step 5:** Commit with message `feat: typed signatures with polarized and self-dual port types`.

---

### Task 3: Lineage IDs and digests

**Files:** Create `src/id.rs`.

**Interfaces (produces):**
```rust
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)] pub struct AgentId(pub u128);
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)] pub struct HoleId(pub u128);
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)] pub struct Digest(pub u128);
// Debug/Display: AgentId -> "a1b2c3d4" (first 8 hex), HoleId -> "h:a1b2c3d4", Digest -> full 32 hex.
// Serialize/Deserialize all three as 32-char lowercase hex strings.
impl AgentId { pub fn from_inject(seq: u64, index: u32) -> AgentId;
               pub fn from_reduction(left: AgentId, right: AgentId, rule: &str, index: u32) -> AgentId; }
impl HoleId  { pub fn from_inject(seq: u64, index: u32) -> HoleId; pub fn for_effect(agent: AgentId) -> HoleId; }
pub struct DigestBuilder;   // wraps SipHasher24::new_with_keys(K0, K1)
impl DigestBuilder { pub fn new(domain: &str) -> Self; pub fn u8(&mut self, x: u8); pub fn u32(&mut self, x: u32); pub fn u64(&mut self, x: u64);
                     pub fn u128(&mut self, x: u128); pub fn bytes(&mut self, b: &[u8]); pub fn str(&mut self, s: &str); pub fn finish(self) -> Digest; }
pub type IdBuildHasher = BuildHasherDefault<IdHasher>;   // identity-ish hasher for already-random u128 keys
```
Keys: `K0 = 0x5375_7475_7265_2d30` ("Suture-0") and `K1 = 0x6c69_6e65_6167_6531` ("lineage1"). Every field goes through `bytes`/`str`, which are length-prefixed, and each constructor begins with a domain string (`"agent/inject"`, `"agent/reduce"`, `"hole/inject"`, `"hole/effect"`).

- [ ] **Step 1: Failing tests**
```rust
#[test] fn deterministic_and_distinct() {
    let a = AgentId::from_inject(0, 0); let b = AgentId::from_inject(0, 1);
    assert_ne!(a, b); assert_eq!(a, AgentId::from_inject(0, 0));
    let c = AgentId::from_reduction(a, b, "r", 0);
    assert_ne!(c, AgentId::from_reduction(b, a, "r", 0));
    assert_ne!(c, AgentId::from_reduction(a, b, "r2", 0));
    assert_ne!(c, AgentId::from_reduction(a, b, "r", 1));
    assert_ne!(HoleId::from_inject(0, 0).0, AgentId::from_inject(0, 0).0, "domain separation");
}
#[test] fn known_answer_pins_the_hash() {
    // Journals persist these IDs; changing the hash would break replay of existing journals.
    assert_eq!(format!("{:032x}", AgentId::from_inject(0, 0).0), KAT_INJECT_0_0);
}
#[test] fn serde_hex_roundtrip() {
    let a = AgentId::from_inject(3, 4);
    let s = serde_json::to_string(&a).unwrap();
    assert_eq!(s.len(), 34);
    assert_eq!(serde_json::from_str::<AgentId>(&s).unwrap(), a);
}
```
`KAT_INJECT_0_0` is a `const &str` set from the value printed by the first green run; record it in the test file with that comment. It is a known-answer test, so its job is to detect *later* changes.
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement, run once, and pin the KAT.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: schedule-independent lineage IDs and stable digests`.

---

### Task 4: Graph store

**Files:** Create `src/graph.rs`.

**Interfaces (produces):**
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Endpoint { Port(AgentId, u8), Hole(HoleId) }          // port 0 = principal
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum Cause { Inject { seq: u64 }, Reduce { left: AgentId, right: AgentId, rule: u32 } }
#[derive(Clone, Debug, PartialEq, Eq)] pub struct Agent { pub kind: KindId, pub payload: Value, pub ports: Vec<Endpoint>, pub cause: Cause }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum HoleOrigin { Inject { seq: u64 }, Effect { agent: AgentId } }
#[derive(Clone, Debug, PartialEq, Eq)] pub struct Hole { pub peer: Endpoint, pub label: Option<String>, pub origin: HoleOrigin }
#[derive(Clone, Debug, Default)] pub struct Graph { /* agents, holes: HashMap<_, _, IdBuildHasher>, labels: HashMap<String, HoleId> */ }
impl Graph {
    pub fn agent(&self, id: AgentId) -> Option<&Agent>; pub fn hole(&self, id: HoleId) -> Option<&Hole>;
    pub fn hole_by_label(&self, label: &str) -> Option<HoleId>;
    pub fn peer(&self, e: Endpoint) -> Option<Endpoint>;
    pub fn agent_count(&self) -> usize; pub fn hole_count(&self) -> usize;
    pub fn agents_sorted(&self) -> Vec<(AgentId, &Agent)>; pub fn holes_sorted(&self) -> Vec<(HoleId, &Hole)>;
    pub fn active_partner(&self, id: AgentId) -> Option<AgentId>;
    pub fn active_pairs(&self) -> Vec<(AgentId, AgentId)>;    // normalized (min,max), sorted
    pub fn check(&self, sig: &Signature) -> Result<(), GraphError>;   // symmetry, arity, payload schema, wire types, labels
    pub fn digest(&self) -> Digest;                            // over sorted agents+holes incl. cause/origin
    pub(crate) fn insert_agent(&mut self, id: AgentId, a: Agent);
    pub(crate) fn remove_agent(&mut self, id: AgentId) -> Option<Agent>;
    pub(crate) fn insert_hole(&mut self, id: HoleId, h: Hole);          // registers label
    pub(crate) fn remove_hole(&mut self, id: HoleId) -> Option<Hole>;   // unregisters label
    pub(crate) fn set_end(&mut self, at: Endpoint, peer: Endpoint);      // one direction
    pub(crate) fn link(&mut self, a: Endpoint, b: Endpoint);             // both directions
}
#[derive(Debug, thiserror::Error)] pub enum GraphError { Dangling(String), Asymmetric(String), SelfWire(String), Arity(String), Payload(String), WireType(String), Label(String) }
```

- [ ] **Step 1: Failing unit tests.** Build agents by hand with the crate-private API: two `S`/`Z` agents, wire Z.0 to S.1, and S.0 to a labeled hole.
```rust
#[test] fn wiring_and_active_pairs() { /* link(Port(z,0), Port(s,1)); link(Port(s,0), Hole(h)); check() ok; active_pairs() empty;
    then rewire: add agent Add, link(Port(s,0), Port(add,0)) -> active_pairs() == [(min,max)] */ }
#[test] fn check_detects_asymmetry_and_bad_types() { /* set_end one-sided -> Asymmetric; link Z.0 to Z.0 of another Z -> WireType */ }
#[test] fn digest_independent_of_insertion_order() { /* build same graph inserting in two orders; digests equal */ }
#[test] fn labels_track_holes() { /* insert_hole with label "x"; hole_by_label("x"); remove_hole -> None */ }
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement. `check()` verifies the following:
  - every agent has `ports.len() == 1 + arity`;
  - the payload is admitted by the kind's schema;
  - for each port, `peer(peer(e)) == e` and `peer(e) != e`;
  - an agent–agent wire needs `compatible` types (hole ends are unconstrained);
  - the label map is a bijection with labeled holes.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: port graph store with holes, labels, well-formedness check, digest`.

---

### Task 5: Nets, path resolution, static checks

**Files:** Create `src/net.rs` and `src/check.rs`.

**Interfaces (produces):**
```rust
// net.rs
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)] pub struct Var(pub u32);
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum Slot { Var(Var), Emit }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] pub struct NetAgent { pub kind: KindId, pub payload: Value, pub ports: Vec<Slot> }
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)] pub struct NetData { pub vars: u32, pub agents: Vec<NetAgent>, pub links: Vec<(Var, Var)> }
impl NetData { pub fn digest(&self) -> Digest; }
pub struct NetBuilder<'s> { /* sig, data, first_error: Option<NetError> */ }
impl<'s> NetBuilder<'s> {
    pub fn new(sig: &'s Signature) -> Self;
    pub(crate) fn with_vars(sig: &'s Signature, n: u32) -> Self;      // pre-allocated interface vars
    pub fn var(&mut self) -> Var;
    pub fn agent(&mut self, kind: KindId, payload: impl Into<Value>, at: Var, aux: &[Var]);   // arity checked here
    pub fn effect(&mut self, kind: KindId, payload: impl Into<Value>, aux: &[Var]);           // principal = Emit
    pub fn link(&mut self, a: Var, b: Var);
    pub fn finish(self) -> Result<NetData, NetError>;
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum HoleRef { Id(HoleId), Label(String) }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum Bind { Hole(HoleRef), New(Option<String>) }
#[derive(Clone, Debug)] pub struct Injection { pub net: NetData, pub binds: Vec<(Var, Bind)>, pub key: Option<String>, pub mint: bool }
impl Injection { pub fn new(net: NetData) -> Self; pub fn bind(self, v: Var, b: Bind) -> Self; pub fn at_label(self, v: Var, l: &str) -> Self;
                 pub fn at_hole(self, v: Var, h: HoleId) -> Self; pub fn new_hole(self, v: Var, label: Option<&str>) -> Self;
                 pub fn key(self, k: impl Into<String>) -> Self; pub fn mint(self) -> Self; }
// resolution (crate-visible)
pub(crate) enum ExtKind { Terminal, Glue(usize) }
pub(crate) enum End { Agent(usize, usize), Ext(usize) }       // (net agent index, port) | external terminal index
pub(crate) struct Resolved { pub conns: Vec<(End, End)>, pub vicious: usize }
pub(crate) fn resolve(net: &NetData, ext_vars: &[Var], ext: &[ExtKind]) -> Result<Resolved, NetError>;
// check.rs
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NetError { Arity{kind:String,expected:usize,got:usize}, UnknownVar(u32), VarUses{var:u32,uses:usize}, ClosedLoop(u32),
    WireType{a:String,b:String}, InterfaceType{port:usize,expected:String,got:String}, EmitOnNonEffect(String), EffectNeedsEmit(String),
    EffectNotAuthorized(String), EffectInInject(String), Payload{kind:String,value:String}, Conservation{created:String,consumed:String},
    MintRequired(String), TooLarge{agents:usize,limit:usize}, Value(String) }
pub(crate) struct PatchSpec<'a> { pub iface: &'a [PortType], pub consumed: [(KindId, &'a Value); 2], pub may_emit: &'a [KindId], pub max_agents: usize }
pub(crate) fn check_patch(sig: &Signature, net: &NetData, spec: &PatchSpec) -> Result<Resolved, NetError>;   // arity, payload, effects, linearity, static types, conservation, size
pub(crate) fn check_inject_static(sig: &Signature, net: &NetData, mint: bool, max_agents: usize) -> Result<(), NetError>;
pub(crate) fn conserved_multiset<'a>(sig: &Signature, items: impl Iterator<Item = (KindId, &'a Value)>) -> Vec<(String, Value)>;  // sorted
```

**The resolution algorithm (the core of splicing).** Each var has exactly two *uses*. A use is an agent slot `Use::Port(i, p)`, an end of a link `Use::Link(l)`, or the implicit external use `Use::Ext(k)`, where `ext_vars[k] == var`. Terminals are `Port` uses and `Ext(k)` uses where `ext[k]` is `Terminal`. Walk from every terminal:
```rust
// from terminal t living in var v: other = the other use of v (not the one we came from)
//   Port/Ext(Terminal) -> emit conn (t, other); done
//   Link(l)            -> let w = other end var of l; continue from use Link(l) inside w
//   Ext(k) with Glue(j)-> continue from use Ext(j) inside ext_vars[j]
// mark visited (var, use) pairs; error ClosedLoop if a link(v,v) or a pure link cycle without terminals exists;
// any unvisited cycle that passes through a Glue edge is a vicious circle: count and drop.
```
Patches call it with `ext_vars = [Var(0)..Var(n_iface)]`. The static check passes all-`Terminal` and types `Ext(i)` as `dual(iface[i])`: the endpoint bound to the interface var for aux `i` must have type `iface[i]`, and `InterfaceType` is reported otherwise. Splicing (Task 7) passes `Glue(j)` wherever aux `i` is wired to aux `j` of the same pair.

- [ ] **Step 1: Failing tests.** In `src/check.rs` `#[cfg(test)]`, using a test signature with Peano kinds plus a conserved `Lease` kind (payload `(Int)`) and an effect `Ask`:
```rust
#[test] fn linearity() { /* var used once -> VarUses{uses:1}; used 3x -> VarUses{uses:3} */ }
#[test] fn static_wire_types() { /* S at v, S at v (Nat+ ~ Nat+) -> WireType */ }
#[test] fn interface_types_preserved() { /* patch for S(x) >< Add(y,r): bind new S principal at iface var y (Add.y is Nat-) -> InterfaceType */ }
#[test] fn add_s_patch_is_valid() { /* the real add_s RHS passes and resolve yields 4 conns */ }
#[test] fn pass_through_link() { /* Z >< Add(y,r): link(y,r) -> conns == [(Ext(0),Ext(1))] (indices for Add aux) */ }
#[test] fn effects_need_emit_and_authority() { /* agent(Ask,..) via agent() -> EffectNeedsEmit; effect(S,..) -> EmitOnNonEffect; effect(Ask) with may_emit=[] -> EffectNotAuthorized */ }
#[test] fn conservation() { /* consumed Lease(1)+X; created Lease(1),Lease(1) -> Conservation; created Lease(2) -> Conservation; created Lease(1) -> ok */ }
#[test] fn glue_vicious_circle() { /* two iface vars linked, ext=[Glue(1),Glue(0)] -> conns empty, vicious==1 */ }
#[test] fn closed_link_loop_is_error() { /* fresh a,b; link(a,b); link(b,a) -> ClosedLoop */ }
#[test] fn too_large() { /* max_agents=1, two agents -> TooLarge */ }
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement `net.rs`, `resolve`, and `check.rs`.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: net builder, splice path resolution, static patch legality`.

---

### Task 6: Rules, rule table, coverage, policies

**Files:** Create `src/rule.rs` and `src/policy.rs`.

**Interfaces (produces):**
```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum RuleMode { Deterministic, Recorded }
pub type RuleBody = dyn Fn(&mut RuleCx<'_>) -> Result<(), RuleError> + Send + Sync;
pub struct Rule { pub name: String, pub version: u32, pub left: KindId, pub right: KindId, pub mode: RuleMode, pub may_emit: Vec<KindId>, pub body: Arc<RuleBody> }
impl Rule { pub fn version(&mut self, v: u32) -> &mut Self; pub fn recorded(&mut self) -> &mut Self; pub fn emits(&mut self, ks: &[KindId]) -> &mut Self; }
#[derive(Debug, thiserror::Error)] pub enum RuleError { #[error("rejected: {0}")] Reject(String), #[error(transparent)] Net(#[from] NetError), #[error(transparent)] Value(#[from] ValueError) }
pub struct RuleCx<'a> { /* sig, left/right kind+payload (cloned), builder, n_left, n_right */ }
impl<'a> RuleCx<'a> {
    pub fn left(&self) -> &Value; pub fn right(&self) -> &Value; pub fn left_kind(&self) -> KindId; pub fn right_kind(&self) -> KindId;
    pub fn sig(&self) -> &Signature;
    pub fn left_aux<const N: usize>(&mut self) -> [Var; N]; pub fn right_aux<const N: usize>(&mut self) -> [Var; N];
    pub fn var(&mut self) -> Var;
    pub fn agent(&mut self, kind: KindId, payload: impl Into<Value>, at: Var, aux: &[Var]);
    pub fn effect(&mut self, kind: KindId, payload: impl Into<Value>, aux: &[Var]);
    pub fn link(&mut self, a: Var, b: Var);
    pub fn reject(&self, why: impl Into<String>) -> RuleError;
}
#[derive(Default)] pub struct RuleTable { /* rules: Vec<Rule>, index: HashMap<(KindId,KindId),usize>, forbidden: BTreeSet<(KindId,KindId)>, errors: Vec<String> */ }
impl RuleTable {
    pub fn new() -> Self;
    pub fn def(&mut self, name: &str, left: KindId, right: KindId, body: impl Fn(&mut RuleCx<'_>) -> Result<(), RuleError> + Send + Sync + 'static) -> &mut Rule;
    pub fn forbid(&mut self, a: KindId, b: KindId);
    pub fn lookup(&self, a: KindId, b: KindId) -> Option<(usize, &Rule, bool)>;   // (rule index, rule, swapped)
    pub fn get(&self, idx: usize) -> &Rule; pub fn by_name(&self, name: &str) -> Option<(usize, &Rule)>;
    pub fn coverage(&self, sig: &Signature) -> Coverage;
    pub fn errors(&self) -> &[String];      // duplicate pairs/names detected by def()
}
pub struct Coverage { pub missing: Vec<(String, String)>, pub forbidden: Vec<(String, String)>, pub never_fires: Vec<String> }
// policy.rs
pub trait Policy: Send + Sync { fn name(&self) -> &str; fn check(&self, v: &PatchView<'_>) -> Result<(), String>; }
pub struct PatchView<'a> { pub sig: &'a Signature, pub rule: &'a str, pub left: (KindId, &'a Value), pub right: (KindId, &'a Value), pub created: &'a [NetAgent] }
impl PatchView<'_> { pub fn created_of(&self, k: KindId) -> impl Iterator<Item = &Value>; pub fn consumed_of(&self, k: KindId) -> impl Iterator<Item = &Value>; }
```
Interface var numbering inside `RuleCx`: the aux ports of the left kind come first, as `Var(0..n_left)`, then the aux ports of the right kind as `Var(n_left..)`, then fresh vars. A pair of the *same* kind is oriented with `left = min(id)`. An arity mismatch in `left_aux::<N>` records `NetError::Arity` in the builder and returns placeholder vars, so the patch fails validation and becomes a fault.

- [ ] **Step 1: Failing tests** (`src/rule.rs` `#[cfg(test)]`, using the Peano signature from Task 5 tests moved into a `#[cfg(test)] pub(crate) mod fixtures` in `src/check.rs`)
```rust
#[test] fn lookup_orients() { /* def("add_s", S, ADD); lookup(ADD, S) -> swapped=true; lookup(S, ADD) -> swapped=false */ }
#[test] fn duplicate_pair_is_recorded() { /* def twice for (S,ADD) and (ADD,S) -> errors().len()==1 */ }
#[test] fn coverage_reports_missing_and_never_fires() {
    /* Peano with era_z omitted -> missing contains ("Z","Era"); a rule (Z,S) whose principals are both Nat+ -> never_fires contains its name;
       forbid(Z, Era) -> moves it to forbidden */ }
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: rule table with orientation, coverage analysis, policy hook`.

---

### Task 7: Machine core (inject, reduce, schedulers, faults, effects)

**Files:** Create `src/machine.rs`, `tests/common/mod.rs`, `tests/kernel.rs`. Modify `src/lib.rs` (re-exports).

**Interfaces (produces):**
```rust
#[derive(Clone, Debug)] pub struct Limits { pub max_patch_agents: usize }       // default 256
#[derive(Clone)] pub struct Program { pub sig: Arc<Signature>, pub rules: Arc<RuleTable>, pub policies: Arc<Vec<Box<dyn Policy>>>, pub limits: Limits }
impl Program { pub fn new(sig: Signature, rules: RuleTable) -> Result<Program, ProgramError>; pub fn with_policy(self, p: impl Policy + 'static) -> Program;
               pub fn with_limits(self, l: Limits) -> Program; pub fn name(&self) -> &str; pub fn version(&self) -> u32; }
#[derive(Debug, thiserror::Error)] pub enum ProgramError { #[error("rule table: {0:?}")] Rules(Vec<String>) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum SchedulerKind { Fifo, Lifo, Random(u64) }
#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum RunOutcome { Quiescent { reductions: u64 }, Budget { reductions: u64 } }
#[derive(Clone, Debug, Default)] pub struct Rounds { pub rounds: u64, pub reductions: u64, pub max_width: usize }
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] pub struct Fault { pub left: AgentId, pub right: AgentId, pub rule: Option<String>, pub error: String }
#[derive(Clone, Debug, PartialEq, Eq)] pub struct InjectReceipt { pub seq: u64, pub new_holes: Vec<(Var, HoleId)>, pub duplicate: bool }
#[derive(Debug, thiserror::Error)] pub enum InjectError { UnknownHole(String), HoleBoundTwice(String), LabelInUse(String), Net(#[from] NetError), Type{a:String,b:String}, Journal(String) }
#[derive(Clone, Debug, PartialEq)] pub struct EffectView { pub agent: AgentId, pub hole: HoleId, pub kind: KindId, pub kind_name: String, pub payload: Value }
#[derive(Clone, Debug, Default)] pub struct Stats { pub reductions: u64, pub injects: u64, pub faults: u64, pub effects: u64, pub vicious: u64,
                                                    pub created: u64, pub max_agents: usize, pub per_rule: BTreeMap<String, u64> }
pub struct Machine { .. }
impl Machine {
    pub fn new(prog: Program) -> Machine;                                                  // MemJournal
    pub fn with_journal(prog: Program, j: Box<dyn Journal>) -> Result<Machine, JournalError>;   // writes Header (Task 10)
    pub fn set_scheduler(&mut self, s: SchedulerKind);
    pub fn inject(&mut self, inj: Injection) -> Result<InjectReceipt, InjectError>;
    pub fn step(&mut self) -> bool;                                   // one reduction; false if none ready
    pub fn run(&mut self, budget: Option<u64>) -> RunOutcome;
    pub fn run_rounds(&mut self, max_rounds: Option<u64>) -> Rounds;  // each round reduces every pair ready at its start
    pub fn graph(&self) -> &Graph; pub fn program(&self) -> &Program; pub fn sig(&self) -> &Signature;
    pub fn outbox(&self) -> Vec<EffectView>;                          // sorted by hole id
    pub fn hole(&self, label: &str) -> Option<HoleId>;
    pub fn hole_peer(&self, h: HoleId) -> Option<(AgentId, usize, &Agent)>;   // agent attached to the hole + port index
    pub fn faults(&self) -> Vec<Fault>; pub fn stats(&self) -> &Stats; pub fn digest(&self) -> Digest;
    pub fn ready_count(&self) -> usize; pub fn is_quiescent(&self) -> bool;
    pub fn check_invariants(&self) -> Result<(), GraphError>;
    pub fn describe(&self, a: AgentId) -> String;                      // "Kind#id8(payload)"
    pub fn census(&self, class: &str) -> Vec<Value>;                   // sorted conserved keys of that class present in the graph
}
```
Reduce algorithm: pop a pair (FIFO, LIFO, or random), look up the rule (no rule is a fault), orient it, run the body under `catch_unwind` (a panic is a fault), then `check_patch`. Next build `PatchView` and run the policies (a violation is a fault whose text names the policy). Compute the ext kinds from the current graph and call `resolve` with glue. Dynamic-check every final connection whose ends are both agent ports (a failure is a fault). Then mutate:
- remove the pair;
- insert the new agents with `AgentId::from_reduction`;
- create effect holes with `HoleId::for_effect` and connect each effect's principal;
- link the connections;
- enqueue new principal–principal connections.

Finally, update the stats and append the `Reduce` record (the journal hook is in Task 10). A fault inserts into `faults`, never re-enqueues the pair, and appends a `Fault` record.

Inject algorithm:
1. Check the idempotency key; a duplicate returns `duplicate = true` and changes nothing.
2. Resolve the binds to holes. Check: no hole bound twice, labels are free or transferred within this inject, and no duplicate new labels.
3. Run `check_inject_static`.
4. Compute ext kinds. A bind to hole H whose peer is a hole also bound in this inject becomes `Glue`. A `New` bind is a terminal at the new hole.
5. `resolve`, then dynamic-type-check the connections.
6. Mutate: remove the consumed holes, insert the new holes, insert agents with `AgentId::from_inject(seq, i)`, link, and enqueue the new active pairs.
7. Append the `Inject` record with the binds resolved to hole IDs.
8. `seq += 1`.

Every error is returned before any mutation.

`tests/common/mod.rs` fixtures:
```rust
pub struct Mini { pub prog: Program, pub nat: TypeId, pub z: KindId, pub s: KindId, pub add: KindId, pub bogus: KindId,
                  pub start: KindId, pub go: KindId, pub ask: KindId, pub reply: KindId, pub result: KindId, pub lease: KindId }
pub fn mini() -> Mini;   // Peano add (add_z, add_s) + echo program (start_go emits Ask(7); reply_ask -> Result(v+n)) + Bogus (Nat+, no rules)
                         // + conserved Lease(Int) kind (Nat+) used only for mint tests; rules boom (Bogus><Add... no) -- see tests
pub fn num(b: &mut NetBuilder, m: &Mini, n: u64) -> Var;      // S^n(Z), returns var at top principal
pub fn read_nat(m: &Machine, mini: &Mini, h: HoleId) -> Option<u64>;
```
Echo program: types are `Ping` (polarized), `Ans`, and `Out`.
- Kinds: `Start: Ping+`, `Go: Ping- (out: Out+)`, `Ask: Ans- (out: Out+)` with payload `Int` and `effect`, `Reply: Ans+` with payload `Int`, and `Result: Out+` with payload `Int`.
- Rules:
  - `start_go` emits `Ask(7)` with `out`;
  - `reply_ask` creates `Result(v + n)` at `out`.
- A `Boom: Ping+` kind pairs with `Go` in a rule that panics, and a `Nope: Ping+` kind pairs with `Go` in a rule that returns `Err(cx.reject("no"))`.

- [ ] **Step 1: Failing tests** in `tests/kernel.rs`:
```rust
mod common; use common::*; use suture::*;

#[test] fn adds_two_and_one() {
    let m0 = mini(); let mut m = Machine::new(m0.prog.clone());
    let mut b = NetBuilder::new(m.sig());
    let x = num(&mut b, &m0, 2); let y = num(&mut b, &m0, 1); let r = b.var();
    b.agent(m0.add, (), x, &[y, r]);
    let rc = m.inject(Injection::new(b.finish().unwrap()).new_hole(r, Some("out"))).unwrap();
    assert_eq!(rc.seq, 0);
    assert!(matches!(m.run(None), RunOutcome::Quiescent { .. }));
    assert_eq!(read_nat(&m, &m0, m.hole("out").unwrap()), Some(3));
    m.check_invariants().unwrap();
    assert!(m.faults().is_empty());
}
#[test] fn inject_errors_leave_graph_unchanged() {
    let m0 = mini(); let mut m = Machine::new(m0.prog.clone());
    let mut b = NetBuilder::new(m.sig()); let r = b.var(); b.agent(m0.z, (), r, &[]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(r, Some("z"))).unwrap();
    let before = m.digest();
    // unknown label
    let mut b = NetBuilder::new(m.sig()); let v = b.var(); b.agent(m0.add, (), v, &[]);  // arity error recorded in builder
    assert!(b.finish().is_err());
    let mut b = NetBuilder::new(m.sig()); let (v, y, r) = (b.var(), b.var(), b.var()); b.agent(m0.add, (), v, &[y, r]);
    let net = b.finish().unwrap();
    assert!(matches!(m.inject(Injection::new(net.clone()).at_label(v, "nope").new_hole(y, None).new_hole(r, None)), Err(InjectError::UnknownHole(_))));
    // hole bound twice
    assert!(matches!(m.inject(Injection::new(net.clone()).at_label(v, "z").at_label(y, "z").new_hole(r, None)), Err(InjectError::HoleBoundTwice(_))));
    // label in use
    assert!(matches!(m.inject(Injection::new(net.clone()).new_hole(v, Some("z")).new_hole(y, None).new_hole(r, None)), Err(InjectError::LabelInUse(_))));
    // type mismatch at hole: Z (Nat+) principal onto hole "z" whose peer is Z.principal (Nat+)
    let mut b = NetBuilder::new(m.sig()); let w = b.var(); b.agent(m0.z, (), w, &[]);
    assert!(matches!(m.inject(Injection::new(b.finish().unwrap()).at_label(w, "z")), Err(InjectError::Type { .. })));
    // payload schema mismatch
    let mut b = NetBuilder::new(m.sig()); let w = b.var(); b.agent(m0.result, "not an int", w, &[]);
    assert!(m.inject(Injection::new(b.finish().unwrap()).new_hole(w, None)).is_err());
    // minting a conserved kind without permission
    let mut b = NetBuilder::new(m.sig()); let w = b.var(); b.agent(m0.lease, tup![1], w, &[]);
    assert!(matches!(m.inject(Injection::new(b.finish().unwrap()).new_hole(w, None)), Err(InjectError::Net(NetError::MintRequired(_)))));
    assert_eq!(m.digest(), before);
    assert_eq!(m.stats().injects, 1);
}
#[test] fn label_transfer_within_one_inject() { /* hole "tail" consumed and re-created in the same inject -> ok; hole("tail") is the new id */ }
#[test] fn effects_roundtrip() {
    // Start >< Go(out) emits Ask(7); outbox shows it; host injects Reply(5) at its hole -> Result(12) at "out"
}
#[test] fn missing_rule_is_local_fault() { /* Bogus >< Add stuck, while an independent 1+1 elsewhere completes */ }
#[test] fn rule_panic_becomes_fault() { /* Boom >< Go: fault.error contains "panic"; machine still usable */ }
#[test] fn rule_reject_becomes_fault() { /* Nope >< Go: fault.error contains "no" */ }
#[test] fn idempotency_key_dedupes() { /* same key twice -> second receipt.duplicate == true, digest unchanged */ }
#[test] fn schedulers_agree_on_digest() {
    // inject 20 independent (a+b) sums plus a nested sum; run under Fifo, Lifo, Random(1..5); digests and reduction counts equal
}
#[test] fn budget_stops_and_resumes() { /* run(Some(1)) -> Budget{reductions:1}; run(None) -> Quiescent; final digest == straight run */ }
#[test] fn rounds_report_width() { /* 20 independent sums: first round width == 20; digest equals Fifo */ }
#[test] fn policy_violation_is_fault() { /* program.with_policy(NoResultAbove(10)); Reply(50) -> fault names policy; no Result created */ }
#[test] fn conserved_census() { /* mint Lease(3) via .mint(); census("lease") == [Int(3)] */ }
```
- [ ] **Step 2:** `cargo test --test kernel` fails.
- [ ] **Step 3:** Implement `machine.rs`. The journal calls go through `self.journal: Box<dyn Journal>` using `MemJournal` stubs defined now in `src/journal.rs`, with just enough code (`Record` enum, `Journal` trait, `MemJournal::append/commit`) to compile; Task 10 completes it.
- [ ] **Step 4:** `cargo test` passes.
- [ ] **Step 5:** Commit with message `feat: machine with validated INTERACT, INJECT, effects, faults, schedulers`.

---

### Task 8: Peano arithmetic (generality demo #1)

**Files:** Create `src/nets/mod.rs`, `src/nets/arith.rs`, `tests/arith.rs`, `examples/arith.rs`.

**Interfaces (produces):**
```rust
pub struct ArithKinds { pub nat: TypeId, pub z: KindId, pub s: KindId, pub add: KindId, pub mul: KindId, pub dup: KindId, pub era: KindId }
pub fn program() -> (Program, ArithKinds);
#[derive(Clone, Debug)] pub enum Expr { Num(u64), Add(Box<Expr>, Box<Expr>), Mul(Box<Expr>, Box<Expr>) }
impl Expr { pub fn eval(&self) -> u64; pub fn random(rng: &mut Rng, depth: u32, max_leaf: u64) -> Expr; }
pub fn build(b: &mut NetBuilder, k: &ArithKinds, e: &Expr) -> Var;
pub fn inject_expr(m: &mut Machine, k: &ArithKinds, e: &Expr, label: &str) -> Result<HoleId, InjectError>;
pub fn readback(m: &Machine, k: &ArithKinds, h: HoleId) -> Option<u64>;
```
Rules (types: `Z: Nat+`, `S: Nat+ (pred: Nat-)`, `Add/Mul: Nat- (y: Nat-, r: Nat+)`, `Dup: Nat- (a: Nat+, b: Nat+)`, `Era: Nat-`):
```text
add_z:  Z    >< Add(y,r)  =>  y ~ r
add_s:  S(x) >< Add(y,r)  =>  Add(y, r1) @ x ;  S(r1) @ r
mul_z:  Z    >< Mul(y,r)  =>  Era @ y ;  Z @ r
mul_s:  S(x) >< Mul(y,r)  =>  Dup(y1,y2) @ y ;  Mul(y1, r1) @ x ;  Add(y2, r) @ r1
dup_z:  Z    >< Dup(a,b)  =>  Z @ a ;  Z @ b
dup_s:  S(x) >< Dup(a,b)  =>  S(a1) @ a ;  S(b1) @ b ;  Dup(a1,b1) @ x
era_z:  Z    >< Era       =>  (nothing)
era_s:  S(x) >< Era       =>  Era @ x
```
- [ ] **Step 1: Failing tests** (`tests/arith.rs`)
```rust
#[test] fn coverage_is_complete() { let (p, _) = arith::program(); let c = p.rules.coverage(&p.sig); assert!(c.missing.is_empty() && c.never_fires.is_empty()); }
#[test] fn small_cases() { for (e, v) in [(mul(num(3), num(4)), 12), (add(num(0), num(0)), 0), (mul(num(0), num(5)), 0), (mul(add(num(2), num(1)), num(3)), 9)] { assert_eq!(eval_on_machine(&e, SchedulerKind::Fifo).0, v); } }
#[test] fn random_expressions_match_rust_under_all_schedulers() {
    for seed in 0..200 { let mut rng = Rng::new(seed); let e = Expr::random(&mut rng, 3, 4);
        let (v_fifo, d_fifo, n_fifo) = eval_on_machine(&e, SchedulerKind::Fifo);
        assert_eq!(v_fifo, e.eval(), "seed {seed}");
        for s in [SchedulerKind::Lifo, SchedulerKind::Random(seed)] { let (v, d, n) = eval_on_machine(&e, s); assert_eq!((v, d, n), (v_fifo, d_fifo, n_fifo), "seed {seed} {s:?}"); } } }
#[test] fn boundary_determinism_inject_interleaving() {
    // Two machines get the same three injects (three expressions). Machine A runs to quiescence after each inject;
    // machine B injects all three then runs. Digests must be equal.
}
#[test] fn graph_is_clean_after_normalization() { /* only the S^n Z chain remains attached to the result hole: agent_count == v + 1 */ }
```
`eval_on_machine(e, sched) -> (u64, Digest, u64 reductions)` is a local helper in the test file.
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement. The example `examples/arith.rs` prints `(3+2)*4` together with stats and rounds.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: Peano arithmetic on the kernel (typed sharing via Dup/Era)`.

---

### Task 9: Symmetric interaction combinators (generality demo #2)

**Files:** Create `src/nets/combinators.rs` and `tests/combinators.rs`.

**Interfaces (produces):**
```rust
pub struct CombKinds { pub c: TypeId, pub con: KindId, pub dup: KindId, pub era: KindId }
pub fn program() -> (Program, CombKinds);
pub fn random_net(sig: &Signature, k: &CombKinds, rng: &mut Rng, agents: usize, free: usize) -> Injection;  // random perfect matching of ports; `free` ports bound to New holes "f0".."fK"
pub fn tree(b: &mut NetBuilder, k: &CombKinds, rng: &mut Rng, depth: u32) -> Var;   // random CON tree with ERA leaves
pub fn readback_tree(m: &Machine, k: &CombKinds, h: HoleId) -> Option<String>;      // "(e (e e))"-style shape; None if not a tree
```
Rules (`C` is self-dual):
```text
con_con: γ(a1,a2) >< γ(b1,b2) => a1~b1, a2~b2      dup_dup: δ(a1,a2) >< δ(b1,b2) => a1~b1, a2~b2
era_era: ε >< ε => ∅                              era_con / era_dup: ε >< α(a1,a2) => ε@a1, ε@a2
con_dup: γ(a1,a2) >< δ(b1,b2) => δ(p,q)@a1, δ(r,s)@a2, γ(p,r)@b1, γ(q,s)@b2
```
- [ ] **Step 1: Failing tests**
```rust
#[test] fn coverage_is_complete() { /* missing empty */ }
#[test] fn strong_confluence_on_random_nets() {
    let (p, k) = combinators::program(); let mut checked = 0;
    for seed in 0..400 { let mut rng = Rng::new(seed);
        let inj = combinators::random_net(&p.sig, &k, &mut rng, 2 + (seed % 24) as usize, (seed % 5) as usize);
        let mut base = Machine::new(p.clone()); base.inject(inj.clone()).unwrap();
        let RunOutcome::Quiescent { reductions } = base.run(Some(20_000)) else { continue };   // skip non-normalizing
        checked += 1;
        for s in [SchedulerKind::Lifo, SchedulerKind::Random(seed ^ 0xABCD)] { let mut m = Machine::new(p.clone()); m.set_scheduler(s); m.inject(inj.clone()).unwrap();
            assert_eq!(m.run(Some(20_000)), RunOutcome::Quiescent { reductions }, "seed {seed}"); assert_eq!(m.digest(), base.digest(), "seed {seed}"); }
        let mut r = Machine::new(p.clone()); r.inject(inj).unwrap(); r.run_rounds(None); assert_eq!(r.digest(), base.digest());
        base.check_invariants().unwrap(); }
    assert!(checked > 300, "only {checked} nets normalized");
}
#[test] fn dup_copies_a_tree() { /* δ(h1,h2) @ root of random γ/ε tree; after run, readback(h1) == readback(h2) == original shape */ }
#[test] fn annihilation_of_self_wired_pair_leaves_nothing() { /* γ(a1,a2)><γ(b1,b2) with a1~b1, a2~b2: agent_count()==0, stats().vicious==2 */ }
#[test] fn non_terminating_net_is_resumable() {
    // A self-reproducing net: δ and γ, each with its two aux ports wired to its own principal... use the known loop:
    // find via search over random nets one that hits Budget at 5_000 and also at 10_000 (seeded search, assert found);
    // run(Some(100)) twice == run(Some(200)) once (same digest), and check_invariants() holds.
}
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: symmetric interaction combinators; strong-confluence property test`.

---

### Task 10: Journal, replay, recovery

**Files:** Complete `src/journal.rs`. Modify `src/machine.rs` (add `recover`, `commit`, `into_journal`, the replay paths). Create `tests/journal.rs`.

**Interfaces (produces):**
```rust
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)] #[serde(tag = "t")]
pub enum Record {
    Header { program: String, version: u32, format: u32 },
    Inject { seq: u64, key: Option<String>, mint: bool, net: NetData, binds: Vec<(Var, RecBind)> },
    Reduce { left: AgentId, right: AgentId, rule: String, version: u32, digest: Digest, patch: Option<NetData> },
    Fault  { left: AgentId, right: AgentId, rule: Option<String>, error: String },
    Commit,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)] pub enum RecBind { Hole(HoleId), New { label: Option<String> } }
pub trait Journal: Send { fn append(&mut self, r: Record) -> Result<(), JournalError>; fn commit(&mut self) -> Result<(), JournalError>;
                          fn durable(&self) -> Result<Vec<Record>, JournalError>; fn crash(&mut self); fn len_durable(&self) -> usize; }
pub struct MemJournal { .. }  impl MemJournal { pub fn new() -> Self; }
pub struct FileJournal { .. } impl FileJournal { pub fn open(path: &Path) -> Result<FileJournal, JournalError>; }   // truncates torn/uncommitted tail
#[derive(Debug, thiserror::Error)] pub enum JournalError { Io(#[from] std::io::Error), Format(String) }
#[derive(Debug, thiserror::Error)] pub enum RecoveryError { Journal(#[from] JournalError), Header(String), UnknownRule(String),
    RuleVersion { rule: String, journal: u32, program: u32 }, Divergence { rule: String, step: usize }, MissingPair { step: usize }, Inject(String) }
impl Machine {
    pub fn recover(prog: Program, j: Box<dyn Journal>) -> Result<Machine, RecoveryError>;
    pub fn commit(&mut self) -> Result<(), JournalError>;
    pub fn into_journal(self) -> Box<dyn Journal>;
}
```
Recovery steps:
1. Read `durable()` (MemJournal drops records after the last `Commit`).
2. Check the header name and version.
3. Apply the records in order through the same internal functions with journaling disabled:
   - `Inject` re-applies with the recorded hole IDs; the receipt's seq must equal the record's.
   - `Reduce` requires the pair to be active, then either applies the recorded patch, revalidated (`Recorded`), or re-runs the rule. A version mismatch is `RuleVersion`; a digest mismatch is `Divergence`.
   - `Fault` re-marks the pair.
4. Rebuild the ready queue as the sorted active pairs minus the faulted ones.
5. Rebuild the idempotency keys.

- [ ] **Step 1: Failing tests** (`tests/journal.rs`, using `mini()` and `arith`)
```rust
#[test] fn replay_equals_original() { /* inject, run, commit; recover from into_journal(); digest, outbox, stats().injects equal */ }
#[test] fn replay_partial_run() { /* run(Some(3)); commit; recover; digest equal; run(None) on both -> equal */ }
#[test] fn crash_drops_uncommitted_and_rerun_converges() {
    // commit after inject #1; inject #2 + run uncommitted; crash(); recover -> digest == state after commit;
    // re-inject #2 with same key, run -> digest == no-crash run
}
#[test] fn file_journal_roundtrip_and_torn_tail() {
    // FileJournal in scratch dir (std::env::temp_dir()/suture-test-<pid>-<n>); commit; append garbage partial line manually;
    // FileJournal::open -> recover ok; continue injecting and committing; reopen -> recover equals
}
#[test] fn replay_detects_rule_drift() { /* journal from arith; recover with a program whose add_s body is altered (same version) -> Err(Divergence) */ }
#[test] fn replay_refuses_version_change() { /* same but bumped version -> Err(RuleVersion) */ }
#[test] fn recorded_rule_replays_without_rerunning() {
    // program with a Recorded rule whose body depends on a global AtomicU64 counter; run, commit; change the counter; recover -> digest equal
}
#[test] fn idempotency_survives_recovery() { /* key used before crash (committed); after recover the same key -> duplicate */ }
#[test] fn header_mismatch_is_refused() { /* recover with a program of a different name -> Err(Header) */ }
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement. `FileJournal` writes JSON lines; `commit` writes the buffered lines plus `{"t":"Commit"}`, then `flush()` and `sync_data()`. `open` scans lines, keeps the bytes through the last complete `Commit` line, and calls `set_len` to truncate the rest.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: journal with commit/crash/replay, digest-verified determinism, recorded rules`.

---

### Task 11: Explain and DOT

**Files:** Create `src/explain.rs`, `src/dot.rs`, `tests/explain.rs`.

**Interfaces (produces):**
```rust
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Terminal { Hole { hole: HoleId, label: Option<String> }, Effect { agent: AgentId, hole: HoleId }, Ready(AgentId, AgentId), Stuck(AgentId, AgentId), Deadlock(Vec<AgentId>) }
#[derive(Clone, Debug)] pub struct WaitGroup { pub terminal: Terminal, pub agents: Vec<AgentId> }
#[derive(Clone, Debug)] pub struct Explanation { pub groups: Vec<WaitGroup> }
impl Explanation { pub fn deadlocks(&self) -> Vec<&WaitGroup>; pub fn stuck(&self) -> Vec<&WaitGroup>; pub fn external(&self) -> Vec<&WaitGroup>; }
impl Machine { pub fn explain(&self) -> Explanation; pub fn render_explanation(&self) -> String; pub fn to_dot(&self) -> String; }
```
Algorithm: `next(a)` is the peer of `a`'s principal.
- `Hole` ends at `Hole` or `Effect`. It is `Effect` when `a`'s kind is an effect.
- `Port(b, 0)` ends at `Ready` or `Stuck`. It is `Stuck` when the pair is faulted.
- `Port(b, k > 0)` follows on to `b`.

Use iterative path following with a color array to memoize terminals and detect cycles. The members of a cycle form a `Deadlock`, sorted with the minimum ID first, and agents that lead into a cycle inherit it.

- [ ] **Step 1: Failing tests**
```rust
#[test] fn external_wait() { /* Add with principal at hole "x" and aux at holes: one group Hole{label:Some("x")} containing Add */ }
#[test] fn pending_effect() { /* echo: after Start><Go, group Effect{..} contains Ask */ }
#[test] fn stuck_pair() { /* Bogus><Add: group Stuck(..) */ }
#[test] fn deadlock_cycle() { /* S1.0~S2.1, S2.0~S1.1 injected closed: deadlocks().len()==1 with 2 agents */ }
#[test] fn chains_group_by_terminal() { /* 3+0 result chain S S S Z waiting on hole "out": one Hole group with 4 agents */ }
#[test] fn render_and_dot_mention_kinds() { /* render contains "waiting on hole 'out'", dot contains "digraph" and "S" */ }
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: structural quiescence explanation and DOT export`.

---

### Task 12: Experiment A program (lease pool) and reference model

**Files:** Create `src/nets/lease.rs` (program, policies, reference model) and `tests/lease.rs` (program-level tests).

**Interfaces (produces):**
```rust
pub struct LeaseKinds { pool, mcons, dispatch, demand, returned, free, fnil, qcons, qnil, seek, enq, matcher, granted, await_lease, run_job, job_done, job_failed, send, route, done, failed, status_ack: KindId }
pub struct LeaseConfig { pub max_attempts: i64, pub buggy_ignore_region: bool }
pub fn program(cfg: &LeaseConfig) -> (Program, LeaseKinds);   // with ResidencyPolicy + TenantPolicy installed
pub const POOL: &str = "pool";
pub struct LeaseSpec { pub id: i64, pub region: String }
pub struct Request { pub req: String, pub tenant: String, pub regions: Vec<String> }
pub fn init_pool(m: &mut Machine, k: &LeaseKinds, leases: &[LeaseSpec]) -> Result<(), InjectError>;    // mint
pub fn admit(m: &mut Machine, k: &LeaseKinds, r: &Request) -> Result<InjectReceipt, InjectError>;      // key "admit:{req}", status hole "op:{req}"
pub fn route(m: &mut Machine, k: &LeaseKinds, send: &EffectView) -> Result<InjectReceipt, InjectError>; // key "route:{hole}"
pub fn reply_job(m: &mut Machine, k: &LeaseKinds, job: &EffectView, ok: bool) -> Result<InjectReceipt, InjectError>; // key "reply:{hole}"
pub fn ack_status(m: &mut Machine, k: &LeaseKinds, req: &str) -> Result<InjectReceipt, InjectError>;
pub enum MailMsg { Demand { req: String, tenant: String, regions: Vec<String>, attempt: i64 }, Returned { lease: i64, region: String } }
pub fn mail_of(m: &Machine, k: &LeaseKinds, send: &EffectView) -> MailMsg;     // inspect the message attached to a Send effect
pub struct RefPool { .. }   // reference model: free: Vec<(i64,String)> (front = head), queue: VecDeque<demand>
impl RefPool { pub fn new(leases: &[LeaseSpec]) -> Self; pub fn deliver(&mut self, msg: &MailMsg) -> Option<(String, i64, i64)>; }  // (req, attempt, lease) granted
```
Signature (types, all polarized: `Mailbox, Msg, FreeList, Queue, Grant, Status, JobReply, RouteT`; demand payload `D = (req:Str, tenant:Str, regions:List<Str>, attempt:Int)`; lease payload `L = (lease:Int, region:Str)`, conserved class `lease` key 0):
```text
Pool       : Mailbox-  (free: FreeList-, queue: Queue-)
MCons      : Mailbox+  (msg: Msg-, rest: Mailbox-)
Dispatch   : Msg-      (fi: FreeList-, qi: Queue-, fo: FreeList+, qo: Queue+)
Demand  [D]: Msg+      (reply: Grant+)
Returned[L]: Msg+                                   conserved
FreeLease[L]: FreeList+ (next: FreeList-)          conserved
FNil       : FreeList+
QCons   [D]: Queue+    (reply: Grant+, next: Queue-)
QNil       : Queue+
Seek    [D]: FreeList- (reply: Grant+, out: FreeList+, qi: Queue-, qo: Queue+)
Enq     [D]: Queue-    (reply: Grant+, out: Queue+)
Match   [L]: Queue-    (out: Queue+, fi: FreeList-, fo: FreeList+)   conserved
Granted [(lease,region,req,tenant)]: Grant+         conserved
AwaitLease[D]: Grant-  (status: Status+)
RunJob [(lease,region,req,tenant,regions,attempt)]: JobReply- (status: Status+)   effect, conserved
JobDone, JobFailed: JobReply+
Send [(dest)]: RouteT- (msg: Msg-)                  effect
Route      : RouteT+   (dest: Mailbox+, next: Mailbox-)
Done [(req, lease)], Failed [(req, attempts)]: Status+
StatusAck  : Status-
```
Rules (`eligible(D, L) = L.region ∈ D.regions`; `buggy_ignore_region` makes `seek_free`/`match_cons` treat everything as eligible):
```text
pool_msg:          MCons(msg,rest) >< Pool(free,queue)      => Dispatch(free,queue,fo,qo) @ msg ; Pool(fo,qo) @ rest
dispatch_demand:   Dispatch(fi,qi,fo,qo) >< Demand[d](reply) => Seek[d](reply,fo,qi,qo) @ fi
dispatch_returned: Dispatch(fi,qi,fo,qo) >< Returned[l]      => Match[l](qo,fi,fo) @ qi
seek_free:   Seek[d](reply,out,qi,qo) >< FreeLease[l](next)
               eligible => Granted[l+d] @ reply ; out ~ next ; qo ~ qi
               else     => FreeLease[l](o2) @ out ; Seek[d](reply,o2,qi,qo) @ next
seek_nil:    Seek[d](reply,out,qi,qo) >< FNil       => FNil @ out ; Enq[d](reply,qo) @ qi
enq_cons:    Enq[d](reply,out) >< QCons[d2](r2,next) => QCons[d2](r2,o2) @ out ; Enq[d](reply,o2) @ next
enq_nil:     Enq[d](reply,out) >< QNil              => QCons[d](reply,n) @ out ; QNil @ n
match_cons:  Match[l](out,fi,fo) >< QCons[d](reply,next)
               eligible => Granted[l+d] @ reply ; out ~ next ; fo ~ fi
               else     => QCons[d](reply,o2) @ out ; Match[l](o2,fi,fo) @ next
match_nil:   Match[l](out,fi,fo) >< QNil            => QNil @ out ; FreeLease[l](fi) @ fo
granted_await: Granted[g] >< AwaitLease[d](status)  => effect RunJob[g+d](status)            (emits RunJob)
job_done:    JobDone >< RunJob[j](status)    => effect Send["pool"](m); Returned[l] @ m; Done[(req,lease)] @ status   (emits Send)
job_failed:  JobFailed >< RunJob[j](status)  => effect Send["pool"](m); Returned[l] @ m;
               attempt < max => effect Send["pool"](m2); Demand[d'](reply) @ m2; AwaitLease[d'](status) @ reply   (d'.attempt+1)
               else          => Failed[(req, attempt)] @ status
route_send:  Route(dest,next) >< Send(msg)   => MCons(msg,next) @ dest
done_ack:    Done >< StatusAck => ∅            failed_ack: Failed >< StatusAck => ∅
```
Policies:
- `ResidencyPolicy`: every created `Granted` has a region in the consumed demand's `regions`. The demand payload is the consumed `Seek` or `QCons`.
- `TenantPolicy`: when `Granted` and `AwaitLease` are consumed, the tenants and reqs must match.

- [ ] **Step 1: Failing tests** (`tests/lease.rs`, deterministic scenarios)
```rust
#[test] fn coverage_is_complete() { /* missing and never_fires empty */ }
#[test] fn single_request_lifecycle() {
    // pool L1(eu); admit r1(eu); run; outbox == [RunJob{lease 1}]; reply ok; run; outbox == [Send]; route; run;
    // status "op:r1" peer is Done; census("lease") == [1]; ack_status; run; explain: all remaining agents wait on hole "pool"
}
#[test] fn fifo_queue_and_return_handoff() {
    // pool L1(eu); admit r1, r2, r3 (eu); r1 granted; r2,r3 queued; complete r1 -> route Returned -> r2 granted (not r3)
}
#[test] fn eligibility_skips_ineligible_leases_and_demands() {
    // pool L1(us), L2(eu); admit r1(eu) -> gets L2; admit r2(us) -> gets L1; admit r3(eu) queued;
    // return L1(us) -> r3 still queued, L1 back to free; return L2(eu) -> r3 granted L2
}
#[test] fn retry_after_failure_requeues_at_back() { /* max_attempts=2: fail r1 once -> Returned + new Demand(attempt 2) */ }
#[test] fn buggy_rule_is_stopped_by_residency_policy() {
    // buggy_ignore_region=true; pool L1(us); admit r1(eu): fault names "residency"; no RunJob in outbox; census still [1]
}
#[test] fn reference_model_agrees_on_scripted_sequence() { /* scripted interleaving; compare grants (req,attempt)->lease */ }
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: Experiment A lease-pool program (INTERACT-only matching) + reference model`.

---

### Task 13: Experiment A simulator (randomized schedules, crashes, invariants, metrics)

**Files:** Modify `src/nets/lease.rs` (add `Sim`) and `tests/lease.rs`. Create `examples/lease_pool.rs`.

**Interfaces (produces):**
```rust
pub struct SimConfig { pub seed: u64, pub requests: Vec<Request>, pub leases: Vec<LeaseSpec>, pub fail_rate: f64, pub max_attempts: i64,
                       pub crash_rate: f64, pub commit_rate: f64, pub dup_rate: f64, pub scheduler: Option<SchedulerKind>, pub file_journal: Option<PathBuf> }
impl SimConfig { pub fn standard(seed: u64) -> SimConfig; }   // 10 requests (mixed eu/us/both, 4 tenants), 3 leases: L1 eu-west-1, L2 eu-west-1, L3 us-east-1
pub struct Provider { /* held: BTreeMap<lease, effect hole>, results: BTreeMap<HoleId, bool>, violations: Vec<String> */ }
pub struct SimReport { pub outcomes: BTreeMap<String, Outcome>, pub grants: BTreeMap<(String, i64), i64>, pub mailbox: Vec<MailMsg>,
                       pub injects: u64, pub crashes: u64, pub duplicates_ignored: u64, pub stats: Stats, pub max_agents: usize,
                       pub provider_violations: Vec<String>, pub final_digest: Digest, pub boundary_log: Vec<Injection> }
pub enum Outcome { Done { lease: i64 }, Failed { attempts: i64 } }
pub fn simulate(cfg: &SimConfig) -> Result<SimReport, String>;   // Err = invariant violation with a description
```
Simulator loop. Commits and crashes:
- After each action, commit with probability `commit_rate`.
- With probability `crash_rate`: `crash()`, `recover()`, run to quiescence, then re-send every inject not yet acknowledged, in its original order and with the same keys.
- With probability `dup_rate`, re-send a random already-acknowledged inject (it must be deduplicated).

At each step, choose uniformly among the enabled actions:
1. Admit the next request.
2. Route a committed pending `Send`.
3. Start a committed pending `RunJob` at the provider (ledger exclusivity check).
4. Finish a started job (ok with probability `1 - fail_rate`) and reply.
5. Ack a finished status.
6. Run with a random budget of 0–8 reductions.

Hold these invariants after every action:
- the census `lease` equals the initial set exactly;
- there are no faults;
- the provider has no violations;
- `check_invariants()` passes every 16 actions.

At the end (no enabled actions, quiescent):
- every request is `Done` or `Failed`;
- the census contains 3 free leases, held as `FreeLease` agents;
- `explain()` has only a `Hole{label:"pool"}` group;
- the provider ledger is empty;
- the reference model fed with `mailbox` (the order in which messages were appended to the pool) yields the same `grants`.

- [ ] **Step 1: Failing tests**
```rust
#[test] fn experiment_a_randomized() { for seed in 0..300 { simulate(&SimConfig::standard(seed)).unwrap_or_else(|e| panic!("seed {seed}: {e}")); } }
#[test] fn experiment_a_with_crashes_and_duplicates() { for seed in 0..150 { let mut c = SimConfig::standard(seed); c.crash_rate = 0.05; c.dup_rate = 0.05; c.commit_rate = 0.5; simulate(&c).unwrap_or_else(|e| panic!("seed {seed}: {e}")); } }
#[test] fn experiment_a_schedule_independence() {
    // run seed with Fifo; replay report.boundary_log into fresh machines under Lifo/Random/rounds, running to quiescence after every inject; final digest == report.final_digest
}
#[test] fn experiment_a_file_journal() { /* one seed with file_journal Some(temp path) and crashes; passes */ }
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement `simulate`, plus `examples/lease_pool.rs`, which runs the 10/3 scenario, prints metrics (reductions per grant, host injects per lease cycle, `per_rule`, max agents, rounds width/depth from a replay of the boundary log with `run_rounds`), and writes `target/lease_final.dot`.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: Experiment A randomized simulator with crashes, invariants, reference model`.

---

### Task 14: Experiment B (session protocol via boundary transport)

**Files:** Create `src/nets/session.rs`, `tests/session.rs`, `examples/session.rs`.

**Interfaces (produces):**
```rust
pub struct SessionKinds { /* msgs: kick, open, accept, data, ack, close, closed, timeout; client: c_start, c_connecting, c_await_ack, c_closing, c_done;
                             server: s_listen, s_established, s_closed; effects: transmit, timer, reject; replies: tx_done, tm_done, rj_done;
                             results: session_ok, session_aborted, delivered */ }
pub struct SessionConfig { pub max_tries: i64 }
pub fn program(cfg: &SessionConfig) -> (Program, SessionKinds);
pub fn open_session(m: &mut Machine, k: &SessionKinds, sid: i64, items: &[i64]) -> Result<(), InjectError>;
    // client inbox label "c:{sid}" pre-loaded with Kick; server inbox "s:{sid}"; status "cstatus:{sid}"; app "app:{sid}"
pub struct NetConfig { pub seed: u64, pub drop: f64, pub dup: f64, pub adversarial: f64, pub items: Vec<i64>, pub max_tries: i64 }
pub struct SessionReport { pub client: ClientOutcome, pub delivered: Vec<i64>, pub transmitted: BTreeMap<String, u64>, pub rejects: u64,
                           pub timeouts_fired: u64, pub stats: Stats, pub explanation: String }
pub enum ClientOutcome { Ok, Aborted(String) }
pub fn simulate(cfg: &NetConfig) -> Result<SessionReport, String>;
```
Message cells (`Inbox+`, aux `rest: Inbox-`), payloads are tuples of `Int`:
- `Kick(sid)`, `Open(sid)`, `Accept(sid)`, `Close(sid)`, `Closed(sid)`
- `Data(sid, seq, val)`
- `Ack(sid, seq)`
- `Timeout(sid, tag)`

Client states (`Inbox-`, aux `status: Status+`):
- `CStart(sid, items)`
- `CConnecting(sid, items, tries, tag)`
- `CAwaitAck(sid, items, seq, tries, tag)`
- `CClosing(sid, tries, tag)`
- `CDone(sid)`, which has no aux port.

Server states (`Inbox-`, aux `app: App+`): `SListen(sid)`, `SEstablished(sid, expected, delivered)`, and `SClosed(sid)` (no aux port).

Effects:
- `Transmit(to, kind, fields)` on `Tx-`, replied to with `TxDone`;
- `Timer(sid, tag)` on `Tm-`, replied to with `TmDone`;
- `Reject(sid, state, msg)` on `Rj-`, replied to with `RjDone`.

Results: `SessionOk(sid)` and `SessionAborted(sid, phase)` on `Status+`, and `Delivered(list)` on `App+`.

There is one rule per (state kind, message kind), generated by looping over the message kinds with a body that matches on `cx.right_kind()`. This gives 40 client rules and 24 server rules. Transitions follow the design §5. Stale timeouts (`tag < current`), duplicate `Accept`, and duplicate `Ack(s < seq)` are ignored and do not count as violations. Every other unexpected message emits `Reject` and leaves the state unchanged. `CDone` and `SClosed` silently drain.

The network simulator:
- takes committed `Transmit` effects, acks them, and moves each message into the in-flight set (dropped with probability `drop`, duplicated with probability `dup`);
- delivers a random in-flight message by appending it to the target inbox: an inject binding `Hole(Label)` plus `New(Label)`;
- acks committed `Timer` effects into a pending set and fires one at random by appending `Timeout`;
- with probability `adversarial`, injects a random invalid message into a random inbox (for example `Data` with `seq + 5`, or `Accept` to the server);
- acks `Reject` effects and counts them.

The run ends when nothing is in flight, no timers are pending, the outbox is empty, and the machine is quiescent.

- [ ] **Step 1: Failing tests**
```rust
#[test] fn coverage_is_complete() {}
#[test] fn perfect_network_delivers_everything() { /* drop=dup=adversarial=0: client Ok, delivered == items, rejects == 0 */ }
#[test] fn total_loss_aborts_after_max_tries() { /* drop=1.0: Aborted("connect"); transmitted["Open"] == max_tries */ }
#[test] fn lossy_networks_preserve_exactly_once_in_order() {
    for seed in 0..300 { let r = simulate(&NetConfig { seed, drop: 0.3, dup: 0.2, adversarial: 0.0, items: vec![10,20,30,40], max_tries: 6 }).unwrap();
        match r.client { ClientOutcome::Ok => assert_eq!(r.delivered, vec![10,20,30,40]), ClientOutcome::Aborted(_) => assert!(vec![10,20,30,40].starts_with(&r.delivered)) } } }
#[test] fn adversarial_messages_are_never_accepted() { /* adversarial=0.3 over 200 seeds: same property; rejects > 0 overall */ }
#[test] fn quiescence_is_explained() { /* final explanation: client CDone and server waiting on their inbox holes; no Stuck, no Deadlock */ }
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement, including the example, which prints the transcript and metrics for one lossy seed.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: Experiment B session protocol with host transport and timeouts`.

---

### Task 15: Experiment B0 (typed in-graph session channel)

**Files:** Create `src/nets/session_typed.rs`. Modify `tests/session.rs`.

**Interfaces (produces):**
```rust
pub struct TypedKinds { pub s: [TypeId; 5], open, accept, data, ack, close, closed, s_await_open, s_await_choice, c_await_accept, c_await_ack, c_await_closed, session_ok, delivered: KindId }
pub enum Bug { None, ClientSendsAckAfterAccept, ServerRepliesClosedToData }
pub fn program(bug: Bug) -> (Program, TypedKinds);
pub fn start(m: &mut Machine, k: &TypedKinds, items: &[i64]) -> Result<(), InjectError>;  // SAwaitOpen >< Open — CAwaitAccept; holes "status", "app"
```
Types are `S0..S4` (polarized). A message has principal `Sk+` and continuation `cont: S(k+1)+` (or `S2+` for `Ack`); a receive state has principal `Sk-`.
```text
Open: S0+ (cont: S1+)   Accept: S1+ (cont: S2+)   Data[(seq,v)]: S2+ (cont: S3+)   Close: S2+ (cont: S4+)   Ack[seq]: S3+ (cont: S2+)   Closed: S4+
SAwaitOpen[acc]: S0- (app: App+)    SAwaitChoice[acc]: S2- (app: App+)
CAwaitAccept[items]: S1- (status: Status+)   CAwaitAck[(items,seq)]: S3- (status)   CAwaitClosed: S4- (status)
s_open:   SAwaitOpen[acc](app) >< Open(cont)        => Accept(c2) @ cont ; SAwaitChoice[acc](app) @ c2
c_accept: CAwaitAccept[items](st) >< Accept(cont)   => items? Data[(0,i0)](c2) @ cont ; CAwaitAck[(items,0)](st) @ c2  :  Close(c2) @ cont ; CAwaitClosed(st) @ c2
s_data:   SAwaitChoice[acc](app) >< Data[(s,v)](cont) => Ack[s](c2) @ cont ; SAwaitChoice[acc+[v]](app) @ c2
s_close:  SAwaitChoice[acc](app) >< Close(cont)     => Closed @ cont ; Delivered[acc] @ app
c_ack:    CAwaitAck[(items,s)](st) >< Ack(cont)     => next < len ? Data(next) + CAwaitAck : Close + CAwaitClosed
c_closed: CAwaitClosed(st) >< Closed                => SessionOk @ st
```
- [ ] **Step 1: Failing tests** (appended to `tests/session.rs`)
```rust
#[test] fn typed_session_runs_to_completion() { /* items [1,2,3]: "app" peer Delivered([1,2,3]); "status" peer SessionOk; 1 + 2*3 + 2 reductions */ }
#[test] fn typed_session_coverage_complete() {}
#[test] fn illegal_send_is_rejected_by_types() {
    for bug in [Bug::ClientSendsAckAfterAccept, Bug::ServerRepliesClosedToData] { /* exactly one fault; error mentions "type"; no Delivered */ } }
#[test] fn boundary_rejects_ill_typed_message_injection() { /* injecting Data at a hole whose peer is SAwaitOpen.0 (S0-) -> Err(InjectError::Type) */ }
```
- [ ] **Step 2:** The run fails.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Passes.
- [ ] **Step 5:** Commit with message `feat: Experiment B0 typed in-graph session channel`.

---

### Task 16: Formal IR spec, experiment findings, readme pointer

**Files:** Create `docs/ir-spec.md` and `docs/experiments.md`. Modify `readme.md` (the status line plus a short "Code" section that links the docs; no other changes).

- [ ] **Step 1:** Run all examples and capture their real metric output (`cargo run --release --example lease_pool`, `--example session`, `--example arith`).
- [ ] **Step 2:** Write `docs/ir-spec.md` with these sections:
  1. Scope and status.
  2. Signature (types and their polarity or self-duality, kinds, payload schemas, effect and conserved flags).
  3. Values.
  4. Nets (agents, endpoints, holes, labels) and well-formedness conditions W1–W6.
  5. Active pairs and rules.
  6. Patches and legality conditions L1–L9.
  7. The `INTERACT` reduction relation, defined over virtual endpoints and splicing, including vicious circles.
  8. Host operations (`INJECT`, effects and the outbox, `OBSERVE`, `COMMIT`).
  9. Memory semantics.
  10. Identity (lineage).
  11. Journal and replay.
  12. Quiescence classification.
  13. Theorems with proof sketches: strong confluence for deterministic payload rules, boundary determinism, and conservation.
  14. A table of which formal properties survive each extension.
  15. Three worked reductions, each as before / patch / after with port types: `S >< Add`, `Seek >< FreeLease` (grant, conservation, policy), and the effect round trip `Granted >< AwaitLease` → `RunJob` → `JobDone`.
- [ ] **Step 3:** Write `docs/experiments.md`. Cover the setup, the measured numbers taken from Step 1 and the test suites, the answers to P0, P1, P2, and P3 with the evidence for each, the readme decision rule applied, and the limitations.
- [ ] **Step 4:** Run `cargo test --release`, `cargo clippy --all-targets -- -D warnings` (when clippy is installed), and `cargo doc --no-deps`. All must be clean.
- [ ] **Step 5:** Commit with message `docs: formal IR + memory-semantics spec, experiment findings`.

---

### Task 17: Final verification and review

- [ ] **Step 1:** Run `cargo test --release` and `cargo build --examples` on a clean build, and confirm the counts.
- [ ] **Step 2:** Review the whole branch against the spec §1 success criteria, item by item, and fix any gaps.
- [ ] **Step 3:** Commit any fixes.
