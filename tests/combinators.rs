use suture::nets::combinators::{self, CombKinds};
use suture::*;

fn tree_shape(p: &Program, k: &CombKinds, seed: u64, depth: u32) -> String {
    let mut m = Machine::new(p.clone());
    let mut b = NetBuilder::new(&p.sig);
    let root = combinators::tree(&mut b, k, &mut Rng::new(seed), depth);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(root, Some("t"))).unwrap();
    m.run(None);
    combinators::readback_tree(&m, k, m.hole("t").unwrap()).unwrap()
}

#[test]
fn coverage_is_complete() {
    let (p, _) = combinators::program();
    let c = p.rules.coverage(&p.sig);
    assert!(c.missing.is_empty() && c.never_fires.is_empty(), "{c:?}");
}

#[test]
fn strong_confluence_on_random_nets() {
    let (p, k) = combinators::program();
    let mut checked = 0;
    for seed in 0..400u64 {
        let mut rng = Rng::new(seed);
        let inj = combinators::random_net(&p.sig, &k, &mut rng, 2 + (seed % 24) as usize, (seed % 5) as usize);
        let mut base = Machine::new(p.clone());
        base.inject(inj.clone()).unwrap();
        let RunOutcome::Quiescent { reductions } = base.run(Some(1_000)) else { continue };
        checked += 1;
        base.check_invariants().unwrap();
        for s in [SchedulerKind::Lifo, SchedulerKind::Random(seed ^ 0xABCD)] {
            let mut m = Machine::new(p.clone());
            m.set_scheduler(s);
            m.inject(inj.clone()).unwrap();
            assert_eq!(m.run(Some(1_000)), RunOutcome::Quiescent { reductions }, "seed {seed} {s:?}");
            assert_eq!(m.digest(), base.digest(), "seed {seed} {s:?}");
        }
        let mut r = Machine::new(p.clone());
        r.inject(inj).unwrap();
        r.run_rounds(None);
        assert_eq!(r.digest(), base.digest(), "seed {seed} rounds");
    }
    assert!(checked > 300, "only {checked} of 400 nets normalized");
}

#[test]
fn dup_copies_a_tree() {
    let (p, k) = combinators::program();
    for seed in 0..20 {
        let shape = tree_shape(&p, &k, seed, 5);
        let mut m = Machine::new(p.clone());
        let mut b = NetBuilder::new(&p.sig);
        let root = combinators::tree(&mut b, &k, &mut Rng::new(seed), 5);
        let (o1, o2) = (b.var(), b.var());
        b.agent(k.dup, (), root, &[o1, o2]);
        m.inject(Injection::new(b.finish().unwrap()).new_hole(o1, Some("h1")).new_hole(o2, Some("h2"))).unwrap();
        m.run(None);
        assert_eq!(combinators::readback_tree(&m, &k, m.hole("h1").unwrap()).as_deref(), Some(shape.as_str()));
        assert_eq!(combinators::readback_tree(&m, &k, m.hole("h2").unwrap()).as_deref(), Some(shape.as_str()));
        m.check_invariants().unwrap();
    }
}

#[test]
fn annihilation_of_self_wired_pair_leaves_nothing() {
    let (p, k) = combinators::program();
    let mut m = Machine::new(p.clone());
    let mut b = NetBuilder::new(&p.sig);
    let (pr, x, y) = (b.var(), b.var(), b.var());
    b.agent(k.con, (), pr, &[x, y]);
    b.agent(k.con, (), pr, &[x, y]);
    m.inject(Injection::new(b.finish().unwrap())).unwrap();
    m.run(None);
    assert_eq!(m.graph().agent_count(), 0);
    assert_eq!(m.stats().vicious, 2);
    m.check_invariants().unwrap();
}

#[test]
fn non_terminating_net_is_resumable() {
    // γ and δ facing each other with their auxiliary ports wired pairwise:
    // every commutation recreates two such pairs, so the net grows forever.
    let (p, k) = combinators::program();
    let looping = || {
        let mut b = NetBuilder::new(&p.sig);
        let (pr, x, y) = (b.var(), b.var(), b.var());
        b.agent(k.con, (), pr, &[x, y]);
        b.agent(k.dup, (), pr, &[x, y]);
        Injection::new(b.finish().unwrap())
    };
    let mut m = Machine::new(p.clone());
    m.inject(looping()).unwrap();
    assert_eq!(m.run(Some(5_000)), RunOutcome::Budget { reductions: 5_000 });
    assert_eq!(m.run(Some(5_000)), RunOutcome::Budget { reductions: 5_000 });
    m.check_invariants().unwrap();

    let mut once = Machine::new(p.clone());
    once.inject(looping()).unwrap();
    once.run(Some(200));
    let mut twice = Machine::new(p.clone());
    twice.inject(looping()).unwrap();
    twice.run(Some(100));
    twice.run(Some(100));
    assert_eq!(once.digest(), twice.digest());
}
