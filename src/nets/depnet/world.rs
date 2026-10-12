//! The static world of Experiment DNC-1: typed capabilities arranged in
//! layers, providers with capacities, regions and alternative dependency
//! recipes, and root requirements.

use crate::rng::Rng;

/// Type 0 is reserved for roots; capability types are `1..`.
pub const ROOT_TYPE: i64 = 0;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderSpec {
    pub id: i64,
    pub ty: i64,
    pub region: i64,
    pub capacity: i64,
    /// Alternative dependency motifs: each recipe lists the capability types
    /// the provider needs when built that way. Leaves have one empty recipe.
    pub recipes: Vec<Vec<i64>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootSpec {
    pub id: i64,
    pub needs: Vec<i64>,
    /// Regions this root's whole dependency tree may use at its first level.
    pub regions: Vec<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldSpec {
    /// `layers[0]` are the types roots need; the last layer holds leaves.
    pub layers: Vec<Vec<i64>>,
    pub providers: Vec<ProviderSpec>,
    pub roots: Vec<RootSpec>,
    pub regions: i64,
}

impl WorldSpec {
    pub fn types(&self) -> impl Iterator<Item = i64> + '_ {
        self.layers.iter().flatten().copied()
    }
    pub fn provider(&self, id: i64) -> Option<&ProviderSpec> {
        self.providers.iter().find(|p| p.id == id)
    }
    pub fn layer_of(&self, ty: i64) -> Option<usize> {
        self.layers.iter().position(|l| l.contains(&ty))
    }
}

/// Parameters of the random world generator.
#[derive(Clone, Debug)]
pub struct GenParams {
    /// Capability layers below the roots (the last one holds leaves).
    pub depth: usize,
    pub types_per_layer: usize,
    pub providers_per_type: (usize, usize),
    pub capacity: (i64, i64),
    /// Recipes per non-leaf provider.
    pub recipes: (usize, usize),
    /// Dependencies per recipe (drawn from the next layer).
    pub deps_per_recipe: (usize, usize),
    pub roots: usize,
    pub needs_per_root: (usize, usize),
    pub regions: usize,
    /// Probability that a root accepts two regions instead of one.
    pub multi_region_roots: f64,
}

impl GenParams {
    /// The training distribution: small, shallow worlds.
    pub fn small() -> Self {
        GenParams {
            depth: 2,
            types_per_layer: 2,
            providers_per_type: (2, 3),
            capacity: (1, 3),
            recipes: (1, 2),
            deps_per_recipe: (0, 2),
            roots: 5,
            needs_per_root: (1, 2),
            regions: 2,
            multi_region_roots: 0.3,
        }
    }
    /// More roots, providers, regions: the same shape, scaled up.
    pub fn large() -> Self {
        GenParams {
            depth: 2,
            types_per_layer: 3,
            providers_per_type: (3, 6),
            capacity: (1, 4),
            recipes: (1, 3),
            deps_per_recipe: (0, 2),
            roots: 16,
            needs_per_root: (1, 3),
            regions: 3,
            multi_region_roots: 0.3,
        }
    }
    /// Deeper dependency chains than ever seen in training.
    pub fn deep() -> Self {
        GenParams { depth: 4, types_per_layer: 2, providers_per_type: (2, 4), roots: 8, ..GenParams::small() }
    }
    /// The training shape with tighter capacity.
    pub fn scarce() -> Self {
        GenParams { capacity: (1, 2), roots: 6, ..GenParams::small() }
    }
}

fn range_usize(rng: &mut Rng, (lo, hi): (usize, usize)) -> usize {
    lo + rng.index(hi - lo + 1)
}

fn range_i64(rng: &mut Rng, (lo, hi): (i64, i64)) -> i64 {
    lo + rng.below((hi - lo + 1) as u64) as i64
}

fn pick_distinct(rng: &mut Rng, from: &[i64], n: usize) -> Vec<i64> {
    let mut xs = from.to_vec();
    rng.shuffle(&mut xs);
    xs.truncate(n.min(from.len()));
    xs.sort();
    xs
}

/// Generates a random world. Deterministic in `seed`.
pub fn generate(p: &GenParams, seed: u64) -> WorldSpec {
    let mut rng = Rng::new(seed ^ 0x5eed_d0c5);
    let mut next_type = 1;
    let layers: Vec<Vec<i64>> = (0..p.depth)
        .map(|_| {
            (0..p.types_per_layer)
                .map(|_| {
                    next_type += 1;
                    next_type - 1
                })
                .collect()
        })
        .collect();
    let mut providers = vec![];
    let mut next_id = 100;
    for (li, layer) in layers.iter().enumerate() {
        let below = layers.get(li + 1);
        for &ty in layer {
            for _ in 0..range_usize(&mut rng, p.providers_per_type) {
                let recipes = match below {
                    None => vec![vec![]],
                    Some(below) => (0..range_usize(&mut rng, p.recipes))
                        .map(|_| {
                            let n = range_usize(&mut rng, p.deps_per_recipe);
                            pick_distinct(&mut rng, below, n)
                        })
                        .collect(),
                };
                providers.push(ProviderSpec {
                    id: next_id,
                    ty,
                    region: rng.below(p.regions as u64) as i64,
                    capacity: range_i64(&mut rng, p.capacity),
                    recipes,
                });
                next_id += 1;
            }
        }
    }
    let roots = (0..p.roots)
        .map(|i| {
            let n = range_usize(&mut rng, p.needs_per_root);
            let first = rng.below(p.regions as u64) as i64;
            let mut regions = vec![first];
            if p.regions > 1 && rng.chance(p.multi_region_roots) {
                regions.push((first + 1) % p.regions as i64);
                regions.sort();
            }
            RootSpec { id: 1 + i as i64, needs: pick_distinct(&mut rng, &layers[0], n.max(1)), regions }
        })
        .collect();
    WorldSpec { layers, providers, roots, regions: p.regions as i64 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generation_is_deterministic_and_layered() {
        let p = GenParams::small();
        let a = generate(&p, 3);
        assert_eq!(a, generate(&p, 3));
        assert_ne!(a, generate(&p, 4));
        for pr in &a.providers {
            let li = a.layer_of(pr.ty).unwrap();
            for r in &pr.recipes {
                for d in r {
                    assert_eq!(a.layer_of(*d), Some(li + 1), "dependencies point one layer down");
                }
            }
            if li + 1 == a.layers.len() {
                assert_eq!(pr.recipes, vec![Vec::<i64>::new()], "leaves have one empty recipe");
            }
            assert!((0..a.regions).contains(&pr.region));
        }
        for r in &a.roots {
            assert!(!r.needs.is_empty());
            assert!(r.needs.iter().all(|t| a.layers[0].contains(t)));
        }
    }
}
