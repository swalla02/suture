//! The centralized baseline: a global planner with full knowledge of the
//! world. It searches for an assignment of requirements to providers and of
//! recipes to activated providers that satisfies as many roots as possible,
//! using the same semantics as the machine (capacities, regions inherited
//! from the provider, sharing of already-built providers, all-or-nothing
//! roots). Depth-first search with full backtracking over a root's goals,
//! greedy across roots, with randomized restarts and a node budget.

use std::collections::{BTreeMap, BTreeSet};

use super::world::{RootSpec, WorldSpec};
use crate::rng::Rng;

/// Environment changes the planner must respect (after disturbances).
#[derive(Clone, Debug, Default)]
pub struct PlanEnv {
    pub dead: BTreeSet<i64>,
    pub capacity: BTreeMap<i64, i64>,
    pub extra_roots: Vec<RootSpec>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub roots: usize,
    pub satisfied: usize,
    /// Search nodes expanded over all restarts.
    pub nodes: u64,
    /// Whether some root search hit the node budget.
    pub exhausted: bool,
}

impl Plan {
    pub fn fraction(&self) -> f64 {
        if self.roots == 0 { 1.0 } else { self.satisfied as f64 / self.roots as f64 }
    }
}

enum Undo {
    Used(usize),
    Recipe(usize),
}

struct Search<'a> {
    spec: &'a WorldSpec,
    by_type: BTreeMap<i64, Vec<usize>>,
    cap: Vec<i64>,
    alive: Vec<bool>,
    used: Vec<i64>,
    recipe: Vec<i64>,
    trail: Vec<Undo>,
    nodes: u64,
    budget: u64,
    exhausted: bool,
}

type Goal = (i64, Vec<i64>);

impl Search<'_> {
    fn undo(&mut self, mark: usize) {
        while self.trail.len() > mark {
            match self.trail.pop().unwrap() {
                Undo::Used(i) => self.used[i] -= 1,
                Undo::Recipe(i) => self.recipe[i] = -1,
            }
        }
    }

    fn candidates(&self, ty: i64, regions: &[i64]) -> Vec<usize> {
        let mut c: Vec<usize> = self
            .by_type
            .get(&ty)
            .into_iter()
            .flatten()
            .copied()
            .filter(|&i| self.alive[i] && regions.contains(&self.spec.providers[i].region) && self.used[i] < self.cap[i])
            .collect();
        let cost = |i: usize| {
            if self.recipe[i] >= 0 {
                -1
            } else {
                self.spec.providers[i].recipes.iter().map(Vec::len).min().unwrap_or(0) as i64
            }
        };
        c.sort_by_key(|&i| (cost(i), i));
        c
    }

    /// Satisfies every goal or leaves the state unchanged.
    fn solve(&mut self, goals: &[Goal]) -> bool {
        let Some(((ty, regions), rest)) = goals.split_first() else { return true };
        if self.nodes >= self.budget {
            self.exhausted = true;
            return false;
        }
        self.nodes += 1;
        for i in self.candidates(*ty, regions) {
            let mark = self.trail.len();
            self.used[i] += 1;
            self.trail.push(Undo::Used(i));
            if self.recipe[i] >= 0 {
                if self.solve(rest) {
                    return true;
                }
            } else {
                let p = &self.spec.providers[i];
                let mut order: Vec<usize> = (0..p.recipes.len()).collect();
                order.sort_by_key(|&r| (p.recipes[r].len(), r));
                for r in order {
                    let p = &self.spec.providers[i];
                    self.recipe[i] = r as i64;
                    self.trail.push(Undo::Recipe(i));
                    let mut sub: Vec<Goal> = p.recipes[r].iter().map(|&t| (t, vec![p.region])).collect();
                    sub.extend_from_slice(rest);
                    if self.solve(&sub) {
                        return true;
                    }
                    self.undo(mark + 1);
                }
            }
            self.undo(mark);
        }
        false
    }
}

