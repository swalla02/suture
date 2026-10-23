//! The host loop (spec §5.4, as corrected by appendix B.3.6). It runs the
//! kernel, routes internal messages, commits, and only then hands outside
//! requests to adapters. In slice 1 the adapters are simulated: a scenario
//! answers for them, one request at a time.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

use suture::{HoleId, Injection, Machine, NetBuilder, Value};
use suture_lang::ir::{candidate, third_person, Admit, Admitted, Program, Route, Ty, READ, TIMED_OUT, UNREAD};
use suture_lang::scenario::Caller;

use crate::interp::{self, duration};
use crate::kernel::{self, Kinds};
use crate::choice::{Answer, Choice, Models, Option_, Reading};
use crate::state::{answer_value, send_from_value, stamped, Ask, Decide, Fact, Facts, Msg, Unit, Val};

pub struct Host {
    pub prog: Arc<Program>,
    pub machine: Machine,
    pub kinds: Kinds,
    /// One line for each thing that happened: each unit's notes, and each answer.
    pub trace: Vec<String>,
    /// How many of the first lines a served host no longer keeps, so a line's
    /// number stays the same (appendix BD.2.2).
    pub trace_dropped: usize,
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
    /// Units something of whose may have freed up, with the bindings
    /// already woken for the first, so each is woken once (appendix S.2.1).
    to_wake: VecDeque<String>,
    woken: BTreeSet<String>,
    /// Every decision of the gate, in order (appendix G.2.2).
    pub gate: Vec<String>,
    /// For each unit, the units that carry its facts: those that refer to
    /// it, and those it granted a binding (appendix G.2.7).
    /// In the order they started reading it.
    readers: BTreeMap<String, Vec<String>>,
    /// Each (unit, reader) of `readers`, to find one without a scan.
    reading: BTreeSet<(String, String)>,
    /// The decision models, by `name@version` (appendix J.2.7).
    pub models: Models,
    /// Every model answer, by choice key, in the order they were given
    /// (appendix L.2.1).
    pub model_answers: Vec<(String, Answer)>,
    /// Answers to give again on replay instead of asking (appendix L.2.3).
    pub replay: VecDeque<(String, Answer)>,
    /// The secret store: each secret value by its number (appendix N.2.4).
    pub secrets: BTreeMap<u64, String>,
    /// The media store: the content of every `Media` value (appendix V.2.2).
    pub media: crate::media::Store,
    /// Whether a model is asked after the step that needs it, not in it
    /// (appendix S.2.2). The service defers; scenarios ask at once.
    pub defer_models: bool,
    /// Whether a source a model reads is read as soon as it is fetched, or
    /// waits as a request until the service asks the model (appendix AF.2.3).
    pub reads_at_once: bool,
    /// The choices put to a model and not answered yet, in the order asked.
    asking: Vec<Asking>,
}

/// A choice waiting for its model's answer (appendix S.2.2).
struct Asking {
    model: String,
    choice: Choice,
    within: u64,
    to: Chooses,
    /// The number of its deadline's timer, so an older ask's timer does
    /// not answer this one.
    timer: u64,
    /// When this asks a router which model makes the choice: the route, and
    /// the choice and deadline the picked model gets (appendix AJ.2.3).
    routing: Option<(Route, Choice, u64)>,
}

/// Where a deferred model's answer goes.
enum Chooses {
    /// A binding's choice, with its hole and pool key.
    Bind { hole: HoleId, decide: Decide, pool: String },
    /// A slot's or a source's choice, with its request's hole.
    Ask { hole: HoleId, ask: Ask },
}

/// What gathering a binding's candidates came to.
enum Gathered {
    /// The pick, and how it was made.
    Pick(String, String),
    /// A model was asked, and answers later.
    Asked,
}

/// A binding waiting on its pool key, with its choice's hole.
#[derive(Clone)]
struct Waiter {
    hole: HoleId,
    decide: Decide,
    kind: String,
    key: String,
}

/// The pick a model's answer comes to, among `names`, best first, and how it
/// was made. Whatever goes wrong leaves the pick to the program's `otherwise`
/// while it is an option (appendix AL.2.2), else to `prefer`, and says why
/// (appendix K.2.5).
fn resolve(model: &str, names: &[&str], answer: &Answer, within: u64, otherwise: Option<&str>) -> (String, String) {
    let how = match answer {
        Answer::Picked(p) if names.contains(&p.as_str()) => return (p.clone(), format!(" by model \"{model}\" (of {})", names.join(", "))),
        Answer::Picked(p) => format!("model \"{model}\" picked {p}, which it was not offered"),
        Answer::Declined => format!("model \"{model}\" declined"),
        Answer::Read(..) => format!("model \"{model}\" read a value instead of picking"),
        Answer::Unknown => format!("model \"{model}\" is not available"),
        Answer::Late => format!("model \"{model}\" did not answer within {within} ms"),
        Answer::Unsure(x) => format!("model \"{model}\" was unsure, at {x:.2}"),
        Answer::Failed(e) => format!("model \"{model}\" failed: {e}"),
    };
    match otherwise.filter(|o| names.contains(o)) {
        Some(o) => (o.to_string(), format!(" by otherwise ({how})")),
        None => (names[0].to_string(), format!(" by prefer ({how})")),
    }
}

