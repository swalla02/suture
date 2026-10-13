# Suture language v0, plan 1 of 5: kernel changes — implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the kernel's host-facing costs independent of system size, and journal which program version wrote each journal. This is milestone M1 of the language spec.

**Architecture:**
- Three self-contained changes to the existing `suture` kernel crate:
  - an indexed outbox with a queue of new effects;
  - a per-agent payload size limit, enforced as part of patch and inject legality;
  - a `Version` journal record carrying a program fingerprint, checked on recovery.
- The interaction-net semantics are unchanged: `INTERACT` stays the only internal computation, and the existing nets behave exactly as before.

**Tech stack:** Rust (edition 2024, rust-version 1.85). No new dependencies.

**Spec:** `docs/superpowers/specs/2026-10-14-suture-language-design.md`. This plan implements §5.1 (kernel changes), the C2 and C3 cost tests of §7.4, and the findings of Appendix A.

**The five plans:**
1. kernel changes (this plan);
2. compiler front end (M2);
3. mechanisms and runtime (M3–M4);
4. testing tools and the shop (M5–M6);
5. baseline and evaluation (M7).

Each plan is written once the previous one is done, because each consumes the interfaces the previous one produces.

## Global Constraints

- **Dependencies:** none new. The kernel stays on `serde`, `serde_json`, `siphasher` and `thiserror`.
- **Payload limit:** `Limits.max_payload_bytes` defaults to **4096**, measured as the length of `Value::encode`.
- **Version record:** `Record::Version { program, version, hash }` is written when a program starts on a journal, and recovery checks it. The journal `FORMAT` becomes **2**.
- **Effect order:** `take_new_effects()` and `outbox()` both return effects sorted by hole identity (spec D10: the same input gives the same output).
- **Cost tests count work, never time** (spec D7). C2 uses **0, 10,000 and 100,000** pending effects (spec §7.4).
- **Existing behavior:** all 130 existing tests keep passing. `tests/journal.rs`'s `replay_refuses_version_change` must still report `RecoveryError::RuleVersion`. DNC-1's depnet driver keeps using `outbox()` unchanged (spec §5.1: depnet is not reused, only left working).
- **Run tests with `--release`.** The development machine has about 3.8 GB of RAM, and test C2 builds 100,000 pending effects.
- **Lints:** `cargo clippy --release --all-targets -- -D warnings` stays clean.
- **Branch:** work on `lang-v0`. Every commit message ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Review Focus

1. **An effect answered before the host takes it** must not come back from `take_new_effects()`: its queue entry is stale. Test in Task 1: `an_effect_answered_before_it_is_taken_is_skipped`.
2. **After a crash and recovery,** every still-pending effect must come back from `take_new_effects()`, so that the host dispatches it again. Test in Task 1: `recovery_queues_every_pending_effect_again`.
3. **A host that only reads `outbox()`** and never calls `take_new_effects()` must not leak memory through the queue. Test in Task 1: `a_host_that_never_takes_keeps_the_queue_bounded`.
4. **The limit's edge:** a payload of exactly 4096 bytes is accepted, and 4097 is refused (an off-by-one would silently change every net). Tests in Task 2: `payload_size_limit` and `a_rule_that_builds_an_oversized_payload_faults`.
5. **A journal that cannot be replayed safely** must be refused with a clear error, never replayed into a wrong state. This covers a format-1 journal, and a program whose name and version match but whose structure differs. Tests in Task 3: `a_format_1_journal_is_refused` and `a_structurally_different_program_is_refused`.

---

## File structure

| File | Change | Responsibility |
|---|---|---|
| `src/machine.rs` | modify | outbox index and new-effect queue (Task 1); `Limits.max_payload_bytes` (Task 2); program fingerprint, `Version` record, recovery check (Task 3) |
| `src/graph.rs` | modify | `GraphError::Outbox` for the invariant check (Task 1) |
| `src/value.rs` | modify | `Value::encoded_len` (Task 2) |
| `src/check.rs` | modify | `NetError::PayloadTooLarge`; size check in `check_agents` (Task 2) |
| `src/journal.rs` | modify | `FORMAT = 2`, `Record::Version`, `RecoveryError::Version` (Task 3) |
| `src/sig.rs` | modify | `Signature::encode_structure` (Task 3) |
| `src/rule.rs` | modify | `RuleTable::encode_structure` (Task 3) |
| `tests/common/mod.rs` | modify | the `Orders` test program and its helpers (Task 1) |
| `tests/outbox.rs` | create | effect queue behavior (Task 1) |
| `tests/cost.rs` | create | cost tests C2 (Task 1) and C3 (Task 2) |
| `tests/payload.rs` | create | payload limit at run time (Task 2) |
| `tests/journal.rs` | modify | version record tests (Task 3) |
| `docs/ir-spec.md` | modify | §8.2 effect queue (Task 1); §6 L8 and §8.1 I1 (Task 2); §11 records and recovery (Task 3) |

---

### Task 1: Effect queue and indexed outbox

**Files:**
- Modify: `src/machine.rs`: imports, `Stats`, `Machine`, `fresh`, `outbox`, `check_invariants`, `install`, `apply_inject`.
- Modify: `src/graph.rs`: the `GraphError` enum.
- Modify: `tests/common/mod.rs`: append the `Orders` program.
- Create: `tests/outbox.rs`, `tests/cost.rs`.
- Modify: `docs/ir-spec.md` §8.2.

**Interfaces:**
- Consumes: nothing new.
- Produces (later plans rely on these exact names):
  - `Machine::take_new_effects(&mut self) -> Vec<EffectView>`: effects created since the last call that are still pending, sorted by hole.
  - `Stats::effects_examined: u64`: queue entries examined by `take_new_effects`, stale ones included. The spec's other counter, steps per operation, is read as the change in the existing `Stats::reductions`, so it needs no new field.
  - `GraphError::Outbox(String)`.
  - In `tests/common`:
    - `pub struct Orders { prog, place, start, capture, order, answer, paid }`;
    - `pub fn orders() -> Orders`;
    - `pub fn place(m: &mut Machine, o: &Orders, first: i64, n: usize)`;
    - `pub fn answer(m: &mut Machine, o: &Orders, hole: HoleId)`.

