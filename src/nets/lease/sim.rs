//! A seeded simulator for Experiment A: random arrival, completion and
//! failure orders; random partial reduction between boundary events; crashes
//! that lose uncommitted work; duplicated deliveries; and an independent
//! provider ledger. Every step is checked against the invariants the readme
//! asks for, and the final grants are compared with the reference model.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use super::{
    ack_injection, admit_injection, init_pool_injection, job_info, mail_of, program, reply_injection, route_injection,
    status, LeaseConfig, LeaseKinds, LeaseSpec, MailMsg, Outcome, RefPool, Request,
};
use crate::explain::Terminal;
use crate::journal::FileJournal;
use crate::machine::{EffectView, InjectReceipt, Machine, Program, SchedulerKind, Stats};
use crate::net::Injection;
use crate::rng::Rng;
use crate::{Digest, HoleId, Value};

#[derive(Clone, Debug)]
pub struct SimConfig {
    pub seed: u64,
    pub requests: Vec<Request>,
    pub leases: Vec<LeaseSpec>,
    /// Probability that a job fails.
    pub fail_rate: f64,
    pub max_attempts: i64,
    /// Probability of a crash after an action.
    pub crash_rate: f64,
    /// Probability of a commit after an action.
    pub commit_rate: f64,
    /// Probability of re-delivering an already acknowledged inject.
    pub dup_rate: f64,
    /// `None` picks FIFO, LIFO or random from the seed.
    pub scheduler: Option<SchedulerKind>,
    pub file_journal: Option<PathBuf>,
}

impl SimConfig {
    /// The readme's scenario: ten requests from four tenants compete for
    /// three leases (two in eu-west-1, one in us-east-1).
    pub fn standard(seed: u64) -> SimConfig {
        let (eu, us) = ("eu-west-1", "us-east-1");
        let reqs: [(&str, &[&str]); 10] = [
            ("acme", &[eu]),
            ("globex", &[us]),
            ("initech", &[eu, us]),
            ("acme", &[eu]),
            ("umbrella", &[us]),
            ("globex", &[eu]),
            ("initech", &[eu]),
            ("umbrella", &[eu, us]),
            ("acme", &[us]),
            ("globex", &[eu]),
        ];
        SimConfig {
            seed,
            requests: reqs.iter().enumerate().map(|(i, (t, rs))| Request::new(&format!("r{i}"), t, rs)).collect(),
            leases: vec![LeaseSpec::new(1, eu), LeaseSpec::new(2, eu), LeaseSpec::new(3, us)],
            fail_rate: 0.25,
            max_attempts: 3,
            crash_rate: 0.0,
            commit_rate: 1.0,
            dup_rate: 0.0,
            scheduler: None,
            file_journal: None,
        }
    }
}

/// The authoritative external ledger: which job currently holds each lease.
/// Idempotent per effect id, as a real provider keyed by idempotency token.
#[derive(Clone, Debug, Default)]
pub struct Provider {
    held: BTreeMap<i64, HoleId>,
    started: BTreeMap<HoleId, i64>,
    finished: BTreeMap<HoleId, bool>,
    pub violations: Vec<String>,
}

impl Provider {
    fn start(&mut self, e: &EffectView) {
        if self.started.contains_key(&e.hole) || self.finished.contains_key(&e.hole) {
            return; // re-dispatch after a crash: idempotent
        }
        let (req, _, lease) = job_info(e);
        let region = e.payload.str(1).unwrap_or_default();
        let allowed = e.payload.list_at(4).map(|rs| rs.iter().any(|r| r.as_str() == Some(region))).unwrap_or(false);
        if !allowed {
            self.violations.push(format!("residency: lease {lease} in {region} used by {req}"));
        }
        if let Some(other) = self.held.get(&lease) {
            self.violations.push(format!("double allocation: lease {lease} held by {other} and {}", e.hole));
        }
        self.held.insert(lease, e.hole);
        self.started.insert(e.hole, lease);
    }

    /// Finishes a started job; a repeated call returns the recorded result.
    fn finish(&mut self, hole: HoleId, ok: bool) -> bool {
        if let Some(&r) = self.finished.get(&hole) {
            return r;
        }
        let lease = self.started.remove(&hole).expect("finish after start");
        self.held.remove(&lease);
        self.finished.insert(hole, ok);
        ok
    }
}

