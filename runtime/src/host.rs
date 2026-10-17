//! The host loop (spec §5.4, as corrected by appendix B.3.6). It runs the
//! kernel, routes internal messages, commits, and only then hands outside
//! requests to adapters. In slice 1 the adapters are simulated: a scenario
//! answers for them, one request at a time.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use suture::{HoleId, Injection, Machine, NetBuilder, Value};
use suture_lang::ir::{Program, TIMED_OUT};

use crate::interp::{self, duration, holds};
use crate::kernel::{self, Kinds};
use crate::state::{answer_value, send_from_value, Ask, Msg, Unit, Val};

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
        Host {
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
        }
    }

    /// Creates a root entity as a new unit labeled `label`, and settles it.
    pub fn create(&mut self, kind: &str, label: &str, given: &[(String, Val)]) -> Result<(), String> {
        self.create_with(kind, label, given, &[])
    }

    /// Creates a root entity with the elements of its lists (appendix E.2.6).
    pub fn create_with(&mut self, kind: &str, label: &str, given: &[(String, Val)], items: &[(String, Vec<(String, Val)>)]) -> Result<(), String> {
        if self.machine.hole(label).is_some() {
            return Err(format!("{label} already exists"));
        }
        let unit = interp::new_unit_with(&self.prog, label, kind, given, items)?;
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let (meet, rest) = (b.var(), b.var());
        b.agent(self.kinds.unit, unit.to_value(), meet, &[]);
        b.agent(self.kinds.msg, Msg::Create.to_value(), meet, &[rest]);
        let net = b.finish().map_err(|e| e.to_string())?;
        self.machine.inject(Injection::new(net).new_hole(rest, Some(label))).map_err(|e| e.to_string())?;
        self.machine.run(None);
        let noted = self.note(label);
        let pumped = self.pump();
        noted.and(pumped)
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
            self.trace.push(format!("{:<4}ignored a late answer for {}: it timed out", ask.label, ask.key));
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
                let within = self.prog.effect_of(&ask.kind).map_or(0, |e| e.within_secs);
                self.trace.push(format!("      {}: no answer within {}   ({})", ask.adapter, duration(within), ask.key));
                self.expired.push(ask);
                self.pump()
            }
            Timer::Goal(label, goal) => {
                let Some(unit) = self.unit(&label) else { return Ok(()) };
                let g = &self.prog.goals[goal as usize];
                let applies = g.filter.iter().all(|f| holds(&self.prog, &unit.root, &[], f));
                if !applies || unit.met.contains(&goal) || unit.alerts.contains(&goal) {
                    return Ok(());
                }
                self.tell(&label, &Msg::GoalDue { goal })
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
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let (to, next) = (b.var(), b.var());
        b.agent(self.kinds.msg, msg.to_value(), to, &[next]);
        let net = b.finish().map_err(|e| e.to_string())?;
        self.machine.inject(Injection::new(net).at_label(to, label).new_hole(next, Some(label))).map_err(|e| e.to_string())?;
        self.machine.run(None);
        let noted = self.note(label);
        let pumped = self.pump();
        noted.and(pumped)
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
                let within = self.prog.effect_of(&ask.kind).map_or(0, |e| e.within_secs);
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
        let (label, _) = send_from_value(payload)?;
        let sig = self.machine.program().sig.clone();
        let mut b = NetBuilder::new(&sig);
        let (meet, to, next) = (b.var(), b.var(), b.var());
        b.agent(self.kinds.deliver, (), meet, &[to, next]);
        let net = b.finish().map_err(|e| e.to_string())?;
        let inj = Injection::new(net).at_hole(meet, hole).at_label(to, &label).new_hole(next, Some(&label));
        self.machine.inject(inj).map_err(|e| e.to_string())?;
        self.machine.run(None);
        self.note(&label)
    }

    /// Adds the unit's notes from its last step to the trace.
    fn note(&mut self, label: &str) -> Result<(), String> {
        let Some(unit) = self.unit(label) else {
            let faults: Vec<String> = self.machine.faults().into_iter().map(|f| f.error).collect();
            return Err(format!("{label}: a step faulted: {}", faults.join("; ")));
        };
        for n in &unit.notes {
            self.trace.push(format!("{label:<4}{n}"));
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
