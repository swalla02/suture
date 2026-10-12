//! Reading the outcome off the graph: which roots are functionally
//! satisfied, and whether the assembled structure is valid. Used for
//! measurement only; the machine itself never sees these global facts.

use std::collections::BTreeMap;

use super::program::Kinds;
use super::state::{Need, Prov, ACTIVE};
use crate::graph::Endpoint;
use crate::machine::Machine;
use crate::AgentId;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Eval {
    pub roots: usize,
    pub satisfied: usize,
    /// Live bindings (consumer entries).
    pub bindings: usize,
    /// Non-root providers that are active or choosing a recipe.
    pub active: usize,
    pub dead: usize,
    /// Structural or policy violations; must stay empty.
    pub violations: Vec<String>,
}

impl Eval {
    pub fn fraction(&self) -> f64 {
        if self.roots == 0 { 1.0 } else { self.satisfied as f64 / self.roots as f64 }
    }
}

/// The provider cells currently in the graph, by id.
pub fn cells(m: &Machine, k: &Kinds) -> BTreeMap<i64, (AgentId, Prov)> {
    m.graph()
        .agents_sorted()
        .into_iter()
        .filter(|(_, a)| a.kind == k.prov)
        .filter_map(|(id, a)| Prov::from_value(&a.payload).ok().map(|p| (p.id, (id, p))))
        .collect()
}

/// Walks a provider's structural consumer list: the binding keys of its
/// consumer entries and, for each, the socket at the other end.
fn consumer_entries(m: &Machine, k: &Kinds, cell: AgentId) -> Result<Vec<([i64; 3], Option<Need>)>, String> {
    let g = m.graph();
    let mut out = vec![];
    let mut e = g.agent(cell).ok_or("missing cell")?.ports[2];
    for _ in 0..100_000 {
        let Endpoint::Port(a, 0) = e else { return Err(format!("consumer list of {cell} ends at {e:?}")) };
        let agent = g.agent(a).ok_or("dangling consumer list")?;
        if agent.kind == k.cnil {
            return Ok(out);
        }
        if agent.kind != k.centry {
            return Err(format!("consumer list of {cell} holds a {}", m.sig().kind(agent.kind).name));
        }
        let p = &agent.payload;
        let key = [p.int(0).unwrap_or(-1), p.int(1).unwrap_or(-1), p.int(2).unwrap_or(-1)];
        let socket = match agent.ports[1] {
            Endpoint::Port(s, 0) => g
                .agent(s)
                .filter(|s| s.kind == k.socket)
                .and_then(|s| s.payload.get(0).ok().and_then(|n| Need::from_value(n).ok())),
            _ => None,
        };
        out.push((key, socket));
        e = agent.ports[2];
    }
    Err("consumer list too long".into())
}

pub fn evaluate(m: &Machine, k: &Kinds) -> Eval {
    let cells = cells(m, k);
    let mut ev = Eval::default();
    let mut bindings: BTreeMap<[i64; 3], i64> = BTreeMap::new();
    for (id, (cell, p)) in &cells {
        if p.root {
            ev.roots += 1;
        } else if p.phase == ACTIVE || p.phase == super::state::CHOOSING {
            ev.active += 1;
        } else if !p.alive() {
            ev.dead += 1;
        }
        match consumer_entries(m, k, *cell) {
            Err(e) => ev.violations.push(e),
            Ok(entries) => {
                let keys: Vec<[i64; 3]> = entries.iter().map(|(k, _)| *k).collect();
                if keys != p.consumers {
                    ev.violations.push(format!("provider {id}: consumer list {keys:?} but state {:?}", p.consumers));
                }
                if !p.root && keys.len() as i64 > p.capacity {
                    ev.violations.push(format!("provider {id}: {} consumers over capacity {}", keys.len(), p.capacity));
                }
                if !p.alive() && !keys.is_empty() {
                    ev.violations.push(format!("dead provider {id} still has consumers"));
                }
                for (key, socket) in entries {
                    match socket {
                        Some(n) if [n.owner, n.dep, n.nonce] == key => {}
                        _ => ev.violations.push(format!("provider {id}: binding {key:?} has no matching socket")),
                    }
                    bindings.insert(key, *id);
                }
            }
        }
    }
    ev.bindings = bindings.len();
    // Every binding an owner believes in is structural, typed and in region.
    for (id, (_, p)) in &cells {
        for (i, d) in p.deps.iter().enumerate() {
            if d.bound < 0 {
                continue;
            }
            let key = [*id, i as i64, d.nonce];
            if bindings.get(&key) != Some(&d.bound) {
                ev.violations.push(format!("owner {id} believes dep {i} is bound to {} but no such binding exists", d.bound));
            }
            match cells.get(&d.bound) {
                Some((_, q)) => {
                    if q.ty != d.ty {
                        ev.violations.push(format!("owner {id} dep {i}: type {} bound to provider of type {}", d.ty, q.ty));
                    }
                    if !p.regions.contains(&q.region) {
                        ev.violations.push(format!("owner {id} dep {i}: region {} not in {:?}", q.region, p.regions));
                    }
                }
                None => ev.violations.push(format!("owner {id} dep {i}: unknown provider {}", d.bound)),
            }
        }
    }
    // Functional closure: active, every dependency bound to a functional provider.
    let mut memo: BTreeMap<i64, bool> = BTreeMap::new();
    fn functional(id: i64, cells: &BTreeMap<i64, (AgentId, Prov)>, memo: &mut BTreeMap<i64, bool>, depth: usize) -> bool {
        if let Some(&f) = memo.get(&id) {
            return f;
        }
        let Some((_, p)) = cells.get(&id) else { return false };
        let f = depth < 64
            && p.phase == ACTIVE
            && p.deps.len() == p.recipes.get(p.recipe.max(0) as usize).map_or(0, Vec::len)
            && p.deps.iter().all(|d| d.bound >= 0 && functional(d.bound, cells, memo, depth + 1));
        memo.insert(id, f);
        f
    }
    ev.satisfied = cells.values().filter(|(_, p)| p.root && functional(p.id, &cells, &mut memo, 0)).count();
    ev
}
