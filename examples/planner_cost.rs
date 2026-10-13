//! Cost of the centralized planner on the DNC-1 suites.
use std::time::Instant;
use suture::nets::depnet::planner::{plan, PlanEnv};
use suture::nets::depnet::world::{generate, GenParams};
fn main() {
    println!("| suite | nodes / world | ms / world | worlds hitting the node budget |\n| --- | --- | --- | --- |");
    for (name, p, s0, n) in [
        ("contended", GenParams::contended(), 1000u64, 40u64),
        ("contended-large", GenParams::contended_large(), 2000, 30),
        ("contended-deep", GenParams::contended_deep(), 3000, 30),
        ("contended-scarce", GenParams::contended_scarce(), 4000, 30),
        ("loose", GenParams::loose(), 5000, 30),
    ] {
        let (mut nodes, mut exhausted) = (0, 0);
        let t = Instant::now();
        for i in 0..n {
            let r = plan(&generate(&p, s0 + i), &PlanEnv::default(), 50_000, 8, s0 + i);
            nodes += r.nodes;
            exhausted += r.exhausted as u32;
        }
        println!("| {name} | {} | {:.1} | {exhausted} / {n} |", nodes / n, t.elapsed().as_secs_f64() * 1000.0 / n as f64);
    }
}
