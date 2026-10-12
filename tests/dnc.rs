//! The decision layer on a minimal program: `Start >< Go` emits a `Pick`
//! decision site offering alternatives [10, 20, 30]; the resolver answers
//! with a `Decided` token; `Decided >< Pick` enacts the choice as `Result`.

use std::sync::Arc;

use suture::dnc::{
    decided_schema, selected_index, DecisionBackend, DecisionError, DecisionRequest, DecisionResult, FirstBackend, Law,
    LinearBackend, NoisyBackend, RandomBackend, Resolver,
};
use suture::*;

struct Toy {
    prog: Program,
    start: KindId,
    go: KindId,
    result: KindId,
}

fn toy(alternatives: Vec<i64>) -> Toy {
    let mut b = SigBuilder::new("toy-decisions", 1);
    let ping = b.polarized("Ping");
    let dec = b.polarized("Dec");
    let out = b.polarized("Out");
    let start = b.kind("Start", ping.pos()).done();
    let go = b.kind("Go", ping.neg()).aux("out", out.pos()).done();
    let pick = b
        .kind("Pick", dec.neg())
        .aux("out", out.pos())
        .payload(ValueType::List(Box::new(ValueType::Int)))
        .effect()
        .done();
    let decided = b.kind("Decided", dec.pos()).payload(decided_schema()).done();
    let result = b.kind("Result", out.pos()).payload(ValueType::Int).done();
    let mut t = RuleTable::new();
    let alts = Value::list(alternatives.iter().map(|&a| Value::Int(a)));
    t.def("start_go", start, go, move |cx| {
        let [o] = cx.right_aux();
        cx.effect(pick, alts.clone(), &[o]);
        Ok(())
    })
    .emits(&[pick]);
    t.def("decided_pick", decided, pick, move |cx| {
        let [o] = cx.right_aux();
        let i = selected_index(cx.left())?;
        let v = cx.right().items().and_then(|xs| xs.get(i)).cloned().ok_or_else(|| cx.reject("selection out of range"))?;
        cx.agent(result, v, o, &[]);
        Ok(())
    });
    let prog = Program::new(b.build().unwrap(), t).unwrap();
    Toy { prog, start, go, result }
}

fn pick_law(t: &Toy) -> Law {
    let site = t.prog.sig.kind_id("Pick").unwrap();
    let token = t.prog.sig.kind_id("Decided").unwrap();
    Law::new(
        "pick",
        site,
        token,
        Arc::new(|payload: &Value| {
            let xs = payload.items().ok_or("not a list")?;
            Ok(xs.iter().map(|x| vec![x.as_int().unwrap_or(0) as f64]).collect())
        }),
    )
}

fn start(t: &Toy, m: &mut Machine, label: &str) {
    let mut b = NetBuilder::new(&t.prog.sig);
    let (p, o) = (b.var(), b.var());
    b.agent(t.start, (), p, &[]);
    b.agent(t.go, (), p, &[o]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(o, Some(label))).unwrap();
}

fn result_at(t: &Toy, m: &Machine, label: &str) -> Option<i64> {
    let (_, _, a) = m.hole_peer(m.hole(label)?)?;
    (a.kind == t.result).then(|| a.payload.as_int()).flatten()
}

fn run_one(t: &Toy, backend: &mut dyn DecisionBackend) -> (Machine, Resolver) {
    let mut m = Machine::new(t.prog.clone());
    start(t, &mut m, "out");
    m.run(None);
    let mut r = Resolver::new(vec![pick_law(t)]);
    r.resolve(&mut m, backend).unwrap();
    m.run(None);
    (m, r)
}

#[test]
fn first_backend_picks_the_first_alternative() {
    let t = toy(vec![10, 20, 30]);
    let (m, r) = run_one(&t, &mut FirstBackend);
    assert_eq!(result_at(&t, &m, "out"), Some(10));
    assert_eq!((r.stats.sites, r.stats.decisions, r.stats.calls), (1, 1, 1));
    m.check_invariants().unwrap();
}

#[test]
fn linear_backend_picks_the_best_scored_alternative() {
    let t = toy(vec![10, 30, 20]);
    let (m, _) = run_one(&t, &mut LinearBackend::new("lin").with("pick", vec![1.0]));
    assert_eq!(result_at(&t, &m, "out"), Some(30));
    let (m, _) = run_one(&t, &mut LinearBackend::new("lin").with("pick", vec![-1.0]));
    assert_eq!(result_at(&t, &m, "out"), Some(10));
}