/// The choice a router gets: the same facts and media, with the candidates
/// as its options, the decision's options as `among`, and its own key,
/// `…?model` (appendices AJ.2.2, AY).
fn routing_choice(c: &Choice, r: &Route) -> Choice {
    Choice {
        point: format!("{}?model", c.point),
        key: format!("{}?model", c.key),
        options: r.among.iter().map(|m| Option_ { label: candidate(m), attrs: vec![] }).collect(),
        sure: r.sure.map(thousandths),
        otherwise: r.otherwise.clone(),
        among: c.options.clone(),
        ..c.clone()
    }
}

/// A confidence in thousandths, as a fraction.
fn thousandths(t: u16) -> f64 {
    f64::from(t) / 1000.0
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
    /// A deferred model's deadline, by choice key and the timer's number
    /// (appendix S.2.2).
    Model(String, u64),
}

/// The rule a step refused itself by, from the unit's notes.
fn refusal_of(u: &Unit) -> Option<String> {
    u.notes.iter().find_map(|n| n.strip_prefix("refused ")?.split_once(": refused by rule ").map(|(_, r)| r.to_string()))
}

/// A request's answer: its outcome and the data it carries.
pub type Outcome = (String, Vec<(String, Val)>);

impl Host {
    pub fn new(prog: Program) -> Host {
        Self::with_journal(prog, Box::new(suture::MemJournal::new()))
    }

