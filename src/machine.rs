//! The machine: graph + program + journal. `INTERACT` is the only internal
//! computation; the host acts through `INJECT`, the outbox, and `COMMIT`.

use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::check::{check_inject_static, check_patch, NetError, PatchSpec};
use crate::graph::{Agent, Cause, Endpoint, Graph, GraphError, Hole, HoleOrigin};
use crate::id::{AgentId, Digest, DigestBuilder, HoleId};
use crate::journal::{Journal, JournalError, MemJournal, RecBind, Record, RecoveryError, UpgradeError, FORMAT};
use crate::net::{resolve, Bind, End, ExtKind, HoleRef, Injection, NetData, Slot, Var};
use crate::policy::{PatchView, Policy};
use crate::rng::Rng;
use crate::rule::{Rule, RuleCx, RuleMode, RuleTable};
use crate::sig::{KindId, PortType, Signature};
use crate::value::Value;

#[derive(Clone, Debug)]
pub struct Limits {
    /// Maximum agents a single rule application may create.
    pub max_patch_agents: usize,
    /// Maximum agents a single inject may create.
    pub max_inject_agents: usize,
    /// Maximum encoded size of one agent's payload, in bytes. Collections
    /// belong in the graph as agents, not inside payloads.
    pub max_payload_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits { max_patch_agents: 256, max_inject_agents: 1 << 20, max_payload_bytes: 4096 }
    }
}

/// A versioned program: signature, rules, hard policies and limits.
#[derive(Clone)]
pub struct Program {
    pub sig: Arc<Signature>,
    pub rules: Arc<RuleTable>,
    pub policies: Arc<Vec<Arc<dyn Policy>>>,
    pub limits: Limits,
    /// Set by `with_fingerprint`, for example to a compiled program file's
    /// hash; mixed into the fingerprint, never a replacement for it.
    fixed_fingerprint: Option<Digest>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProgramError {
    #[error("rule table: {0:?}")]
    Rules(Vec<String>),
}

impl Program {
    pub fn new(sig: Signature, rules: RuleTable) -> Result<Program, ProgramError> {
        if !rules.errors().is_empty() {
            return Err(ProgramError::Rules(rules.errors().to_vec()));
        }
        Ok(Program {
            sig: Arc::new(sig),
            rules: Arc::new(rules),
            policies: Arc::new(vec![]),
            limits: Limits::default(),
            fixed_fingerprint: None,
        })
    }

    pub fn with_policy(mut self, p: impl Policy + 'static) -> Program {
        let mut v: Vec<Arc<dyn Policy>> = self.policies.iter().cloned().collect();
        v.push(Arc::new(p));
        self.policies = Arc::new(v);
        self
    }

    pub fn with_limits(mut self, l: Limits) -> Program {
        self.limits = l;
        self
    }

    /// Pins a hash into the fingerprint, for example a compiled program
    /// file's. The structure is still covered (see [`Program::fingerprint`]).
    pub fn with_fingerprint(mut self, d: Digest) -> Program {
        self.fixed_fingerprint = Some(d);
        self
    }

    /// What journal `Version` records identify this program by: a digest of
    /// the signature, the rule table's structure, the policy names and the
    /// limits, combined with the pinned hash when one is set. Rule versions
    /// and rule code are left out, because replay checks them step by step
    /// (`RuleVersion`, `Divergence`).
    pub fn fingerprint(&self) -> Digest {
        let mut d = match self.fixed_fingerprint {
            Some(pin) => {
                let mut d = DigestBuilder::new("program/pinned");
                d.u128(pin.0);
                d
            }
            None => DigestBuilder::new("program"),
        };
        self.sig.encode_structure(&mut d);
        self.rules.encode_structure(&mut d);
        d.u64(self.policies.len() as u64);
        for p in self.policies.iter() {
            d.str(p.name());
        }
        d.u64(self.limits.max_patch_agents as u64);
        d.u64(self.limits.max_inject_agents as u64);
        d.u64(self.limits.max_payload_bytes as u64);
        d.finish()
    }

    pub fn name(&self) -> &str {
        self.sig.name()
    }

    pub fn version(&self) -> u32 {
        self.sig.version()
    }
}

/// Which ready pair to reduce next. Results never depend on the choice.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedulerKind {
    Fifo,
    Lifo,
    Random(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Quiescent { reductions: u64 },
    Budget { reductions: u64 },
}

/// Parallelism profile of a run: each round reduces every pair that was
/// ready at its start, so `rounds` is the critical-path depth.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rounds {
    pub rounds: u64,
    pub reductions: u64,
    pub max_width: usize,
}

