//! The host loop (spec §5.4, as corrected by appendix B.3.6). It runs the
//! kernel, routes internal messages, commits, and only then hands outside
//! requests to adapters. In slice 1 the adapters are simulated: a scenario
//! answers for them, one request at a time.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use suture::{HoleId, Injection, Machine, NetBuilder, Value};
use suture_lang::ir::{third_person, Admit, Admitted, Program, Ty, TIMED_OUT};
use suture_lang::scenario::Caller;

use crate::interp::{self, duration};
use crate::kernel::{self, Kinds};
use crate::choice::{Answer, Choice, Models, Option_};
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
    /// The decision models, by `name@version` (appendix J.2.7).
    pub models: Models,
    /// Every model answer, by choice key, in the order they were given
    /// (appendix L.2.1).
    pub model_answers: Vec<(String, Answer)>,
    /// Answers to give again on replay instead of asking (appendix L.2.3).
    pub replay: VecDeque<(String, Answer)>,
    /// The secret store: each secret value by its number (appendix N.2.4).
    pub secrets: BTreeMap<u64, String>,
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
            models: Models::default(),
            model_answers: vec![],
            replay: VecDeque::new(),
            secrets: BTreeMap::new(),
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
    ///
    /// With `part`, the event goes to the unit's one part of that kind
    /// (appendix R.2.2).
    pub fn input_event(&mut self, by: Option<&Caller>, target: &str, part: Option<&str>, data: &[(String, Val)]) -> Result<(), String> {
        let unit = self.unit(target).ok_or_else(|| format!("there is no unit {target}"))?;
        let kind = part.map_or_else(|| unit.root.kind.clone(), str::to_string);
        let fits = |a: &Admitted| matches!(a, Admitted::Event { verb, kind: k } if by.is_some_and(|c| third_person(verb) == c.verb) && *k == kind);
        let at = match part {
            Some(p) => interp::part_at(&unit, p),
            None => Ok(vec![]),
        };
        let shown_target = part.map_or_else(|| target.to_string(), |p| format!("{target}'s {p}"));
        let checked = at.and_then(|at| self.event_data(fits, data).map(|d| (at, d)));
        let (at, data) = match (by, checked) {
            (Some(c), Err(e)) => {
                let line = format!("refused at the gate: {} {} {} {shown_target}: {e}", c.role, c.name, c.verb);
                self.trace.push(format!("gate: {line}"));
                self.gate.push(line);
                return Err(format!("refused at the gate: {e}"));
            }
            (_, d) => d.unwrap_or_default(),
        };
        self.gate_with(by, fits, &shown_target, &unit, &data, &at)?;
        let Some(by) = by else { return Ok(()) };
        if !self.prog.reactions.iter().any(|r| r.kind == kind && r.outcome == by.verb && r.by.contains(&by.role)) {
            return Err(format!("nothing reacts when a {} {} {}", by.role, by.verb, interp::a_kind(&kind)));
        }
        let mut facts = Facts::new();
        if self.prog.role(&by.role).is_some_and(|r| r.kind.is_some()) {
            self.carry(&Val::Ref(by.name.clone()), &mut facts);
        }
        let part = part.unwrap_or_default().to_string();
        self.tell(target, &Msg::Event { verb: by.verb.clone(), by: format!("{} {}", by.role, by.name), part, data, facts })?;
        let refused = self.unit(target).and_then(|u| u.notes.iter().find_map(|n| n.strip_prefix("refused ")?.split_once(": refused by rule ").map(|(_, r)| r.to_string())));
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
        let value = self.seal(&kind, vec![(attr.to_string(), value)])?.remove(0).1;
        self.gate(by, fits, &format!("{target}'s {attr} to {}", interp::shown(&self.prog, attr, &value)), &unit)?;
        let who = by.map_or(String::new(), |c| format!("{} {}", c.role, c.name));
        let msg = Msg::Change { attr: attr.to_string(), value, by: who };
        let refusal = |u: &Unit| u.notes.iter().find_map(|n| n.strip_prefix("refused ")?.split_once(": refused by rule ").map(|(_, r)| r.to_string()));
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
        let given = self.seal(kind, given)?;
        let mut elements = vec![];
        for (list, attrs) in items {
            let element = self.prog.lists.iter().find(|l| l.owner == kind && l.name == *list).map_or(String::new(), |l| l.element.clone());
            let attrs = self.resolve(&element, attrs)?;
            elements.push((list.clone(), self.seal(&element, attrs)?));
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
        let kind = b.target.clone();
        let key = self.pool_key(&u, &d);
        match self.gather(&u, &d, &key) {
            Some((label, how)) => self.pick(hole, &label, &d, &how),
            None => {
                self.waiters.push(Waiter { hole, decide: d.clone(), kind, key });
                self.post(&d.label, &Msg::Waiting { key: d.key() })
            }
        }
    }

    /// The pool key a binding's choice draws from.
    fn pool_key(&self, u: &Unit, d: &Decide) -> String {
        let empty = self.prog.bindings.get(d.binding as usize).is_some_and(|b| b.key_attr.is_empty());
        match interp::decide_key(&self.prog, u, d) {
            // A key that names the unit itself (appendix F.2.11).
            Some(Val::Text(l)) if empty => key_text(&Val::Ref(l)),
            Some(k) => key_text(&k),
            None => String::new(),
        }
    }

    /// The pick among the pool's candidates for this key, if any, and how it
    /// was made: by `prefer`, or by the model that handles the choice, which
    /// may pick only among the options (appendix J.2.4).
    fn gather(&mut self, u: &Unit, d: &Decide, key: &str) -> Option<(String, String)> {
        let prog = self.prog.clone();
        let b = prog.bindings.get(d.binding as usize)?;
        let labels = self.pools.get(&(b.target.clone(), b.key_attr.clone()))?.get(key)?.clone();
        let candidates = labels.iter().filter(|l| !d.skip.contains(l)).filter_map(|l| self.unit(l)).collect::<Vec<_>>();
        let options = interp::options(&prog, u, d, candidates.into_iter());
        let first = options.first()?.label.clone();
        // With one option there is no choice, and no model is asked (appendix K.2.2).
        let model = interp::handler(&prog, u, d).and_then(|h| Some((h.model.clone()?, h.within_ms))).filter(|_| options.len() > 1);
        let Some((model, within)) = model else { return Some((first, String::new())) };
        let (chooser, facts) = interp::chooser(&prog, u, d).unwrap_or_default();
        let choice = Choice {
            point: b.target.clone(),
            unit: u.label.clone(),
            key: d.key(),
            chooser,
            facts: unmarked(&prog, facts),
            options: options.iter().map(|o| Option_ { label: o.label.clone(), attrs: unmarked(&prog, o.root.attrs.clone().into_iter().collect()) }).collect(),
            currency: prog.currency.clone(),
        };
        Some(self.consult(&model, &choice, within))
    }

    /// The model's pick among the choice's options, which come best first, and
    /// how it was made. Whatever goes wrong leaves the pick to `prefer`, and
    /// says why (appendix K.2.5).
    fn consult(&mut self, model: &str, c: &Choice, within: u64) -> (String, String) {
        let names: Vec<&str> = c.options.iter().map(|o| o.label.as_str()).collect();
        let answer = match self.replay.front() {
            Some((k, _)) if *k == c.key => self.replay.pop_front().expect("a front").1,
            _ => self.models.ask(model, c, within),
        };
        self.model_answers.push((c.key.clone(), answer.clone()));
        let how = match answer {
            Answer::Picked(p) if names.contains(&p.as_str()) => return (p, format!(" by model \"{model}\" (of {})", names.join(", "))),
            Answer::Picked(p) => format!("model \"{model}\" picked {p}, which it was not offered"),
            Answer::Declined => format!("model \"{model}\" declined"),
            Answer::Unknown => format!("model \"{model}\" is not available"),
            Answer::Late => format!("model \"{model}\" did not answer within {within} ms"),
            Answer::Unsure(x) => format!("model \"{model}\" was unsure, at {x:.2}"),
            Answer::Failed(e) => format!("model \"{model}\" failed: {e}"),
        };
        (names[0].to_string(), format!(" by prefer ({how})"))
    }

    /// A slot's or a source's choice put to a model (appendix K.2.3–K.2.4):
    /// asked at once, and its pick, or `prefer`'s, injected as the answer, so
    /// the journal holds it.
    fn ask_model(&mut self, ask: &Ask, hole: HoleId) -> Result<(), String> {
        let u = self.unit(&ask.label).ok_or_else(|| format!("{}: a choice for a unit that is gone", ask.label))?;
        let Some((chooser, facts, options)) = interp::choice_of(&u, &ask.key) else {
            // Nothing is being chosen any more: an empty answer, which the unit ignores.
            return self.answer_model(hole, &[]);
        };
        let within = match ask.sends.as_slice() {
            [(_, Val::Count(ms))] => u64::try_from(*ms).unwrap_or(0),
            _ => 0,
        };
        let c = Choice {
            point: ask.kind.clone(),
            unit: ask.label.clone(),
            key: ask.key.clone(),
            chooser,
            facts: unmarked(&self.prog, facts),
            options: options.iter().map(|o| Option_ { label: o.clone(), attrs: vec![] }).collect(),
            currency: self.prog.currency.clone(),
        };
        let (pick, how) = self.consult(&ask.adapter, &c, within);
        self.answer_model(hole, &[("pick".into(), Val::Text(pick.clone()))])?;
        self.trace.push(format!("      choice: {pick} for {}{how}", ask.key));
        Ok(())
    }

    fn answer_model(&mut self, hole: HoleId, data: &[(String, Val)]) -> Result<(), String> {
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let meet = b.var();
        b.agent(self.kinds.answer, answer_value("picked", data), meet, &[]);
        let net = b.finish().map_err(|e| e.to_string())?;
        let inj = Injection::new(net).at_hole(meet, hole).key(format!("model:{:032x}", hole.0));
        self.machine.inject(inj).map(|_| ()).map_err(|e| e.to_string())
    }

    fn pick(&mut self, hole: HoleId, label: &str, d: &Decide, how: &str) -> Result<(), String> {
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let meet = b.var();
        b.agent(self.kinds.pick, label, meet, &[]);
        let net = b.finish().map_err(|e| e.to_string())?;
        let inj = Injection::new(net).at_hole(meet, hole).key(format!("pick:{:032x}", hole.0));
        self.machine.inject(inj).map_err(|e| e.to_string())?;
        self.trace.push(format!("      choice: {label} for {}{how}", d.key()));
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
                Some((pick, how)) => {
                    self.waiters.remove(i);
                    self.pick(w.hole, &pick, &fresh, &how)?;
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
        self.gate_with(by, fits, what, target, &[], &[])
    }

    /// The data an event carries, checked against its `admit`'s `with`
    /// (appendix Q.2.1): every value named, of its type, and nothing else.
    fn event_data(&self, fits: impl Fn(&Admitted) -> bool, data: &[(String, Val)]) -> Result<Vec<(String, Val)>, String> {
        // With no admit for it, the gate refuses the event, not its data.
        let Some(want) = self.prog.admits.iter().find(|a| fits(&a.what)).map(|a| a.data.as_slice()) else { return Ok(data.to_vec()) };
        for (n, _) in data {
            if !want.iter().any(|(w, _)| w == n) {
                return Err(format!("the event carries no `{n}`"));
            }
        }
        let mut out = vec![];
        for (n, ty) in want {
            let v = data.iter().find(|(d, _)| d == n).map(|(_, v)| v).ok_or_else(|| format!("the event carries `{n}`; give it"))?;
            if v.ty() != *ty {
                return Err(format!("`{n}` is {ty:?}, not {:?}", v.ty()));
            }
            out.push((n.clone(), v.clone()));
        }
        Ok(out)
    }

    /// The gate, for an input whose condition is read at the node `at` of
    /// the target, with the event's `data`.
    fn gate_with(&mut self, by: Option<&Caller>, fits: impl Fn(&Admitted) -> bool, what: &str, target: &Unit, data: &[(String, Val)], at: &[usize]) -> Result<(), String> {
        if self.prog.admits.is_empty() {
            return Ok(());
        }
        let decision = self.gate_check(by, fits, target, data, at);
        let input = by.map_or(what.to_string(), |c| format!("{} {} {} {what}", c.role, c.name, c.verb));
        let line = match &decision {
            Ok(()) => format!("admitted: {input}"),
            Err(e) => format!("refused at the gate: {input}: {e}"),
        };
        self.trace.push(format!("gate: {line}"));
        self.gate.push(line);
        decision.map_err(|e| format!("refused at the gate: {e}"))
    }

    fn gate_check(&self, by: Option<&Caller>, fits: impl Fn(&Admitted) -> bool, target: &Unit, data: &[(String, Val)], at: &[usize]) -> Result<(), String> {
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
        if admits.iter().any(|a| a.cond.as_ref().is_none_or(|c| interp::admits(&self.prog, &target, caller, data, at, c))) {
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
        let what = target.map(|t| format!(" for {t}")).unwrap_or_default();
        self.answer_where(adapter, outcome, data, &what, |a| a.adapter == adapter && target.is_none_or(|t| a.label == t))
    }

    /// The adapter answers the request with this idempotency key (appendix L.2.6).
    pub fn answer_key(&mut self, adapter: &str, outcome: &str, data: &[(String, Val)], key: &str) -> Result<(), String> {
        self.answer_where(adapter, outcome, data, &format!(" with key {key}"), |a| a.adapter == adapter && a.key == key)
    }

    fn answer_where(&mut self, adapter: &str, outcome: &str, data: &[(String, Val)], what: &str, matches: impl Fn(&Ask) -> bool) -> Result<(), String> {
        if outcome == TIMED_OUT {
            return Err(format!("only the runtime reports `{TIMED_OUT}`; {adapter} must answer with an outcome of its effect"));
        }
        let Some(i) = self.pending.iter().position(|(a, _)| matches(a)) else {
            let Some(ask) = self.expired.iter().rfind(|a| matches(a)).cloned() else {
                return Err(format!("{adapter} has no pending request{what}"));
            };
            self.trace.push(format!("      {adapter}: {outcome}   ({})", ask.key));
            self.trace.push(format!("{:<3} ignored a late answer for {}: it timed out", ask.label, ask.key));
            return Ok(());
        };
        let hole = self.pending[i].1;
        let kind = self.pending[i].0.kind.clone();
        let data = &self.seal(&kind, data.to_vec())?;
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let meet = b.var();
        b.agent(self.kinds.answer, answer_value(outcome, data), meet, &[]);
        let net = b.finish().map_err(|e| e.to_string())?;
        let inj = Injection::new(net).at_hole(meet, hole).key(format!("answer:{:032x}", hole.0));
        // An answer that cannot be delivered leaves its request waiting (spec §5.5).
        self.machine.inject(inj).map_err(|e| e.to_string())?;
        let (ask, _) = self.pending.remove(i);
        let shown: Vec<String> = data.iter().map(|(k, v)| format!("{k} {}", interp::shown(&self.prog, k, v))).collect();
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

    /// When the next timer falls due, if any.
    pub fn next_due(&self) -> Option<u64> {
        self.timers.first_key_value().map(|(&(due, _), _)| due)
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
                if !interp::goal_applies(&self.prog, &unit, goal) || unit.met.contains(&goal) || unit.alerts.contains(&goal) {
                    return Ok(());
                }
                self.tell(&label, &Msg::GoalDue { goal })
            }
            Timer::Patience(label, id) if id.contains('~') => {
                let Some(unit) = self.unit(&label) else { return Ok(()) };
                if !interp::fetch_due_matters(&unit, &id) {
                    return Ok(());
                }
                self.tell(&label, &Msg::Due { id })
            }
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

    /// Puts each secret value of an entity of `kind` in the secret store, and
    /// a reference to it in its place (appendix N.2.4). A reference already
    /// in the store, as a replayed log holds, is kept.
    pub fn seal(&mut self, kind: &str, attrs: Vec<(String, Val)>) -> Result<Vec<(String, Val)>, String> {
        let secret = |a: &str| self.prog.kind(kind).and_then(|k| k.attr(a)).is_some_and(|a| a.mark == Some(suture_lang::ir::Mark::Secret));
        let mut out = vec![];
        for (a, v) in attrs {
            let v = match v {
                Val::Text(s) if secret(&a) => {
                    let n = self.secrets.keys().next_back().map_or(1, |n| n + 1);
                    self.secrets.insert(n, s);
                    Val::Secret(n)
                }
                Val::Secret(n) if !self.secrets.contains_key(&n) => return Err(format!("{kind}.{a}: secret {n} is not in the secret store")),
                v => v,
            };
            out.push((a, v));
        }
        Ok(out)
    }

    /// A value as an adapter granted it receives it: a secret's own value.
    pub fn reveal(&self, v: &Val) -> Val {
        match v {
            Val::Secret(n) => self.secrets.get(n).map_or_else(|| v.clone(), |s| Val::Text(s.clone())),
            v => v.clone(),
        }
    }

    /// The live units, in the order they were created.
    pub fn units(&self) -> Vec<Unit> {
        self.machine.labels().iter().filter_map(|l| self.unit(l)).collect()
    }

    /// For each goal of the running program, by index, its index in `new`:
    /// goals are matched by their text (appendix M.2.5).
    fn goal_map(&self, new: &Program) -> Vec<Option<u32>> {
        self.prog.goals.iter().map(|g| new.goals.iter().position(|n| n.source == g.source).map(|i| i as u32)).collect()
    }

    /// For each binding of the running program, by index, its index in `new`.
    fn binding_map(&self, new: &Program) -> Vec<Option<usize>> {
        self.prog.bindings.iter().map(|b| new.bindings.iter().position(|n| n.id == b.id && n.subject == b.subject && n.per == b.per)).collect()
    }

    /// Every live unit carried over to `new` on its own, then, for each, the
    /// facts it carries as those units are under `new` (appendix M.2.5).
    fn carried(&self, new: &Program) -> Vec<(Unit, Facts)> {
        let units: Vec<Unit> = self.units();
        let over: BTreeMap<String, Unit> = units
            .iter()
            .map(|u| {
                let mut c = u.clone();
                interp::carry_over(new, &mut c);
                (c.label.clone(), c)
            })
            .collect();
        units
            .into_iter()
            .map(|u| {
                let facts = u
                    .facts
                    .iter()
                    .map(|(l, f)| match over.get(l) {
                        Some(c) => (l.clone(), Fact { kind: c.root.kind.clone(), created: c.root.created, attrs: c.root.attrs.clone() }),
                        None => (l.clone(), f.clone()),
                    })
                    .collect();
                (u, facts)
            })
            .collect()
    }

    /// The deploy check (appendix M.2.4): every break that `new` would make
    /// in the live units, each with the units it concerns and how it can be
    /// resolved. Empty when the deploy can go ahead.
    pub fn deploy_check(&self, new: &Program) -> Result<Vec<String>, String> {
        if new.module != self.prog.module {
            return Err(format!("this is module {}, not {}", new.module, self.prog.module));
        }
        if new.version <= self.prog.version {
            return Err(format!("{} v{} is running, so a deploy needs a higher version than {}", self.prog.module, self.prog.version, new.version));
        }
        let goals = self.goal_map(new);
        let mut breaks: Vec<(String, Vec<String>)> = vec![];
        let mut add = |b: String, label: &str| match breaks.iter_mut().find(|(x, _)| *x == b) {
            Some((_, units)) if units.iter().any(|u| u == label) => {}
            Some((_, units)) => units.push(label.to_string()),
            None => breaks.push((b, vec![label.to_string()])),
        };
        for (u, facts) in self.carried(new) {
            for b in interp::upgrade_breaks(&self.prog, new, &u, &goals, &facts, self.clock()) {
                add(b, &u.label);
            }
        }
        let bindings = self.binding_map(new);
        for w in &self.waiters {
            if bindings.get(w.decide.binding as usize).copied().flatten().is_none() {
                let b = &self.prog.bindings[w.decide.binding as usize];
                add(format!("{} uses a {} that is removed, and live units wait for one", b.subject, b.target), &w.decide.label);
            }
        }
        Ok(breaks
            .into_iter()
            .map(|(b, units)| {
                let shown = if units.len() > 5 { format!("{} and {} more", units[..5].join(", "), units.len() - 5) } else { units.join(", ") };
                format!("{b}\n    live in {shown}\n    {}", resolution(new, &b, &units, self))
            })
            .collect())
    }

    /// Deploys `new` over the running program, once [`Host::deploy_check`]
    /// finds nothing: the kernel takes its rules, every live unit is carried
    /// over and settles once under it, and only then are the messages they
    /// sent each other delivered (appendix M.2.5). An error before the
    /// kernel takes the new program leaves everything as it was.
    pub fn deploy(&mut self, new: Program) -> Result<(), String> {
        let breaks = self.deploy_check(&new)?;
        if !breaks.is_empty() {
            return Err(format!("the deploy is refused: {} would break live units:\n  {}", new.version, breaks.join("\n  ")));
        }
        let goals = self.goal_map(&new);
        let bindings = self.binding_map(&new);
        let carried: Vec<(String, Facts)> = self.carried(&new).into_iter().map(|(u, f)| (u.label, f)).collect();
        let new = Arc::new(new);
        let (kernel, kinds) = kernel::program(new.clone());
        self.machine.upgrade(kernel).map_err(|e| e.to_string())?;
        let old = std::mem::replace(&mut self.prog, new);
        self.kinds = kinds;
        self.goal_timers = std::mem::take(&mut self.goal_timers)
            .into_iter()
            .filter_map(|(l, g)| goals.get(g as usize).copied().flatten().map(|g| (l, g)))
            .collect();
        self.timers.retain(|_, t| !matches!(t, Timer::Goal(_, g) if goals.get(*g as usize).copied().flatten().is_none()));
        for t in self.timers.values_mut() {
            if let Timer::Goal(_, g) = t {
                *g = goals[*g as usize].expect("kept above");
            }
        }
        for w in &mut self.waiters {
            w.decide.binding = bindings[w.decide.binding as usize].expect("the check refuses a removed binding") as u32;
        }
        self.trace.push(format!("deployed {} v{} over v{}", self.prog.module, self.prog.version, old.version));
        let mut first_error = None;
        for (l, facts) in carried {
            if let Err(e) = self.post(&l, &Msg::Upgrade { goals: goals.clone(), facts }) {
                first_error.get_or_insert(e);
            }
        }
        // The pools and the waiters' keys follow the new bindings and attributes.
        self.pools.clear();
        for l in self.machine.labels() {
            self.index(&l);
        }
        let waiters = std::mem::take(&mut self.waiters);
        for mut w in waiters {
            let Some(u) = self.unit(&w.decide.label) else { continue };
            w.key = self.pool_key(&u, &w.decide);
            w.kind = self.prog.bindings[w.decide.binding as usize].target.clone();
            self.waiters.push(w);
        }
        if let Err(e) = self.pump() {
            first_error.get_or_insert(e);
        }
        for l in self.machine.labels() {
            if let Err(e) = self.wake(&l).and_then(|_| self.pump()) {
                first_error.get_or_insert(e);
            }
        }
        for g in self.prog.globals.clone() {
            if self.unit(&g).is_none()
                && let Err(e) = self.create(&g, &g, &[]) {
                first_error.get_or_insert(e);
            }
        }
        first_error.map_or(Ok(()), Err)
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
                if ask.verb == interp::MODEL_VERB {
                    if let Err(x) = self.ask_model(&ask, hole) {
                        first_error.get_or_insert(x);
                    }
                    continue;
                }
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
            if let Some(secs) = within
                && self.goal_timers.insert((label.to_string(), g)) {
                self.schedule(self.now + secs, Timer::Goal(label.to_string(), g));
            }
        }
        Ok(())
    }
}

/// How a break the deploy check found can be resolved (appendix M.2.4).
fn resolution(new: &Program, b: &str, units: &[String], host: &Host) -> String {
    if let Some(rest) = b.strip_prefix("rule ") {
        let name = rest.split(' ').next().unwrap_or_default();
        let Some(r) = new.rules.iter().find(|r| r.name == name) else { return String::new() };
        if r.tier != "local" {
            return format!("resolve: rule {name} is {}; change it, or the units, so that it holds", r.tier);
        }
        let root = units.first().and_then(|l| host.unit(l)).map_or_else(|| r.subject.clone(), |u| u.root.kind);
        let plural = new.kind(&root).map_or(root.clone(), |k| k.plural.clone());
        return format!("resolve: `apply rule {name} to {plural} created from version {}`, or change the rule", new.version);
    }
    if b.starts_with("kind ") {
        return "resolve: keep the kind until no live unit has it".into();
    }
    if b.contains(" changes from ") {
        return "resolve: keep the type, and add an attribute of the new type under another name".into();
    }
    "resolve: give it a default, as in `cancelled: Bool = no`".into()
}

/// Facts without the marked ones: a decision model is not granted any
/// (appendix N.2.4–N.2.5).
fn unmarked(prog: &Program, xs: Vec<(String, Val)>) -> Vec<(String, Val)> {
    xs.into_iter().filter(|(k, v)| !matches!(v, Val::Secret(_)) && interp::shown(prog, k, v) != "(personal)").collect()
}
