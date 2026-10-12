use suture::nets::arith::{self, Expr};
use suture::*;

fn eval_on_machine(e: &Expr, sched: SchedulerKind) -> (u64, Digest, u64) {
    let (prog, k) = arith::program();
    let mut m = Machine::new(prog);
    m.set_scheduler(sched);
    let h = arith::inject_expr(&mut m, &k, e, "result").unwrap();
    let RunOutcome::Quiescent { reductions } = m.run(Some(1_000_000)) else { panic!("did not normalize: {e:?}") };
    m.check_invariants().unwrap();
    assert!(m.faults().is_empty());
    (arith::readback(&m, &k, h).expect("result is a numeral"), m.digest(), reductions)
}

#[test]
fn coverage_is_complete() {
    let (p, _) = arith::program();
    let c = p.rules.coverage(&p.sig);
    assert!(c.missing.is_empty(), "{:?}", c.missing);
    assert!(c.never_fires.is_empty(), "{:?}", c.never_fires);
}

#[test]
fn small_cases() {
    use Expr as E;
    let cases = [
        (E::mul(E::num(3), E::num(4)), 12),
        (E::add(E::num(0), E::num(0)), 0),
        (E::mul(E::num(0), E::num(5)), 0),
        (E::mul(E::num(5), E::num(0)), 0),
        (E::mul(E::add(E::num(2), E::num(1)), E::num(3)), 9),
        (E::mul(E::mul(E::num(2), E::num(2)), E::mul(E::num(3), E::num(1))), 12),
    ];
    for (e, v) in cases {
        assert_eq!(e.eval(), v);
        assert_eq!(eval_on_machine(&e, SchedulerKind::Fifo).0, v, "{e:?}");
    }
}

#[test]
fn random_expressions_match_rust_under_all_schedulers() {
    for seed in 0..200 {
        let mut rng = Rng::new(seed);
        let e = Expr::random(&mut rng, 3, 4);
        let fifo = eval_on_machine(&e, SchedulerKind::Fifo);
        assert_eq!(fifo.0, e.eval(), "seed {seed}: {e:?}");
        for s in [SchedulerKind::Lifo, SchedulerKind::Random(seed)] {
            assert_eq!(eval_on_machine(&e, s), fifo, "seed {seed} {s:?}");
        }
    }
}

#[test]
fn boundary_determinism_inject_interleaving() {
    use Expr as E;
    let es = [E::mul(E::num(2), E::num(3)), E::add(E::num(4), E::num(1)), E::mul(E::num(3), E::add(E::num(1), E::num(1)))];
    let (prog, k) = arith::program();
    let mut a = Machine::new(prog.clone());
    for (i, e) in es.iter().enumerate() {
        arith::inject_expr(&mut a, &k, e, &format!("r{i}")).unwrap();
        a.run(None);
    }
    let mut b = Machine::new(prog);
    b.set_scheduler(SchedulerKind::Lifo);
    for (i, e) in es.iter().enumerate() {
        arith::inject_expr(&mut b, &k, e, &format!("r{i}")).unwrap();
        b.run(Some(2)); // partial progress between injects
    }
    b.run(None);
    assert_eq!(a.digest(), b.digest());
}

#[test]
fn graph_is_clean_after_normalization() {
    use Expr as E;
    let e = E::mul(E::add(E::num(2), E::num(2)), E::num(3));
    let (prog, k) = arith::program();
    let mut m = Machine::new(prog);
    let h = arith::inject_expr(&mut m, &k, &e, "result").unwrap();
    m.run(None);
    assert_eq!(arith::readback(&m, &k, h), Some(12));
    assert_eq!(m.graph().agent_count(), 13, "only S^12(Z) remains");
    assert_eq!(m.graph().hole_count(), 1);
}