- [ ] **Step 1: Add the shared test program.** Append to the end of `tests/common/mod.rs`:

```rust
/// Orders waiting on an outside payment: `Place(id) >< Start` emits
/// `Capture(id)`, with an `Order(id)` waiting on its reply. The host answers
/// by injecting `Answer` at the effect's hole: `Answer >< Capture` gives
/// `Paid(id)`, and `Paid >< Order` settles both.
pub struct Orders {
    pub prog: Program,
    pub place: KindId,
    pub start: KindId,
    pub capture: KindId,
    pub order: KindId,
    pub answer: KindId,
    pub paid: KindId,
}

pub fn orders() -> Orders {
    let mut b = SigBuilder::new("orders", 1);
    let (ord, pay, res) = (b.polarized("Ord"), b.polarized("Pay"), b.polarized("Res"));
    let place = b.kind("Place", ord.pos()).payload(ValueType::Int).done();
    let start = b.kind("Start", ord.neg()).done();
    let capture = b.kind("Capture", pay.neg()).aux("reply", res.pos()).payload(ValueType::Int).effect().done();
    let order = b.kind("Order", res.neg()).payload(ValueType::Int).done();
    let answer = b.kind("Answer", pay.pos()).done();
    let paid = b.kind("Paid", res.pos()).payload(ValueType::Int).done();
    let mut t = RuleTable::new();
    t.def("place", place, start, move |cx| {
        let id = cx.left().clone();
        let r = cx.var();
        cx.effect(capture, id.clone(), &[r]);
        cx.agent(order, id, r, &[]);
        Ok(())
    })
    .emits(&[capture]);
    t.def("answer", answer, capture, move |cx| {
        let [reply] = cx.right_aux();
        cx.agent(paid, cx.right().clone(), reply, &[]);
        Ok(())
    });
    t.def("settle", paid, order, |_cx| Ok(()));
    Orders { prog: Program::new(b.build().unwrap(), t).unwrap(), place, start, capture, order, answer, paid }
}

/// Places orders `first .. first + n` and reduces until quiescent, leaving
/// one pending `Capture` effect per order.
pub fn place(m: &mut Machine, o: &Orders, first: i64, n: usize) {
    let ids: Vec<i64> = (first..first + n as i64).collect();
    for batch in ids.chunks(10_000) {
        let mut b = NetBuilder::new(&o.prog.sig);
        for &id in batch {
            let v = b.var();
            b.agent(o.place, id, v, &[]);
            b.agent(o.start, (), v, &[]);
        }
        m.inject(Injection::new(b.finish().unwrap())).unwrap();
    }
    m.run(None);
}

/// Answers the pending effect at `hole` and reduces until quiescent.
pub fn answer(m: &mut Machine, o: &Orders, hole: HoleId) {
    let mut b = NetBuilder::new(&o.prog.sig);
    let v = b.var();
    b.agent(o.answer, (), v, &[]);
    m.inject(Injection::new(b.finish().unwrap()).at_hole(v, hole)).unwrap();
    m.run(None);
}
```

- [ ] **Step 2: Write the failing behavior tests.** Create `tests/outbox.rs`:

```rust
mod common;

use common::*;
use suture::*;

#[test]
fn new_effects_are_taken_once_in_hole_order() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    place(&mut m, &o, 0, 3);
    let taken = m.take_new_effects();
    let holes: Vec<HoleId> = taken.iter().map(|e| e.hole).collect();
    let mut sorted = holes.clone();
    sorted.sort();
    assert_eq!(holes, sorted);
    assert_eq!(taken.len(), 3);
    assert!(taken.iter().all(|e| e.kind == o.capture));
    assert!(m.take_new_effects().is_empty(), "already taken");
    assert_eq!(m.outbox(), taken, "taken effects stay pending until answered");
    m.check_invariants().unwrap();
}

#[test]
fn an_effect_answered_before_it_is_taken_is_skipped() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    place(&mut m, &o, 0, 3);
    let first = m.outbox()[0].hole;
    answer(&mut m, &o, first);
    let before = m.stats().effects_examined;
    let taken = m.take_new_effects();
    assert_eq!(taken.len(), 2);
    assert!(taken.iter().all(|e| e.hole != first));
    assert_eq!(m.stats().effects_examined - before, 3, "the stale entry is examined, then dropped");
    m.check_invariants().unwrap();
}

#[test]
fn outbox_lists_pending_effects_in_hole_order() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    place(&mut m, &o, 0, 50);
    for e in m.outbox().into_iter().step_by(7) {
        answer(&mut m, &o, e.hole);
    }
    let scanned: Vec<HoleId> = m
        .graph()
        .holes_sorted()
        .into_iter()
        .filter(|(_, h)| matches!(h.origin, HoleOrigin::Effect { .. }))
        .map(|(id, _)| id)
        .collect();
    let listed: Vec<HoleId> = m.outbox().iter().map(|e| e.hole).collect();
    assert_eq!(listed, scanned);
    assert_eq!(listed.len(), 50 - 8);
    m.check_invariants().unwrap();
}

#[test]
fn recovery_queues_every_pending_effect_again() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    place(&mut m, &o, 0, 3);
    let pending = m.take_new_effects();
    answer(&mut m, &o, pending[0].hole);
    m.commit().unwrap();
    let mut r = Machine::recover(o.prog.clone(), m.into_journal()).unwrap();
    let again = r.take_new_effects();
    assert_eq!(again, pending[1..].to_vec(), "the host must dispatch these again after a crash");
    assert_eq!(r.outbox(), again);
    r.check_invariants().unwrap();
}

#[test]
fn a_host_that_never_takes_keeps_the_queue_bounded() {
    let o = orders();
    let mut m = Machine::new(o.prog.clone());
    for id in 0..5_000 {
        place(&mut m, &o, id, 1);
        let hole = m.outbox()[0].hole;
        answer(&mut m, &o, hole);
    }
    assert!(m.outbox().is_empty());
    let before = m.stats().effects_examined;
    assert!(m.take_new_effects().is_empty());
    assert!(m.stats().effects_examined - before < 2_000, "the queue must not keep answered effects");
}
```

- [ ] **Step 3: Write the failing cost test C2.** Create `tests/cost.rs`:

```rust
//! Work-count acceptance tests (language spec §7.4). They count work and
//! never time it.

mod common;

use common::*;
use suture::*;

/// C2: taking new effects examines only the new ones, however many are pending.
#[test]
fn c2_taking_new_effects_ignores_the_backlog() {
    for pending in [0usize, 10_000, 100_000] {
        let o = orders();
        let mut m = Machine::new(o.prog.clone());
        place(&mut m, &o, 0, pending);
        assert_eq!(m.take_new_effects().len(), pending);
        let before = m.stats().effects_examined;
        place(&mut m, &o, pending as i64, 5);
        assert_eq!(m.take_new_effects().len(), 5);
        assert_eq!(m.stats().effects_examined - before, 5, "with {pending} pending");
        assert_eq!(m.outbox().len(), pending + 5);
    }
}
```

- [ ] **Step 4: Run the tests and see them fail.**

Run: `cargo test --release --test outbox --test cost`
Expected: compile errors `no method named 'take_new_effects' found for struct 'Machine'` and `no field 'effects_examined' on type '&Stats'`.

- [ ] **Step 5: Add the error variant.** In `src/graph.rs`, in `enum GraphError`, replace

```rust
    #[error("ready queue inconsistent: {0}")]
    Ready(String),
}
```

with

```rust
    #[error("ready queue inconsistent: {0}")]
    Ready(String),
    #[error("effect index inconsistent: {0}")]
    Outbox(String),
}
```

- [ ] **Step 6: Index effects in the machine.** All edits are in `src/machine.rs`.

(a) Imports: replace `use std::collections::{BTreeMap, HashMap, VecDeque};` with `use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};`.

(b) In `pub struct Stats`, replace

```rust
    pub max_agents: usize,
    pub per_rule: BTreeMap<String, u64>,
}
```

with

```rust
    pub max_agents: usize,
    pub per_rule: BTreeMap<String, u64>,
    /// Queue entries examined by `take_new_effects`, stale ones included.
    pub effects_examined: u64,
}
```

(c) In `pub struct Machine`, replace

```rust
    sched: SchedulerKind,
    rng: Rng,
}
```

with

```rust
    sched: SchedulerKind,
    rng: Rng,
    /// Effect holes still waiting for an answer: the outbox index.
    pending_effects: BTreeSet<HoleId>,
    /// Effect holes created since the last `take_new_effects`.
    new_effects: Vec<HoleId>,
}
```

(d) In `fn fresh`, replace

```rust
            sched: SchedulerKind::Fifo,
            rng: Rng::new(0),
        };
```

with

```rust
            sched: SchedulerKind::Fifo,
            rng: Rng::new(0),
            pending_effects: BTreeSet::new(),
            new_effects: Vec::new(),
        };
```

(e) Replace the whole `outbox` method:

```rust
    /// Pending effects (effect agents still waiting at their outbox hole),
    /// sorted by hole identity.
    pub fn outbox(&self) -> Vec<EffectView> {
        self.graph
            .holes_sorted()
            .into_iter()
            .filter_map(|(h, hole)| {
                let HoleOrigin::Effect { agent } = hole.origin else { return None };
                if hole.peer != Endpoint::Port(agent, 0) {
                    return None;
                }
                let a = self.graph.agent(agent)?;
                Some(EffectView {
                    agent,
                    hole: h,
                    kind: a.kind,
                    kind_name: self.prog.sig.kind(a.kind).name.clone(),
                    payload: a.payload.clone(),
                })
            })
            .collect()
    }
```

with

```rust
    /// Pending effects (effect agents still waiting at their outbox hole),
    /// sorted by hole identity. Reads the outbox index, never the whole
    /// graph; hosts that poll should use [`Machine::take_new_effects`].
    pub fn outbox(&self) -> Vec<EffectView> {
        self.pending_effects.iter().filter_map(|&h| self.effect_view(h)).collect()
    }

    /// The effects created since the last call that are still pending,
    /// sorted by hole identity. Examines only those queue entries (counted in
    /// `Stats::effects_examined`), however many effects are pending.
    pub fn take_new_effects(&mut self) -> Vec<EffectView> {
        let mut batch = std::mem::take(&mut self.new_effects);
        self.stats.effects_examined += batch.len() as u64;
        batch.sort();
        batch.into_iter().filter(|h| self.pending_effects.contains(h)).filter_map(|h| self.effect_view(h)).collect()
    }

    fn effect_view(&self, h: HoleId) -> Option<EffectView> {
        let hole = self.graph.hole(h)?;
        let HoleOrigin::Effect { agent } = hole.origin else { return None };
        if hole.peer != Endpoint::Port(agent, 0) {
            return None;
        }
        let a = self.graph.agent(agent)?;
        Some(EffectView {
            agent,
            hole: h,
            kind: a.kind,
            kind_name: self.prog.sig.kind(a.kind).name.clone(),
            payload: a.payload.clone(),
        })
    }

    /// Indexes a new effect hole and queues it for `take_new_effects`. A host
    /// that only reads `outbox()` never drains the queue, so the queue is
    /// pruned to pending effects whenever it outgrows them.
    fn note_effect(&mut self, h: HoleId) {
        self.pending_effects.insert(h);
        self.new_effects.push(h);
        if self.new_effects.len() > 2 * self.pending_effects.len() + 1024 {
            self.new_effects.retain(|h| self.pending_effects.contains(h));
        }
    }
```

(f) In `check_invariants`, replace

```rust
        if active != queued {
            return Err(GraphError::Ready(format!("active {active:?} vs queued {queued:?}")));
        }
        Ok(())
    }
```

with

```rust
        if active != queued {
            return Err(GraphError::Ready(format!("active {active:?} vs queued {queued:?}")));
        }
        let effects: Vec<HoleId> = self
            .graph
            .holes_sorted()
            .into_iter()
            .filter(|(_, h)| matches!(h.origin, HoleOrigin::Effect { .. }))
            .map(|(id, _)| id)
            .collect();
        let indexed: Vec<HoleId> = self.pending_effects.iter().copied().collect();
        if effects != indexed {
            return Err(GraphError::Outbox(format!("holes {effects:?} vs index {indexed:?}")));
        }
        Ok(())
    }
```

