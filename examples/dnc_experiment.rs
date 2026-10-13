//! Experiment DNC-1: can a shared, learned, local decision law help a
//! dependency network assemble itself and recover from disturbances, on
//! configurations it has never seen? Prints the tables of
//! docs/dnc-experiment-1.md (release build recommended).

use std::time::Instant;

use suture::dnc::{DecisionBackend, FirstBackend, NoisyBackend, RandomBackend};
use suture::nets::depnet::planner::{plan, PlanEnv};
use suture::nets::depnet::state::Ctl;
use suture::nets::depnet::train::{law, score, train, TrainConfig};
use suture::nets::depnet::world::{generate, GenParams, RootSpec, WorldSpec};
use suture::nets::depnet::{greedy, DepConfig, Depnet, RunConfig, BIND_FEATURES, RECIPE_FEATURES};
use suture::Rng;

type Factory<'a> = Box<dyn Fn(u64) -> Box<dyn DecisionBackend> + 'a>;

struct Suite {
    name: &'static str,
    worlds: Vec<WorldSpec>,
    seed0: u64,
}

fn suite(name: &'static str, p: GenParams, seed0: u64, n: u64) -> Suite {
    Suite { name, worlds: (0..n).map(|i| generate(&p, seed0 + i)).collect(), seed0 }
}

#[derive(Default, Clone)]
struct Run {
    frac: f64,
    decisions: u64,
    calls: u64,
    trivial: u64,
    reductions: u64,
    rounds: usize,
    violations: usize,
    exhausted: bool,
}

fn run_world(w: &WorldSpec, backend: &mut dyn DecisionBackend, rc: &RunConfig) -> (Depnet, Run) {
    let mut d = Depnet::new(w, &DepConfig::default()).unwrap();
    let st = d.run(backend, rc);
    let e = d.evaluate();
    let r = Run {
        frac: e.fraction(),
        decisions: d.resolver.stats.decisions,
        calls: d.resolver.stats.calls,
        trivial: d.resolver.stats.trivial,
        reductions: d.m.stats().reductions,
        rounds: st.rounds,
        violations: e.violations.len(),
        exhausted: st.budget_exhausted,
    };
    (d, r)
}

fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len().max(1) as f64
}

fn se(xs: &[f64]) -> f64 {
    if xs.len() < 2 {
        return 0.0;
    }
    let m = mean(xs);
    (xs.iter().map(|x| (x - m).powi(2)).sum::<f64>() / (xs.len() - 1) as f64).sqrt() / (xs.len() as f64).sqrt()
}

fn pct(x: f64) -> String {
    format!("{:.1}%", 100.0 * x)
}

/// A deterministic disturbance: remove 20% of providers, cut capacity of
/// another 10%, add two new roots.
fn disturbance(w: &WorldSpec, seed: u64) -> (Vec<(i64, i64)>, Vec<(i64, i64, i64)>, Vec<RootSpec>) {
    let mut rng = Rng::new(seed ^ 0xd157_0b);
    let mut ids: Vec<usize> = (0..w.providers.len()).collect();
    rng.shuffle(&mut ids);
    let nk = (w.providers.len() / 5).max(1);
    let nc = (w.providers.len() / 10).max(1);
    let kills = ids[..nk].iter().map(|&i| (w.providers[i].ty, w.providers[i].id)).collect();
    let caps = ids[nk..nk + nc]
        .iter()
        .map(|&i| (w.providers[i].ty, w.providers[i].id, (w.providers[i].capacity - 1).max(0)))
        .collect();
    let roots = (0..2)
        .map(|j| RootSpec {
            id: 900 + j,
            needs: vec![w.layers[0][rng.index(w.layers[0].len())]],
            regions: vec![rng.below(w.regions as u64) as i64],
        })
        .collect();
    (kills, caps, roots)
}