#[derive(Clone, Debug)]
pub struct SimReport {
    pub outcomes: BTreeMap<String, Outcome>,
    /// `(req, attempt) -> lease`, from the emitted `RunJob` effects.
    pub grants: BTreeMap<(String, i64), i64>,
    /// The pool mailbox order (what the reference model consumes).
    pub mailbox: Vec<MailMsg>,
    pub actions: u64,
    pub injects: u64,
    pub crashes: u64,
    pub duplicates_ignored: u64,
    pub stats: Stats,
    pub provider_violations: Vec<String>,
    pub final_digest: Digest,
    /// Every applied inject, in order: replaying it reproduces the run.
    pub boundary_log: Vec<Injection>,
}

struct Sim<'a> {
    cfg: &'a SimConfig,
    rng: Rng,
    prog: Program,
    k: LeaseKinds,
    m: Option<Machine>,
    sched: SchedulerKind,
    next_admit: usize,
    unacked: Vec<Injection>,
    acked: Vec<Injection>,
    committed_effects: BTreeSet<HoleId>,
    provider: Provider,
    mailbox: Vec<MailMsg>,
    grants: BTreeMap<(String, i64), i64>,
    seen_jobs: BTreeSet<HoleId>,
    outcomes: BTreeMap<String, Outcome>,
    log: Vec<Injection>,
    initial: Vec<Value>,
    crashes: u64,
    dups: u64,
    actions: u64,
}