(g) In `install`, replace

```rust
                self.graph.set_end(Endpoint::Port(ids[i], 0), Endpoint::Hole(h));
                self.stats.effects += 1;
```

with

```rust
                self.graph.set_end(Endpoint::Port(ids[i], 0), Endpoint::Hole(h));
                self.note_effect(h);
                self.stats.effects += 1;
```

(h) In `apply_inject`, replace

```rust
        for h in &plan.consumed {
            self.graph.remove_hole(*h);
        }
```

with

```rust
        for h in &plan.consumed {
            self.graph.remove_hole(*h);
            self.pending_effects.remove(h);
        }
```

- [ ] **Step 7: Run the new tests and see them pass.**

Run: `cargo test --release --test outbox --test cost`
Expected: `test result: ok. 5 passed` (outbox), then `test result: ok. 1 passed` (cost).

- [ ] **Step 8: Run the whole suite.** `check_invariants` now also checks the index on every existing test that calls it.

Run: `cargo test --release`
Expected: every `test result:` line reports `ok`, with 136 tests in total.

- [ ] **Step 9: Document the queue.** In `docs/ir-spec.md` §8.2, after the bullet that starts with `- **Authority.**`, add:

```markdown
- **Effect queue.** The machine indexes pending effects by hole and queues each new one.
  - `take_new_effects()` returns the effects created since its last call that are still pending, sorted by hole. It examines only those entries, counted in `Stats::effects_examined`.
  - `outbox()` lists every pending effect from the same index, in hole order, without scanning the graph.
  - After recovery, every pending effect is queued again, so a host dispatches it again. Adapters deduplicate on the hole identity.
```

- [ ] **Step 10: Commit.**

```bash
git add src/machine.rs src/graph.rs tests/common/mod.rs tests/outbox.rs tests/cost.rs docs/ir-spec.md
git commit -m "feat(kernel): queue new effects and index the outbox

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 2: Payload size limit

**Files:**
- Modify: `src/value.rs`: add `Value::encoded_len`, and append a test module.
- Modify: `src/check.rs`: `NetError`, `PatchSpec`, `check_agents`, `check_patch`, `check_inject_static`, and the existing tests' literals.
- Modify: `src/machine.rs`: `Limits`, `prepare`, `plan_inject`.
- Create: `tests/payload.rs`.
- Modify: `tests/cost.rs`: add C3.
- Modify: `docs/ir-spec.md` §6 L8 and §8.1 I1.

**Interfaces:**
- Consumes: `tests/cost.rs` from Task 1.
- Produces:
  - `Value::encoded_len(&self) -> usize`;
  - `Limits.max_payload_bytes: usize` (default 4096);
  - `NetError::PayloadTooLarge { kind: String, bytes: usize, limit: usize }`, displayed as `payload of {kind} is {bytes} bytes, limit {limit}`;
  - `PatchSpec.max_payload: usize`;
  - `check_inject_static(sig, net, mint, max_agents, max_payload)`.

- [ ] **Step 1: Write the failing unit test for `encoded_len`.** Append to the end of `src/value.rs`:

```rust
#[cfg(test)]
mod size_tests {
    use super::*;
    use crate::{tup, vlist};

    #[test]
    fn encoded_len_matches_encode() {
        let values = [
            Value::Unit,
            Value::Bool(true),
            Value::Int(-5),
            Value::Str("héllo".into()),
            tup![1, "ab", vlist![true, ()]],
            Value::list([]),
        ];
        for v in values {
            let mut buf = vec![];
            v.encode(&mut buf);
            assert_eq!(v.encoded_len(), buf.len(), "{v}");
        }
    }
}
```

- [ ] **Step 2: Write the failing unit test for the limit's edge.** In `src/check.rs`, replace the end of the test module

```rust
        assert_eq!(check_inject_static(&f.sig, &net, true, 16).unwrap_err(), NetError::EffectInInject("Ask".into()));
    }
}
```

with

```rust
        assert_eq!(check_inject_static(&f.sig, &net, true, 16).unwrap_err(), NetError::EffectInInject("Ask".into()));
    }

    #[test]
    fn payload_size_limit() {
        let f = fx();
        let mut b = NetBuilder::new(&f.sig);
        let v = b.var();
        b.agent(f.lease, tup![1], v, &[]); // tuple tag + length + one int: 18 bytes
        let net = b.finish().unwrap();
        assert!(check_inject_static(&f.sig, &net, true, 16, 18).is_ok(), "exactly at the limit");
        assert_eq!(
            check_inject_static(&f.sig, &net, true, 16, 17).unwrap_err(),
            NetError::PayloadTooLarge { kind: "Lease".into(), bytes: 18, limit: 17 }
        );
    }
}
```

- [ ] **Step 3: Write the failing run-time tests.** Create `tests/payload.rs`:

```rust
use suture::*;

struct Grow {
    prog: Program,
    seed: KindId,
    grow: KindId,
    big: KindId,
}

/// `Seed(n) >< Grow` creates `Big`, whose payload is a string of `n` bytes.
fn grow(limits: Limits) -> Grow {
    let mut b = SigBuilder::new("grow", 1);
    let (t, u) = (b.polarized("T"), b.polarized("U"));
    let seed = b.kind("Seed", t.pos()).payload(ValueType::Int).done();
    let grow = b.kind("Grow", t.neg()).aux("out", u.pos()).done();
    let big = b.kind("Big", u.pos()).payload(ValueType::Str).done();
    let mut r = RuleTable::new();
    r.def("grow", seed, grow, move |cx| {
        let n = cx.left().as_int().unwrap() as usize;
        let [out] = cx.right_aux();
        cx.agent(big, "x".repeat(n).as_str(), out, &[]);
        Ok(())
    });
    Grow { prog: Program::new(b.build().unwrap(), r).unwrap().with_limits(limits), seed, grow, big }
}

fn start(g: &Grow, n: i64) -> Injection {
    let mut b = NetBuilder::new(&g.prog.sig);
    let (p, o) = (b.var(), b.var());
    b.agent(g.seed, n, p, &[]);
    b.agent(g.grow, (), p, &[o]);
    Injection::new(b.finish().unwrap()).new_hole(o, Some("out"))
}

