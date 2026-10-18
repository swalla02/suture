//! The host loop (spec §5.4, as corrected by appendix B.3.6). It runs the
//! kernel, routes internal messages, commits, and only then hands outside
//! requests to adapters. In slice 1 the adapters are simulated: a scenario
//! answers for them, one request at a time.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use suture::{HoleId, Injection, Machine, NetBuilder, Value};
use suture_lang::ir::{third_person, Admit, Admitted, Program, Ty, TIMED_OUT};
use suture_lang::scenario::Caller;

use crate::interp::{self, duration, holds};
use crate::kernel::{self, Kinds};
use crate::state::{answer_value, send_from_value, stamped, Ask, Decide, Fact, Facts, Msg, Unit, Val};

pub struct Host {
    pub prog: Arc<Program>,
    pub machine: Machine,
    pub kinds: Kinds,
    /// One line for each thing that happened: each unit's notes, and each answer.
    pub trace: Vec<String>,
    /// Outside requests waiting for an answer, oldest first, with their effect holes.
    pending: Vec<(Ask, HoleId)>,
    /// The clock, in seconds from the start. Only `pass` moves it (appendix C.3.1).
    pub now: u64,
    /// Timers by due time, then by the order they were started (appendix C.3.2).
    timers: BTreeMap<(u64, u64), Timer>,
    started: u64,
    /// The goal timers already started, by unit and goal.
    goal_timers: BTreeSet<(String, u32)>,
    /// The patience timers already started, by id.
    patience_timers: BTreeSet<String>,
    /// Requests whose deadline passed, so every late answer is noted, not an error.
    expired: Vec<Ask>,
    /// Shared entities by kind and key attribute, then by key, in creation
    /// order (appendix F.2.6).
    pools: BTreeMap<(String, String), BTreeMap<String, Vec<String>>>,
    /// Bindings with no candidate left, in arrival order (appendix F.2.9).
    waiters: Vec<Waiter>,
    /// Every decision of the gate, in order (appendix G.2.2).
    pub gate: Vec<String>,
    /// For each unit, the units that carry its facts: those that refer to
    /// it, and those it granted a binding (appendix G.2.7).
    /// In the order they started reading it.
    readers: BTreeMap<String, Vec<String>>,
}

/// A binding waiting on its pool key, with its choice's hole.
#[derive(Clone)]
struct Waiter {
    hole: HoleId,
    decide: Decide,
    kind: String,
    key: String,
}

/// A pool key as the host indexes it.
fn key_text(v: &Val) -> String {
    format!("{v:?}")
}

/// What a timer is for.
enum Timer {
    /// The deadline of the request whose effect hole this is.
    Deadline(HoleId),
    /// A goal's deadline for a unit.
    Goal(String, u32),
    /// A slot's patience, for a unit (appendix D.3.5).
    Patience(String, String),
}

impl Host {
    pub fn new(prog: Program) -> Host {
        let prog = Arc::new(prog);
        let (kernel, kinds) = kernel::program(prog.clone());
        let mut h = Host {
            prog,
            machine: Machine::new(kernel),
            kinds,
            trace: vec![],
            pending: vec![],
            now: 0,
            timers: BTreeMap::new(),
            started: 0,
            goal_timers: BTreeSet::new(),
            patience_timers: BTreeSet::new(),
            expired: vec![],
            pools: BTreeMap::new(),
            waiters: vec![],
            gate: vec![],
            readers: BTreeMap::new(),
        };
        // One unit holds each total `checked globally` (appendix F.2.13).
        for g in h.prog.globals.clone() {
            if let Err(e) = h.create(&g, &g, &[]) {
                h.trace.push(format!("{g}: {e}"));
            }
        }
        h
    }

    /// Creates a root entity as a new unit labeled `label`, and settles it.
    pub fn create(&mut self, kind: &str, label: &str, given: &[(String, Val)]) -> Result<(), String> {
        self.create_with(kind, label, given, &[])
    }

    /// Creates a root entity with the elements of its lists (appendix E.2.6),
    /// as part of the world: it does not pass the gate.
    pub fn create_with(&mut self, kind: &str, label: &str, given: &[(String, Val)], items: &[(String, Vec<(String, Val)>)]) -> Result<(), String> {
        self.make(None, kind, label, given, items)
    }