fn main() {
    let t0 = Instant::now();
    let dep = DepConfig::default();
    let train_worlds: Vec<WorldSpec> = (0..32).map(|i| generate(&GenParams::contended(), 100 + i)).collect();
    let tc = TrainConfig::default();
    let scratch = train(&train_worlds, &dep, &tc);
    let greedy_theta: Vec<f64> = ["bind", "recipe"].iter().flat_map(|l| greedy().weights(l).unwrap().to_vec()).collect();
    let refined = train(&train_worlds, &dep, &TrainConfig { init: Some(greedy_theta.clone()), init_std: 0.5, seed: 8, ..tc.clone() });
    println!("## Training\n");
    println!(
        "Cross-entropy method, {} iterations x {} candidates, on {} small contended worlds: {} machine runs per training, {:.1}s for both.\n",
        tc.iterations,
        tc.population,
        train_worlds.len(),
        scratch.runs,
        t0.elapsed().as_secs_f64()
    );
    println!("Score on the training worlds (the objective):\n");
    println!("| law | training score |\n| --- | --- |");
    let on_train = |make: &dyn Fn(u64) -> Box<dyn DecisionBackend>| score(&train_worlds, &dep, tc.seed, make);
    println!("| list order (fixed) | {} |", pct(on_train(&|_| Box::new(FirstBackend))));
    println!("| random | {} |", pct(on_train(&|s| Box::new(RandomBackend::new(s)))));
    println!("| greedy (handcrafted) | {} |", pct(on_train(&|_| Box::new(greedy()))));
    println!("| learned from scratch | {} |", pct(scratch.fitness));
    println!("| learned, refined from greedy | {} |", pct(refined.fitness));
    println!("\n| iteration | from scratch: best / elite mean | refined: best / elite mean |\n| --- | --- | --- |");
    for (i, ((b1, m1), (b2, m2))) in scratch.history.iter().zip(&refined.history).enumerate() {
        if i % 4 == 0 || i + 1 == scratch.history.len() {
            println!("| {} | {} / {} | {} / {} |", i + 1, pct(*b1), pct(*m1), pct(*b2), pct(*m2));
        }
    }
    println!("\nWeights (bind law, then recipe law):\n");
    println!("| feature | greedy | learned from scratch | refined from greedy |\n| --- | --- | --- | --- |");
    let names = BIND_FEATURES.iter().map(|f| format!("bind.{f}")).chain(RECIPE_FEATURES.iter().map(|f| format!("recipe.{f}")));
    for (i, name) in names.enumerate() {
        println!("| {name} | {:+.2} | {:+.2} | {:+.2} |", greedy_theta[i], scratch.theta[i], refined.theta[i]);
    }
    let (theta, theta_r) = (scratch.theta.clone(), refined.theta.clone());

    let suites = vec![
        suite("contended (held-out, training distribution)", GenParams::contended(), 1000, 40),
        suite("contended-large (3x roots, 3 regions, more types)", GenParams::contended_large(), 2000, 30),
        suite("contended-deep (one more dependency layer)", GenParams::contended_deep(), 3000, 30),
        suite("contended-scarce (fewer leaves, more roots)", GenParams::contended_scarce(), 4000, 30),
        suite("loose (plentiful leaves, optional deps)", GenParams::loose(), 5000, 30),
    ];
    let policies: Vec<(&str, Factory)> = vec![
        ("list order (fixed)", Box::new(|_| Box::new(FirstBackend))),
        ("random", Box::new(|s| Box::new(RandomBackend::new(s)))),
        ("greedy (handcrafted)", Box::new(|_| Box::new(greedy()))),
        ("learned (scratch)", Box::new(|_| Box::new(law("learned", &theta)))),
        ("learned (refined)", Box::new(|_| Box::new(law("refined", &theta_r)))),
    ];

    println!("\n## Assembly: satisfied roots by policy\n");
    println!("Mean fraction of roots functionally satisfied at quiescence (± standard error). The planner has global knowledge and searches; it is a reference, not a local policy.\n");
    print!("| suite | planner |");
    for (p, _) in &policies {
        print!(" {p} |");
    }
    println!(" refined − greedy (paired) |\n| --- | --- |{}--- |", " --- |".repeat(policies.len()));
    let mut cost_rows = vec![];
    for s in &suites {
        let pl: Vec<f64> = s.worlds.iter().enumerate().map(|(i, w)| plan(w, &PlanEnv::default(), 50_000, 8, s.seed0 + i as u64).fraction()).collect();
        print!("| {} | {} |", s.name, pct(mean(&pl)));
        let mut per_policy = vec![];
        for (pname, make) in &policies {
            let t = Instant::now();
            let runs: Vec<Run> = s
                .worlds
                .iter()
                .enumerate()
                .map(|(i, w)| run_world(w, make(s.seed0 + i as u64).as_mut(), &RunConfig { seed: s.seed0 + i as u64, ..RunConfig::default() }).1)
                .collect();
            let fr: Vec<f64> = runs.iter().map(|r| r.frac).collect();
            assert!(runs.iter().all(|r| r.violations == 0 && !r.exhausted), "{} {}", s.name, pname);
            print!(" {} ± {} |", pct(mean(&fr)), pct(se(&fr)));
            if pname.starts_with("learned (refined") {
                let n = runs.len() as f64;
                cost_rows.push((
                    s.name,
                    runs.iter().map(|r| r.decisions).sum::<u64>() as f64 / n,
                    runs.iter().map(|r| r.calls).sum::<u64>() as f64 / n,
                    runs.iter().map(|r| r.trivial).sum::<u64>() as f64 / n,
                    runs.iter().map(|r| r.reductions).sum::<u64>() as f64 / n,
                    runs.iter().map(|r| r.rounds).sum::<usize>() as f64 / n,
                    t.elapsed().as_secs_f64() * 1000.0 / n,
                ));
            }
            per_policy.push(fr);
        }
        let diff: Vec<f64> = per_policy[4].iter().zip(&per_policy[2]).map(|(l, g)| l - g).collect();
        let wins = diff.iter().filter(|d| **d > 1e-9).count();
        let losses = diff.iter().filter(|d| **d < -1e-9).count();
        println!(" {:+.1} pts ± {:.1} ({wins} better / {losses} worse) |", 100.0 * mean(&diff), 100.0 * se(&diff));
    }

    println!("\n## Decision cost (refined learned law)\n");
    println!("| suite | decisions / run | batched calls / run | mean batch | trivial sites / run | reductions / run | rounds / run | ms / run |");
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    for (name, dec, calls, triv, red, rounds, ms) in &cost_rows {
        println!("| {name} | {dec:.1} | {calls:.1} | {:.1} | {triv:.1} | {red:.0} | {rounds:.1} | {ms:.1} |", dec / calls.max(1e-9));
    }

    println!("\n## Disturbances: reorganization without a coordinator\n");
    println!("After assembly: 20% of providers removed, 10% lose one unit of capacity, two new roots arrive. The same policy keeps running; the planner re-plans the disturbed world from scratch with full knowledge.\n");
    println!("| suite | policy | before | after | planner (re-plan) | recovered share of planner |\n| --- | --- | --- | --- | --- | --- |");
    for s in [&suites[0], &suites[1]] {
        let mut plan_after = vec![];
        for (i, w) in s.worlds.iter().enumerate() {
            let (kills, caps, roots) = disturbance(w, s.seed0 + i as u64);
            let env = PlanEnv {
                dead: kills.iter().map(|k| k.1).collect(),
                capacity: caps.iter().map(|c| (c.1, c.2)).collect(),
                extra_roots: roots,
            };
            plan_after.push(plan(w, &env, 50_000, 8, s.seed0 + i as u64).fraction());
        }
        for (pname, make) in &policies {
            let (mut before, mut after) = (vec![], vec![]);
            for (i, w) in s.worlds.iter().enumerate() {
                let seed = s.seed0 + i as u64;
                let mut backend = make(seed);
                let rc = RunConfig { seed, ..RunConfig::default() };
                let (mut d, r) = run_world(w, backend.as_mut(), &rc);
                before.push(r.frac);
                let (kills, caps, roots) = disturbance(w, seed);
                for (ty, id) in kills {
                    d.deliver(ty, Ctl::Kill { provider: id }).unwrap();
                }
                for (ty, id, c) in caps {
                    d.deliver(ty, Ctl::SetCap { provider: id, capacity: c }).unwrap();
                }
                for root in &roots {
                    d.add_root(root).unwrap();
                }
                let st = d.run(backend.as_mut(), &rc);
                let e = d.evaluate();
                assert!(e.violations.is_empty() && !st.budget_exhausted, "{:?}", e.violations);
                after.push(e.fraction());
            }
            let share = mean(&after) / mean(&plan_after).max(1e-9);
            println!("| {} | {pname} | {} | {} | {} | {} |", s.name.split(' ').next().unwrap(), pct(mean(&before)), pct(mean(&after)), pct(mean(&plan_after)), pct(share));
        }
    }

    println!("\n## Sensitivity to execution order\n");
    println!("Twelve held-out contended worlds, eight driver seeds each, asynchronous transport (half of the pending messages routed per round, shuffled). Mean over worlds of the per-world standard deviation of the satisfied fraction.\n");
    println!("| policy | mean satisfied | mean per-world std across orders |\n| --- | --- | --- |");
    for (pname, make) in &policies {
        let (mut all, mut stds) = (vec![], vec![]);
        for (i, w) in suites[0].worlds.iter().take(12).enumerate() {
            let fr: Vec<f64> = (0..8u64)
                .map(|k| {
                    let rc = RunConfig { seed: 7000 + 31 * k + i as u64, route_fraction: 0.5, ..RunConfig::default() };
                    run_world(w, make(suites[0].seed0 + i as u64).as_mut(), &rc).1.frac
                })
                .collect();
            let m = mean(&fr);
            stds.push((fr.iter().map(|x| (x - m).powi(2)).sum::<f64>() / fr.len() as f64).sqrt());
            all.extend(fr);
        }
        println!("| {pname} | {} | {:.1} pts |", pct(mean(&all)), 100.0 * mean(&stds));
    }

    println!("\n## Uncertain or contradictory decisions\n");
    println!("The refined learned law with each decision replaced by a uniformly random admissible one with probability eps (contended held-out suite).\n");
    println!("| eps | satisfied |\n| --- | --- |");
    for eps in [0.0, 0.1, 0.3, 0.5, 1.0] {
        let fr: Vec<f64> = suites[0]
            .worlds
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let seed = suites[0].seed0 + i as u64;
                let mut b = NoisyBackend::new(law("refined", &theta_r), eps, seed);
                run_world(w, &mut b, &RunConfig { seed, ..RunConfig::default() }).1.frac
            })
            .collect();
        println!("| {eps} | {} ± {} |", pct(mean(&fr)), pct(se(&fr)));
    }
    println!("\nTotal time {:.1}s.", t0.elapsed().as_secs_f64());
}