#[test]
fn decisions_are_journaled_and_replay_without_the_model() {
    let t = toy(vec![10, 20, 30, 40, 50]);
    let mut m = Machine::new(t.prog.clone());
    for i in 0..8 {
        start(&t, &mut m, &format!("o{i}"));
    }
    m.run(None);
    let mut r = Resolver::new(vec![pick_law(&t)]);
    r.resolve(&mut m, &mut RandomBackend::new(7)).unwrap();
    m.run(None);
    m.commit().unwrap();
    let chosen: Vec<_> = (0..8).map(|i| result_at(&t, &m, &format!("o{i}"))).collect();
    assert!(chosen.iter().all(Option::is_some));
    let d = m.digest();
    // Recovery takes no backend at all: the recorded tokens are re-applied.
    let rec = Machine::recover(t.prog.clone(), m.into_journal()).unwrap();
    assert_eq!(rec.digest(), d);
    let again: Vec<_> = (0..8).map(|i| result_at(&t, &rec, &format!("o{i}"))).collect();
    assert_eq!(again, chosen);
}

struct Fixed(usize, Option<Vec<f64>>);
impl DecisionBackend for Fixed {
    fn model(&self) -> String {
        "fixed".into()
    }
    fn decide(&mut self, batch: &[DecisionRequest]) -> Result<Vec<DecisionResult>, DecisionError> {
        Ok(batch
            .iter()
            .map(|_| DecisionResult { selected: self.0, scores: self.1.clone(), model: self.model(), trace: String::new() })
            .collect())
    }
}

struct Down;
impl DecisionBackend for Down {
    fn model(&self) -> String {
        "down".into()
    }
    fn decide(&mut self, _: &[DecisionRequest]) -> Result<Vec<DecisionResult>, DecisionError> {
        Err(DecisionError::Unavailable("service down".into()))
    }
}

#[test]
fn invalid_selections_fall_back() {
    let t = toy(vec![10, 20, 30]);
    let (m, r) = run_one(&t, &mut Fixed(99, None));
    assert_eq!(result_at(&t, &m, "out"), Some(10));
    assert_eq!(r.stats.invalid, 1);
}

#[test]
fn unavailable_backend_falls_back() {
    let t = toy(vec![10, 20, 30]);
    let (m, r) = run_one(&t, &mut Down);
    assert_eq!(result_at(&t, &m, "out"), Some(10));
    assert_eq!(r.stats.fallback, 1);
}

#[test]
fn low_confidence_falls_back() {
    let t = toy(vec![10, 20, 30]);
    let mut m = Machine::new(t.prog.clone());
    start(&t, &mut m, "out");
    m.run(None);
    let mut r = Resolver::new(vec![pick_law(&t)]).with_min_margin(0.5);
    r.resolve(&mut m, &mut Fixed(2, Some(vec![0.1, 0.2, 0.3]))).unwrap();
    m.run(None);
    assert_eq!(result_at(&t, &m, "out"), Some(10));
    assert_eq!(r.stats.low_confidence, 1);
}

struct Counting(usize);
impl DecisionBackend for Counting {
    fn model(&self) -> String {
        "counting".into()
    }
    fn decide(&mut self, batch: &[DecisionRequest]) -> Result<Vec<DecisionResult>, DecisionError> {
        self.0 += 1;
        FirstBackend.decide(batch)
    }
}

#[test]
fn trivial_sites_skip_the_model_and_sites_are_batched() {
    let t = toy(vec![42]);
    let mut c = Counting(0);
    let (m, r) = run_one(&t, &mut c);
    assert_eq!(result_at(&t, &m, "out"), Some(42));
    assert_eq!((c.0, r.stats.trivial, r.stats.calls), (0, 1, 0));

    let t = toy(vec![1, 2]);
    let mut m = Machine::new(t.prog.clone());
    for i in 0..5 {
        start(&t, &mut m, &format!("o{i}"));
    }
    m.run(None);
    let mut c = Counting(0);
    let mut r = Resolver::new(vec![pick_law(&t)]);
    assert_eq!(r.resolve(&mut m, &mut c).unwrap(), 5);
    assert_eq!((c.0, r.stats.calls, r.stats.decisions), (1, 1, 5), "five sites, one batched call");
}

#[test]
fn noisy_backend_interpolates_between_inner_and_random() {
    let t = toy(vec![10, 20, 30]);
    let (m, _) = run_one(&t, &mut NoisyBackend::new(LinearBackend::new("lin").with("pick", vec![1.0]), 0.0, 1));
    assert_eq!(result_at(&t, &m, "out"), Some(30));
    let mut seen = std::collections::BTreeSet::new();
    for seed in 0..40 {
        let (m, _) = run_one(&t, &mut NoisyBackend::new(LinearBackend::new("lin").with("pick", vec![1.0]), 1.0, seed));
        seen.insert(result_at(&t, &m, "out").unwrap());
    }
    assert_eq!(seen.len(), 3, "with eps = 1 every alternative occurs");
}