    /// A caller's input that creates a root entity: it passes the gate first
    /// (appendix G.2.2).
    pub fn input_new(
        &mut self,
        by: Option<&Caller>,
        kind: &str,
        label: &str,
        given: &[(String, Val)],
        items: &[(String, Vec<(String, Val)>)],
    ) -> Result<(), String> {
        self.make(Some(by), kind, label, given, items)
    }

    /// A caller's admitted event on an existing unit (appendix G.2.4). It
    /// passes the gate, then its reactions run as one step, which a rule may
    /// refuse.
    pub fn input_event(&mut self, by: Option<&Caller>, target: &str) -> Result<(), String> {
        let unit = self.unit(target).ok_or_else(|| format!("there is no unit {target}"))?;
        let kind = unit.root.kind.clone();
        let fits = |a: &Admitted| matches!(a, Admitted::Event { verb, kind: k } if by.is_some_and(|c| third_person(verb) == c.verb) && *k == kind);
        self.gate(by, fits, target, &unit)?;
        let Some(by) = by else { return Ok(()) };
        if !self.prog.reactions.iter().any(|r| r.kind == kind && r.outcome == by.verb && r.by.contains(&by.role)) {
            return Err(format!("nothing reacts when a {} {} {}", by.role, by.verb, interp::a_kind(&kind)));
        }
        self.tell(target, &Msg::Event { verb: by.verb.clone(), by: format!("{} {}", by.role, by.name) })?;
        let refused = self.unit(target).and_then(|u| u.notes.iter().find_map(|n| n.split_once(": refused by rule ").map(|(_, r)| r.to_string())));
        match refused {
            Some(r) => Err(format!("refused by rule {r}")),
            None => Ok(()),
        }
    }

    /// A caller's admitted change to an attribute (appendix G.2.5, G.2.7).
    /// It passes the gate; then the changed unit and every unit that carries
    /// its facts are checked with the new value before anything changes.
    pub fn input_change(&mut self, by: Option<&Caller>, target: &str, attr: &str, value: Val) -> Result<(), String> {
        let unit = self.unit(target).ok_or_else(|| format!("there is no unit {target}"))?;
        let kind = unit.root.kind.clone();
        if let Some(why) = suture_lang::check::unchangeable(&self.prog, &kind, attr) {
            return Err(why);
        }
        let ty = self.prog.kind(&kind).and_then(|k| k.attr(attr)).map(|a| a.ty).ok_or_else(|| format!("{} has no attribute {attr}", interp::a_kind(&kind)))?;
        let value = match (ty, value) {
            (Ty::Ref | Ty::Set, _) => return Err(format!("{kind}.{attr} refers to shared entities, which cannot be changed yet")),
            (t, v) if v.ty() == t => v,
            (t, v) => return Err(format!("{kind}.{attr} is {t:?}, not {}", v.show(&self.prog.currency))),
        };
        let fits = |a: &Admitted| matches!(a, Admitted::Change { kind: k, attr: x } if by.is_some_and(|c| c.verb == "changes") && *k == kind && x == attr);
        self.gate(by, fits, &format!("{target}'s {attr} to {}", value.show(&self.prog.currency)), &unit)?;
        let who = by.map_or(String::new(), |c| format!("{} {}", c.role, c.name));
        let msg = Msg::Change { attr: attr.to_string(), value, by: who };
        let refusal = |u: &Unit| u.notes.iter().find_map(|n| n.split_once(": refused by rule ").map(|(_, r)| r.to_string()));
        let after = interp::step_at(&self.prog, &unit, &msg, self.clock()).unit;
        if let Some(r) = refusal(&after) {
            self.trace.push(format!("{target:<3} {}", after.notes.join("; ")));
            return Err(format!("refused by rule {r}"));
        }
        let fact = Fact { kind, created: after.root.created, attrs: after.root.attrs.clone() };
        let refresh = Msg::Refresh { facts: Facts::from([(target.to_string(), fact)]) };
        let readers: Vec<String> = self.readers.get(target).cloned().unwrap_or_default();
        for r in &readers {
            let Some(u) = self.unit(r) else { continue };
            let after = interp::step_at(&self.prog, &u, &refresh, self.clock()).unit;
            if let Some(rule) = interp::newly_violated(&self.prog, &u, &after) {
                self.trace.push(format!("{target:<3} change refused: {r} would break rule {}", rule.name));
                return Err(format!("refused by rule {}: {} (in {r})", rule.name, rule.paraphrase));
            }
        }
        // Bindings already waiting get the change first, in arrival order
        // (appendix F.2.9); then each reader, in the order it started reading.
        self.tell(target, &msg)?;
        self.wake(target)?;
        self.pump()?;
        for r in &readers {
            self.tell(r, &refresh)?;
        }
        Ok(())
    }

