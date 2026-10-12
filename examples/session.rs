//! Experiment B: a stop-and-wait session over a lossy, duplicating,
//! reordering network with host-fired timeouts and an adversary injecting
//! invalid messages. Prints the measurements reported in docs/experiments.md.

use std::collections::BTreeMap;

use suture::nets::session::{simulate, ClientOutcome, NetConfig};

fn main() {
    let items = vec![10, 20, 30, 40];
    for (label, drop, dup, adversarial) in
        [("perfect", 0.0, 0.0, 0.0), ("lossy", 0.3, 0.2, 0.0), ("lossy + adversary", 0.3, 0.2, 0.3)]
    {
        let seeds = 300;
        let (mut ok, mut aborted, mut rejects, mut fired, mut reductions) = (0, 0, 0, 0, 0);
        let mut sent: BTreeMap<String, u64> = BTreeMap::new();
        let mut phases: BTreeMap<String, u64> = BTreeMap::new();
        for seed in 0..seeds {
            let cfg = NetConfig { seed, drop, dup, adversarial, items: items.clone(), max_tries: 6 };
            let r = simulate(&cfg).expect("session invariants hold");
            match &r.client {
                ClientOutcome::Ok => {
                    assert_eq!(r.delivered, items);
                    ok += 1
                }
                ClientOutcome::Aborted(p) => {
                    assert!(items.starts_with(&r.delivered));
                    aborted += 1;
                    *phases.entry(p.clone()).or_default() += 1;
                }
            }
            rejects += r.rejects;
            fired += r.timeouts_fired;
            reductions += r.stats.reductions;
            for (k, n) in r.transmitted {
                *sent.entry(k).or_default() += n;
            }
        }
        let total: u64 = sent.values().sum();
        println!("== {label} (drop {drop}, dup {dup}, adversarial {adversarial}), {seeds} sessions of 4 items ==");
        println!("closed ok: {ok}   aborted: {aborted} {phases:?}");
        println!(
            "messages transmitted per session: {:.1}  (Data {:.1}, Ack {:.1}, Open {:.1})",
            total as f64 / seeds as f64,
            sent.get("Data").copied().unwrap_or(0) as f64 / seeds as f64,
            sent.get("Ack").copied().unwrap_or(0) as f64 / seeds as f64,
            sent.get("Open").copied().unwrap_or(0) as f64 / seeds as f64
        );
        println!(
            "timeouts fired per session: {:.1}   invalid messages rejected: {rejects}   reductions per session: {:.1}\n",
            fired as f64 / seeds as f64,
            reductions as f64 / seeds as f64
        );
    }
    let r = simulate(&NetConfig { seed: 5, drop: 0.2, dup: 0.1, adversarial: 0.0, items, max_tries: 5 }).unwrap();
    println!("final state of one session:\n{}", r.explanation);
}
