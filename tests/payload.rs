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
