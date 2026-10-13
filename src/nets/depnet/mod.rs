//! Experiment DNC-1: a self-organizing dependency network.
//!
//! Requirements, capabilities with capacities and regions, alternative
//! dependency recipes, and disturbances. The network assembles itself:
//! needs travel to markets, local `bind` decisions connect them to
//! providers, activated providers expand into one of their recipes (a
//! `recipe` decision) and emit needs of their own, failures and losses
//! travel back up and trigger new local decisions. No component sees the
//! whole network; see `docs/dnc.md` for the semantics.

pub mod eval;
pub mod planner;
pub mod program;
pub mod state;
pub mod train;
pub mod world;

pub use eval::{evaluate, Eval};
pub use program::{bind_features, greedy, laws, market_label, program, recipe_features, DepConfig, Kinds, BIND_FEATURES, RECIPE_FEATURES};

use crate::dnc::{DecisionBackend, Resolver};
use crate::machine::{EffectView, InjectError, Machine, SchedulerKind};
use crate::net::{Injection, NetBuilder};
use crate::rng::Rng;
use crate::NetError;
use state::{Ctl, Prov, INACTIVE};
use world::{ProviderSpec, RootSpec, WorldSpec, ROOT_TYPE};

/// How the driver interleaves transport and decisions.
#[derive(Clone, Debug)]
pub struct RunConfig {
    pub seed: u64,
    /// Rounds before giving up (a round: reduce, route, resolve).
    pub max_rounds: usize,
    /// Fraction of pending messages routed per round (asynchrony).
    pub route_fraction: f64,
    /// Shuffle the order in which messages are routed and sites resolved.
    pub shuffle: bool,
    pub scheduler: SchedulerKind,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig { seed: 0, max_rounds: 20_000, route_fraction: 1.0, shuffle: true, scheduler: SchedulerKind::Fifo }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RunStats {
    pub rounds: usize,
    pub routed: u64,
    pub budget_exhausted: bool,
}

/// A dependency-network machine with its decision resolver.
pub struct Depnet {
    pub m: Machine,
    pub k: Kinds,
    pub resolver: Resolver,
}

fn prov_cell(p: &ProviderSpec) -> Prov {
    Prov {
        id: p.id,
        ty: p.ty,
        region: p.region,
        capacity: p.capacity,
        consumers: vec![],
        phase: INACTIVE,
        recipes: p.recipes.clone(),
        recipe: -1,
        deps: vec![],
        tried: vec![],
        fails: 0,
        regions: vec![p.region],
        root: false,
        nonce: 0,
        epoch: 0,
    }
}

fn root_cell(r: &RootSpec) -> Prov {
    Prov {
        id: r.id,
        ty: ROOT_TYPE,
        region: -1,
        capacity: 0,
        consumers: vec![],
        phase: INACTIVE,
        recipes: vec![r.needs.clone()],
        recipe: -1,
        deps: vec![],
        tried: vec![],
        fails: 0,
        regions: r.regions.clone(),
        root: true,
        nonce: 0,
        epoch: 0,
    }
}

impl Depnet {
    /// Builds every market with its provider cells and starts every root.
    pub fn new(spec: &WorldSpec, cfg: &DepConfig) -> Result<Depnet, String> {
        let (prog, k) = program(cfg);
        let mut m = Machine::new(prog.clone());
        let mut b = NetBuilder::new(&prog.sig);
        let mut inj_holes = vec![];
        let mut markets: Vec<(i64, Vec<Prov>)> = vec![(ROOT_TYPE, spec.roots.iter().map(root_cell).collect())];
        for ty in spec.types() {
            markets.push((ty, spec.providers.iter().filter(|p| p.ty == ty).map(prov_cell).collect()));
        }
        for (ty, cells) in &markets {
            let (mb, l0, q0) = (b.var(), b.var(), b.var());
            b.agent(k.market, *ty, mb, &[l0, q0]);
            let mut cur = l0;
            for c in cells {
                let (next, cons) = (b.var(), b.var());
                b.agent(k.prov, c.to_value(), cur, &[next, cons]);
                b.agent(k.cnil, (), cons, &[]);
                cur = next;
            }
            b.agent(k.pnil, (), cur, &[]);
            b.agent(k.pdnil, (), q0, &[]);
            inj_holes.push((mb, market_label(*ty)));
        }
        let mut inj = Injection::new(b.finish().map_err(|e| e.to_string())?).mint().key("init");
        for (v, label) in &inj_holes {
            inj = inj.new_hole(*v, Some(label));
        }
        m.inject(inj).map_err(|e| e.to_string())?;
        let resolver = Resolver::new(laws(&k));
        let mut d = Depnet { m, k, resolver };
        for r in &spec.roots {
            d.deliver(ROOT_TYPE, Ctl::Start { provider: r.id }).map_err(|e| e.to_string())?;
        }
        Ok(d)
    }