impl Sim<'_> {
    fn m(&mut self) -> &mut Machine {
        self.m.as_mut().expect("machine")
    }

    fn apply(&mut self, inj: Injection) -> Result<InjectReceipt, String> {
        let rc = self.m().inject(inj.clone()).map_err(|e| format!("inject failed: {e}"))?;
        if rc.duplicate {
            return Err("fresh inject reported as duplicate".into());
        }
        self.unacked.push(inj.clone());
        self.log.push(inj);
        Ok(rc)
    }

    fn observe_jobs(&mut self) {
        let run_job = self.k.run_job;
        for e in self.m().outbox() {
            if e.kind == run_job && self.seen_jobs.insert(e.hole) {
                let (req, attempt, lease) = job_info(&e);
                self.grants.insert((req, attempt), lease);
            }
        }
    }

    fn commit(&mut self) -> Result<(), String> {
        self.m().commit().map_err(|e| e.to_string())?;
        self.acked.append(&mut self.unacked);
        self.committed_effects = self.m().outbox().into_iter().map(|e| e.hole).collect();
        Ok(())
    }

    fn crash(&mut self) -> Result<(), String> {
        self.crashes += 1;
        let m = self.m.take().unwrap();
        let journal: Box<dyn crate::Journal> = match &self.cfg.file_journal {
            Some(path) => {
                drop(m); // the process dies; its buffered records are gone
                Box::new(FileJournal::open(path).map_err(|e| e.to_string())?)
            }
            None => {
                let mut j = m.into_journal();
                j.crash();
                j
            }
        };
        let mut m = Machine::recover(self.prog.clone(), journal).map_err(|e| format!("recovery failed: {e}"))?;
        m.set_scheduler(self.sched);
        m.run(None);
        self.m = Some(m);
        // Clients retry everything that was never acknowledged, in order.
        for inj in std::mem::take(&mut self.unacked) {
            let rc = self.m().inject(inj.clone()).map_err(|e| format!("re-send after crash failed: {e}"))?;
            if rc.duplicate {
                return Err("an unacknowledged inject survived the crash".into());
            }
            self.unacked.push(inj);
        }
        Ok(())
    }

    fn check(&mut self) -> Result<(), String> {
        let census = self.m().census("lease");
        if census != self.initial {
            return Err(format!("lease census {census:?} != {:?}", self.initial));
        }
        if let Some(f) = self.m().faults().first() {
            return Err(format!("fault: {f:?}"));
        }
        if let Some(v) = self.provider.violations.first() {
            return Err(format!("provider: {v}"));
        }
        if self.actions.is_multiple_of(16) {
            self.m().check_invariants().map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn pending(&mut self, kind: crate::KindId) -> Vec<EffectView> {
        let committed = self.committed_effects.clone();
        self.m().outbox().into_iter().filter(|e| e.kind == kind && committed.contains(&e.hole)).collect()
    }

    /// One random host action. Returns false when only waiting remains.
    fn act(&mut self) -> Result<bool, String> {
        let (send, run_job) = (self.k.send, self.k.run_job);
        let sends = self.pending(send);
        let jobs = self.pending(run_job);
        let (unstarted, started): (Vec<_>, Vec<_>) =
            jobs.into_iter().partition(|e| !self.provider.started.contains_key(&e.hole) && !self.provider.finished.contains_key(&e.hole));
        let finished: Vec<String> = self
            .cfg
            .requests
            .iter()
            .filter(|r| !self.outcomes.contains_key(&r.req))
            .filter(|r| status(self.m.as_ref().unwrap(), &self.k, &r.req).is_some())
            .map(|r| r.req.clone())
            .collect();
        let mut choices: Vec<u8> = vec![];
        if self.next_admit < self.cfg.requests.len() {
            choices.push(0);
        }
        if !sends.is_empty() {
            choices.push(1);
        }
        if !unstarted.is_empty() {
            choices.push(2);
        }
        if !started.is_empty() {
            choices.push(3);
        }
        if !finished.is_empty() {
            choices.push(4);
        }
        let busy = !self.m.as_ref().unwrap().is_quiescent();
        if choices.is_empty() && !busy {
            // Effects created since the last commit cannot be dispatched yet:
            // commit to make them visible, or stop if nothing is pending.
            let uncommitted = self.m().outbox().iter().any(|e| !self.committed_effects.contains(&e.hole));
            if uncommitted {
                self.commit()?;
                return Ok(true);
            }
            return Ok(false);
        }
        if busy || self.rng.chance(0.2) {
            choices.push(5);
        }
        match choices[self.rng.index(choices.len())] {
            0 => {
                let r = self.cfg.requests[self.next_admit].clone();
                self.next_admit += 1;
                self.mailbox.push(MailMsg::Demand {
                    req: r.req.clone(),
                    tenant: r.tenant.clone(),
                    regions: r.regions.clone(),
                    attempt: 1,
                });
                let inj = admit_injection(&self.prog.sig, &self.k, &r).map_err(|e| e.to_string())?;
                self.apply(inj)?;
            }
            1 => {
                let s = sends[self.rng.index(sends.len())].clone();
                let msg = mail_of(self.m.as_ref().unwrap(), &self.k, &s);
                self.mailbox.push(msg);
                let inj = route_injection(&self.prog.sig, &self.k, &s).map_err(|e| e.to_string())?;
                self.apply(inj)?;
            }
            2 => {
                let j = unstarted[self.rng.index(unstarted.len())].clone();
                self.provider.start(&j);
            }
            3 => {
                let j = started[self.rng.index(started.len())].clone();
                let ok = !self.rng.chance(self.cfg.fail_rate);
                let ok = self.provider.finish(j.hole, ok);
                let inj = reply_injection(&self.prog.sig, &self.k, &j, ok).map_err(|e| e.to_string())?;
                self.apply(inj)?;
            }
            4 => {
                let req = finished[self.rng.index(finished.len())].clone();
                let outcome = status(self.m.as_ref().unwrap(), &self.k, &req).unwrap();
                self.outcomes.insert(req.clone(), outcome);
                let inj = ack_injection(&self.prog.sig, &self.k, &req).map_err(|e| e.to_string())?;
                self.apply(inj)?;
            }
            _ => {
                let budget = self.rng.below(9);
                self.m().run(Some(budget));
            }
        }
        Ok(true)
    }
}

pub fn simulate(cfg: &SimConfig) -> Result<SimReport, String> {
    let (prog, k) = program(&LeaseConfig { max_attempts: cfg.max_attempts, buggy_ignore_region: false });
    let sched = cfg.scheduler.unwrap_or(match cfg.seed % 3 {
        0 => SchedulerKind::Fifo,
        1 => SchedulerKind::Lifo,
        _ => SchedulerKind::Random(cfg.seed),
    });
    let mut m = match &cfg.file_journal {
        Some(path) => {
            let j = FileJournal::open(path).map_err(|e| e.to_string())?;
            Machine::with_journal(prog.clone(), Box::new(j)).map_err(|e| e.to_string())?
        }
        None => Machine::new(prog.clone()),
    };
    m.set_scheduler(sched);
    let mut initial: Vec<Value> = cfg.leases.iter().map(|l| Value::Int(l.id)).collect();
    initial.sort();
    let mut sim = Sim {
        cfg,
        rng: Rng::new(cfg.seed),
        prog: prog.clone(),
        k,
        m: Some(m),
        sched,
        next_admit: 0,
        unacked: vec![],
        acked: vec![],
        committed_effects: BTreeSet::new(),
        provider: Provider::default(),
        mailbox: vec![],
        grants: BTreeMap::new(),
        seen_jobs: BTreeSet::new(),
        outcomes: BTreeMap::new(),
        log: vec![],
        initial,
        crashes: 0,
        dups: 0,
        actions: 0,
    };
    let init = init_pool_injection(&prog.sig, &sim.k, &cfg.leases).map_err(|e| e.to_string())?;
    sim.apply(init)?;
    sim.commit()?;

    const MAX_ACTIONS: u64 = 50_000;
    loop {
        sim.actions += 1;
        if sim.actions > MAX_ACTIONS {
            return Err(format!("no termination after {MAX_ACTIONS} actions"));
        }
        let progressed = sim.act()?;
        sim.observe_jobs();
        if sim.rng.chance(cfg.dup_rate) && !sim.acked.is_empty() {
            let inj = sim.acked[sim.rng.index(sim.acked.len())].clone();
            let rc = sim.m().inject(inj).map_err(|e| format!("duplicate delivery failed: {e}"))?;
            if !rc.duplicate {
                return Err("an acknowledged inject was applied twice".into());
            }
            sim.dups += 1;
        }
        if sim.rng.chance(cfg.commit_rate) {
            sim.commit()?;
        }
        if sim.rng.chance(cfg.crash_rate) {
            sim.crash()?;
            sim.observe_jobs();
        }
        sim.check()?;
        if !progressed {
            break;
        }
    }
    sim.m().run(None);
    sim.commit()?;
    sim.m().check_invariants().map_err(|e| e.to_string())?;

    // Final state: everything returned, every request answered, nothing pending.
    let missing: Vec<_> = cfg.requests.iter().filter(|r| !sim.outcomes.contains_key(&r.req)).collect();
    if !missing.is_empty() {
        return Err(format!("requests without outcome: {missing:?}\n{}", sim.m().render_explanation()));
    }
    let free_kind = sim.k.free;
    let free = sim.m().graph().agents_sorted().iter().filter(|(_, a)| a.kind == free_kind).count();
    if free != cfg.leases.len() {
        return Err(format!("{free} leases free at the end, expected {}", cfg.leases.len()));
    }
    let e = sim.m().explain();
    let only_pool = e.groups.len() == 1
        && matches!(&e.groups[0].terminal, Terminal::Hole { label: Some(l), .. } if l == super::POOL);
    if !only_pool {
        return Err(format!("unexpected final state:\n{}", sim.m().render_explanation()));
    }
    if !sim.provider.held.is_empty() {
        return Err(format!("provider still holds {:?}", sim.provider.held));
    }
    let mut model = RefPool::new(&cfg.leases);
    let mut expected = BTreeMap::new();
    for msg in &sim.mailbox {
        if let Some((req, attempt, lease)) = model.deliver(msg) {
            expected.insert((req, attempt), lease);
        }
    }
    if expected != sim.grants {
        return Err(format!("grants differ from the reference model:\nnet   {:?}\nmodel {expected:?}", sim.grants));
    }
    let stats = sim.m().stats().clone();
    let final_digest = sim.m().digest();
    Ok(SimReport {
        outcomes: sim.outcomes,
        grants: sim.grants,
        mailbox: sim.mailbox,
        actions: sim.actions,
        injects: sim.log.len() as u64,
        crashes: sim.crashes,
        duplicates_ignored: sim.dups,
        stats,
        provider_violations: sim.provider.violations,
        final_digest,
        boundary_log: sim.log,
    })
}