#[test]
fn the_default_limit_is_4096_bytes() {
    assert_eq!(Limits::default().max_payload_bytes, 4096);
}

#[test]
fn a_rule_that_builds_an_oversized_payload_faults() {
    let g = grow(Limits::default());
    let mut m = Machine::new(g.prog.clone());
    m.inject(start(&g, 4_087)).unwrap(); // 9 + 4087 = 4096 bytes: allowed
    m.run(None);
    assert!(m.faults().is_empty(), "{:?}", m.faults());
    let mut m = Machine::new(g.prog.clone());
    m.inject(start(&g, 4_088)).unwrap(); // 4097 bytes
    m.run(None);
    let faults = m.faults();
    assert_eq!(faults.len(), 1);
    assert!(faults[0].error.contains("payload of Big is 4097 bytes, limit 4096"), "{}", faults[0].error);
}

#[test]
fn an_oversized_inject_is_refused_and_changes_nothing() {
    let g = grow(Limits::default());
    let mut m = Machine::new(g.prog.clone());
    let mut b = NetBuilder::new(&g.prog.sig);
    let v = b.var();
    b.agent(g.big, "y".repeat(5_000).as_str(), v, &[]);
    let inj = Injection::new(b.finish().unwrap()).new_hole(v, Some("big"));
    let err = m.inject(inj).unwrap_err();
    assert!(matches!(err, InjectError::Net(NetError::PayloadTooLarge { bytes: 5_009, limit: 4096, .. })), "{err}");
    assert_eq!((m.graph().agent_count(), m.graph().hole_count(), m.stats().injects), (0, 0, 0));
}

#[test]
fn the_limit_is_configurable() {
    let g = grow(Limits { max_payload_bytes: 100, ..Limits::default() });
    let mut m = Machine::new(g.prog.clone());
    m.inject(start(&g, 92)).unwrap(); // 101 bytes
    m.run(None);
    assert_eq!(m.faults().len(), 1);
}
```

- [ ] **Step 4: Write the failing cost test C3.** In `tests/cost.rs`, replace

```rust
use common::*;
use suture::*;
```

with

```rust
use common::*;
use suture::dnc::FirstBackend;
use suture::nets::depnet::world::{generate, GenParams};
use suture::nets::depnet::{DepConfig, Depnet, RunConfig};
use suture::*;
```

Then append to the end of `tests/cost.rs`:

```rust
/// C3, payload half: DNC-1's worlds, which carry the largest payloads among
/// the existing nets, fit the default limit. The other half of C3 (a step
/// reads only its two meeting agents) holds by construction, because
/// `RuleCx` and `PatchView` expose only the meeting pair and the patch.
#[test]
fn c3_existing_nets_fit_the_payload_limit() {
    let limit = Limits::default().max_payload_bytes;
    let worlds = [
        ("small", GenParams::small()),
        ("contended", GenParams::contended()),
        ("contended_large", GenParams::contended_large()),
        ("loose", GenParams::loose()),
    ];
    for (name, params) in worlds {
        let mut d = Depnet::new(&generate(&params, 7), &DepConfig::default()).unwrap();
        d.run(&mut FirstBackend, &RunConfig::default());
        assert!(d.m.faults().iter().all(|f| !f.error.contains("bytes, limit")), "{name}: {:?}", d.m.faults());
        let largest = d.m.graph().agents_sorted().iter().map(|(_, a)| a.payload.encoded_len()).max().unwrap_or(0);
        assert!(largest <= limit, "{name}: the largest payload is {largest} bytes");
    }
}
```

- [ ] **Step 5: Run the tests and see them fail.**

Run: `cargo test --release --lib && cargo test --release --test payload --test cost`
Expected: compile errors `no method named 'encoded_len'`, `this function takes 4 arguments but 5 arguments were supplied` (check_inject_static), `no variant named 'PayloadTooLarge'`, and `no field 'max_payload_bytes'`.

- [ ] **Step 6: Implement `encoded_len`.** In `src/value.rs`, in `impl Value`, replace

```rust
    /// Appends a canonical, tagged, length-prefixed encoding of the value.
```

with

```rust
    /// The length of [`Value::encode`]'s output, computed without encoding.
    pub fn encoded_len(&self) -> usize {
        match self {
            Value::Unit => 1,
            Value::Bool(_) => 2,
            Value::Int(_) => 9,
            Value::Str(s) => 9 + s.len(),
            Value::Tuple(xs) | Value::List(xs) => 9 + xs.iter().map(Value::encoded_len).sum::<usize>(),
        }
    }

    /// Appends a canonical, tagged, length-prefixed encoding of the value.
```

- [ ] **Step 7: Check sizes in patch and inject legality.** All edits are in `src/check.rs`.

(a) In `enum NetError`, replace

```rust
    #[error("patch has {agents} agents, limit {limit}")]
    TooLarge { agents: usize, limit: usize },
```

with

```rust
    #[error("patch has {agents} agents, limit {limit}")]
    TooLarge { agents: usize, limit: usize },
    #[error("payload of {kind} is {bytes} bytes, limit {limit}")]
    PayloadTooLarge { kind: String, bytes: usize, limit: usize },
