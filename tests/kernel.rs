mod common;

use common::*;
use suture::*;

fn single(m: &Mini, kind: KindId, payload: Value, hole: Bind) -> Injection {
    let mut nb = NetBuilder::new(&m.prog.sig);
    let v = nb.var();
    nb.agent(kind, payload, v, &[]);
    Injection::new(nb.finish().unwrap()).bind(v, hole)
}

#[test]
fn adds_two_and_one() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    let rc = m.inject(sum(&m0, 2, 1, "out")).unwrap();
    assert_eq!(rc.seq, 0);
    assert!(!rc.duplicate);
    assert!(matches!(m.run(None), RunOutcome::Quiescent { reductions: 3 }));
    assert_eq!(read_nat(&m, &m0, m.hole("out").unwrap()), Some(3));
    m.check_invariants().unwrap();
    assert!(m.faults().is_empty());
    assert_eq!(m.stats().reductions, 3);
    assert_eq!(m.stats().per_rule.get("add_s"), Some(&2));
}

#[test]
fn inject_errors_leave_graph_unchanged() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(single(&m0, m0.z, Value::Unit, Bind::New(Some("z".into())))).unwrap();
    let before = m.digest();

    let mut b = NetBuilder::new(&m0.prog.sig);
    let v = b.var();
    b.agent(m0.add, (), v, &[]);
    assert!(b.finish().is_err(), "arity misuse is caught by the builder");

    let mut b = NetBuilder::new(&m0.prog.sig);
    let (v, y, r) = (b.var(), b.var(), b.var());
    b.agent(m0.add, (), v, &[y, r]);
    let net = b.finish().unwrap();
    let res = m.inject(Injection::new(net.clone()).at_label(v, "nope").new_hole(y, None).new_hole(r, None));
    assert!(matches!(res, Err(InjectError::UnknownHole(_))), "{res:?}");
    let res = m.inject(Injection::new(net.clone()).at_label(v, "z").at_label(y, "z").new_hole(r, None));
    assert!(matches!(res, Err(InjectError::HoleBoundTwice(_))), "{res:?}");
    let res = m.inject(Injection::new(net.clone()).new_hole(v, Some("z")).new_hole(y, None).new_hole(r, None));
    assert!(matches!(res, Err(InjectError::LabelInUse(_))), "{res:?}");
    let res = m.inject(Injection::new(net.clone()).new_hole(v, Some("w")).new_hole(y, Some("w")).new_hole(r, None));
    assert!(matches!(res, Err(InjectError::LabelInUse(_))), "{res:?}");
    let res = m.inject(single(&m0, m0.z, Value::Unit, Bind::Hole(HoleRef::Label("z".into()))));
    assert!(matches!(res, Err(InjectError::Type { .. })), "Nat+ onto Nat+: {res:?}");
    let res = m.inject(single(&m0, m0.result, Value::from("not an int"), Bind::New(None)));
    assert!(matches!(res, Err(InjectError::Net(NetError::Payload { .. }))), "{res:?}");
    let res = m.inject(single(&m0, m0.lease, tup![1], Bind::New(None)));
    assert!(matches!(res, Err(InjectError::Net(NetError::MintRequired(_)))), "{res:?}");
    let mut b = NetBuilder::new(&m0.prog.sig);
    let v = b.var();
    b.agent(m0.z, (), v, &[]);
    let res = m.inject(Injection::new(b.finish().unwrap())); // open variable never bound
    assert!(matches!(res, Err(InjectError::Net(NetError::VarUses { .. }))), "{res:?}");

    assert_eq!(m.digest(), before);
    assert_eq!(m.stats().injects, 1);
    assert_eq!(m.inject(sum(&m0, 0, 0, "ok")).unwrap().seq, 1, "failed injects consume no sequence number");
}