    /// Appends a control message to a market's mailbox (host operation).
    pub fn deliver(&mut self, ty: i64, ctl: Ctl) -> Result<(), InjectError> {
        let k = self.k;
        let mut b = NetBuilder::new(&self.m.program().sig);
        let (tail, msg, rest) = (b.var(), b.var(), b.var());
        b.agent(k.mcons, (), tail, &[msg, rest]);
        b.agent(k.ctl, ctl.to_value(), msg, &[]);
        let label = market_label(ty);
        let inj = Injection::new(b.finish()?).at_label(tail, &label).new_hole(rest, Some(&label));
        self.m.inject(inj).map(|_| ())
    }

    /// Adds a new root requirement (disturbance) and starts it.
    pub fn add_root(&mut self, r: &RootSpec) -> Result<(), InjectError> {
        let k = self.k;
        let mut b = NetBuilder::new(&self.m.program().sig);
        let (tail, msg, rest, cell, next, cons) = (b.var(), b.var(), b.var(), b.var(), b.var(), b.var());
        b.agent(k.mcons, (), tail, &[msg, rest]);
        b.agent(k.add_prov, (), msg, &[cell]);
        b.agent(k.prov, root_cell(r).to_value(), cell, &[next, cons]);
        b.agent(k.pnil, (), next, &[]);
        b.agent(k.cnil, (), cons, &[]);
        let label = market_label(ROOT_TYPE);
        let inj = Injection::new(b.finish()?).at_label(tail, &label).new_hole(rest, Some(&label)).mint();
        self.m.inject(inj)?;
        self.deliver(ROOT_TYPE, Ctl::Start { provider: r.id })
    }

    /// The current state of a provider (or root) cell.
    pub fn provider(&self, id: i64) -> Option<Prov> {
        eval::cells(&self.m, &self.k).remove(&id).map(|(_, p)| p)
    }

    pub fn evaluate(&self) -> Eval {
        evaluate(&self.m, &self.k)
    }

    /// Delivers one pending `Send` effect into the mailbox it names.
    pub fn route(&mut self, send: &EffectView) -> Result<(), InjectError> {
        let dest = send.payload.str(0).map_err(|e| NetError::Value(e.to_string()))?.to_string();
        let mut b = NetBuilder::new(&self.m.program().sig);
        let (p, d, n) = (b.var(), b.var(), b.var());
        b.agent(self.k.route, (), p, &[d, n]);
        let inj = Injection::new(b.finish()?)
            .at_hole(p, send.hole)
            .at_label(d, &dest)
            .new_hole(n, Some(&dest))
            .key(format!("route:{:032x}", send.hole.0));
        self.m.inject(inj).map(|_| ())
    }

    /// Runs until nothing is pending: reduce, route messages, resolve
    /// decision sites (one batched backend call per round), repeat.
    pub fn run(&mut self, backend: &mut dyn DecisionBackend, rc: &RunConfig) -> RunStats {
        self.m.set_scheduler(rc.scheduler);
        let mut rng = Rng::new(rc.seed ^ 0xd21e);
        let mut st = RunStats::default();
        for _ in 0..rc.max_rounds {
            self.m.run(None);
            let ob = self.m.outbox();
            if ob.is_empty() {
                return st;
            }
            let (mut sends, mut sites): (Vec<_>, Vec<_>) = ob.into_iter().partition(|e| e.kind == self.k.send);
            if rc.shuffle {
                rng.shuffle(&mut sends);
                rng.shuffle(&mut sites);
            }
            let n = ((sends.len() as f64 * rc.route_fraction).ceil() as usize).clamp(1, sends.len().max(1));
            for s in sends.iter().take(n) {
                self.route(s).expect("routing a pending message");
                st.routed += 1;
            }
            if !sites.is_empty() {
                self.resolver.resolve_sites(&mut self.m, backend, &sites).expect("resolving decision sites");
            }
            st.rounds += 1;
        }
        st.budget_exhausted = true;
        st
    }
}