    fn read_by(&mut self, read: &str, by: &str) {
        let readers = self.readers.entry(read.to_string()).or_default();
        if !readers.iter().any(|r| r == by) {
            readers.push(by.to_string());
        }
    }

    /// The gate's decisions about the unit called `name`.
    pub fn gate_for(&self, name: &str) -> Vec<&String> {
        self.gate.iter().filter(|l| l.split(|c: char| !c.is_alphanumeric()).any(|w| w == name)).collect()
    }

    fn make(
        &mut self,
        gated: Option<Option<&Caller>>,
        kind: &str,
        label: &str,
        given: &[(String, Val)],
        items: &[(String, Vec<(String, Val)>)],
    ) -> Result<(), String> {
        if self.machine.hole(label).is_some() {
            return Err(format!("{label} already exists"));
        }
        let given = self.resolve(kind, given)?;
        let mut elements = vec![];
        for (list, attrs) in items {
            let element = self.prog.lists.iter().find(|l| l.owner == kind && l.name == *list).map_or(String::new(), |l| l.element.clone());
            elements.push((list.clone(), self.resolve(&element, attrs)?));
        }
        let mut facts = Facts::new();
        for (_, v) in given.iter().chain(elements.iter().flat_map(|(_, attrs)| attrs)) {
            self.carry(v, &mut facts);
        }
        let unit = interp::build_unit(&self.prog, label, kind, &given, &elements, facts, self.clock())?;
        if let Some(by) = gated {
            let fits = |a: &Admitted| matches!(a, Admitted::New { verb, kind: k } if by.is_some_and(|c| third_person(verb) == c.verb) && k == kind);
            self.gate(by, fits, &format!("a new {kind} {label}"), &unit)?;
        }
        interp::refused_at_creation(&self.prog, &unit, self.clock())?;
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let (meet, rest) = (b.var(), b.var());
        b.agent(self.kinds.unit, unit.to_value(), meet, &[]);
        b.agent(self.kinds.msg, stamped(self.clock(), &Msg::Create.to_value()), meet, &[rest]);
        let net = b.finish().map_err(|e| e.to_string())?;
        self.machine.inject(Injection::new(net).new_hole(rest, Some(label))).map_err(|e| e.to_string())?;
        self.machine.run(None);
        for l in unit.facts.keys() {
            self.read_by(l, label);
        }
        let noted = self.note(label);
        let pumped = self.pump();
        // A unit that exists is a candidate, even if its first step reported an error.
        self.index(label);
        noted.and(pumped)?;
        let woken = self.wake(label);
        let pumped = self.pump();
        woken.and(pumped)
    }

    /// Adds a new unit to the pools of every binding to its kind.
    fn index(&mut self, label: &str) {
        let Some(u) = self.unit(label) else { return };
        for (attr, keys) in self.keys_of(&u) {
            let pool = self.pools.entry((u.root.kind.clone(), attr)).or_default();
            for k in keys {
                pool.entry(k).or_default().push(label.to_string());
            }
        }
    }

    /// The pool keys a shared entity is indexed by, for each key attribute.
    fn keys_of(&self, u: &Unit) -> Vec<(String, Vec<String>)> {
        let mut attrs: Vec<&str> = self.prog.bindings.iter().filter(|b| b.target == u.root.kind).map(|b| b.key_attr.as_str()).collect();
        attrs.sort();
        attrs.dedup();
        attrs
            .into_iter()
            .map(|a| {
                if a.is_empty() {
                    return (String::new(), vec![key_text(&Val::Ref(u.label.clone()))]);
                }
                let keys = match u.root.attrs.get(a) {
                    Some(Val::Set(ls)) => ls.iter().map(|l| key_text(&Val::Ref(l.clone()))).collect(),
                    Some(v) => vec![key_text(v)],
                    None => vec![],
                };
                (a.to_string(), keys)
            })
            .collect()
    }