/// An active pair that cannot reduce: no rule, a rule error or panic, or a
/// patch rejected by validation or policy. Faults are local and permanent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    pub left: AgentId,
    pub right: AgentId,
    pub rule: Option<String>,
    pub error: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InjectReceipt {
    pub seq: u64,
    pub new_holes: Vec<(Var, HoleId)>,
    /// True when the idempotency key was already applied; nothing changed.
    pub duplicate: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum InjectError {
    #[error("unknown hole {0}")]
    UnknownHole(String),
    #[error("hole bound twice: {0}")]
    HoleBoundTwice(String),
    #[error("label already in use: {0}")]
    LabelInUse(String),
    #[error(transparent)]
    Net(#[from] NetError),
    #[error("ill-typed connection {a} ~ {b}")]
    Type { a: String, b: String },
}

/// A pending effect: an effect agent waiting at its outbox hole.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectView {
    pub agent: AgentId,
    pub hole: HoleId,
    pub kind: KindId,
    pub kind_name: String,
    pub payload: Value,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    pub reductions: u64,
    pub injects: u64,
    pub faults: u64,
    pub effects: u64,
    pub vicious: u64,
    pub created: u64,
    pub max_agents: usize,
    pub per_rule: BTreeMap<String, u64>,
    /// Queue entries examined by `take_new_effects`, stale ones included.
    pub effects_examined: u64,
}

/// A resolved wire end during installation: a port of a new agent, or an
/// endpoint already in the graph.
#[derive(Clone, Copy, Debug)]
enum Ep {
    New(usize, u8),
    Old(Endpoint),
}

pub(crate) struct Prepared {
    conns: Vec<(Ep, Ep)>,
    vicious: usize,
}

struct InjectPlan {
    consumed: Vec<HoleId>,
    created: Vec<(Var, HoleId, Option<String>)>,
    rec_binds: Vec<(Var, RecBind)>,
    conns: Vec<(Ep, Ep)>,
}

enum Target {
    Hole(HoleId),
    New(HoleId, Option<String>),
}

pub struct Machine {
    prog: Program,
    graph: Graph,
    ready: VecDeque<(AgentId, AgentId)>,
    faults: BTreeMap<(AgentId, AgentId), Fault>,
    journal: Box<dyn Journal>,
    replaying: bool,
    inject_seq: u64,
    keys: HashMap<String, u64>,
    stats: Stats,
    sched: SchedulerKind,
    rng: Rng,
    /// Effect holes still waiting for an answer: the outbox index.
    pending_effects: BTreeSet<HoleId>,
    /// Effect holes created since the last `take_new_effects`.
    new_effects: Vec<HoleId>,
}

fn norm(a: AgentId, b: AgentId) -> (AgentId, AgentId) {
    if a <= b { (a, b) } else { (b, a) }
}

fn panic_text(p: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "non-string panic payload".to_string()
    }
}

impl Machine {
    /// A fresh machine with an in-memory journal.
    pub fn new(prog: Program) -> Machine {
        Self::fresh(prog, Box::new(MemJournal::new()))
    }

    /// A fresh machine writing to `journal`, which must be empty
    /// (use `Machine::recover` to continue an existing journal).
    pub fn with_journal(prog: Program, journal: Box<dyn Journal>) -> Result<Machine, JournalError> {
        if journal.len_durable() != 0 {
            return Err(JournalError::Format("journal is not empty; use Machine::recover".into()));
        }
        let mut m = Self::fresh(prog, journal);
        m.journal.commit()?;
        Ok(m)
    }

    fn fresh(prog: Program, journal: Box<dyn Journal>) -> Machine {
        let mut m = Machine {
            prog,
            graph: Graph::default(),
            ready: VecDeque::new(),
            faults: BTreeMap::new(),
            journal,
            replaying: false,
            inject_seq: 0,
            keys: HashMap::new(),
            stats: Stats::default(),
            sched: SchedulerKind::Fifo,
            rng: Rng::new(0),
            pending_effects: BTreeSet::new(),
            new_effects: Vec::new(),
        };
        let (program, version) = (m.prog.name().to_string(), m.prog.version());
        m.journal.append(Record::Header { program: program.clone(), version, format: FORMAT });
        m.journal.append(Record::Version { program, version, hash: m.prog.fingerprint() });
        m
    }