#[test]
fn label_transfer_within_one_inject() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    let mut b = NetBuilder::new(&m0.prog.sig);
    let (x, y, r) = (b.var(), b.var(), b.var());
    b.agent(m0.add, (), x, &[y, r]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(x, Some("tail")).new_hole(y, Some("y")).new_hole(r, Some("r")))
        .unwrap();
    let first_tail = m.hole("tail").unwrap();
    // Feed S onto the tail and expose its predecessor as the new tail.
    let mut b = NetBuilder::new(&m0.prog.sig);
    let (top, pred) = (b.var(), b.var());
    b.agent(m0.s, (), top, &[pred]);
    m.inject(Injection::new(b.finish().unwrap()).at_label(top, "tail").new_hole(pred, Some("tail"))).unwrap();
    let second_tail = m.hole("tail").unwrap();
    assert_ne!(first_tail, second_tail);
    m.inject(single(&m0, m0.z, Value::Unit, Bind::Hole(HoleRef::Label("tail".into())))).unwrap();
    m.inject(single(&m0, m0.z, Value::Unit, Bind::Hole(HoleRef::Label("y".into())))).unwrap();
    m.run(None);
    assert_eq!(read_nat(&m, &m0, m.hole("r").unwrap()), Some(1));
    assert!(m.hole("tail").is_none());
    m.check_invariants().unwrap();
}

#[test]
fn effects_roundtrip() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(start_go(&m0, "out")).unwrap();
    m.run(None);
    let ob = m.outbox();
    assert_eq!(ob.len(), 1);
    assert_eq!((ob[0].kind_name.as_str(), ob[0].payload.clone()), ("Ask", Value::Int(7)));
    m.inject(single(&m0, m0.reply, Value::Int(5), Bind::Hole(HoleRef::Id(ob[0].hole)))).unwrap();
    m.run(None);
    assert!(m.outbox().is_empty());
    let (_, port, ag) = m.hole_peer(m.hole("out").unwrap()).unwrap();
    assert_eq!((port, ag.kind, ag.payload.clone()), (0, m0.result, Value::Int(12)));
    assert_eq!(m.stats().effects, 1);
    m.check_invariants().unwrap();
}

#[test]
fn missing_rule_is_local_fault() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    let mut b = NetBuilder::new(&m0.prog.sig);
    let (p, y, r) = (b.var(), b.var(), b.var());
    b.agent(m0.bogus, (), p, &[]);
    b.agent(m0.add, (), p, &[y, r]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(y, None).new_hole(r, None)).unwrap();
    m.inject(sum(&m0, 1, 1, "out")).unwrap();
    m.run(None);
    let faults = m.faults();
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].rule, None);
    assert!(faults[0].error.contains("no rule"), "{}", faults[0].error);
    assert_eq!(read_nat(&m, &m0, m.hole("out").unwrap()), Some(2));
    assert!(m.is_quiescent());
    m.check_invariants().unwrap();
}

#[test]
fn rule_panic_becomes_fault() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    let mut b = NetBuilder::new(&m0.prog.sig);
    let (p, o) = (b.var(), b.var());
    b.agent(m0.boom, (), p, &[]);
    b.agent(m0.go, (), p, &[o]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(o, None)).unwrap();
    m.run(None);
    let faults = m.faults();
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].rule.as_deref(), Some("boom_go"));
    assert!(faults[0].error.contains("panic") && faults[0].error.contains("boom"), "{}", faults[0].error);
    m.inject(sum(&m0, 1, 1, "out")).unwrap();
    m.run(None);
    assert_eq!(read_nat(&m, &m0, m.hole("out").unwrap()), Some(2));
    m.check_invariants().unwrap();
}

#[test]
fn rule_reject_becomes_fault() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    let mut b = NetBuilder::new(&m0.prog.sig);
    let (p, o) = (b.var(), b.var());
    b.agent(m0.nope, (), p, &[]);
    b.agent(m0.go, (), p, &[o]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(o, None)).unwrap();
    m.run(None);
    let faults = m.faults();
    assert_eq!(faults.len(), 1);
    assert!(faults[0].error.contains("no"), "{}", faults[0].error);
}