    /// The choice for a binding (appendix F.2.7): the pick is injected at the
    /// request's hole, so the journal holds it. With no candidate left, the
    /// binding waits, and its unit is told.
    fn decide(&mut self, hole: HoleId, payload: &Value) -> Result<(), String> {
        let d = Decide::from_value(payload)?;
        // A newer request for the same binding supersedes any waiter for it.
        self.waiters.retain(|w| w.decide.key() != d.key());
        let u = self.unit(&d.label).ok_or_else(|| format!("{}: a choice for a unit that is gone", d.label))?;
        let b = self.prog.bindings.get(d.binding as usize).ok_or("a choice for an unknown binding")?;
        let key = match interp::decide_key(&self.prog, &u, &d) {
            // A key that names the unit itself (appendix F.2.11).
            Some(Val::Text(l)) if b.key_attr.is_empty() => key_text(&Val::Ref(l)),
            Some(k) => key_text(&k),
            None => String::new(),
        };
        let kind = b.target.clone();
        match self.gather(&u, &d, &key) {
            Some(label) => self.pick(hole, &label, &d),
            None => {
                self.waiters.push(Waiter { hole, decide: d.clone(), kind, key });
                self.post(&d.label, &Msg::Waiting { key: d.key() })
            }
        }
    }

    /// The pick among the pool's candidates for this key, if any.
    fn gather(&self, u: &Unit, d: &Decide, key: &str) -> Option<String> {
        let b = self.prog.bindings.get(d.binding as usize)?;
        let labels = self.pools.get(&(b.target.clone(), b.key_attr.clone()))?.get(key)?;
        let candidates = labels.iter().filter(|l| !d.skip.contains(l)).filter_map(|l| self.unit(l));
        interp::choose_among(&self.prog, u, d, candidates).map(|(label, _)| label)
    }

    fn pick(&mut self, hole: HoleId, label: &str, d: &Decide) -> Result<(), String> {
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let meet = b.var();
        b.agent(self.kinds.pick, label, meet, &[]);
        let net = b.finish().map_err(|e| e.to_string())?;
        let inj = Injection::new(net).at_hole(meet, hole).key(format!("pick:{:032x}", hole.0));
        self.machine.inject(inj).map_err(|e| e.to_string())?;
        self.trace.push(format!("      choice: {label} for {}", d.key()));
        Ok(())
    }

    /// Something of `label`'s may have freed up: the bindings waiting on its
    /// keys gather again from scratch, in arrival order.
    fn wake(&mut self, label: &str) -> Result<(), String> {
        let Some(u) = self.unit(label) else { return Ok(()) };
        let keys: Vec<String> = self.keys_of(&u).into_iter().flat_map(|(_, k)| k).collect();
        let mut i = 0;
        while i < self.waiters.len() {
            let w = self.waiters[i].clone();
            if w.kind != u.root.kind || !keys.contains(&w.key) {
                i += 1;
                continue;
            }
            let still = self.unit(&w.decide.label).filter(|r| interp::waiting(r, &w.decide.key()));
            let Some(requester) = still else {
                self.waiters.remove(i);
                continue;
            };
            let fresh = Decide { skip: vec![], ..w.decide.clone() };
            match self.gather(&requester, &fresh, &w.key) {
                Some(pick) => {
                    self.waiters.remove(i);
                    self.pick(w.hole, &pick, &fresh)?;
                }
                None => i += 1,
            }
        }
        Ok(())
    }

    /// The clock, as entities and messages read it.
    fn clock(&self) -> i64 {
        i64::try_from(self.now).unwrap_or(i64::MAX)
    }

    /// Given values as the kind's attributes hold them: a name given for a
    /// reference (or a set) must name a unit of that kind (appendix F.2.2).
    fn resolve(&self, kind: &str, given: &[(String, Val)]) -> Result<Vec<(String, Val)>, String> {
        let Some(k) = self.prog.kind(kind) else { return Ok(given.to_vec()) };
        let mut out = vec![];
        for (name, v) in given {
            let Some(a) = k.attr(name).filter(|a| matches!(a.ty, Ty::Ref | Ty::Set)) else {
                out.push((name.clone(), v.clone()));
                continue;
            };
            let target = a.of.as_deref().unwrap_or_default();
            let names = match v {
                Val::Text(l) | Val::Ref(l) => vec![l.clone()],
                Val::Set(ls) => ls.clone(),
                other => return Err(format!("{kind}.{name} refers to a {target}, not {}", other.show(&self.prog.currency))),
            };
            for l in &names {
                match self.unit(l) {
                    Some(u) if u.root.kind == target => {}
                    Some(u) => return Err(format!("{kind}.{name} refers to a {target}, but {l} is a {}", u.root.kind)),
                    None => return Err(format!("{kind}.{name}: there is no {target} {l}")),
                }
            }
            let v = match (a.ty, names.len()) {
                (Ty::Ref, 1) => Val::Ref(names[0].clone()),
                (Ty::Ref, _) => return Err(format!("{kind}.{name} refers to one {target}")),
                _ => {
                    let mut names = names;
                    names.sort();
                    names.dedup();
                    Val::Set(names)
                }
            };
            out.push((name.clone(), v));
        }
        Ok(out)
    }