    pub fn set_scheduler(&mut self, s: SchedulerKind) {
        if let SchedulerKind::Random(seed) = s {
            self.rng = Rng::new(seed);
        }
        self.sched = s;
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn program(&self) -> &Program {
        &self.prog
    }

    pub fn sig(&self) -> &Signature {
        &self.prog.sig
    }

    pub fn stats(&self) -> &Stats {
        &self.stats
    }

    pub fn digest(&self) -> Digest {
        self.graph.digest()
    }

    pub fn ready_count(&self) -> usize {
        self.ready.len()
    }

    pub fn is_quiescent(&self) -> bool {
        self.ready.is_empty()
    }

    pub fn faults(&self) -> Vec<Fault> {
        self.faults.values().cloned().collect()
    }

    pub(crate) fn is_faulted(&self, a: AgentId, b: AgentId) -> bool {
        self.faults.contains_key(&norm(a, b))
    }

    pub fn hole(&self, label: &str) -> Option<HoleId> {
        self.graph.hole_by_label(label)
    }

    /// Every hole label, in the order the holes were created.
    pub fn labels(&self) -> Vec<String> {
        self.graph.labels().into_iter().map(|(_, l)| l.to_string()).collect()
    }

    /// Replaces the program with a later version of it: the same name, a
    /// higher version and the same kinds, so the graph stays well formed.
    /// Its rules apply from the next reduction on; the journal records the
    /// change as a `Version` record, and `recover_with` replays across it.
    pub fn upgrade(&mut self, prog: Program) -> Result<(), UpgradeError> {
        if prog.name() != self.prog.name() {
            return Err(UpgradeError(format!("{} is not {}", prog.name(), self.prog.name())));
        }
        if prog.version() <= self.prog.version() {
            return Err(UpgradeError(format!("version {} is not after {}", prog.version(), self.prog.version())));
        }
        let kinds = |p: &Program| p.sig.kinds().map(|(_, d)| format!("{d:?}")).collect::<Vec<_>>();
        if kinds(&prog) != kinds(&self.prog) {
            return Err(UpgradeError("the kinds differ".into()));
        }
        self.prog = prog;
        let (program, version, hash) = (self.prog.name().to_string(), self.prog.version(), self.prog.fingerprint());
        self.record(Record::Version { program, version, hash });
        Ok(())
    }

    /// The agent attached to hole `h` and the port it is attached by.
    pub fn hole_peer(&self, h: HoleId) -> Option<(AgentId, usize, &Agent)> {
        match self.graph.hole(h)?.peer {
            Endpoint::Port(a, p) => Some((a, p as usize, self.graph.agent(a)?)),
            Endpoint::Hole(_) => None,
        }
    }

    /// Pending effects (effect agents still waiting at their outbox hole),
    /// sorted by hole identity. Reads the outbox index, never the whole
    /// graph; hosts that poll should use [`Machine::take_new_effects`].
    pub fn outbox(&self) -> Vec<EffectView> {
        self.pending_effects.iter().filter_map(|&h| self.effect_view(h)).collect()
    }

    /// The effects created since the last call that are still pending,
    /// sorted by hole identity. Examines only those queue entries (counted in
    /// `Stats::effects_examined`), however many effects are pending.
    pub fn take_new_effects(&mut self) -> Vec<EffectView> {
        let mut batch = std::mem::take(&mut self.new_effects);
        self.stats.effects_examined += batch.len() as u64;
        batch.sort();
        batch.into_iter().filter(|h| self.pending_effects.contains(h)).filter_map(|h| self.effect_view(h)).collect()
    }

    fn effect_view(&self, h: HoleId) -> Option<EffectView> {
        let hole = self.graph.hole(h)?;
        let HoleOrigin::Effect { agent } = hole.origin else { return None };
        if hole.peer != Endpoint::Port(agent, 0) {
            return None;
        }
        let a = self.graph.agent(agent)?;
        Some(EffectView {
            agent,
            hole: h,
            kind: a.kind,
            kind_name: self.prog.sig.kind(a.kind).name.clone(),
            payload: a.payload.clone(),
        })
    }

    /// Indexes a new effect hole and queues it for `take_new_effects`. A host
    /// that only reads `outbox()` never drains the queue, so the queue is
    /// pruned to pending effects whenever it outgrows them.
    fn note_effect(&mut self, h: HoleId) {
        self.pending_effects.insert(h);
        self.new_effects.push(h);
        if self.new_effects.len() > 2 * self.pending_effects.len() + 1024 {
            self.new_effects.retain(|h| self.pending_effects.contains(h));
        }
    }

    /// Sorted keys of the conserved agents of `class` currently in the graph.
    pub fn census(&self, class: &str) -> Vec<Value> {
        let mut keys: Vec<Value> = self
            .graph
            .agents_sorted()
            .into_iter()
            .filter_map(|(_, a)| self.prog.sig.conserved_key(a.kind, &a.payload))
            .filter(|(c, _)| c == class)
            .map(|(_, k)| k)
            .collect();
        keys.sort();
        keys
    }

    /// `Kind#id(payload)`, for traces and explanations.
    pub fn describe(&self, a: AgentId) -> String {
        match self.graph.agent(a) {
            None => format!("<gone {a}>"),
            Some(ag) => {
                let name = &self.prog.sig.kind(ag.kind).name;
                if ag.payload == Value::Unit { format!("{name}#{a}") } else { format!("{name}#{a}{}", paren(&ag.payload)) }
            }
        }
    }

    /// Graph well-formedness plus scheduler consistency: the ready queue
    /// holds exactly the active, non-faulted pairs.
    pub fn check_invariants(&self) -> Result<(), GraphError> {
        self.graph.check(&self.prog.sig)?;
        let active: Vec<_> =
            self.graph.active_pairs().into_iter().filter(|p| !self.faults.contains_key(p)).collect();
        let mut queued: Vec<_> = self.ready.iter().copied().collect();
        queued.sort();
        if active != queued {
            return Err(GraphError::Ready(format!("active {active:?} vs queued {queued:?}")));
        }
        let effects: Vec<HoleId> = self
            .graph
            .holes_sorted()
            .into_iter()
            .filter(|(_, h)| matches!(h.origin, HoleOrigin::Effect { .. }))
            .map(|(id, _)| id)
            .collect();
        let indexed: Vec<HoleId> = self.pending_effects.iter().copied().collect();
        if effects != indexed {
            return Err(GraphError::Outbox(format!("holes {effects:?} vs index {indexed:?}")));
        }
        Ok(())
    }

    // ----------------------------------------------------------------- run

    fn pop_ready(&mut self) -> Option<(AgentId, AgentId)> {
        match self.sched {
            SchedulerKind::Fifo => self.ready.pop_front(),
            SchedulerKind::Lifo => self.ready.pop_back(),
            SchedulerKind::Random(_) => {
                if self.ready.is_empty() {
                    return None;
                }
                let i = self.rng.index(self.ready.len());
                self.ready.swap_remove_back(i)
            }
        }
    }

    /// Reduces one ready pair. Returns false when nothing is ready.
    pub fn step(&mut self) -> bool {
        match self.pop_ready() {
            Some((a, b)) => {
                self.reduce_pair(a, b);
                true
            }
            None => false,
        }
    }

    /// Reduces until quiescent or until `budget` pairs have been processed.
    pub fn run(&mut self, budget: Option<u64>) -> RunOutcome {
        let mut n = 0;
        loop {
            if self.ready.is_empty() {
                return RunOutcome::Quiescent { reductions: n };
            }
            if budget.is_some_and(|b| n >= b) {
                return RunOutcome::Budget { reductions: n };
            }
            self.step();
            n += 1;
        }
    }

    /// Reduces in rounds: every pair ready at the start of a round is
    /// reduced (in sorted order) before any pair created during the round.
    pub fn run_rounds(&mut self, max_rounds: Option<u64>) -> Rounds {
        let mut out = Rounds::default();
        while !self.ready.is_empty() && max_rounds.is_none_or(|m| out.rounds < m) {
            let mut batch: Vec<_> = self.ready.drain(..).collect();
            batch.sort();
            out.rounds += 1;
            out.max_width = out.max_width.max(batch.len());
            for (a, b) in batch {
                self.reduce_pair(a, b);
                out.reductions += 1;
            }
        }
        out
    }

    fn enqueue(&mut self, a: AgentId, b: AgentId) {
        self.ready.push_back(norm(a, b));
    }

    fn record(&mut self, r: Record) {
        if !self.replaying {
            self.journal.append(r);
        }
    }

    fn fault(&mut self, l: AgentId, r: AgentId, rule: Option<String>, error: String) {
        self.stats.faults += 1;
        let f = Fault { left: l, right: r, rule: rule.clone(), error: error.clone() };
        self.faults.insert(norm(l, r), f);
        self.record(Record::Fault { left: l, right: r, rule, error });
    }

    /// Looks up and orients the rule for an active pair.
    fn orient(&self, a: AgentId, b: AgentId) -> Result<(usize, AgentId, AgentId), String> {
        let (ka, kb) = (self.graph.agent(a).map(|x| x.kind), self.graph.agent(b).map(|x| x.kind));
        let (Some(ka), Some(kb)) = (ka, kb) else { return Err("pair no longer exists".into()) };
        match self.prog.rules.lookup(ka, kb) {
            None => {
                let (na, nb) = (&self.prog.sig.kind(ka).name, &self.prog.sig.kind(kb).name);
                Err(format!("no rule for {na} >< {nb}"))
            }
            Some((idx, _, swapped)) => Ok(if swapped && ka != kb { (idx, b, a) } else { (idx, a, b) }),
        }
    }

    fn reduce_pair(&mut self, a: AgentId, b: AgentId) {
        let (ridx, l, r) = match self.orient(a, b) {
            Ok(x) => x,
            Err(e) => return self.fault(a, b, None, e),
        };
        let prog = self.prog.clone();
        let rule = prog.rules.get(ridx);
        let net = match self.run_rule(rule, l, r) {
            Ok(net) => net,
            Err(e) => return self.fault(l, r, Some(rule.name.clone()), e),
        };
        match self.prepare(rule, l, r, &net) {
            Ok(prep) => {
                self.install(ridx, rule, l, r, &net, prep);
                let patch = (rule.mode == RuleMode::Recorded).then(|| net.clone());
                self.record(Record::Reduce {
                    left: l,
                    right: r,
                    rule: rule.name.clone(),
                    version: rule.version,
                    digest: net.digest(),
                    patch,
                });
            }
            Err(e) => self.fault(l, r, Some(rule.name.clone()), e),
        }
    }

    /// Runs a rule body, converting errors and panics into fault messages.
    pub(crate) fn run_rule(&self, rule: &Rule, l: AgentId, r: AgentId) -> Result<NetData, String> {
        let (la, ra) = (&self.graph.agent(l).unwrap(), &self.graph.agent(r).unwrap());
        let mut cx = RuleCx::new(&self.prog.sig, la.kind, la.payload.clone(), ra.kind, ra.payload.clone());
        let body = rule.body.clone();
        match catch_unwind(AssertUnwindSafe(|| body(&mut cx))) {
            Err(p) => Err(format!("rule panicked: {}", panic_text(p))),
            Ok(Err(e)) => Err(e.to_string()),
            Ok(Ok(())) => cx.finish().map_err(|e| e.to_string()),
        }
    }

    fn port_type(&self, net: Option<&NetData>, e: Ep) -> Option<PortType> {
        let sig = &self.prog.sig;
        match e {
            Ep::New(i, p) => Some(sig.port_type(net.unwrap().agents[i].kind, p as usize)),
            Ep::Old(Endpoint::Port(a, p)) => Some(sig.port_type(self.graph.agent(a)?.kind, p as usize)),
            Ep::Old(Endpoint::Hole(_)) => None,
        }
    }

    fn describe_ep(&self, net: Option<&NetData>, e: Ep) -> String {
        let sig = &self.prog.sig;
        let ty = self.port_type(net, e).map(|t| sig.fmt_port_type(t)).unwrap_or_else(|| "hole".into());
        match e {
            Ep::New(i, p) => format!("new {}.{p}:{ty}", sig.kind(net.unwrap().agents[i].kind).name),
            Ep::Old(Endpoint::Port(a, p)) => format!("{}.{p}:{ty}", self.describe(a)),
            Ep::Old(Endpoint::Hole(h)) => format!("{h}"),
        }
    }

    /// Every connection between two typed ports must be compatible.
    fn check_conns(&self, net: Option<&NetData>, conns: &[(Ep, Ep)]) -> Result<(), (String, String)> {
        for &(a, b) in conns {
            if let (Some(ta), Some(tb)) = (self.port_type(net, a), self.port_type(net, b))
                && !self.prog.sig.compatible(ta, tb) {
                return Err((self.describe_ep(net, a), self.describe_ep(net, b)));
            }
        }
        Ok(())
    }

    /// Validates a patch for the pair `(l, r)` and resolves its wiring
    /// against the current graph. Never mutates.
    pub(crate) fn prepare(&self, rule: &Rule, l: AgentId, r: AgentId, net: &NetData) -> Result<Prepared, String> {
        let sig = &*self.prog.sig;
        let (la, ra) = (self.graph.agent(l).unwrap(), self.graph.agent(r).unwrap());
        let n_left = la.ports.len() - 1;
        let iface: Vec<PortType> = (1..la.ports.len())
            .map(|p| sig.port_type(la.kind, p))
            .chain((1..ra.ports.len()).map(|p| sig.port_type(ra.kind, p)))
            .collect();
        let spec = PatchSpec {
            iface: &iface,
            consumed: [(la.kind, &la.payload), (ra.kind, &ra.payload)],
            may_emit: &rule.may_emit,
            max_agents: self.prog.limits.max_patch_agents,
            max_payload: self.prog.limits.max_payload_bytes,
        };
        check_patch(sig, net, &spec).map_err(|e| e.to_string())?;
        let view = PatchView {
            sig,
            rule: &rule.name,
            left: (la.kind, &la.payload),
            right: (ra.kind, &ra.payload),
            created: &net.agents,
        };
        for p in self.prog.policies.iter() {
            p.check(&view).map_err(|e| format!("policy {} rejected the patch: {e}", p.name()))?;
        }
        // The outside of every auxiliary port; aux ports wired to each other glue.
        let outside: Vec<Endpoint> = la.ports[1..].iter().chain(ra.ports[1..].iter()).copied().collect();
        let slot_of = |e: Endpoint| match e {
            Endpoint::Port(x, p) if x == l && p > 0 => Some(p as usize - 1),
            Endpoint::Port(x, p) if x == r && p > 0 => Some(n_left + p as usize - 1),
            _ => None,
        };
        let ext: Vec<ExtKind> =
            outside.iter().map(|&e| slot_of(e).map_or(ExtKind::Terminal, ExtKind::Glue)).collect();
        let ext_vars: Vec<Var> = (0..outside.len() as u32).map(Var).collect();
        let resolved = resolve(net, &ext_vars, &ext).map_err(|e| e.to_string())?;
        let map = |e: End| match e {
            End::Agent(i, p) => Ep::New(i, p as u8),
            End::Ext(k) => Ep::Old(outside[k]),
        };
        let conns: Vec<(Ep, Ep)> = resolved.conns.iter().map(|&(a, b)| (map(a), map(b))).collect();
        self.check_conns(Some(net), &conns).map_err(|(a, b)| format!("ill-typed wire {a} ~ {b}"))?;
        Ok(Prepared { conns, vicious: resolved.vicious })
    }

    /// Atomically replaces the pair by the prepared patch.
    pub(crate) fn install(&mut self, ridx: usize, rule: &Rule, l: AgentId, r: AgentId, net: &NetData, prep: Prepared) {
        self.graph.remove_agent(l);
        self.graph.remove_agent(r);
        let ids: Vec<AgentId> =
            (0..net.agents.len()).map(|i| AgentId::from_reduction(l, r, &rule.name, i as u32)).collect();
        let placeholder = Endpoint::Hole(HoleId(0));
        for (i, na) in net.agents.iter().enumerate() {
            let agent = Agent {
                kind: na.kind,
                payload: na.payload.clone(),
                ports: vec![placeholder; na.ports.len()],
                cause: Cause::Reduce { left: l, right: r, rule: ridx as u32 },
            };
            self.graph.insert_agent(ids[i], agent);
            if na.ports[0] == Slot::Emit {
                let h = HoleId::for_effect(ids[i]);
                let hole = Hole { peer: Endpoint::Port(ids[i], 0), label: None, origin: HoleOrigin::Effect { agent: ids[i] } };
                self.graph.insert_hole(h, hole);
                self.graph.set_end(Endpoint::Port(ids[i], 0), Endpoint::Hole(h));
                self.note_effect(h);
                self.stats.effects += 1;
            }
        }
        self.connect(&ids, &prep.conns);
        self.stats.reductions += 1;
        self.stats.created += net.agents.len() as u64;
        self.stats.vicious += prep.vicious as u64;
        *self.stats.per_rule.entry(rule.name.clone()).or_default() += 1;
        self.stats.max_agents = self.stats.max_agents.max(self.graph.agent_count());
    }

    fn connect(&mut self, ids: &[AgentId], conns: &[(Ep, Ep)]) {
        let real = |e: Ep| match e {
            Ep::New(i, p) => Endpoint::Port(ids[i], p),
            Ep::Old(x) => x,
        };
        for &(a, b) in conns {
            let (ea, eb) = (real(a), real(b));
            self.graph.link(ea, eb);
            if let (Endpoint::Port(x, 0), Endpoint::Port(y, 0)) = (ea, eb) {
                self.enqueue(x, y);
            }
        }
    }

    // -------------------------------------------------------------- inject

    /// `INJECT`: admits a net, binding its open variables to existing holes
    /// (consuming them) or to new holes. Validation happens before any
    /// mutation; on error the graph is unchanged.
    pub fn inject(&mut self, inj: Injection) -> Result<InjectReceipt, InjectError> {
        if let Some(k) = &inj.key
            && let Some(&seq) = self.keys.get(k) {
            return Ok(InjectReceipt { seq, new_holes: vec![], duplicate: true });
        }
        let seq = self.inject_seq;
        let plan = self.plan_inject(&inj, seq)?;
        let new_holes = plan.created.iter().map(|(v, h, _)| (*v, *h)).collect();
        self.apply_inject(seq, &inj.net, &plan);
        if let Some(k) = &inj.key {
            self.keys.insert(k.clone(), seq);
        }
        self.record(Record::Inject { seq, key: inj.key, mint: inj.mint, net: inj.net, binds: plan.rec_binds });
        Ok(InjectReceipt { seq, new_holes, duplicate: false })
    }

    fn plan_inject(&self, inj: &Injection, seq: u64) -> Result<InjectPlan, InjectError> {
        let sig = &*self.prog.sig;
        let limits = &self.prog.limits;
        check_inject_static(sig, &inj.net, inj.mint, limits.max_inject_agents, limits.max_payload_bytes)?;
        let mut targets = Vec::with_capacity(inj.binds.len());
        let mut consumed: Vec<HoleId> = vec![];
        let mut labels: Vec<&str> = vec![];
        for (k, (_, bind)) in inj.binds.iter().enumerate() {
            match bind {
                Bind::Hole(r) => {
                    let found = match r {
                        HoleRef::Id(h) => self.graph.hole(*h).map(|_| *h),
                        HoleRef::Label(l) => self.graph.hole_by_label(l),
                    };
                    let h = found.ok_or_else(|| InjectError::UnknownHole(format!("{r:?}")))?;
                    if consumed.contains(&h) {
                        return Err(InjectError::HoleBoundTwice(format!("{r:?}")));
                    }
                    consumed.push(h);
                    targets.push(Target::Hole(h));
                }
                Bind::New(label) => {
                    if let Some(l) = label {
                        if labels.contains(&l.as_str()) {
                            return Err(InjectError::LabelInUse(l.clone()));
                        }
                        labels.push(l);
                    }
                    targets.push(Target::New(HoleId::from_inject(seq, k as u32), label.clone()));
                }
            }
        }
        for l in &labels {
            if let Some(h) = self.graph.hole_by_label(l)
                && !consumed.contains(&h) {
                return Err(InjectError::LabelInUse(l.to_string()));
            }
        }
        let ext: Vec<ExtKind> = targets
            .iter()
            .map(|t| match t {
                Target::Hole(h) => match self.graph.hole(*h).unwrap().peer {
                    Endpoint::Hole(h2) => targets
                        .iter()
                        .position(|t2| matches!(t2, Target::Hole(x) if *x == h2))
                        .map_or(ExtKind::Terminal, ExtKind::Glue),
                    Endpoint::Port(..) => ExtKind::Terminal,
                },
                Target::New(..) => ExtKind::Terminal,
            })
            .collect();
        let ext_vars: Vec<Var> = inj.binds.iter().map(|(v, _)| *v).collect();
        let resolved = resolve(&inj.net, &ext_vars, &ext)?;
        let map = |e: End| match e {
            End::Agent(i, p) => Ep::New(i, p as u8),
            End::Ext(k) => match &targets[k] {
                Target::Hole(h) => Ep::Old(self.graph.hole(*h).unwrap().peer),
                Target::New(h, _) => Ep::Old(Endpoint::Hole(*h)),
            },
        };
        let conns: Vec<(Ep, Ep)> = resolved.conns.iter().map(|&(a, b)| (map(a), map(b))).collect();
        self.check_conns(Some(&inj.net), &conns).map_err(|(a, b)| InjectError::Type { a, b })?;
        let mut created = vec![];
        let rec_binds = inj
            .binds
            .iter()
            .zip(&targets)
            .map(|((v, _), t)| match t {
                Target::Hole(h) => (*v, RecBind::Hole(*h)),
                Target::New(h, label) => {
                    created.push((*v, *h, label.clone()));
                    (*v, RecBind::New { label: label.clone() })
                }
            })
            .collect();
        Ok(InjectPlan { consumed, created, rec_binds, conns })
    }

    fn apply_inject(&mut self, seq: u64, net: &NetData, plan: &InjectPlan) {
        for h in &plan.consumed {
            self.graph.remove_hole(*h);
            self.pending_effects.remove(h);
        }
        let placeholder = Endpoint::Hole(HoleId(0));
        for (_, h, label) in &plan.created {
            let hole = Hole { peer: placeholder, label: label.clone(), origin: HoleOrigin::Inject { seq } };
            self.graph.insert_hole(*h, hole);
        }
        let ids: Vec<AgentId> = (0..net.agents.len()).map(|i| AgentId::from_inject(seq, i as u32)).collect();
        for (i, na) in net.agents.iter().enumerate() {
            let agent = Agent {
                kind: na.kind,
                payload: na.payload.clone(),
                ports: vec![placeholder; na.ports.len()],
                cause: Cause::Inject { seq },
            };
            self.graph.insert_agent(ids[i], agent);
        }
        self.connect(&ids, &plan.conns);
        self.inject_seq = seq + 1;
        self.stats.injects += 1;
        self.stats.created += net.agents.len() as u64;
        self.stats.max_agents = self.stats.max_agents.max(self.graph.agent_count());
    }

    // ------------------------------------------------------------ recovery

    /// Rebuilds a machine from the committed records of `journal` and keeps
    /// appending to it. Deterministic reductions are re-run and must
    /// reproduce their recorded digest; recorded (opaque) reductions apply
    /// the journaled patch without re-running the rule.
    pub fn recover(prog: Program, journal: Box<dyn Journal>) -> Result<Machine, RecoveryError> {
        Self::recover_with(prog, vec![], journal)
    }

    /// [`Machine::recover`] for a journal that upgrades: `later` holds the
    /// programs it upgrades to, and each `Version` record after the first
    /// switches to the one it names.
    pub fn recover_with(prog: Program, mut later: Vec<Program>, mut journal: Box<dyn Journal>) -> Result<Machine, RecoveryError> {
        journal.crash(); // only committed records survive recovery
        let records = journal.durable()?;
        let mut m = Self::fresh(prog, journal);
        let Some(first) = records.first() else { return Ok(m) };
        m.journal.crash(); // drop the header `fresh` buffered: the journal has one
        let Record::Header { program, version, format } = first else {
            return Err(RecoveryError::Header("first record is not a header".into()));
        };
        if program != m.prog.name() || *version != m.prog.version() || *format != FORMAT {
            return Err(RecoveryError::Header(format!(
                "journal is {program} v{version} (format {format}), program is {} v{} (format {FORMAT})",
                m.prog.name(),
                m.prog.version()
            )));
        }
        m.replaying = true;
        for (step, rec) in records.iter().enumerate().skip(1) {
            match rec {
                Record::Inject { seq, key, mint, net, binds } => {
                    let mut inj = Injection { net: net.clone(), binds: vec![], key: key.clone(), mint: *mint };
                    for (v, b) in binds {
                        let bind = match b {
                            RecBind::Hole(h) => Bind::Hole(HoleRef::Id(*h)),
                            RecBind::New { label } => Bind::New(label.clone()),
                        };
                        inj.binds.push((*v, bind));
                    }
                    let rc = m.inject(inj).map_err(|e| RecoveryError::Inject(format!("record {step}: {e}")))?;
                    if rc.seq != *seq || rc.duplicate {
                        return Err(RecoveryError::Inject(format!("record {step}: sequence {} != {seq}", rc.seq)));
                    }
                }
                Record::Reduce { left, right, rule, version, digest, patch } => {
                    m.replay_reduce(step, *left, *right, rule, *version, *digest, patch.as_ref())?;
                }
                Record::Fault { left, right, rule, error } => {
                    m.stats.faults += 1;
                    let f = Fault { left: *left, right: *right, rule: rule.clone(), error: error.clone() };
                    m.faults.insert(norm(*left, *right), f);
                }
                Record::Version { program, version, hash } => {
                    if let Err(e) = m.check_version(program, *version, *hash) {
                        // A program it upgrades to, if given; otherwise the mismatch stands.
                        let i = later.iter().position(|p| p.name() == program && p.version() == *version && p.fingerprint() == *hash).ok_or(e)?;
                        let p = later.remove(i);
                        m.upgrade(p).map_err(|e| RecoveryError::Upgrade(e.to_string()))?;
                    }
                }
                Record::Commit | Record::Header { .. } => {}
            }
        }
        m.replaying = false;
        m.ready = m.graph.active_pairs().into_iter().filter(|p| !m.faults.contains_key(p)).collect();
        Ok(m)
    }

    fn check_version(&self, program: &str, version: u32, hash: Digest) -> Result<(), RecoveryError> {
        let (p, v, h) = (self.prog.name(), self.prog.version(), self.prog.fingerprint());
        if program == p && version == v && hash == h {
            return Ok(());
        }
        Err(RecoveryError::Version { journal: format!("{program} v{version} {hash}"), program: format!("{p} v{v} {h}") })
    }

    #[allow(clippy::too_many_arguments)]
    fn replay_reduce(
        &mut self,
        step: usize,
        l: AgentId,
        r: AgentId,
        rule_name: &str,
        version: u32,
        digest: Digest,
        patch: Option<&NetData>,
    ) -> Result<(), RecoveryError> {
        if self.graph.active_partner(l) != Some(r) {
            return Err(RecoveryError::MissingPair { step });
        }
        let prog = self.prog.clone();
        let (ridx, rule) = prog.rules.by_name(rule_name).ok_or_else(|| RecoveryError::UnknownRule(rule_name.into()))?;
        let diverged = || RecoveryError::Divergence { rule: rule_name.to_string(), step };
        match self.orient(l, r) {
            Ok((idx, ol, or)) if idx == ridx && ol == l && or == r => {}
            _ => return Err(diverged()),
        }
        let net = match patch {
            Some(p) => {
                if p.digest() != digest {
                    return Err(diverged());
                }
                p.clone()
            }
            None => {
                if rule.version != version {
                    return Err(RecoveryError::RuleVersion {
                        rule: rule_name.to_string(),
                        journal: version,
                        program: rule.version,
                    });
                }
                let net = self.run_rule(rule, l, r).map_err(|_| diverged())?;
                if net.digest() != digest {
                    return Err(diverged());
                }
                net
            }
        };
        let prep = self.prepare(rule, l, r, &net).map_err(|_| diverged())?;
        self.install(ridx, rule, l, r, &net, prep);
        Ok(())
    }

    // -------------------------------------------------------------- commit

    /// `COMMIT`: makes every record so far durable.
    pub fn commit(&mut self) -> Result<(), JournalError> {
        self.journal.commit()
    }

    /// How many records the journal has committed.
    pub fn journal_len(&self) -> usize {
        self.journal.len_durable()
    }

    /// Releases the journal (e.g. to simulate a crash and recover from it).
    pub fn into_journal(self) -> Box<dyn Journal> {
        self.journal
    }
}

fn paren(v: &Value) -> String {
    match v {
        Value::Tuple(_) => v.to_string(),
        _ => format!("({v})"),
    }
}