```

(b) In `struct PatchSpec`, replace

```rust
    pub may_emit: &'a [KindId],
    pub max_agents: usize,
}
```

with

```rust
    pub may_emit: &'a [KindId],
    pub max_agents: usize,
    /// Maximum encoded payload size of each created agent, in bytes.
    pub max_payload: usize,
}
```

(c) Replace

```rust
fn check_agents(sig: &Signature, net: &NetData, max_agents: usize, may_emit: Option<&[KindId]>) -> Result<(), NetError> {
```

with

```rust
fn check_agents(
    sig: &Signature,
    net: &NetData,
    max_agents: usize,
    max_payload: usize,
    may_emit: Option<&[KindId]>,
) -> Result<(), NetError> {
```

(d) In `check_agents`, replace

```rust
        if !d.payload.admits(&a.payload) {
            return Err(NetError::Payload { kind: d.name.clone(), value: a.payload.to_string() });
        }
```

with

```rust
        if !d.payload.admits(&a.payload) {
            return Err(NetError::Payload { kind: d.name.clone(), value: a.payload.to_string() });
        }
        let bytes = a.payload.encoded_len();
        if bytes > max_payload {
            return Err(NetError::PayloadTooLarge { kind: d.name.clone(), bytes, limit: max_payload });
        }
```

(e) In `check_patch`, replace `check_agents(sig, net, spec.max_agents, Some(spec.may_emit))?;` with `check_agents(sig, net, spec.max_agents, spec.max_payload, Some(spec.may_emit))?;`.

(f) Replace

```rust
pub(crate) fn check_inject_static(sig: &Signature, net: &NetData, mint: bool, max_agents: usize) -> Result<(), NetError> {
    check_agents(sig, net, max_agents, None)?;
```

with

```rust
pub(crate) fn check_inject_static(
    sig: &Signature,
    net: &NetData,
    mint: bool,
    max_agents: usize,
    max_payload: usize,
) -> Result<(), NetError> {
    check_agents(sig, net, max_agents, max_payload, None)?;
```

(g) Update the existing tests in the same file:
- replace every occurrence of `max_agents: 16` with `max_agents: 16, max_payload: 4096` (8 occurrences);
- replace the single `max_agents: 1 }` with `max_agents: 1, max_payload: 4096 }`;
- replace these three calls in `inject_static_rules`:
  - `check_inject_static(&f.sig, &net, false, 16)` with `check_inject_static(&f.sig, &net, false, 16, 4096)`;
  - `check_inject_static(&f.sig, &net, true, 16).is_ok()` with `check_inject_static(&f.sig, &net, true, 16, 4096).is_ok()`;
  - `check_inject_static(&f.sig, &net, true, 16).unwrap_err()` with `check_inject_static(&f.sig, &net, true, 16, 4096).unwrap_err()`.

- [ ] **Step 8: Add the limit to the machine.** All edits are in `src/machine.rs`.

(a) Replace

```rust
    /// Maximum agents a single inject may create.
    pub max_inject_agents: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_patch_agents: 256, max_inject_agents: 1 << 20 }
    }
}
```

with

```rust
    /// Maximum agents a single inject may create.
    pub max_inject_agents: usize,
    /// Maximum encoded size of one agent's payload, in bytes. Collections
    /// belong in the graph as agents, not inside payloads.
    pub max_payload_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_patch_agents: 256, max_inject_agents: 1 << 20, max_payload_bytes: 4096 }
    }
}
```

(b) In `prepare`, replace

```rust
            may_emit: &rule.may_emit,
            max_agents: self.prog.limits.max_patch_agents,
        };
```

with

```rust
            may_emit: &rule.may_emit,
            max_agents: self.prog.limits.max_patch_agents,
            max_payload: self.prog.limits.max_payload_bytes,
        };
```

(c) In `plan_inject`, replace

```rust
        check_inject_static(sig, &inj.net, inj.mint, self.prog.limits.max_inject_agents)?;
```

with

```rust
        let limits = &self.prog.limits;
        check_inject_static(sig, &inj.net, inj.mint, limits.max_inject_agents, limits.max_payload_bytes)?;
```

- [ ] **Step 9: Run the new tests and see them pass.**

Run: `cargo test --release --lib && cargo test --release --test payload --test cost`
Expected: the library's tests pass, including `encoded_len_matches_encode` and `payload_size_limit`. Then `test result: ok. 4 passed` (payload) and `test result: ok. 2 passed` (cost).

- [ ] **Step 10: Run the whole suite.**

Run: `cargo test --release`
Expected: every `test result:` line reports `ok`, with 143 tests in total.

- [ ] **Step 11: Document the limit.** In `docs/ir-spec.md`:
- replace `- **L8 Bounded work.** The patch has at most `Limits::max_patch_agents` agents (256 by default).` with:

  `- **L8 Bounded work.** The patch has at most `Limits::max_patch_agents` agents (256 by default), and each payload encodes to at most `Limits::max_payload_bytes` bytes (4096 by default). Collections belong in the graph as agents, not inside payloads.`
- replace `- **I1.** L1–L3, and L8 against `Limits::max_inject_agents`, hold with bindings counted as uses.` with:

  `- **I1.** L1–L3, and L8 against `Limits::max_inject_agents` and `Limits::max_payload_bytes`, hold with bindings counted as uses.`

- [ ] **Step 12: Commit.**

```bash
git add src/value.rs src/check.rs src/machine.rs tests/payload.rs tests/cost.rs docs/ir-spec.md
git commit -m "feat(kernel): limit payload size per agent

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```

---

### Task 3: Version record and program fingerprint

**Files:**
- Modify: `src/journal.rs`: `FORMAT`, `Record`, `RecoveryError`.
- Modify: `src/sig.rs`: an import, `Signature::encode_structure`, and two helper functions.
- Modify: `src/rule.rs`: an import and `RuleTable::encode_structure`.
- Modify: `src/machine.rs`: an import, `Program`, `fresh`, `recover`, and a new `check_version`.
- Modify: `tests/journal.rs`: append tests.
- Modify: `docs/ir-spec.md` §11.

**Interfaces:**
- Consumes: `Limits.max_payload_bytes` (Task 2), which the fingerprint includes.
- Produces (plan 3's runtime pins the program file's hash with `with_fingerprint`):
  - `Program::fingerprint(&self) -> Digest`;
  - `Program::with_fingerprint(self, Digest) -> Program`;
  - `Record::Version { program: String, version: u32, hash: Digest }`;
  - `RecoveryError::Version { journal: String, program: String }`;
  - `journal::FORMAT == 2`.

- [ ] **Step 1: Write the failing tests.** Append to the end of `tests/journal.rs`:

```rust
struct AllowAll;

impl Policy for AllowAll {
    fn name(&self) -> &str {
        "allow-all"
    }
    fn check(&self, _: &PatchView<'_>) -> Result<(), String> {
        Ok(())
    }
}