    /// The gate (appendix G.2.2): whether some `admit` lets this caller do
    /// what `fits` describes to `target` (`what`, as in `a new Order O1`).
    /// The decision is kept either way.
    fn gate(&mut self, by: Option<&Caller>, fits: impl Fn(&Admitted) -> bool, what: &str, target: &Unit) -> Result<(), String> {
        if self.prog.admits.is_empty() {
            return Ok(());
        }
        let decision = self.gate_check(by, fits, target);
        let input = by.map_or(what.to_string(), |c| format!("{} {} {} {what}", c.role, c.name, c.verb));
        let line = match &decision {
            Ok(()) => format!("admitted: {input}"),
            Err(e) => format!("refused at the gate: {input}: {e}"),
        };
        self.trace.push(format!("gate: {line}"));
        self.gate.push(line);
        decision.map_err(|e| format!("refused at the gate: {e}"))
    }

    fn gate_check(&self, by: Option<&Caller>, fits: impl Fn(&Admitted) -> bool, target: &Unit) -> Result<(), String> {
        let by = by.ok_or("an input needs a caller")?;
        let role = self.prog.role(&by.role).ok_or_else(|| format!("there is no role {}", by.role))?;
        let mut target = target.clone();
        let caller = match &role.kind {
            Some(k) => {
                match self.unit(&by.name) {
                    Some(u) if u.root.kind == *k => {}
                    _ => return Err(format!("there is no {k} {}", by.name)),
                }
                self.carry(&Val::Ref(by.name.clone()), &mut target.facts);
                Some(by.name.as_str())
            }
            None => None,
        };
        let admits: Vec<&Admit> = self.prog.admits.iter().filter(|a| fits(&a.what) && a.roles.contains(&by.role)).collect();
        let Some(first) = admits.first() else {
            return Err(format!("no `admit` lets a {} do that", by.role));
        };
        if admits.iter().any(|a| a.cond.as_ref().is_none_or(|c| interp::admits(&self.prog, &target, caller, c))) {
            Ok(())
        } else {
            Err(format!("the condition of `admit {}` does not hold", first.source))
        }
    }

    /// Carries the units a value refers to, with the facts they carry
    /// themselves (appendix F.2.3).
    fn carry(&self, v: &Val, facts: &mut Facts) {
        let labels = match v {
            Val::Ref(l) => vec![l.clone()],
            Val::Set(ls) => ls.clone(),
            _ => return,
        };
        for l in labels {
            if facts.contains_key(&l) {
                continue;
            }
            let Some(u) = self.unit(&l) else { continue };
            facts.insert(l, Fact { kind: u.root.kind.clone(), created: u.root.created, attrs: u.root.attrs.clone() });
            for (k, f) in u.facts {
                facts.entry(k).or_insert(f);
            }
        }
    }