/// Plans the world under `env`. `budget` bounds the nodes per root search.
pub fn plan(spec: &WorldSpec, env: &PlanEnv, budget: u64, restarts: usize, seed: u64) -> Plan {
    let mut by_type: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, p) in spec.providers.iter().enumerate() {
        by_type.entry(p.ty).or_default().push(i);
    }
    let cap: Vec<i64> = spec.providers.iter().map(|p| *env.capacity.get(&p.id).unwrap_or(&p.capacity)).collect();
    let alive: Vec<bool> = spec.providers.iter().map(|p| !env.dead.contains(&p.id)).collect();
    let roots: Vec<&RootSpec> = spec.roots.iter().chain(env.extra_roots.iter()).collect();
    let mut s = Search {
        spec,
        by_type,
        cap,
        alive,
        used: vec![0; spec.providers.len()],
        recipe: vec![-1; spec.providers.len()],
        trail: vec![],
        nodes: 0,
        budget: 0,
        exhausted: false,
    };
    let mut rng = Rng::new(seed);
    let mut best = 0;
    let mut total_nodes = 0;
    for restart in 0..restarts.max(1) {
        s.used.iter_mut().for_each(|u| *u = 0);
        s.recipe.iter_mut().for_each(|r| *r = -1);
        s.trail.clear();
        let mut order: Vec<usize> = (0..roots.len()).collect();
        if restart > 0 {
            rng.shuffle(&mut order);
        }
        let mut count = 0;
        for &ri in &order {
            let r = roots[ri];
            let mark = s.trail.len();
            s.nodes = 0;
            s.budget = budget;
            let goals: Vec<Goal> = r.needs.iter().map(|&t| (t, r.regions.clone())).collect();
            if s.solve(&goals) {
                count += 1;
            } else {
                s.undo(mark);
            }
            total_nodes += s.nodes;
        }
        best = best.max(count);
    }
    Plan { roots: roots.len(), satisfied: best, nodes: total_nodes, exhausted: s.exhausted }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nets::depnet::world::ProviderSpec;

    fn prov(id: i64, ty: i64, region: i64, capacity: i64, recipes: Vec<Vec<i64>>) -> ProviderSpec {
        ProviderSpec { id, ty, region, capacity, recipes }
    }

    #[test]
    fn plans_respect_capacity_regions_and_recipes() {
        let spec = WorldSpec {
            layers: vec![vec![1], vec![2, 3]],
            providers: vec![prov(100, 1, 0, 2, vec![vec![2], vec![3]]), prov(300, 3, 0, 1, vec![vec![]])],
            roots: vec![
                RootSpec { id: 1, needs: vec![1], regions: vec![0] },
                RootSpec { id: 2, needs: vec![1], regions: vec![0] },
                RootSpec { id: 3, needs: vec![1], regions: vec![1] },
            ],
            regions: 2,
        };
        let p = plan(&spec, &PlanEnv::default(), 10_000, 1, 0);
        // Provider 100 is built once with recipe [3] and shared by both EU roots;
        // the root restricted to region 1 has no provider at all.
        assert_eq!((p.roots, p.satisfied), (3, 2));
        let killed = PlanEnv { dead: [300].into(), ..PlanEnv::default() };
        assert_eq!(plan(&spec, &killed, 10_000, 1, 0).satisfied, 0);
        let squeezed = PlanEnv { capacity: [(100, 1)].into(), ..PlanEnv::default() };
        assert_eq!(plan(&spec, &squeezed, 10_000, 1, 0).satisfied, 1);
    }

    #[test]
    fn backtracks_across_goals() {
        // Root needs types 1 and 2; the cheap choice for type 1 (provider 100,
        // recipe [3]) takes the only type-3 slot that type 2's provider needs.
        let spec = WorldSpec {
            layers: vec![vec![1, 2], vec![3, 4]],
            providers: vec![
                prov(100, 1, 0, 1, vec![vec![3]]),
                prov(101, 1, 0, 1, vec![vec![4]]),
                prov(200, 2, 0, 1, vec![vec![3]]),
                prov(300, 3, 0, 1, vec![vec![]]),
                prov(400, 4, 0, 1, vec![vec![]]),
            ],
            roots: vec![RootSpec { id: 1, needs: vec![1, 2], regions: vec![0] }],
            regions: 1,
        };
        assert_eq!(plan(&spec, &PlanEnv::default(), 10_000, 1, 0).satisfied, 1);
    }
}