    /// A host whose machine journals to `journal`. A served host keeps none,
    /// since its log is what replays (appendix BD.2.1).
    pub fn with_journal(prog: Program, journal: Box<dyn suture::Journal>) -> Host {
        let prog = Arc::new(prog);
        let (kernel, kinds) = kernel::program(prog.clone());
        let mut h = Host {
            prog,
            machine: Machine::with_journal(kernel, journal).expect("a new journal is empty"),
            kinds,
            trace: vec![],
            trace_dropped: 0,
            pending: vec![],
            now: 0,
            timers: BTreeMap::new(),
            started: 0,
            goal_timers: BTreeSet::new(),
            patience_timers: BTreeSet::new(),
            expired: vec![],
            pools: BTreeMap::new(),
            waiters: vec![],
            to_wake: VecDeque::new(),
            defer_models: false,
            reads_at_once: true,
            asking: vec![],
            woken: BTreeSet::new(),
            gate: vec![],
            readers: BTreeMap::new(),
            reading: BTreeSet::new(),
            models: Models::default(),
            model_answers: vec![],
            replay: VecDeque::new(),
            secrets: BTreeMap::new(),
            media: crate::media::Store::default(),
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
            return Err(format!("nothing reacts when a {} {} {}", by.role, by.verb, suture_lang::check::a(&kind)));
        }
        let mut facts = Facts::new();
        if self.prog.role(&by.role).is_some_and(|r| r.kind.is_some()) {
            self.carry(&Val::Ref(by.name.clone()), &mut facts);
        }
        let part = part.unwrap_or_default().to_string();
        let msg = Msg::Event { verb: by.verb.clone(), by: format!("{} {}", by.role, by.name), part, data, facts };
        let limit = self.machine.program().limits.max_payload_bytes;
        let bytes = stamped(self.clock(), &msg.to_value()).encoded_len();
        if bytes > limit {
            let why = format!("the event would carry {bytes} bytes, and a message carries at most {limit}");
            self.trace.push(format!("{target:<3} refused: {why}"));
            return Err(format!("refused: {why}"));
        }
        let pre = interp::step_at(&self.prog, &unit, &msg, self.clock());
        self.fits(&pre.unit)?;
        self.refused_across(&pre, &format!("{} {} {} it", by.role, by.name, by.verb))?;
        self.tell(target, &msg)?;
        match self.unit(target).and_then(|u| refusal_of(&u)) {
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
        let a = self.prog.kind(&kind).and_then(|k| k.attr(attr)).ok_or_else(|| format!("{} has no attribute {attr}", suture_lang::check::a(&kind)))?;
        let value = match (a.ty, value) {
            (Ty::Ref | Ty::Set, _) => return Err(format!("{kind}.{attr} refers to shared entities, which cannot be changed yet")),
            (t, v) if v.ty() == t => v,
            (t, v) => return Err(format!("{kind}.{attr} is {t:?}, not {}", v.show(&self.prog.currency))),
        };
        interp::state_value(&kind, a, &value)?;
        let fits = |a: &Admitted| matches!(a, Admitted::Change { kind: k, attr: x } if by.is_some_and(|c| c.verb == "changes") && *k == kind && x == attr);
        let value = self.seal(&kind, vec![(attr.to_string(), value)])?.remove(0).1;
        self.gate(by, fits, &format!("{target}'s {attr} to {}", interp::shown(&self.prog, attr, &value)), &unit)?;
        let who = by.map_or(String::new(), |c| format!("{} {}", c.role, c.name));
        let msg = Msg::Change { attr: attr.to_string(), value, by: who.clone() };
        let pre = interp::step_at(&self.prog, &unit, &msg, self.clock());
        let after = pre.unit.clone();
        if let Some(r) = refusal_of(&after) {
            self.trace.push(format!("{target:<3} {}", after.notes.join("; ")));
            return Err(format!("refused by rule {r}"));
        }
        self.fits(&after)?;
        self.refused_across(&pre, &format!("{who} changes {attr}"))?;
        let fact = interp::fact_of(&self.prog, &after);
        let refresh = Msg::Refresh { facts: Facts::from([(target.to_string(), fact)]) };
        let readers: Vec<String> = self.readers.get(target).cloned().unwrap_or_default();
        for r in &readers {
            let Some(u) = self.unit(r) else { continue };
            let pre = interp::step_at(&self.prog, &u, &refresh, self.clock());
            // A reader whose filter the change makes hold is checked as the
            // changed unit is (appendix AA.2.2).
            self.refused_across(&pre, &format!("{who} changes {target}'s {attr}"))?;
            let after = pre.unit;
            if let Some(rule) = interp::newly_violated(&self.prog, &u, &after) {
                self.trace.push(format!("{target:<3} change refused: {r} would break rule {}", rule.name));
                return Err(format!("refused by rule {}: {} (in {r})", rule.name, rule.paraphrase));
            }
            // A reader keeps a copy of the changed facts, so it grows too.
            self.fits(&after)?;
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

    /// Refuses an input that would make a unit larger than the machine lets
    /// one be, before anything changes (appendix T.2.2).
    fn fits(&mut self, u: &Unit) -> Result<(), String> {
        let (bytes, limit) = (u.to_value().encoded_len(), self.machine.program().limits.max_payload_bytes);
        if bytes <= limit {
            return Ok(());
        }
        let why = format!("{} would hold {bytes} bytes, and a unit holds at most {limit}", u.label);
        self.trace.push(format!("{:<3} refused: {why}", u.label));
        Err(format!("refused: {why}"))
    }

    fn read_by(&mut self, read: &str, by: &str) {
        if self.reading.insert((read.to_string(), by.to_string())) {
            self.readers.entry(read.to_string()).or_default().push(by.to_string());
        }
    }

    /// Keeps only the last `keep` trace lines, gate decisions and timed-out
    /// requests, once there are twice as many; and forgets the model answers,
    /// which the service has logged (appendix BD.2.2). Called between inputs.
    pub fn trim(&mut self, keep: usize) {
        self.model_answers.clear();
        if self.expired.len() > 2 * keep {
            self.expired.drain(..self.expired.len() - keep);
        }
        if self.trace.len() > 2 * keep {
            let n = self.trace.len() - keep;
            self.trace.drain(..n);
            self.trace_dropped += n;
        }
        if self.gate.len() > 2 * keep {
            self.gate.drain(..self.gate.len() - keep);
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
        let unit = match interp::build_unit(&self.prog, label, kind, &given, &elements, facts, self.clock()) {
            Ok(u) => u,
            // An input that leaves out a required attribute is refused at the gate (appendix AE.2.2).
            Err(e) => match (gated, interp::required(&self.prog, kind, &given, &elements)) {
                (Some(Some(c)), Err(missing)) if missing == e => {
                    let line = format!("refused at the gate: {} {} {} a new {kind} {label}: {e}", c.role, c.name, c.verb);
                    self.trace.push(format!("gate: {line}"));
                    self.gate.push(line);
                    return Err(format!("refused at the gate: {e}"));
                }
                _ => return Err(e),
            },
        };
        if let Some(by) = gated {
            let fits = |a: &Admitted| matches!(a, Admitted::New { verb, kind: k } if by.is_some_and(|c| third_person(verb) == c.verb) && k == kind);
            self.gate(by, fits, &format!("a new {kind} {label}"), &unit)?;
        }
        interp::refused_at_creation(&self.prog, &unit, self.clock())?;
        self.fits(&unit)?;
        let first = interp::step_at(&self.prog, &unit, &Msg::Create, self.clock());
        self.fits(&first.unit)?;
        if let Some(by) = gated.flatten() {
            self.refused_across(&first, &format!("{} {} {} it", by.role, by.name, by.verb))?;
        }
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
        match self.gather(&u, &d, &key, hole) {
            Some(Gathered::Pick(label, how)) => self.pick(hole, &label, &d, &how),
            Some(Gathered::Asked) => Ok(()),
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
    fn gather(&mut self, u: &Unit, d: &Decide, key: &str, hole: HoleId) -> Option<Gathered> {
        let prog = self.prog.clone();
        let b = prog.bindings.get(d.binding as usize)?;
        let options = self.options(u, d, key);
        let first = options.first()?.label.clone();
        // With one option there is no choice, and no model is asked (appendix K.2.2).
        let model = interp::handler(&prog, u, d).and_then(|h| Some((h.asked()?.to_string(), h.within_ms, h.route.clone(), h.sure, h.otherwise.clone()))).filter(|_| options.len() > 1);
        let Some((model, within, route, sure, otherwise)) = model else { return Some(Gathered::Pick(first, String::new())) };
        let (chooser, facts) = interp::chooser(&prog, u, d).unwrap_or_default();
        let choice = Choice {
            point: b.target.clone(),
            unit: u.label.clone(),
            key: d.key(),
            chooser,
            facts: unmarked(&prog, facts),
            options: options.iter().map(|o| Option_ { label: o.label.clone(), attrs: unmarked(&prog, o.root.attrs.clone().into_iter().collect()) }).collect(),
            currency: prog.currency.clone(),
            media: BTreeMap::new(),
            sure: sure.map(thousandths),
            otherwise,
            among: vec![],
        };
        if self.defer_models {
            self.defer_routed(model, route, choice, within, Chooses::Bind { hole, decide: d.clone(), pool: key.to_string() });
            return Some(Gathered::Asked);
        }
        let (pick, how) = self.consult_routed(&model, route.as_ref(), &choice, within);
        Some(Gathered::Pick(pick, how))
    }

    /// The pool's candidates for a binding that its filter lets through,
    /// best first under `prefer`, decoded only as far as they are needed.
    /// A binding that is neither held nor unique is offered only candidates
    /// that would grant its take now, so full ones are passed over rather
    /// than asked and refused (appendix BC.2.1). When none has room, the
    /// first choice still asks the best, so its refusal names the rule.
    fn options(&self, u: &Unit, d: &Decide, key: &str) -> Vec<Unit> {
        let Some(b) = self.prog.bindings.get(d.binding as usize) else { return vec![] };
        let Some(labels) = self.pools.get(&(b.target.clone(), b.key_attr.clone())).and_then(|p| p.get(key)) else { return vec![] };
        let candidates = || labels.iter().filter(|l| !d.skip.contains(l)).filter_map(|l| self.unit(l));
        let grants = (b.held.is_none() && !b.unique).then(|| self.clock());
        let roomy = interp::options(&self.prog, u, d, candidates(), grants);
        if !roomy.is_empty() || grants.is_none() || !d.skip.is_empty() {
            return roomy;
        }
        interp::options(&self.prog, u, d, candidates(), None).into_iter().take(1).collect()
    }

    /// Puts a choice to its model after the step: the service asks it, and
    /// its answer comes back as an input. If none comes within the model's
    /// deadline, `prefer` picks (appendix S.2.2).
    fn defer(&mut self, model: String, choice: Choice, within: u64, to: Chooses) {
        self.trace.push(format!("      choice for {}: model \"{model}\" is asked", choice.key));
        let secs = within.div_ceil(1000).max(1);
        let timer = self.started + 1;
        self.schedule(self.now.saturating_add(secs), Timer::Model(choice.key.clone(), timer));
        self.asking.retain(|a| a.choice.key != choice.key);
        self.asking.push(Asking { model, choice, within, to, timer, routing: None });
    }

    /// [`Host::defer`], asking a routed choice's router first (appendix AJ.2.3).
    fn defer_routed(&mut self, model: String, route: Option<Route>, choice: Choice, within: u64, to: Chooses) {
        let Some(r) = route else { return self.defer(model, choice, within, to) };
        let rc = routing_choice(&choice, &r);
        self.defer(r.router.clone(), rc, r.within_ms, to);
        self.asking.last_mut().expect("just deferred").routing = Some((r, choice, within));
    }

    /// The choices waiting for a model, as (key, unit), in the order asked.
    pub fn model_asks(&self) -> Vec<(String, String)> {
        self.asking.iter().map(|a| (a.choice.key.clone(), a.choice.unit.clone())).collect()
    }

    /// Asks the model of a waiting choice, and gives its answer without
    /// applying it: the caller logs it and applies it with [`Host::model_answer`].
    pub fn ask_model_now(&mut self, key: &str) -> Option<Answer> {
        let (model, choice, within) = self.model_job(key)?;
        Some(self.models.ask(&model, &choice, within))
    }

    /// What asking the model of a waiting choice takes: its name, the choice
    /// with its media, and its milliseconds to answer.
    pub fn model_job(&self, key: &str) -> Option<(String, Choice, u64)> {
        let a = self.asking.iter().find(|a| a.choice.key == key)?;
        Some((a.model.clone(), self.with_media(&a.choice), a.within))
    }

    /// The choice with the content of its media, read from the store just
    /// before the model is asked (appendix V.2.3).
    fn with_media(&self, c: &Choice) -> Choice {
        let mut c = c.clone();
        c.media = c.media_named().into_iter().filter_map(|h| Some((h.to_string(), self.media.get(h)?))).collect();
        c
    }

    /// A deferred model's answer (appendix S.2.2). The options are gathered
    /// again, since the world may have moved on since it was asked: a pick
    /// that is no longer an option leaves the choice to `prefer`.
    pub fn model_answer(&mut self, key: &str, answer: &Answer) -> Result<(), String> {
        let Some(i) = self.asking.iter().position(|a| a.choice.key == key) else {
            self.trace.push(format!("      model answer for {key} ignored: nothing waits for it"));
            return Ok(());
        };
        let a = self.asking.remove(i);
        if let Some((r, choice, within)) = a.routing {
            // The router has picked the model; now that model is asked (appendix AJ.2.3).
            let names: Vec<String> = r.among.iter().map(candidate).collect();
            let (m, how) = resolve(&r.router, &names.iter().map(String::as_str).collect::<Vec<_>>(), answer, r.within_ms, r.otherwise.as_deref());
            self.trace.push(format!("      model for {}: {m}{how}", choice.key));
            if m != "prefer" {
                self.defer(m, choice, within, a.to);
                return self.pump();
            }
            // `prefer` makes the choice, as the router picked.
            let how = format!(" by prefer (as model \"{}\" picked)", r.router);
            return self.settle(a.to, &|names: &[&str]| (names[0].to_string(), how.clone()));
        }
        let (model, within, otherwise) = (a.model.clone(), a.within, a.choice.otherwise.clone());
        self.settle(a.to, &|names: &[&str]| resolve(&model, names, answer, within, otherwise.as_deref()))
    }

    /// Applies a deferred choice, picked among its options as gathered now by
    /// `decide`, which gets them best first.
    fn settle(&mut self, to: Chooses, choose: &dyn Fn(&[&str]) -> (String, String)) -> Result<(), String> {
        match to {
            Chooses::Bind { hole, decide, pool } => {
                let Some(u) = self.unit(&decide.label) else { return Ok(()) };
                let options = self.options(&u, &decide, &pool);
                let names: Vec<&str> = options.iter().map(|o| o.label.as_str()).collect();
                if names.is_empty() {
                    let kind = self.prog.bindings.get(decide.binding as usize).map(|b| b.target.clone()).unwrap_or_default();
                    self.waiters.push(Waiter { hole, decide: decide.clone(), kind, key: pool });
                    self.post(&decide.label, &Msg::Waiting { key: decide.key() })?;
                } else {
                    let (pick, how) = choose(&names);
                    self.pick(hole, &pick, &decide, &how)?;
                }
            }
            Chooses::Ask { hole, ask } => {
                let options = self.unit(&ask.label).and_then(|u| interp::choice_of(&u, &ask.key)).map(|(_, _, o)| o);
                match options {
                    None => self.answer_model(hole, &[])?,
                    Some(options) => {
                        let names: Vec<&str> = options.iter().map(String::as_str).collect();
                        let (pick, how) = choose(&names);
                        self.answer_model(hole, &[("pick".into(), Val::Text(pick.clone()))])?;
                        self.trace.push(format!("      choice: {pick} for {}{how}", ask.key));
                    }
                }
            }
        }
        self.pump()
    }

    /// The model's pick among the choice's options, which come best first, and
    /// how it was made. Whatever goes wrong leaves the pick to `prefer`, and
    /// says why (appendix K.2.5).
    fn consult(&mut self, model: &str, c: &Choice, within: u64) -> (String, String) {
        let names: Vec<&str> = c.options.iter().map(|o| o.label.as_str()).collect();
        let answer = match self.replay.front() {
            Some((k, _)) if *k == c.key => self.replay.pop_front().expect("a front").1,
            _ => {
                let c = self.with_media(c);
                self.models.ask(model, &c, within)
            }
        };
        self.model_answers.push((c.key.clone(), answer.clone()));
        resolve(model, &names, &answer, within, c.otherwise.as_deref())
    }

    /// [`Host::consult`], asking a routed choice's router first which
    /// candidate makes it (appendix AJ.2.3). `model` is the router then.
    fn consult_routed(&mut self, model: &str, route: Option<&Route>, c: &Choice, within: u64) -> (String, String) {
        let Some(r) = route else { return self.consult(model, c, within) };
        let (m, how) = self.consult(&r.router, &routing_choice(c, r), r.within_ms);
        self.trace.push(format!("      model for {}: {m}{how}", c.key));
        if m == "prefer" {
            return (c.options[0].label.clone(), format!(" by prefer (as model \"{}\" picked)", r.router));
        }
        self.consult(&m, c, within)
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
        let sent = |name: &str| ask.sends.iter().find(|(n, _)| n == name).and_then(|(_, v)| if let Val::Count(x) = v { usize::try_from(*x).ok() } else { None });
        let within = sent("within").unwrap_or(0) as u64;
        let decision = sent("decision").and_then(|i| self.prog.decisions.get(i));
        let (route, sure, otherwise) = (decision.and_then(|d| d.route.clone()), decision.and_then(|d| d.sure), decision.and_then(|d| d.otherwise.clone()));
        let c = Choice {
            point: ask.kind.clone(),
            unit: ask.label.clone(),
            key: ask.key.clone(),
            chooser,
            facts: unmarked(&self.prog, facts),
            options: options.iter().map(|o| Option_ { label: o.clone(), attrs: vec![] }).collect(),
            currency: self.prog.currency.clone(),
            media: BTreeMap::new(),
            sure: sure.map(thousandths),
            otherwise,
            among: vec![],
        };
        if self.defer_models {
            self.defer_routed(ask.adapter.clone(), route, c, within, Chooses::Ask { hole, ask: ask.clone() });
            return Ok(());
        }
        let (pick, how) = self.consult_routed(&ask.adapter, route.as_ref(), &c, within);
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
    /// keys are woken when the host is next idle, one at a time (appendix S.2.1).
    fn wake(&mut self, label: &str) -> Result<(), String> {
        if !self.to_wake.iter().any(|l| l == label) {
            self.to_wake.push_back(label.to_string());
        }
        Ok(())
    }

    /// Wakes the first waiter, in arrival order, that can be served by what
    /// `label` has now, and says whether there was one. Its take is then
    /// applied before the next waiter gathers, so waiters the freed amount
    /// cannot serve stay asleep and send nothing.
    fn wake_one(&mut self, label: &str) -> Result<bool, String> {
        let Some(u) = self.unit(label) else { return Ok(false) };
        let keys: Vec<String> = self.keys_of(&u).into_iter().flat_map(|(_, k)| k).collect();
        let mut i = 0;
        while i < self.waiters.len() {
            let w = &self.waiters[i];
            if w.kind != u.root.kind || !keys.contains(&w.key) || self.woken.contains(&w.decide.key()) {
                i += 1;
                continue;
            }
            let w = w.clone();
            let still = self.unit(&w.decide.label).filter(|r| interp::waiting(r, &w.decide.key()));
            let Some(requester) = still else {
                self.waiters.remove(i);
                continue;
            };
            let fresh = Decide { skip: vec![], ..w.decide.clone() };
            // Only `label` changed: a waiter it cannot serve stays asleep.
            if !interp::would_grant(&self.prog, &u, &fresh, self.clock()) {
                i += 1;
                continue;
            }
            match self.gather(&requester, &fresh, &w.key, w.hole) {
                Some(Gathered::Pick(pick, how)) => {
                    self.waiters.remove(i);
                    self.woken.insert(w.decide.key());
                    self.pick(w.hole, &pick, &fresh, &how)?;
                    return Ok(true);
                }
                Some(Gathered::Asked) => {
                    self.waiters.remove(i);
                    self.woken.insert(w.decide.key());
                    return Ok(true);
                }
                None => i += 1,
            }
        }
        Ok(false)
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
            facts.insert(l, interp::fact_of(&self.prog, &u));
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
        let (ask, hole) = self.pending[i].clone();
        // An answer that cannot be delivered leaves its request waiting (spec §5.5).
        self.deliver(hole, &ask, adapter, outcome, data)?;
        self.pending.remove(i);
        self.pump()
    }

    /// Injects a request's answer at its hole, and traces it.
    fn deliver(&mut self, hole: HoleId, ask: &Ask, adapter: &str, outcome: &str, data: &[(String, Val)]) -> Result<(), String> {
        let data = &self.seal(&ask.kind, data.to_vec())?;
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let meet = b.var();
        b.agent(self.kinds.answer, answer_value(outcome, data), meet, &[]);
        let net = b.finish().map_err(|e| e.to_string())?;
        let inj = Injection::new(net).at_hole(meet, hole).key(format!("answer:{:032x}", hole.0));
        self.machine.inject(inj).map_err(|e| e.to_string())?;
        let shown: Vec<String> = data.iter().map(|(k, v)| format!("{k} {}", interp::shown(&self.prog, k, v))).collect();
        let tail = if shown.is_empty() { String::new() } else { format!(", {}", shown.join(", ")) };
        self.trace.push(format!("      {adapter}: {outcome}{tail}   ({})", ask.key));
        Ok(())
    }

    /// Asks the model of a source it reads (appendix AF.2.1), and gives the
    /// request's answer without applying it: `read` with the value, or
    /// `unread`, with why in the trace.
    pub fn read_model(&mut self, ask: &Ask) -> Outcome {
        match self.reading_of(ask) {
            Ok((model, r, within)) => {
                let answer = self.models.read(&model, &r, within);
                self.judged(ask, answer)
            }
            Err(unread) => unread,
        }
    }

    /// What a model source's request asks its model: the model, the reading
    /// with its media, and its milliseconds to answer; or, with no source
    /// that reads it, the request's answer.
    pub fn reading_of(&mut self, ask: &Ask) -> Result<(String, Reading, u64), Outcome> {
        let model = ask.adapter.clone();
        let Some(src) = self.prog.sources.iter().find(|s| s.model && s.request_kind() == ask.kind).cloned() else {
            self.trace.push(format!("      model \"{model}\": no source of the program is read by it   ({})", ask.key));
            return Err((UNREAD.into(), vec![]));
        };
        let mut r = Reading {
            unit: ask.label.clone(),
            key: ask.key.clone(),
            kind: src.kind.clone(),
            facts: ask.sends.clone(),
            attr: src.attr.clone(),
            ty: src.ty,
            asking: src.asking.clone(),
            scale: src.scale,
            currency: self.prog.currency.clone(),
            media: BTreeMap::new(),
        };
        r.media = r.media_named().into_iter().filter_map(|h| Some((h.to_string(), self.media.get(h)?))).collect();
        Ok((model, r, src.within_secs.saturating_mul(1000)))
    }

    /// The answer to a model source's request, from what its model read:
    /// a value it may use, or none, and why (appendix AF.2.3).
    pub fn judged(&mut self, ask: &Ask, answer: Answer) -> Outcome {
        let model = ask.adapter.clone();
        let Some(src) = self.prog.sources.iter().find(|s| s.model && s.request_kind() == ask.kind).cloned() else { return (UNREAD.into(), vec![]) };
        let why = match answer {
            Answer::Read(v, _) if v.ty() != src.ty => format!("it gave {}, which is not a {:?}", interp::shown(&self.prog, &src.attr, &v), src.ty),
            // A score outside its scale is not used (appendix AZ.2.2).
            Answer::Read(Val::Count(n), _) if src.scale.is_some_and(|(a, b)| !(a..=b).contains(&n)) => {
                let (a, b) = src.scale.unwrap_or_default();
                format!("it scored {n}, outside the scale from {a} to {b}")
            }
            Answer::Read(v, c) => match (src.sure.map(|t| f64::from(t) / 1000.0), c) {
                (Some(t), Some(c)) if c < t => format!("it read {} at {c:.2}, and the source needs at least {t} sure", interp::shown(&self.prog, &src.attr, &v)),
                (Some(t), None) => format!("it gave no confidence, and the source needs at least {t} sure"),
                (_, c) => {
                    if let Some(c) = c {
                        self.trace.push(format!("      model \"{model}\" read {}'s {} at {c:.2}", ask.label, src.attr));
                    }
                    return (READ.into(), vec![(src.attr.clone(), v)]);
                }
            },
            Answer::Declined => format!("it found no {}", src.attr),
            Answer::Unknown => "it is not available".into(),
            Answer::Late => format!("it did not answer within {}", duration(src.within_secs)),
            Answer::Unsure(x) => format!("it was unsure, at {x:.2}"),
            Answer::Failed(e) => format!("it failed: {e}"),
            Answer::Picked(p) => format!("it picked {p} instead of reading a value"),
        };
        self.trace.push(format!("      model \"{model}\" did not read {}'s {}: {why}", ask.label, src.attr));
        (UNREAD.into(), vec![])
    }

    /// Moves the clock forward by `secs`, firing in order every timer due by
    /// then; while a timer fires, the clock reads its due time (appendix C.3.2).
    pub fn pass(&mut self, secs: u64) -> Result<(), String> {
        let until = self.now.saturating_add(secs);
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
            Timer::Model(key, timer) => {
                if !self.asking.iter().any(|a| a.choice.key == key && a.timer == timer) {
                    return Ok(());
                }
                self.model_answer(&key, &Answer::Late)
            }
            Timer::Goal(label, goal) => {
                let Some(unit) = self.unit(&label) else { return Ok(()) };
                if !interp::goal_applies(&self.prog, &unit, goal) || unit.met.contains(&goal) || unit.alerts.contains(&goal) {
                    return Ok(());
                }
                self.tell(&label, &Msg::GoalDue { goal })
            }
            Timer::Patience(label, id) if id.contains(interp::PASS_MARK) => {
                let Some(unit) = self.unit(&label) else { return Ok(()) };
                if interp::pass_matters(&unit, &id).is_none() {
                    return Ok(());
                }
                self.tell(&label, &Msg::Due { id })
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
                        Some(c) => (l.clone(), interp::fact_of(new, c)),
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
        for a in &self.asking {
            let Chooses::Bind { decide, .. } = &a.to else { continue };
            if bindings.get(decide.binding as usize).copied().flatten().is_none() {
                let b = &self.prog.bindings[decide.binding as usize];
                add(format!("{} uses a {} that is removed, and live units wait for a model to pick one", b.subject, b.target), &decide.label);
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
        for a in &mut self.asking {
            if let Chooses::Bind { decide, .. } = &mut a.to {
                decide.binding = bindings[decide.binding as usize].expect("the check refuses a removed binding") as u32;
            }
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
        // So do the pool keys of the choices waiting for a model.
        let asking = std::mem::take(&mut self.asking);
        for mut a in asking {
            let label = match &a.to {
                Chooses::Bind { decide, .. } => decide.label.clone(),
                Chooses::Ask { ask, .. } => ask.label.clone(),
            };
            let Some(u) = self.unit(&label) else { continue };
            if let Chooses::Bind { decide, pool, .. } = &mut a.to {
                *pool = self.pool_key(&u, decide);
            }
            self.asking.push(a);
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
                if ask.verb == interp::READ_VERB && self.reads_at_once {
                    let (outcome, data) = self.read_model(&ask);
                    if let Err(x) = self.deliver(hole, &ask, &ask.adapter.clone(), &outcome, &data) {
                        first_error.get_or_insert(x);
                    }
                    continue;
                }
                if ask.verb == interp::MODEL_VERB {
                    if let Err(x) = self.ask_model(&ask, hole) {
                        first_error.get_or_insert(x);
                    }
                    continue;
                }
                let within = self.prog.request_within(&ask.kind);
                self.expired.retain(|a| a.key != ask.key);
                self.schedule(self.now.saturating_add(within), Timer::Deadline(hole));
                self.pending.push((ask, hole));
            }
            if effects.is_empty() {
                // Idle: wake the next waiter, or move on to the next unit.
                let mut woke = false;
                while let Some(label) = self.to_wake.front().cloned() {
                    match self.wake_one(&label) {
                        Ok(true) => {
                            woke = true;
                            break;
                        }
                        Ok(false) => {}
                        Err(x) => {
                            first_error.get_or_insert(x);
                        }
                    }
                    self.to_wake.pop_front();
                    self.woken.clear();
                }
                if !woke {
                    return first_error.map_or(Ok(()), Err);
                }
            }
        }
    }

    /// Delivers an internal message to its unit's mailbox, and notes what the unit did.
    fn route(&mut self, hole: HoleId, payload: &Value) -> Result<(), String> {
        let (label, msg) = send_from_value(payload)?;
        let release = matches!(Msg::from_value(&msg), Ok(Msg::Release { .. }));
        // The totals other units read, before a take or a give-back changes them.
        let before = match Msg::from_value(&msg) {
            Ok(Msg::Take { .. } | Msg::Release { .. }) => self.unit(&label).map(|u| interp::fact_of(&self.prog, &u).taken),
            _ => None,
        };
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
        if let (Some(before), Some(u)) = (before, self.unit(&label)) {
            let fact = interp::fact_of(&self.prog, &u);
            if fact.taken != before {
                self.refresh_readers(&label, fact)?;
            }
        }
        if release { self.wake(&label) } else { Ok(()) }
    }

    /// Gives every unit that carries `label` its new totals (appendix AA.2.3).
    fn refresh_readers(&mut self, label: &str, fact: Fact) -> Result<(), String> {
        let refresh = Msg::Refresh { facts: Facts::from([(label.to_string(), fact)]) };
        for r in self.readers.get(label).cloned().unwrap_or_default() {
            self.post(&r, &refresh)?;
        }
        Ok(())
    }

    /// An event or a change whose step would make a filtered total across
    /// units take more than its owner's rules allow is refused by that rule,
    /// before anything changes (appendix AA.2.2); so is an admitted new
    /// entity whose filter holds when it is made (appendix AD.2.3).
    fn refused_across(&mut self, after: &interp::Outcome, what: &str) -> Result<(), String> {
        for d in &after.decides {
            let Some(b) = self.prog.bindings.get(d.binding as usize).filter(|b| b.held.is_some() || b.unique) else { continue };
            let key = self.pool_key(&after.unit, d);
            let Some(target) = self.options(&after.unit, d, &key).into_iter().next() else { continue };
            let take = Msg::Take { from: d.label.clone(), key: d.key(), taken: d.taken.clone() };
            let o = interp::step_at(&self.prog, &target, &take, self.clock());
            let refused = o.sends.iter().find_map(|(_, m)| match m {
                Msg::Refused { rule, .. } if !rule.is_empty() => Some(rule.clone()),
                _ => None,
            });
            if let Some(rule) = refused {
                let total = b.taking.as_ref().map_or(String::new(), |t| t.name.clone());
                let paraphrase = self.prog.rules.iter().find(|r| r.name == rule).map_or(String::new(), |r| r.paraphrase.clone());
                self.trace.push(format!("{:<3} refused {what}: {} would break rule {rule} with its total {total}: {paraphrase}", d.label, target.label));
                return Err(format!("refused by rule {rule}"));
            }
        }
        Ok(())
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
                self.schedule(self.now.saturating_add(*secs), Timer::Patience(label.to_string(), id.clone()));
            }
        }
        for &g in &unit.matched {
            let within = self.prog.goals.get(g as usize).and_then(|x| x.within_secs);
            if let Some(secs) = within
                && self.goal_timers.insert((label.to_string(), g)) {
                self.schedule(self.now.saturating_add(secs), Timer::Goal(label.to_string(), g));
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