    /// The adapter answers its oldest pending request, or the oldest for `target`.
    /// An answer for a request that already timed out is noted and ignored.
    pub fn answer(&mut self, adapter: &str, outcome: &str, data: &[(String, Val)], target: Option<&str>) -> Result<(), String> {
        if outcome == TIMED_OUT {
            return Err(format!("only the runtime reports `{TIMED_OUT}`; {adapter} must answer with an outcome of its effect"));
        }
        let matches = |a: &Ask| a.adapter == adapter && target.is_none_or(|t| a.label == t);
        let Some(i) = self.pending.iter().position(|(a, _)| matches(a)) else {
            let Some(ask) = self.expired.iter().rfind(|a| matches(a)).cloned() else {
                return Err(format!("{adapter} has no pending request{}", target.map(|t| format!(" for {t}")).unwrap_or_default()));
            };
            self.trace.push(format!("      {adapter}: {outcome}   ({})", ask.key));
            self.trace.push(format!("{:<3} ignored a late answer for {}: it timed out", ask.label, ask.key));
            return Ok(());
        };
        let hole = self.pending[i].1;
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let meet = b.var();
        b.agent(self.kinds.answer, answer_value(outcome, data), meet, &[]);
        let net = b.finish().map_err(|e| e.to_string())?;
        let inj = Injection::new(net).at_hole(meet, hole).key(format!("answer:{:032x}", hole.0));
        // An answer that cannot be delivered leaves its request waiting (spec §5.5).
        self.machine.inject(inj).map_err(|e| e.to_string())?;
        let (ask, _) = self.pending.remove(i);
        let shown: Vec<String> = data.iter().map(|(k, v)| format!("{k} {}", v.show(&self.prog.currency))).collect();
        let tail = if shown.is_empty() { String::new() } else { format!(", {}", shown.join(", ")) };
        self.trace.push(format!("      {adapter}: {outcome}{tail}   ({})", ask.key));
        self.pump()
    }

    /// Moves the clock forward by `secs`, firing in order every timer due by
    /// then; while a timer fires, the clock reads its due time (appendix C.3.2).
    pub fn pass(&mut self, secs: u64) -> Result<(), String> {
        let until = self.now + secs;
        let mut first_error = None;
        while let Some((&(due, seq), _)) = self.timers.first_key_value() {
            if due > until {
                break;
            }
            let timer = self.timers.remove(&(due, seq)).expect("the first timer exists");
            self.now = due;
            if let Err(e) = self.fire(timer) {
                first_error.get_or_insert(e);
            }
        }
        self.now = until;
        first_error.map_or(Ok(()), Err)
    }

    /// When the request with this key is due, if it is pending.
    pub fn deadline(&self, key: &str) -> Option<u64> {
        let hole = self.pending.iter().find(|(a, _)| a.key == key)?.1;
        self.timers.iter().find(|(_, t)| matches!(t, Timer::Deadline(h) if *h == hole)).map(|(&(due, _), _)| due)
    }

    fn schedule(&mut self, due: u64, timer: Timer) {
        self.started += 1;
        self.timers.insert((due, self.started), timer);
    }

    /// Fires a timer. One whose reason is gone does nothing: no step.
    fn fire(&mut self, timer: Timer) -> Result<(), String> {
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        match timer {
            Timer::Deadline(hole) => {
                let Some(i) = self.pending.iter().position(|(_, h)| *h == hole) else { return Ok(()) };
                let meet = b.var();
                b.agent(self.kinds.answer, answer_value(TIMED_OUT, &[]), meet, &[]);
                let net = b.finish().map_err(|e| e.to_string())?;
                let inj = Injection::new(net).at_hole(meet, hole).key(format!("deadline:{:032x}", hole.0));
                self.machine.inject(inj).map_err(|e| e.to_string())?;
                let (ask, _) = self.pending.remove(i);
                let within = self.prog.request_within(&ask.kind);
                self.trace.push(format!("      {}: no answer within {}   ({})", ask.adapter, duration(within), ask.key));
                self.expired.push(ask);
                self.pump()
            }
            Timer::Goal(label, goal) => {
                let Some(unit) = self.unit(&label) else { return Ok(()) };
                let g = &self.prog.goals[goal as usize];
                let applies = g.filter.iter().all(|f| holds(&self.prog, &unit, &[], f));
                if !applies || unit.met.contains(&goal) || unit.alerts.contains(&goal) {
                    return Ok(());
                }
                self.tell(&label, &Msg::GoalDue { goal })
            }
            Timer::Patience(label, id) if id.contains('~') => self.tell(&label, &Msg::Due { id }),
            Timer::Patience(label, id) => {
                let Some(unit) = self.unit(&label) else { return Ok(()) };
                if interp::patience_due(&self.prog, &unit, &id).is_none() {
                    return Ok(());
                }
                self.tell(&label, &Msg::Patience { id })
            }
        }
    }

    /// Delivers a message from the host straight to a unit's mailbox, as a
    /// journaled step, and notes what the unit did.
    fn tell(&mut self, label: &str, msg: &Msg) -> Result<(), String> {
        let posted = self.post(label, msg);
        let pumped = self.pump();
        posted.and(pumped)
    }