#[test]
fn a_journal_starts_with_header_and_version() {
    let m0 = mini();
    let path = tmp("version");
    let m = Machine::with_journal(m0.prog.clone(), Box::new(FileJournal::open(&path).unwrap())).unwrap();
    drop(m);
    let records = FileJournal::open(&path).unwrap().durable().unwrap();
    assert_eq!(records[0], Record::Header { program: "mini".into(), version: 1, format: suture::journal::FORMAT });
    assert_eq!(records[1], Record::Version { program: "mini".into(), version: 1, hash: m0.prog.fingerprint() });
    assert_eq!(suture::journal::FORMAT, 2);
}

#[test]
fn fingerprints_cover_structure_but_not_rule_versions() {
    assert_eq!(mini().prog.fingerprint(), mini().prog.fingerprint());
    assert_eq!(mini().prog.fingerprint(), mini_with(1, 2).prog.fingerprint(), "replay checks rule versions and code");
    assert_ne!(mini().prog.fingerprint(), mini().prog.with_policy(AllowAll).fingerprint());
    let tighter = Limits { max_payload_bytes: 100, ..Limits::default() };
    assert_ne!(mini().prog.fingerprint(), mini().prog.with_limits(tighter).fingerprint());
}

#[test]
fn a_structurally_different_program_is_refused() {
    let j = committed_journal(&mini());
    let changed = mini().prog.with_policy(AllowAll); // same name and version, one more policy
    let err = Machine::recover(changed, j).err().expect("must refuse");
    assert!(matches!(err, RecoveryError::Version { .. }), "{err}");
}

#[test]
fn a_pinned_fingerprint_is_journaled_and_checked() {
    let m0 = mini();
    let journal = || {
        let mut m = Machine::new(m0.prog.clone().with_fingerprint(Digest(42)));
        m.inject(sum(&m0, 1, 1, "a")).unwrap();
        m.run(None);
        m.commit().unwrap();
        m.into_journal()
    };
    assert!(Machine::recover(m0.prog.clone().with_fingerprint(Digest(42)), journal()).is_ok());
    let err = Machine::recover(m0.prog.clone().with_fingerprint(Digest(43)), journal()).err().expect("must refuse");
    assert!(matches!(err, RecoveryError::Version { .. }), "{err}");
}

#[test]
fn a_format_1_journal_is_refused() {
    let path = tmp("format1");
    std::fs::write(&path, "{\"t\":\"Header\",\"program\":\"mini\",\"version\":1,\"format\":1}\n{\"t\":\"Commit\"}\n").unwrap();
    let err = Machine::recover(mini().prog, Box::new(FileJournal::open(&path).unwrap())).err().expect("must refuse");
    assert!(matches!(err, RecoveryError::Header(_)), "{err}");
}
```

- [ ] **Step 2: Run the tests and see them fail.**

Run: `cargo test --release --test journal`
Expected: compile errors `no variant named 'Version' found for enum 'Record'` and `no method named 'fingerprint' found for struct 'Program'`.

- [ ] **Step 3: Add the record and the error.** All edits are in `src/journal.rs`.

(a) Replace

```rust
/// Journal format version written in every header.
pub const FORMAT: u32 = 1;
```

with

```rust
/// Journal format version written in every header. Format 2 adds `Version`.
pub const FORMAT: u32 = 2;
```

(b) Replace

```rust
    Header { program: String, version: u32, format: u32 },
```

with

```rust
    Header { program: String, version: u32, format: u32 },
    /// The program that writes the records that follow: its name, version
    /// and fingerprint (`Program::fingerprint`).
    Version { program: String, version: u32, hash: Digest },
```

(c) Replace

```rust
    #[error("journal header mismatch: {0}")]
    Header(String),
```

with

```rust
    #[error("journal header mismatch: {0}")]
    Header(String),
    #[error("journal was written by {journal}; the running program is {program}")]
    Version { journal: String, program: String },
```

- [ ] **Step 4: Fingerprint the signature.** All edits are in `src/sig.rs`.

(a) Replace `use crate::value::{Value, ValueType};` with:

```rust
use crate::id::DigestBuilder;
use crate::value::{Value, ValueType};
```

(b) Inside `impl Signature`, replace

```rust
    pub fn name(&self) -> &str {
        &self.name
    }
```

with

```rust
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Feeds the signature's structure into a program fingerprint.
    pub(crate) fn encode_structure(&self, d: &mut DigestBuilder) {
        d.str(&self.name);
        d.u32(self.version);
        d.u64(self.types.len() as u64);
        for t in &self.types {
            d.str(&t.name);
            d.u8(matches!(t.mode, TypeMode::SelfDual) as u8);
        }
        d.u64(self.kinds.len() as u64);
        for k in &self.kinds {
            d.str(&k.name);
            encode_port(k.principal, d);
            d.u64(k.aux.len() as u64);
            for p in &k.aux {
                d.str(&p.name);
                encode_port(p.ty, d);
            }
            encode_value_type(&k.payload, d);
            d.u8(k.effect as u8);
            match &k.conserved {
                None => d.u8(0),
                Some(c) => {
                    d.u8(1);
                    d.str(&c.class);
                    d.u64(c.key as u64);
                }
            }
        }
    }
