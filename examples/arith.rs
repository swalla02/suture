//! Evaluates (3 + 2) * 4 on the interaction kernel and prints the work and
//! parallelism profile.

use suture::nets::arith::{self, Expr};
use suture::{Machine, SchedulerKind};

fn main() {
    let e = Expr::mul(Expr::add(Expr::num(3), Expr::num(2)), Expr::num(4));
    let (prog, k) = arith::program();

    let mut m = Machine::new(prog.clone());
    let h = arith::inject_expr(&mut m, &k, &e, "result").unwrap();
    m.run(None);
    println!("(3 + 2) * 4 = {}", arith::readback(&m, &k, h).unwrap());
    let st = m.stats();
    println!("reductions: {}  agents created: {}  peak agents: {}", st.reductions, st.created, st.max_agents);
    for (rule, n) in &st.per_rule {
        println!("  {rule:<8} {n}");
    }

    let mut r = Machine::new(prog);
    r.set_scheduler(SchedulerKind::Fifo);
    arith::inject_expr(&mut r, &k, &e, "result").unwrap();
    let rounds = r.run_rounds(None);
    println!(
        "rounds (critical path): {}  max parallel width: {}  work/depth: {:.2}",
        rounds.rounds,
        rounds.max_width,
        rounds.reductions as f64 / rounds.rounds as f64
    );
    assert_eq!(r.digest(), m.digest(), "round-based and sequential reduction agree");

    // Raw throughput on a larger term: 60 * 60 (validated, journaled reductions).
    let big = Expr::mul(Expr::num(60), Expr::num(60));
    let (prog, k) = arith::program();
    let mut m = Machine::new(prog);
    let h = arith::inject_expr(&mut m, &k, &big, "result").unwrap();
    let t = std::time::Instant::now();
    m.run(None);
    let dt = t.elapsed();
    assert_eq!(arith::readback(&m, &k, h), Some(3600));
    let n = m.stats().reductions;
    println!("60 * 60: {n} reductions in {dt:.2?} ({:.0} reductions/s)", n as f64 / dt.as_secs_f64());
}
