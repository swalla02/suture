//! Offline training of the shared decision law (directive §3: train
//! offline, deploy frozen). The law is linear in the observation features of
//! each alternative (`BIND_FEATURES`, `RECIPE_FEATURES`); its weights are
//! found by the cross-entropy method, maximizing the mean fraction of
//! satisfied roots on a set of training worlds. Inference stays local: the
//! trained weights see only what a decision site sees.

use super::program::{BIND_FEATURES, RECIPE_FEATURES};
use super::world::WorldSpec;
use super::{DepConfig, Depnet, RunConfig};
use crate::dnc::{DecisionBackend, LinearBackend};
use crate::rng::Rng;

pub const DIMS: usize = BIND_FEATURES.len() + RECIPE_FEATURES.len();

/// The decision law with parameters `theta` (bind weights, then recipe weights).
pub fn law(name: &str, theta: &[f64]) -> LinearBackend {
    let b = BIND_FEATURES.len();
    LinearBackend::new(name).with("bind", theta[..b].to_vec()).with("recipe", theta[b..].to_vec())
}

/// Mean satisfied fraction of `make(seed)`'s decisions over `worlds`; world
/// `i` runs with driver seed `seed + i`.
pub fn score(worlds: &[WorldSpec], cfg: &DepConfig, seed: u64, make: &dyn Fn(u64) -> Box<dyn DecisionBackend>) -> f64 {
    let mut total = 0.0;
    for (i, w) in worlds.iter().enumerate() {
        let s = seed + i as u64;
        let mut d = Depnet::new(w, cfg).expect("world builds");
        d.run(make(s).as_mut(), &RunConfig { seed: s, ..RunConfig::default() });
        total += d.evaluate().fraction();
    }
    total / worlds.len().max(1) as f64
}

#[derive(Clone, Debug)]
pub struct TrainConfig {
    pub iterations: usize,
    pub population: usize,
    pub elite: usize,
    pub init_std: f64,
    pub seed: u64,
    pub threads: usize,
}

impl Default for TrainConfig {
    fn default() -> Self {
        TrainConfig { iterations: 20, population: 32, elite: 6, init_std: 1.0, seed: 7, threads: 8 }
    }
}

#[derive(Clone, Debug)]
pub struct Trained {
    pub theta: Vec<f64>,
    pub fitness: f64,
    /// `(best, elite mean)` fitness per iteration.
    pub history: Vec<(f64, f64)>,
    /// Machine runs spent on training.
    pub runs: u64,
}

fn gauss(rng: &mut Rng) -> f64 {
    let unit = |rng: &mut Rng| ((rng.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64;
    let (u, v) = (unit(rng), unit(rng));
    (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
}

/// Cross-entropy method over the law's weights. Deterministic in `cfg.seed`.
pub fn train(worlds: &[WorldSpec], dep: &DepConfig, cfg: &TrainConfig) -> Trained {
    let mut rng = Rng::new(cfg.seed);
    let (mut mu, mut sigma) = (vec![0.0; DIMS], vec![cfg.init_std; DIMS]);
    let mut best = (f64::MIN, vec![0.0; DIMS]);
    let mut history = vec![];
    let mut runs = 0;
    for _ in 0..cfg.iterations {
        let pop: Vec<Vec<f64>> =
            (0..cfg.population).map(|_| (0..DIMS).map(|d| mu[d] + sigma[d] * gauss(&mut rng)).collect()).collect();
        let mut fit = vec![0.0; pop.len()];
        let chunk = pop.len().div_ceil(cfg.threads.max(1));
        std::thread::scope(|s| {
            for (thetas, out) in pop.chunks(chunk).zip(fit.chunks_mut(chunk)) {
                s.spawn(move || {
                    for (theta, f) in thetas.iter().zip(out.iter_mut()) {
                        *f = score(worlds, dep, cfg.seed, &|_| Box::new(law("candidate", theta)));
                    }
                });
            }
        });
        runs += (pop.len() * worlds.len()) as u64;
        let mut order: Vec<usize> = (0..pop.len()).collect();
        order.sort_by(|&a, &b| fit[b].partial_cmp(&fit[a]).unwrap().then(a.cmp(&b)));
        let elite: Vec<&Vec<f64>> = order.iter().take(cfg.elite).map(|&i| &pop[i]).collect();
        if fit[order[0]] > best.0 {
            best = (fit[order[0]], pop[order[0]].clone());
        }
        let elite_mean = order.iter().take(cfg.elite).map(|&i| fit[i]).sum::<f64>() / cfg.elite as f64;
        history.push((fit[order[0]], elite_mean));
        for d in 0..DIMS {
            mu[d] = elite.iter().map(|t| t[d]).sum::<f64>() / elite.len() as f64;
            let var = elite.iter().map(|t| (t[d] - mu[d]).powi(2)).sum::<f64>() / elite.len() as f64;
            sigma[d] = var.sqrt().max(0.05);
        }
    }
    Trained { theta: best.1, fitness: best.0, history, runs }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nets::depnet::world::{generate, GenParams};

    #[test]
    fn training_is_deterministic_and_never_worse_than_its_start() {
        let worlds: Vec<_> = (0..3).map(|s| generate(&GenParams::small(), 50 + s)).collect();
        let cfg = TrainConfig { iterations: 2, population: 6, elite: 2, threads: 3, ..TrainConfig::default() };
        let a = train(&worlds, &DepConfig::default(), &cfg);
        let b = train(&worlds, &DepConfig::default(), &cfg);
        assert_eq!(a.theta, b.theta);
        assert_eq!(a.history.len(), 2);
        let again = score(&worlds, &DepConfig::default(), cfg.seed, &|_| Box::new(law("again", &a.theta)));
        assert_eq!(again, a.fitness, "the reported fitness is reproducible from the returned weights");
        assert!(a.history[1].0 <= a.fitness && a.history[0].0 <= a.fitness);
        assert_eq!(a.runs, 2 * 6 * 3);
    }
}
