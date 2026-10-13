//! The dependency-network program: markets holding provider cells, bindings
//! as one-way channels, activation spawning dependency needs, failure and
//! loss cascades, recipe switching — and two decision laws (`bind`,
//! `recipe`) whose sites are resolved by a shared decision backend.

use std::collections::BTreeSet;
use std::sync::Arc;

use super::state::{Cand, Ctl, Dep, Need, Prov, ACTIVE, CHOOSING, DEAD, GAVE_UP, INACTIVE};
use crate::dnc::{decided_schema, selected_index, Law, LinearBackend};
use crate::machine::Program;
use crate::policy::{PatchView, Policy};
use crate::rule::RuleCx;
use crate::value::{Value, ValueType};
use crate::{tup, KindId, RuleTable, SigBuilder};

#[derive(Clone, Debug)]
pub struct DepConfig {
    /// Needs per dependency before the owner gives up on its current recipe.
    pub max_attempts: i64,
    /// Stale decisions a need tolerates before failing.
    pub max_conflicts: i64,
    /// Candidates a gathering walk records (the bounded observation).
    pub candidates: usize,
}

impl Default for DepConfig {
    fn default() -> Self {
        DepConfig { max_attempts: 3, max_conflicts: 4, candidates: 6 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Kinds {
    pub market: KindId,
    pub mcons: KindId,
    pub dispatch: KindId,
    pub prov: KindId,
    pub pnil: KindId,
    pub centry: KindId,
    pub cnil: KindId,
    pub pcons: KindId,
    pub pdnil: KindId,
    pub need: KindId,
    pub ctl: KindId,
    pub add_prov: KindId,
    pub gather: KindId,
    pub unpark: KindId,
    pub bind: KindId,
    pub visit: KindId,
    pub append: KindId,
    pub cremove: KindId,
    pub cevict: KindId,
    pub await_bind: KindId,
    pub socket: KindId,
    pub granted: KindId,
    pub failed: KindId,
    pub lost: KindId,
    pub released: KindId,
    pub send: KindId,
    pub route: KindId,
    pub bind_site: KindId,
    pub recipe_site: KindId,
    pub decided: KindId,
}

pub fn market_label(ty: i64) -> String {
    format!("market:{ty}")
}

/// Features the `bind` law sees for one candidate provider.
pub const BIND_FEATURES: [&str; 7] = ["active", "activation_cost", "load", "free", "fails", "lost_before", "recipes"];

pub fn bind_features(c: &Cand) -> Vec<f64> {
    let cap = c.capacity.max(1) as f64;
    vec![
        c.active as f64,
        c.cost as f64,
        c.used as f64 / cap,
        (c.capacity - c.used) as f64 / 4.0,
        c.fails.min(5) as f64 / 5.0,
        c.in_history as f64,
        c.recipes as f64 / 3.0,
    ]
}

/// Features the `recipe` law sees for one admissible recipe.
pub const RECIPE_FEATURES: [&str; 2] = ["deps", "failed_type_overlap"];

pub fn recipe_features(alt: &Value) -> Vec<f64> {
    vec![alt.int(1).unwrap_or(0) as f64, alt.int(2).unwrap_or(0) as f64]
}

/// The decision laws of this program.
pub fn laws(k: &Kinds) -> Vec<Law> {
    let bind = Law::new(
        "bind",
        k.bind_site,
        k.decided,
        Arc::new(|v: &Value| {
            let cands = v.list_at(2).map_err(|e| e.to_string())?;
            cands.iter().map(|c| Cand::from_value(c).map(|c| bind_features(&c)).map_err(|e| e.to_string())).collect()
        }),
    );
    let recipe = Law::new(
        "recipe",
        k.recipe_site,
        k.decided,
        Arc::new(|v: &Value| Ok(v.list_at(3).map_err(|e| e.to_string())?.iter().map(recipe_features).collect())),
    );
    vec![bind, recipe]
}

/// The handcrafted greedy law: reuse active providers, avoid activation
/// cost, load and providers that failed or were lost before; prefer recipes
/// with few and untroubled dependencies.
pub fn greedy() -> LinearBackend {
    LinearBackend::new("greedy")
        .with("bind", vec![3.0, -1.0, -1.0, 0.5, -1.0, -2.0, 0.0])
        .with("recipe", vec![-1.0, -2.0])
}

// ------------------------------------------------------------ owner logic

#[derive(Default)]
enum ConsOp {
    #[default]
    None,
    Remove(Value),
    Evict(i64),
}

#[derive(Default)]
struct Effects {
    cons: ConsOp,
    ctls: Vec<(i64, Ctl)>,
    needs: Vec<Need>,
    recipe_site: Option<Value>,
}

fn need_for(p: &Prov, idx: usize) -> Need {
    let d = &p.deps[idx];
    Need {
        owner_ty: p.ty,
        owner: p.id,
        dep: idx as i64,
        nonce: d.nonce,
        ty: d.ty,
        regions: p.regions.clone(),
        history: d.history.clone(),
        attempt: d.attempts,
        conflicts: 0,
    }
}

fn untried(p: &Prov) -> Vec<usize> {
    (0..p.recipes.len()).filter(|i| !p.tried.contains(&(*i as i64))).collect()
}

/// Expands the provider into recipe `r`: one need per dependency.
fn choose(p: &mut Prov, r: usize, eff: &mut Effects) {
    p.recipe = r as i64;
    p.phase = ACTIVE;
    p.deps = p.recipes[r]
        .iter()
        .map(|&ty| {
            p.nonce += 1;
            Dep { ty, bound: -1, nonce: p.nonce, attempts: 0, history: vec![] }
        })
        .collect();
    for i in 0..p.deps.len() {
        eff.needs.push(need_for(p, i));
    }
}

fn recipe_site(p: &Prov, alts: &[usize]) -> Value {
    let troubled: BTreeSet<i64> = p.tried.iter().flat_map(|&r| p.recipes[r as usize].iter().copied()).collect();
    let items = alts.iter().map(|&r| {
        let rec = &p.recipes[r];
        tup![r as i64, rec.len() as i64, rec.iter().filter(|t| troubled.contains(t)).count() as i64]
    });
    tup![p.ty, p.id, p.epoch, Value::list(items)]
}

/// Picks among the untried recipes: directly if only one remains,
/// otherwise through a `recipe` decision site.
fn start_choice(p: &mut Prov, eff: &mut Effects) {
    let alts = untried(p);
    if alts.len() == 1 {
        choose(p, alts[0], eff);
    } else {
        p.phase = CHOOSING;
        eff.recipe_site = Some(recipe_site(p, &alts));
    }
}

fn activate(p: &mut Prov, eff: &mut Effects) {
    p.tried.clear();
    start_choice(p, eff);
}

fn release_deps(p: &mut Prov, eff: &mut Effects) {
    for (i, d) in p.deps.iter().enumerate() {
        if d.bound >= 0 {
            eff.ctls.push((d.ty, Ctl::Release { provider: d.bound, owner: p.id, dep: i as i64, nonce: d.nonce }));
        }
    }
    p.deps.clear();
}

fn deactivate(p: &mut Prov, eff: &mut Effects) {
    release_deps(p, eff);
    p.phase = INACTIVE;
    p.recipe = -1;
    p.tried.clear();
    p.epoch += 1;
}

fn die(p: &mut Prov, eff: &mut Effects) {
    release_deps(p, eff);
    p.phase = DEAD;
    p.recipe = -1;
    if !p.consumers.is_empty() {
        eff.cons = ConsOp::Evict(-1);
    }
    p.consumers.clear();
    p.epoch += 1;
}

/// A dependency could not be satisfied (no admissible provider) or was lost.
fn dep_failure(p: &mut Prov, idx: usize, eff: &mut Effects, cfg: &DepConfig) {
    p.fails += 1;
    p.deps[idx].attempts += 1;
    if p.deps[idx].attempts < cfg.max_attempts {
        p.nonce += 1;
        p.deps[idx].nonce = p.nonce;
        eff.needs.push(need_for(p, idx));
        return;
    }
    if p.root {
        release_deps(p, eff);
        p.phase = GAVE_UP;
        return;
    }
    p.tried.push(p.recipe);
    release_deps(p, eff);
    p.epoch += 1;
    if untried(p).is_empty() {
        die(p, eff);
    } else {
        start_choice(p, eff);
    }
}

fn is_current(p: &Prov, dep: i64, nonce: i64) -> bool {
    p.phase == ACTIVE && p.deps.get(dep as usize).is_some_and(|d| d.nonce == nonce)
}

/// The owner's reaction to a control message addressed to it.
fn handle(mut p: Prov, ctl: &Ctl, cfg: &DepConfig) -> (Prov, Effects) {
    let mut eff = Effects::default();
    let live = |p: &Prov| p.phase == ACTIVE || p.phase == CHOOSING;
    match *ctl {
        Ctl::Start { .. } => {
            if p.phase == INACTIVE {
                activate(&mut p, &mut eff);
            }
        }
        Ctl::Release { owner, dep, nonce, .. } => {
            if let Some(pos) = p.consumers.iter().position(|k| *k == [owner, dep, nonce]) {
                p.consumers.remove(pos);
                eff.cons = ConsOp::Remove(tup![owner, dep, nonce]);
                if p.consumers.is_empty() && !p.root && live(&p) {
                    deactivate(&mut p, &mut eff);
                }
            }
        }
        Ctl::Kill { .. } => {
            if p.alive() && !p.root {
                die(&mut p, &mut eff);
            }
        }
        Ctl::DepBound { dep, nonce, provider, provider_ty, .. } => {
            if is_current(&p, dep, nonce) && p.deps[dep as usize].bound < 0 {
                p.deps[dep as usize].bound = provider;
            } else {
                // Stale: the owner moved on; give the binding back.
                eff.ctls.push((provider_ty, Ctl::Release { provider, owner: p.id, dep, nonce }));
            }
        }
        Ctl::DepFailed { dep, nonce, .. } => {
            if is_current(&p, dep, nonce) && p.deps[dep as usize].bound < 0 {
                dep_failure(&mut p, dep as usize, &mut eff, cfg);
            }
        }
        Ctl::DepLost { dep, nonce, provider, .. } => {
            // The transport may deliver the loss before the grant notification:
            // a pending dependency with the same nonce lost that very binding.
            let bound = if is_current(&p, dep, nonce) { p.deps[dep as usize].bound } else { -2 };
            if bound == provider || bound == -1 {
                p.deps[dep as usize].bound = -1;
                p.deps[dep as usize].history.push(provider);
                dep_failure(&mut p, dep as usize, &mut eff, cfg);
            }
        }
        Ctl::Recipe { recipe, epoch, .. } => {
            if p.phase == CHOOSING && epoch == p.epoch && untried(&p).contains(&(recipe as usize)) {
                choose(&mut p, recipe as usize, &mut eff);
            }
        }
        Ctl::SetCap { capacity, .. } => {
            if p.alive() && !p.root {
                p.capacity = capacity.max(0);
                let excess = p.consumers.len() as i64 - p.capacity;
                if excess > 0 {
                    p.consumers.drain(0..excess as usize);
                    eff.cons = ConsOp::Evict(excess);
                    if p.consumers.is_empty() && live(&p) {
                        deactivate(&mut p, &mut eff);
                    }
                }
            }
        }
        Ctl::Choice { .. } => {}
    }
    (p, eff)
}

// ------------------------------------------------------------ patch helpers

fn send_ctl(cx: &mut RuleCx<'_>, k: &Kinds, dest_ty: i64, ctl: &Ctl) {
    let m = cx.var();
    cx.effect(k.send, tup![market_label(dest_ty)], &[m]);
    cx.agent(k.ctl, ctl.to_value(), m, &[]);
}

/// A new need travelling to its market, its reply channel ending at an
/// `AwaitBind` agent on the owner's side.
fn spawn_need(cx: &mut RuleCx<'_>, k: &Kinds, need: &Need) {
    let (m, r) = (cx.var(), cx.var());
    cx.effect(k.send, tup![market_label(need.ty)], &[m]);
    cx.agent(k.need, need.to_value(), m, &[r]);
    cx.agent(k.await_bind, need.to_value(), r, &[]);
}

fn emit(cx: &mut RuleCx<'_>, k: &Kinds, eff: Effects) {
    for (ty, ctl) in &eff.ctls {
        send_ctl(cx, k, *ty, ctl);
    }
    for need in &eff.needs {
        spawn_need(cx, k, need);
    }
    if let Some(site) = eff.recipe_site {
        cx.effect(k.recipe_site, site, &[]);
    }
}

/// A stale bind decision: send the need back to its market (keeping its
/// reply channel) to be decided again, or fail it after too many conflicts.
fn requeue(cx: &mut RuleCx<'_>, k: &Kinds, mut need: Need, reply: crate::Var, max_conflicts: i64) {
    need.conflicts += 1;
    if need.conflicts > max_conflicts {
        cx.agent(k.failed, (), reply, &[]);
        return;
    }
    let m = cx.var();
    cx.effect(k.send, tup![market_label(need.ty)], &[m]);
    cx.agent(k.need, need.to_value(), m, &[reply]);
}

// ------------------------------------------------------------ hard policies

/// Every binding goes to a live provider of the right type in a region the
/// need allows — whatever the decision said.
struct BindingPolicy {
    bind: KindId,
    prov: KindId,
    granted: KindId,
}

impl Policy for BindingPolicy {
    fn name(&self) -> &str {
        "binding"
    }
    fn check(&self, v: &PatchView<'_>) -> Result<(), String> {
        if v.created_of(self.granted).next().is_none() {
            return Ok(());
        }
        let walker = v.consumed_of(self.bind).next().ok_or("a grant without a binding walk")?;
        let need = Need::from_value(walker.get(0).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let p = Prov::from_value(v.consumed_of(self.prov).next().ok_or("a grant without a provider")?)
            .map_err(|e| e.to_string())?;
        if !need.regions.contains(&p.region) {
            return Err(format!("provider {} in region {} bound to a need allowing {:?}", p.id, p.region, need.regions));
        }
        if p.ty != need.ty || !p.alive() || p.root {
            return Err(format!("provider {} cannot serve a need for type {}", p.id, need.ty));
        }
        Ok(())
    }
}

/// No provider cell ever holds more consumers than its capacity.
struct CapacityPolicy {
    prov: KindId,
}

impl Policy for CapacityPolicy {
    fn name(&self) -> &str {
        "capacity"
    }
    fn check(&self, v: &PatchView<'_>) -> Result<(), String> {
        for p in v.created_of(self.prov) {
            let p = Prov::from_value(p).map_err(|e| e.to_string())?;
            if !p.root && p.consumers.len() as i64 > p.capacity {
                return Err(format!("provider {} would hold {} consumers, capacity {}", p.id, p.consumers.len(), p.capacity));
            }
        }
        Ok(())
    }
}

// ------------------------------------------------------------ the program

pub fn program(cfg: &DepConfig) -> (Program, Kinds) {
    use ValueType as T;
    let mut b = SigBuilder::new("depnet", 1);
    let mailbox = b.polarized("Mailbox");
    let msg = b.polarized("Msg");
    let plist = b.polarized("PList");
    let clist = b.polarized("CList");
    let pend = b.polarized("Pend");
    let chan = b.polarized("Chan");
    let routet = b.polarized("RouteT");
    let dec = b.polarized("Dec");
    let key = T::Tuple(vec![T::Int; 3]);
    let cands = T::List(Box::new(Cand::schema()));
    let alts = T::List(Box::new(T::Tuple(vec![T::Int; 3])));
    let k = Kinds {
        market: b.kind("Market", mailbox.neg()).aux("prov", plist.neg()).aux("pend", pend.neg()).payload(T::Int).done(),
        mcons: b.kind("MCons", mailbox.pos()).aux("msg", msg.neg()).aux("rest", mailbox.neg()).done(),
        dispatch: b
            .kind("Dispatch", msg.neg())
            .aux("pi", plist.neg())
            .aux("qi", pend.neg())
            .aux("po", plist.pos())
            .aux("qo", pend.pos())
            .payload(T::Int)
            .done(),
        prov: b
            .kind("Prov", plist.pos())
            .aux("next", plist.neg())
            .aux("cons", clist.neg())
            .payload(Prov::schema())
            .conserved("provider", 0)
            .done(),
        pnil: b.kind("PNil", plist.pos()).done(),
        centry: b.kind("CEntry", clist.pos()).aux("chan", chan.pos()).aux("next", clist.neg()).payload(key.clone()).done(),
        cnil: b.kind("CNil", clist.pos()).done(),
        pcons: b.kind("PCons", pend.pos()).aux("reply", chan.pos()).aux("next", pend.neg()).payload(Need::schema()).done(),
        pdnil: b.kind("PdNil", pend.pos()).done(),
        need: b.kind("Need", msg.pos()).aux("reply", chan.pos()).payload(Need::schema()).done(),
        ctl: b.kind("Ctl", msg.pos()).payload(Ctl::schema()).done(),
        add_prov: b.kind("AddProv", msg.pos()).aux("cell", plist.neg()).done(),
        gather: b
            .kind("Gather", plist.neg())
            .aux("reply", chan.pos())
            .aux("out", plist.pos())
            .aux("qi", pend.neg())
            .aux("qo", pend.pos())
            .payload(T::Tuple(vec![Need::schema(), cands.clone()]))
            .done(),
        unpark: b
            .kind("Unpark", pend.neg())
            .aux("out", pend.pos())
            .aux("pi", plist.neg())
            .aux("po", plist.pos())
            .payload(Ctl::schema())
            .done(),
        bind: b
            .kind("Bind", plist.neg())
            .aux("reply", chan.pos())
            .aux("out", plist.pos())
            .payload(T::Tuple(vec![Need::schema(), T::Int]))
            .done(),
        visit: b.kind("Visit", plist.neg()).aux("out", plist.pos()).payload(Ctl::schema()).done(),
        append: b.kind("Append", plist.neg()).aux("cell", plist.neg()).aux("out", plist.pos()).done(),
        cremove: b.kind("CRemove", clist.neg()).aux("out", clist.pos()).payload(key.clone()).done(),
        cevict: b.kind("CEvict", clist.neg()).aux("out", clist.pos()).payload(T::Int).done(),
        await_bind: b.kind("AwaitBind", chan.neg()).payload(Need::schema()).done(),
        socket: b.kind("Socket", chan.neg()).payload(T::Tuple(vec![Need::schema(), T::Int])).done(),
        granted: b.kind("Granted", chan.pos()).aux("cont", chan.neg()).payload(T::Int).done(),
        failed: b.kind("Failed", chan.pos()).done(),
        lost: b.kind("Lost", chan.pos()).done(),
        released: b.kind("Released", chan.pos()).done(),
        send: b.kind("Send", routet.neg()).aux("msg", msg.neg()).payload(T::Tuple(vec![T::Str])).effect().done(),
        route: b.kind("Route", routet.pos()).aux("dest", mailbox.pos()).aux("next", mailbox.neg()).done(),
        bind_site: b.kind("BindSite", dec.neg()).payload(T::Tuple(vec![T::Int, Need::schema(), cands])).effect().done(),
        recipe_site: b.kind("RecipeSite", dec.neg()).payload(T::Tuple(vec![T::Int, T::Int, T::Int, alts])).effect().done(),
        decided: b.kind("Decided", dec.pos()).payload(decided_schema()).done(),
    };
    let sig = b.build().expect("depnet signature");
    let rules = rules(&k, cfg);
    let prog = Program::new(sig, rules)
        .expect("depnet rules")
        .with_policy(BindingPolicy { bind: k.bind, prov: k.prov, granted: k.granted })
        .with_policy(CapacityPolicy { prov: k.prov });
    (prog, k)
}

fn rules(k: &Kinds, cfg: &DepConfig) -> RuleTable {
    let mut t = RuleTable::new();
    let k = *k;
    let effects = [k.send, k.recipe_site];

    // A market takes the next message from its mailbox and keeps listening.
    t.def("market_msg", k.mcons, k.market, move |cx| {
        let [msg, rest] = cx.left_aux();
        let [prov, pend] = cx.right_aux();
        let ty = cx.right().clone();
        let (po, qo) = (cx.var(), cx.var());
        cx.agent(k.dispatch, ty.clone(), msg, &[prov, pend, po, qo]);
        cx.agent(k.market, ty, rest, &[po, qo]);
        Ok(())
    });
    t.def("dispatch_need", k.dispatch, k.need, move |cx| {
        let [pi, qi, po, qo] = cx.left_aux();
        let [reply] = cx.right_aux();
        let need = cx.right().clone();
        cx.agent(k.gather, tup![need, Value::list([])], pi, &[reply, po, qi, qo]);
        Ok(())
    });
    t.def("dispatch_ctl", k.dispatch, k.ctl, move |cx| {
        let [pi, qi, po, qo] = cx.left_aux();
        let ctl = cx.right().clone();
        if matches!(Ctl::from_value(&ctl)?, Ctl::Choice { .. }) {
            cx.agent(k.unpark, ctl, qi, &[qo, pi, po]);
        } else {
            cx.agent(k.visit, ctl, pi, &[po]);
            cx.link(qo, qi);
        }
        Ok(())
    });
    t.def("dispatch_add", k.dispatch, k.add_prov, move |cx| {
        let [pi, qi, po, qo] = cx.left_aux();
        let [cell] = cx.right_aux();
        cx.agent(k.append, (), pi, &[cell, po]);
        cx.link(qo, qi);
        Ok(())
    });

    // Gathering: walk the provider list, recording up to K admissible candidates.
    let kmax = cfg.candidates;
    t.def("gather_prov", k.gather, k.prov, move |cx| {
        let [reply, out, qi, qo] = cx.left_aux();
        let [next, cons] = cx.right_aux();
        let need = Need::from_value(cx.left().get(0)?)?;
        let mut cands = cx.left().list_at(1)?.to_vec();
        let p = Prov::from_value(cx.right())?;
        if p.eligible_for(&need.regions) && cands.len() < kmax {
            cands.push(Cand::of(&p, &need).to_value());
        }
        let o2 = cx.var();
        cx.agent(k.prov, cx.right().clone(), out, &[o2, cons]);
        cx.agent(k.gather, tup![need.to_value(), Value::list(cands)], next, &[reply, o2, qi, qo]);
        Ok(())
    });
    // End of the walk: no candidate fails the need; otherwise the need is
    // parked and a `bind` decision site is opened.
    t.def("gather_end", k.gather, k.pnil, move |cx| {
        let [reply, out, qi, qo] = cx.left_aux();
        let (need, cands) = (cx.left().get(0)?.clone(), cx.left().get(1)?.clone());
        cx.agent(k.pnil, (), out, &[]);
        if cands.items().is_none_or(|c| c.is_empty()) {
            cx.agent(k.failed, (), reply, &[]);
            cx.link(qo, qi);
        } else {
            let ty = Need::from_value(&need)?.ty;
            cx.agent(k.pcons, need.clone(), qo, &[reply, qi]);
            cx.effect(k.bind_site, tup![ty, need, cands], &[]);
        }
        Ok(())
    })
    .emits(&[k.bind_site]);

    // A bind decision arrives: find the parked need, then walk to the provider.
    t.def("unpark_cons", k.unpark, k.pcons, move |cx| {
        let [out, pi, po] = cx.left_aux();
        let [reply, next] = cx.right_aux();
        let Ctl::Choice { owner, dep, nonce, provider } = Ctl::from_value(cx.left())? else {
            return Err(cx.reject("unpark without a choice"));
        };
        let need = Need::from_value(cx.right())?;
        if (need.owner, need.dep, need.nonce) == (owner, dep, nonce) {
            cx.link(out, next);
            cx.agent(k.bind, tup![need.to_value(), provider], pi, &[reply, po]);
        } else {
            let o2 = cx.var();
            cx.agent(k.pcons, cx.right().clone(), out, &[reply, o2]);
            cx.agent(k.unpark, cx.left().clone(), next, &[o2, pi, po]);
        }
        Ok(())
    });
    t.def("unpark_end", k.unpark, k.pdnil, move |cx| {
        let [out, pi, po] = cx.left_aux();
        cx.agent(k.pdnil, (), out, &[]);
        cx.link(po, pi);
        Ok(())
    });

    // Enactment of a bind decision, re-validated against the provider's
    // current state (it may have filled up or died since the decision).
    let max_conflicts = cfg.max_conflicts;
    t.def("bind_prov", k.bind, k.prov, move |cx| {
        let [reply, out] = cx.left_aux();
        let [next, cons] = cx.right_aux();
        let need = Need::from_value(cx.left().get(0)?)?;
        let target = cx.left().int(1)?;
        let mut p = Prov::from_value(cx.right())?;
        if p.id != target {
            let o2 = cx.var();
            cx.agent(k.prov, cx.right().clone(), out, &[o2, cons]);
            cx.agent(k.bind, cx.left().clone(), next, &[reply, o2]);
            return Ok(());
        }
        if !p.eligible_for(&need.regions) {
            cx.agent(k.prov, cx.right().clone(), out, &[next, cons]);
            requeue(cx, &k, need, reply, max_conflicts);
            return Ok(());
        }
        p.consumers.insert(0, [need.owner, need.dep, need.nonce]);
        let mut eff = Effects::default();
        if p.phase == INACTIVE {
            activate(&mut p, &mut eff);
        }
        let (c, cons2) = (cx.var(), cx.var());
        cx.agent(k.prov, p.to_value(), out, &[next, cons2]);
        cx.agent(k.centry, need.key(), cons2, &[c, cons]);
        cx.agent(k.granted, p.id, reply, &[c]);
        emit(cx, &k, eff);
        Ok(())
    })
    .emits(&effects);
    t.def("bind_end", k.bind, k.pnil, move |cx| {
        let [reply, out] = cx.left_aux();
        let need = Need::from_value(cx.left().get(0)?)?;
        cx.agent(k.pnil, (), out, &[]);
        requeue(cx, &k, need, reply, max_conflicts);
        Ok(())
    })
    .emits(&[k.send]);

    // Control messages for one provider cell (the owner logic).
    let cfg2 = cfg.clone();
    t.def("visit_prov", k.visit, k.prov, move |cx| {
        let [out] = cx.left_aux();
        let [next, cons] = cx.right_aux();
        let ctl = Ctl::from_value(cx.left())?;
        let p = Prov::from_value(cx.right())?;
        if ctl.target() != Some(p.id) {
            let o2 = cx.var();
            cx.agent(k.prov, cx.right().clone(), out, &[o2, cons]);
            cx.agent(k.visit, cx.left().clone(), next, &[o2]);
            return Ok(());
        }
        let (p, mut eff) = handle(p, &ctl, &cfg2);
        let cons2 = match std::mem::take(&mut eff.cons) {
            ConsOp::None => cons,
            ConsOp::Remove(key) => {
                let c2 = cx.var();
                cx.agent(k.cremove, key, cons, &[c2]);
                c2
            }
            ConsOp::Evict(n) => {
                let c2 = cx.var();
                cx.agent(k.cevict, n, cons, &[c2]);
                c2
            }
        };
        cx.agent(k.prov, p.to_value(), out, &[next, cons2]);
        emit(cx, &k, eff);
        Ok(())
    })
    .emits(&effects);
    t.def("visit_end", k.visit, k.pnil, move |cx| {
        let [out] = cx.left_aux();
        cx.agent(k.pnil, (), out, &[]);
        Ok(())
    });

    // Appending a host-provided cell (a new root or provider) at the end.
    t.def("append_prov", k.append, k.prov, move |cx| {
        let [cell, out] = cx.left_aux();
        let [next, cons] = cx.right_aux();
        let o2 = cx.var();
        cx.agent(k.prov, cx.right().clone(), out, &[o2, cons]);
        cx.agent(k.append, (), next, &[cell, o2]);
        Ok(())
    });
    t.def("append_end", k.append, k.pnil, move |cx| {
        let [cell, out] = cx.left_aux();
        cx.link(out, cell);
        Ok(())
    });

    // Consumer lists: remove one binding by key, or evict the first n (or all).
    t.def("cremove_entry", k.cremove, k.centry, move |cx| {
        let [out] = cx.left_aux();
        let [chan, next] = cx.right_aux();
        if cx.left() == cx.right() {
            cx.agent(k.released, (), chan, &[]);
            cx.link(out, next);
        } else {
            let o2 = cx.var();
            cx.agent(k.centry, cx.right().clone(), out, &[chan, o2]);
            cx.agent(k.cremove, cx.left().clone(), next, &[o2]);
        }
        Ok(())
    });
    t.def("cremove_end", k.cremove, k.cnil, move |cx| {
        let [out] = cx.left_aux();
        cx.agent(k.cnil, (), out, &[]);
        Ok(())
    });
    t.def("cevict_entry", k.cevict, k.centry, move |cx| {
        let [out] = cx.left_aux();
        let [chan, next] = cx.right_aux();
        let n = cx.left().as_int().unwrap_or(0);
        if n == 0 {
            cx.agent(k.centry, cx.right().clone(), out, &[chan, next]);
        } else {
            cx.agent(k.lost, (), chan, &[]);
            cx.agent(k.cevict, if n > 0 { n - 1 } else { n }, next, &[out]);
        }
        Ok(())
    });
    t.def("cevict_end", k.cevict, k.cnil, move |cx| {
        let [out] = cx.left_aux();
        cx.agent(k.cnil, (), out, &[]);
        Ok(())
    });

    // The consumer side of a binding channel.
    t.def("granted", k.granted, k.await_bind, move |cx| {
        let [cont] = cx.left_aux();
        let p = cx.left().as_int().unwrap_or(-1);
        let need = Need::from_value(cx.right())?;
        cx.agent(k.socket, tup![need.to_value(), p], cont, &[]);
        let bound = Ctl::DepBound { owner: need.owner, dep: need.dep, nonce: need.nonce, provider: p, provider_ty: need.ty };
        send_ctl(cx, &k, need.owner_ty, &bound);
        Ok(())
    })
    .emits(&[k.send]);
    t.def("failed", k.failed, k.await_bind, move |cx| {
        let need = Need::from_value(cx.right())?;
        send_ctl(cx, &k, need.owner_ty, &Ctl::DepFailed { owner: need.owner, dep: need.dep, nonce: need.nonce });
        Ok(())
    })
    .emits(&[k.send]);
    t.def("lost", k.lost, k.socket, move |cx| {
        let need = Need::from_value(cx.right().get(0)?)?;
        let provider = cx.right().int(1)?;
        send_ctl(cx, &k, need.owner_ty, &Ctl::DepLost { owner: need.owner, dep: need.dep, nonce: need.nonce, provider });
        Ok(())
    })
    .emits(&[k.send]);
    t.def("released", k.released, k.socket, |_| Ok(()));
    // A binding channel carries Granted or Failed first, then Lost or Released.
    t.forbid(k.granted, k.socket);
    t.forbid(k.failed, k.socket);
    t.forbid(k.lost, k.await_bind);
    t.forbid(k.released, k.await_bind);

    // Transport: the host splices a routed message into a mailbox.
    t.def("route_send", k.route, k.send, move |cx| {
        let [dest, next] = cx.left_aux();
        let [msg] = cx.right_aux();
        cx.agent(k.mcons, (), dest, &[msg, next]);
        Ok(())
    });

    // Decision tokens, enacted by ordinary rules.
    t.def("bind_decided", k.decided, k.bind_site, move |cx| {
        let i = selected_index(cx.left())?;
        let site = cx.right().clone();
        let need = Need::from_value(site.get(1)?)?;
        let cand = site.list_at(2)?.get(i).cloned().ok_or_else(|| cx.reject("selection out of range"))?;
        let provider = Cand::from_value(&cand)?.id;
        let choice = Ctl::Choice { owner: need.owner, dep: need.dep, nonce: need.nonce, provider };
        send_ctl(cx, &k, site.int(0)?, &choice);
        Ok(())
    })
    .emits(&[k.send]);
    t.def("recipe_decided", k.decided, k.recipe_site, move |cx| {
        let i = selected_index(cx.left())?;
        let site = cx.right().clone();
        let alt = site.list_at(3)?.get(i).cloned().ok_or_else(|| cx.reject("selection out of range"))?;
        let recipe = Ctl::Recipe { provider: site.int(1)?, recipe: alt.int(0)?, epoch: site.int(2)? };
        send_ctl(cx, &k, site.int(0)?, &recipe);
        Ok(())
    })
    .emits(&[k.send]);
    t
}