    /// [`Host::tell`] without running what the step sends out.
    fn post(&mut self, label: &str, msg: &Msg) -> Result<(), String> {
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let (to, next) = (b.var(), b.var());
        b.agent(self.kinds.msg, stamped(self.clock(), &msg.to_value()), to, &[next]);
        let net = b.finish().map_err(|e| e.to_string())?;
        self.machine.inject(Injection::new(net).at_label(to, label).new_hole(next, Some(label))).map_err(|e| e.to_string())?;
        self.machine.run(None);
        self.note(label)
    }

    /// A unit's current state, read from the graph.
    pub fn unit(&self, label: &str) -> Option<Unit> {
        let h = self.machine.hole(label)?;
        let (_, port, agent) = self.machine.hole_peer(h)?;
        if agent.kind != self.kinds.unit || port != 0 {
            return None;
        }
        Unit::from_value(&agent.payload).ok()
    }

    /// The outside requests still waiting for an answer, oldest first.
    pub fn pending(&self) -> Vec<&Ask> {
        self.pending.iter().map(|(a, _)| a).collect()
    }

    /// Runs until nothing is left to do. Internal messages are routed as they
    /// appear. Then the journal is committed, and only after that are new
    /// outside requests handed to adapters (`ir-spec.md` §8.2). A step that
    /// faulted is committed too, so the journal replays it; the first error
    /// is returned once everything else is done.
    fn pump(&mut self) -> Result<(), String> {
        let mut first_error = None;
        loop {
            self.machine.run(None);
            let effects = self.machine.take_new_effects();
            let mut outside = vec![];
            for e in &effects {
                let done = if e.kind == self.kinds.send {
                    self.route(e.hole, &e.payload)
                } else if e.kind == self.kinds.decide {
                    self.decide(e.hole, &e.payload)
                } else if e.kind == self.kinds.ask {
                    Ask::from_value(&e.payload).map(|a| outside.push((a, e.hole)))
                } else {
                    Ok(())
                };
                if let Err(x) = done {
                    first_error.get_or_insert(x);
                }
            }
            self.machine.commit().map_err(|e| e.to_string())?;
            for (ask, hole) in outside {
                let within = self.prog.request_within(&ask.kind);
                self.expired.retain(|a| a.key != ask.key);
                self.schedule(self.now + within, Timer::Deadline(hole));
                self.pending.push((ask, hole));
            }
            if effects.is_empty() {
                return first_error.map_or(Ok(()), Err);
            }
        }
    }

    /// Delivers an internal message to its unit's mailbox, and notes what the unit did.
    fn route(&mut self, hole: HoleId, payload: &Value) -> Result<(), String> {
        let (label, msg) = send_from_value(payload)?;
        let release = matches!(Msg::from_value(&msg), Ok(Msg::Release { .. }));
        if let Ok(Msg::Granted { facts, .. }) = Msg::from_value(&msg) {
            for l in facts.keys() {
                self.read_by(l, &label);
            }
        }
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let (meet, to, next) = (b.var(), b.var(), b.var());
        b.agent(self.kinds.deliver, self.clock(), meet, &[to, next]);
        let net = b.finish().map_err(|e| e.to_string())?;
        let inj = Injection::new(net).at_hole(meet, hole).at_label(to, &label).new_hole(next, Some(&label));
        self.machine.inject(inj).map_err(|e| e.to_string())?;
        self.machine.run(None);
        self.note(&label)?;
        if release { self.wake(&label) } else { Ok(()) }
    }

    /// Adds the unit's notes from its last step to the trace.
    fn note(&mut self, label: &str) -> Result<(), String> {
        let Some(unit) = self.unit(label) else {
            let faults: Vec<String> = self.machine.faults().into_iter().map(|f| f.error).collect();
            return Err(format!("{label}: a step faulted: {}", faults.join("; ")));
        };
        for n in &unit.notes {
            self.trace.push(format!("{label:<3} {n}"));
        }
        for (id, secs) in &unit.timers {
            if self.patience_timers.insert(id.clone()) {
                self.schedule(self.now + secs, Timer::Patience(label.to_string(), id.clone()));
            }
        }
        for &g in &unit.matched {
            let within = self.prog.goals.get(g as usize).and_then(|x| x.within_secs);
            if let Some(secs) = within {
                if self.goal_timers.insert((label.to_string(), g)) {
                    self.schedule(self.now + secs, Timer::Goal(label.to_string(), g));
                }
            }
        }
        Ok(())
    }
}
