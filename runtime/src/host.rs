//! The host loop (spec §5.4, as corrected by appendix B.3.6). It runs the
//! kernel, routes internal messages, commits, and only then hands outside
//! requests to adapters. In slice 1 the adapters are simulated: a scenario
//! answers for them, one request at a time.

use std::sync::Arc;

use suture::{HoleId, Injection, Machine, NetBuilder, Value};
use suture_lang::ir::Program;

use crate::interp;
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
}

impl Host {
    pub fn new(prog: Program) -> Host {
        let prog = Arc::new(prog);
        let (kernel, kinds) = kernel::program(prog.clone());
        Host { prog, machine: Machine::new(kernel), kinds, trace: vec![], pending: vec![] }
    }

    /// Creates a root entity as a new unit labeled `label`, and settles it.
    pub fn create(&mut self, kind: &str, label: &str, given: &[(String, Val)]) -> Result<(), String> {
        if self.machine.hole(label).is_some() {
            return Err(format!("{label} already exists"));
        }
        let unit = interp::new_unit(&self.prog, label, kind, given)?;
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
    pub fn answer(&mut self, adapter: &str, outcome: &str, data: &[(String, Val)], target: Option<&str>) -> Result<(), String> {
        let i = self
            .pending
            .iter()
            .position(|(a, _)| a.adapter == adapter && target.is_none_or(|t| a.label == t))
            .ok_or_else(|| format!("{adapter} has no pending request{}", target.map(|t| format!(" for {t}")).unwrap_or_default()))?;
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
            self.pending.extend(outside);
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
        Ok(())
    }
}