#[test]
fn idempotency_key_dedupes() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    let first = m.inject(sum(&m0, 1, 2, "out").key("k1")).unwrap();
    let d = m.digest();
    let again = m.inject(sum(&m0, 1, 2, "out").key("k1")).unwrap();
    assert!(again.duplicate);
    assert_eq!(again.seq, first.seq);
    assert_eq!(m.digest(), d);
    assert_eq!(m.stats().injects, 1);
}

fn many_sums(m0: &Mini, sched: SchedulerKind) -> (Digest, RunOutcome) {
    let mut m = Machine::new(m0.prog.clone());
    m.set_scheduler(sched);
    for i in 0..20u64 {
        m.inject(sum(m0, i % 5, i % 3, &format!("o{i}"))).unwrap();
    }
    let mut b = NetBuilder::new(&m0.prog.sig);
    let (a, c) = (num(&mut b, m0, 2), num(&mut b, m0, 1));
    let r1 = b.var();
    b.agent(m0.add, (), a, &[c, r1]);
    let (d, r) = (num(&mut b, m0, 3), b.var());
    b.agent(m0.add, (), r1, &[d, r]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(r, Some("nested"))).unwrap();
    let out = m.run(None);
    assert_eq!(read_nat(&m, m0, m.hole("nested").unwrap()), Some(6));
    m.check_invariants().unwrap();
    (m.digest(), out)
}

#[test]
fn schedulers_agree_on_digest() {
    let m0 = mini();
    let base = many_sums(&m0, SchedulerKind::Fifo);
    for s in [SchedulerKind::Lifo, SchedulerKind::Random(1), SchedulerKind::Random(2), SchedulerKind::Random(3)] {
        assert_eq!(many_sums(&m0, s), base, "{s:?}");
    }
}

#[test]
fn budget_stops_and_resumes() {
    let m0 = mini();
    let mut straight = Machine::new(m0.prog.clone());
    straight.inject(sum(&m0, 2, 3, "out")).unwrap();
    straight.run(None);
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 2, 3, "out")).unwrap();
    assert_eq!(m.run(Some(1)), RunOutcome::Budget { reductions: 1 });
    assert!(!m.is_quiescent());
    m.check_invariants().unwrap();
    assert_eq!(m.run(None), RunOutcome::Quiescent { reductions: 2 });
    assert_eq!(m.digest(), straight.digest());
}

#[test]
fn rounds_report_width() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    for i in 0..20u64 {
        m.inject(sum(&m0, i % 5, i % 3, &format!("o{i}"))).unwrap();
    }
    let mut fifo = Machine::new(m0.prog.clone());
    for i in 0..20u64 {
        fifo.inject(sum(&m0, i % 5, i % 3, &format!("o{i}"))).unwrap();
    }
    let rounds = m.run_rounds(None);
    fifo.run(None);
    assert_eq!(rounds.max_width, 20);
    assert_eq!(rounds.rounds, 5, "the deepest sum (4 + _) needs five sequential steps");
    assert_eq!(rounds.reductions, fifo.stats().reductions);
    assert_eq!(m.digest(), fifo.digest());
}

#[test]
fn policy_violation_is_fault() {
    let m0 = mini();
    let prog = m0.prog.clone().with_policy(NoResultAbove(10, m0.result));
    let mut m = Machine::new(prog);
    m.inject(start_go(&m0, "out")).unwrap();
    m.run(None);
    let hole = m.outbox()[0].hole;
    m.inject(single(&m0, m0.reply, Value::Int(50), Bind::Hole(HoleRef::Id(hole)))).unwrap();
    m.run(None);
    let faults = m.faults();
    assert_eq!(faults.len(), 1);
    assert!(faults[0].error.contains("no-result-above"), "{}", faults[0].error);
    assert!(m.graph().agents_sorted().iter().all(|(_, a)| a.kind != m0.result));
    m.check_invariants().unwrap();
}

#[test]
fn conserved_census() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(single(&m0, m0.lease, tup![3], Bind::New(None)).mint()).unwrap();
    assert_eq!(m.census("lease"), vec![Value::Int(3)]);
    assert!(m.census("other").is_empty());
}