```

(c) Replace

```rust
#[cfg(test)]
mod tests {
```

(the test module near the end of `src/sig.rs`) with

```rust
fn encode_port(p: PortType, d: &mut DigestBuilder) {
    d.u32(p.ty.0 as u32);
    d.u8(matches!(p.pol, Polarity::Neg) as u8);
}

fn encode_value_type(t: &ValueType, d: &mut DigestBuilder) {
    match t {
        ValueType::Any => d.u8(0),
        ValueType::Unit => d.u8(1),
        ValueType::Bool => d.u8(2),
        ValueType::Int => d.u8(3),
        ValueType::Str => d.u8(4),
        ValueType::Tuple(xs) => {
            d.u8(5);
            d.u64(xs.len() as u64);
            for x in xs {
                encode_value_type(x, d);
            }
        }
        ValueType::List(x) => {
            d.u8(6);
            encode_value_type(x, d);
        }
    }
}

#[cfg(test)]
mod tests {
```

- [ ] **Step 5: Fingerprint the rule table.** All edits are in `src/rule.rs`.

(a) Replace `use crate::net::{NetBuilder, NetData, Var};` with:

```rust
use crate::id::DigestBuilder;
use crate::net::{NetBuilder, NetData, Var};
```

(b) Inside `impl RuleTable`, replace

```rust
    pub fn errors(&self) -> &[String] {
        &self.errors
    }
```

with

```rust
    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    /// Feeds the table's structure into a program fingerprint: rule names,
    /// kind pairs, modes, effect authority and forbidden pairs. Versions and
    /// rule code are left out, because replay checks them step by step.
    pub(crate) fn encode_structure(&self, d: &mut DigestBuilder) {
        d.u64(self.rules.len() as u64);
        for r in &self.rules {
            d.str(&r.name);
            d.u32(r.left.0 as u32);
            d.u32(r.right.0 as u32);
            d.u8(matches!(r.mode, RuleMode::Recorded) as u8);
            d.u64(r.may_emit.len() as u64);
            for k in &r.may_emit {
                d.u32(k.0 as u32);
            }
        }
        d.u64(self.forbidden.len() as u64);
        for (a, b) in &self.forbidden {
            d.u32(a.0 as u32);
            d.u32(b.0 as u32);
        }
    }
```

- [ ] **Step 6: Fingerprint the program, write `Version`, and check it on recovery.** All edits are in `src/machine.rs`.

(a) Replace `use crate::id::{AgentId, Digest, HoleId};` with `use crate::id::{AgentId, Digest, DigestBuilder, HoleId};`.

(b) Replace

```rust
    pub policies: Arc<Vec<Arc<dyn Policy>>>,
    pub limits: Limits,
}
```

with

```rust
    pub policies: Arc<Vec<Arc<dyn Policy>>>,
    pub limits: Limits,
    /// Set by `with_fingerprint`, for example to a compiled program file's hash.
    fixed_fingerprint: Option<Digest>,
}
```

(c) Replace

```rust
        Ok(Program { sig: Arc::new(sig), rules: Arc::new(rules), policies: Arc::new(vec![]), limits: Limits::default() })
```

with

```rust
        Ok(Program {
            sig: Arc::new(sig),
            rules: Arc::new(rules),
            policies: Arc::new(vec![]),
            limits: Limits::default(),
            fixed_fingerprint: None,
        })
```

(d) Replace

```rust
    pub fn with_limits(mut self, l: Limits) -> Program {
        self.limits = l;
        self
    }
```

with

```rust
    pub fn with_limits(mut self, l: Limits) -> Program {
        self.limits = l;
        self
    }

    /// Pins the fingerprint, for example to a compiled program file's hash.
    pub fn with_fingerprint(mut self, d: Digest) -> Program {
        self.fixed_fingerprint = Some(d);
        self
    }

    /// What journal `Version` records identify this program by. That is the
    /// pinned fingerprint, or else a digest of the signature, the rule table's
    /// structure, the policy names and the limits. Rule versions and rule code
    /// are left out, because replay checks them step by step (`RuleVersion`,
    /// `Divergence`).
    pub fn fingerprint(&self) -> Digest {
        if let Some(d) = self.fixed_fingerprint {
            return d;
        }
        let mut d = DigestBuilder::new("program");
        self.sig.encode_structure(&mut d);
        self.rules.encode_structure(&mut d);
        d.u64(self.policies.len() as u64);
        for p in self.policies.iter() {
            d.str(p.name());
        }
        d.u64(self.limits.max_patch_agents as u64);
        d.u64(self.limits.max_inject_agents as u64);
        d.u64(self.limits.max_payload_bytes as u64);
        d.finish()
    }
```

(e) In `fresh`, replace

```rust
        let header =
            Record::Header { program: m.prog.name().to_string(), version: m.prog.version(), format: FORMAT };
        m.journal.append(header);
        m
    }
```

with

```rust
        let (program, version) = (m.prog.name().to_string(), m.prog.version());
        m.journal.append(Record::Header { program: program.clone(), version, format: FORMAT });
        m.journal.append(Record::Version { program, version, hash: m.prog.fingerprint() });
        m
    }
```

(f) In `recover`, replace

```rust
                Record::Commit | Record::Header { .. } => {}
```

with

```rust
                Record::Version { program, version, hash } => m.check_version(program, *version, *hash)?,
                Record::Commit | Record::Header { .. } => {}
```

(g) Directly above `#[allow(clippy::too_many_arguments)]` (the attribute on `fn replay_reduce`), insert:

```rust
    fn check_version(&self, program: &str, version: u32, hash: Digest) -> Result<(), RecoveryError> {
        let (p, v, h) = (self.prog.name(), self.prog.version(), self.prog.fingerprint());
        if program == p && version == v && hash == h {
            return Ok(());
        }
        Err(RecoveryError::Version { journal: format!("{program} v{version} {hash}"), program: format!("{p} v{v} {h}") })
    }

```

- [ ] **Step 7: Run the journal tests and see them pass.**

Run: `cargo test --release --test journal`
Expected: `test result: ok. 16 passed`. That is 11 existing tests plus 5 new ones, and `replay_refuses_version_change` still reports `RuleVersion`.

- [ ] **Step 8: Document the record.** In `docs/ir-spec.md` §11:
- after the row `| `Header` | program, version, format |`, add the row `| `Version` | program, version, hash |`;
- after the paragraph that starts with `` `append` buffers records.``, add:

  `A journal starts with `Header` (format 2) and then `Version`. Its hash is the program's fingerprint: a pinned hash such as a compiled program file's, or else a digest of the signature, the rule table's structure, the policy names and the limits.`
- replace `**Recovery** replays the committed records in order, using the same code paths with journaling disabled:` with:

  ```markdown
  **Recovery** replays the committed records in order, using the same code paths with journaling disabled:
  - `Version` must match the running program's name, version and fingerprint, or recovery fails with `Version`.
  ```

- [ ] **Step 9: Verify everything.**

Run: `cargo test --release`
Expected: every `test result:` line reports `ok`, with 148 tests in total (130 before this plan, plus 6 from Task 1, 7 from Task 2 and 5 from Task 3).

Run: `cargo clippy --release --all-targets -- -D warnings`
Expected: `Finished` with no warnings.

- [ ] **Step 10: Commit.**

```bash
git add src/journal.rs src/sig.rs src/rule.rs src/machine.rs tests/journal.rs docs/ir-spec.md
git commit -m "feat(kernel): journal the program version and fingerprint

Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
```
