//! Experiment A: ten workspace requests compete for three database leases.
//! Prints the measurements reported in docs/experiments.md.

use std::collections::BTreeMap;

use suture::nets::lease::{self, simulate, LeaseConfig, Outcome, SimConfig};
use suture::Machine;

fn main() {
    // 1. Aggregate over many randomized schedules (no crashes).
    let seeds = 300u64;
    let (mut grants, mut reductions, mut injects, mut failed, mut peak) = (0usize, 0u64, 0u64, 0usize, 0usize);
    let mut by_type: BTreeMap<String, u64> = BTreeMap::new();
    let mut per_rule: BTreeMap<String, u64> = BTreeMap::new();
    for seed in 0..seeds {
        let r = simulate(&SimConfig::standard(seed)).expect("invariants hold");
        grants += r.grants.len();
        reductions += r.stats.reductions;
        injects += r.injects;
        failed += r.outcomes.values().filter(|o| matches!(o, Outcome::Failed { .. })).count();
        peak = peak.max(r.stats.max_agents);
        for inj in &r.boundary_log {
            let kind = inj.key.as_deref().unwrap_or("?").split(':').next().unwrap().to_string();
            *by_type.entry(kind).or_default() += 1;
        }
        for (rule, n) in &r.stats.per_rule {
            *per_rule.entry(rule.clone()).or_default() += n;
        }
    }
    println!("== Experiment A: {seeds} randomized schedules, 10 requests x 3 leases ==");
    println!("grants (lease cycles):      {grants}  ({:.2} per run)", grants as f64 / seeds as f64);
    println!("requests failed after 3 attempts: {failed}");
    println!("reductions per lease cycle: {:.1}", reductions as f64 / grants as f64);
    println!("host injects per lease cycle: {:.2}", injects as f64 / grants as f64);
    for (kind, n) in &by_type {
        println!("  {kind:<10} {:>6}  ({:.2} per cycle)", n, *n as f64 / grants as f64);
    }
    println!("peak agents in any run: {peak}");
    println!("reductions by rule (per lease cycle):");
    for (rule, n) in &per_rule {
        println!("  {rule:<18} {:>6.2}", *n as f64 / grants as f64);
    }

    // 2. Crashes and duplicate deliveries.
    let (mut crashes, mut dups) = (0, 0);
    for seed in 0..150 {
        let mut c = SimConfig::standard(seed);
        c.crash_rate = 0.05;
        c.dup_rate = 0.05;
        c.commit_rate = 0.5;
        let r = simulate(&c).expect("invariants hold under crashes");
        crashes += r.crashes;
        dups += r.duplicates_ignored;
    }
    println!("\n== crash runs: 150 seeds, {crashes} crashes recovered, {dups} duplicate deliveries ignored ==");

    // 3. Parallel profile of admission: ten demands arrive at once.
    let (prog, k) = lease::program(&LeaseConfig { max_attempts: 3, ..LeaseConfig::default() });
    let cfg = SimConfig::standard(0);
    let mut m = Machine::new(prog);
    lease::init_pool(&mut m, &k, &cfg.leases).unwrap();
    for r in &cfg.requests {
        lease::admit(&mut m, &k, r).unwrap();
    }
    let rounds = m.run_rounds(None);
    println!(
        "\n== admission of 10 simultaneous demands: {} reductions in {} rounds (max width {}) ==",
        rounds.reductions, rounds.rounds, rounds.max_width
    );
    println!("{}", m.render_explanation());
    let path = std::path::Path::new("target").join("lease_admitted.dot");
    std::fs::write(&path, m.to_dot()).unwrap();
    println!("graph written to {}", path.display());
}
