//! Experiment A: scarce linear resources.
//!
//! Workspace requests compete for a pool of database leases, each with a
//! region. Matching happens *inside the net*, by interaction only — there is
//! no `BIND` and no host-side planner:
//!
//! * The pool consumes a mailbox stream. Each message spawns a `Dispatch`.
//! * A `Demand` starts a `Seek` along the free list: the first eligible lease
//!   is granted, otherwise the demand is appended to the wait queue (`Enq`).
//! * A `Returned` lease starts a `Match` along the queue: the first eligible
//!   waiting demand gets it, otherwise it is pushed onto the free list.
//! * Successive scans pipeline: a later `Seek` waits on the output wire of the
//!   earlier one, so acquisition is ordered and race-free by construction.
//!
//! A lease's lifecycle is a sequence of conserved kinds keyed by lease id —
//! `FreeLease → Granted → RunJob → Returned → (Match →) FreeLease` — so every
//! rewrite is checked to neither duplicate, drop nor forge a lease. Returning
//! a lease to the pool is a many-to-one merge, which deterministic nets cannot
//! express; it goes through the boundary as a `Send` effect that the host
//! routes into the pool mailbox (`Route >< Send`).

use std::collections::VecDeque;

mod sim;
pub use sim::{simulate, Provider, SimConfig, SimReport};

use crate::machine::{EffectView, InjectError, InjectReceipt, Machine, Program};
use crate::net::{Injection, NetBuilder};
use crate::policy::{PatchView, Policy};
use crate::value::{Value, ValueError, ValueType};
use crate::{tup, Endpoint, KindId, NetError, RuleTable, SigBuilder, Signature};

/// The pool's mailbox label.
pub const POOL: &str = "pool";

pub struct LeaseKinds {
    pub pool: KindId,
    pub mcons: KindId,
    pub dispatch: KindId,
    pub demand: KindId,
    pub returned: KindId,
    pub free: KindId,
    pub fnil: KindId,
    pub qcons: KindId,
    pub qnil: KindId,
    pub seek: KindId,
    pub enq: KindId,
    pub matcher: KindId,
    pub granted: KindId,
    pub await_lease: KindId,
    pub run_job: KindId,
    pub job_done: KindId,
    pub job_failed: KindId,
    pub send: KindId,
    pub route: KindId,
    pub done: KindId,
    pub failed: KindId,
    pub status_ack: KindId,
}

#[derive(Clone, Debug)]
pub struct LeaseConfig {
    /// Attempts per request (a failed job re-demands until this many).
    pub max_attempts: i64,
    /// Fault injection: grant leases regardless of region (the residency
    /// policy must stop it).
    pub buggy_ignore_region: bool,
}

impl Default for LeaseConfig {
    fn default() -> Self {
        LeaseConfig { max_attempts: 2, buggy_ignore_region: false }
    }
}

#[derive(Clone, Debug)]
pub struct LeaseSpec {
    pub id: i64,
    pub region: String,
}

impl LeaseSpec {
    pub fn new(id: i64, region: &str) -> Self {
        LeaseSpec { id, region: region.to_string() }
    }
}

#[derive(Clone, Debug)]
pub struct Request {
    pub req: String,
    pub tenant: String,
    pub regions: Vec<String>,
}

impl Request {
    pub fn new(req: &str, tenant: &str, regions: &[&str]) -> Self {
        Request { req: req.into(), tenant: tenant.into(), regions: regions.iter().map(|s| s.to_string()).collect() }
    }
}

/// A message appended to the pool mailbox.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MailMsg {
    Demand { req: String, tenant: String, regions: Vec<String>, attempt: i64 },
    Returned { lease: i64, region: String },
}

/// Final state of a request, as read from its status hole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Done { lease: i64 },
    Failed { attempts: i64 },
}

fn demand_type() -> ValueType {
    ValueType::Tuple(vec![ValueType::Str, ValueType::Str, ValueType::List(Box::new(ValueType::Str)), ValueType::Int])
}
fn lease_type() -> ValueType {
    ValueType::Tuple(vec![ValueType::Int, ValueType::Str])
}

/// Whether demand payload `d` (req, tenant, regions, attempt) accepts `region`.
fn eligible(d: &Value, region: &str) -> Result<bool, ValueError> {
    Ok(d.list_at(2)?.iter().any(|r| r.as_str() == Some(region)))
}

pub fn program(cfg: &LeaseConfig) -> (Program, LeaseKinds) {
    let mut b = SigBuilder::new("lease-pool", 1);
    let mailbox = b.polarized("Mailbox");
    let msg = b.polarized("Msg");
    let flist = b.polarized("FreeList");
    let queue = b.polarized("Queue");
    let grant = b.polarized("Grant");
    let status = b.polarized("Status");
    let jobr = b.polarized("JobReply");
    let routet = b.polarized("RouteT");
    let d = demand_type;
    let l = lease_type;
    let g = ValueType::Tuple(vec![ValueType::Int, ValueType::Str, ValueType::Str, ValueType::Str]);
    let j = ValueType::Tuple(vec![
        ValueType::Int,
        ValueType::Str,
        ValueType::Str,
        ValueType::Str,
        ValueType::List(Box::new(ValueType::Str)),
        ValueType::Int,
    ]);
    let k = LeaseKinds {
        pool: b.kind("Pool", mailbox.neg()).aux("free", flist.neg()).aux("queue", queue.neg()).done(),
        mcons: b.kind("MCons", mailbox.pos()).aux("msg", msg.neg()).aux("rest", mailbox.neg()).done(),
        dispatch: b
            .kind("Dispatch", msg.neg())
            .aux("fi", flist.neg())
            .aux("qi", queue.neg())
            .aux("fo", flist.pos())
            .aux("qo", queue.pos())
            .done(),
        demand: b.kind("Demand", msg.pos()).aux("reply", grant.pos()).payload(d()).done(),
        returned: b.kind("Returned", msg.pos()).payload(l()).conserved("lease", 0).done(),
        free: b.kind("FreeLease", flist.pos()).aux("next", flist.neg()).payload(l()).conserved("lease", 0).done(),
        fnil: b.kind("FNil", flist.pos()).done(),
        qcons: b.kind("QCons", queue.pos()).aux("reply", grant.pos()).aux("next", queue.neg()).payload(d()).done(),
        qnil: b.kind("QNil", queue.pos()).done(),
        seek: b
            .kind("Seek", flist.neg())
            .aux("reply", grant.pos())
            .aux("out", flist.pos())
            .aux("qi", queue.neg())
            .aux("qo", queue.pos())
            .payload(d())
            .done(),
        enq: b.kind("Enq", queue.neg()).aux("reply", grant.pos()).aux("out", queue.pos()).payload(d()).done(),
        matcher: b
            .kind("Match", queue.neg())
            .aux("out", queue.pos())
            .aux("fi", flist.neg())
            .aux("fo", flist.pos())
            .payload(l())
            .conserved("lease", 0)
            .done(),
        granted: b.kind("Granted", grant.pos()).payload(g).conserved("lease", 0).done(),
        await_lease: b.kind("AwaitLease", grant.neg()).aux("status", status.pos()).payload(d()).done(),
        run_job: b.kind("RunJob", jobr.neg()).aux("status", status.pos()).payload(j).effect().conserved("lease", 0).done(),
        job_done: b.kind("JobDone", jobr.pos()).done(),
        job_failed: b.kind("JobFailed", jobr.pos()).done(),
        send: b.kind("Send", routet.neg()).aux("msg", msg.neg()).payload(ValueType::Tuple(vec![ValueType::Str])).effect().done(),
        route: b.kind("Route", routet.pos()).aux("dest", mailbox.pos()).aux("next", mailbox.neg()).done(),
        done: b.kind("Done", status.pos()).payload(ValueType::Tuple(vec![ValueType::Str, ValueType::Int])).done(),
        failed: b.kind("Failed", status.pos()).payload(ValueType::Tuple(vec![ValueType::Str, ValueType::Int])).done(),
        status_ack: b.kind("StatusAck", status.neg()).done(),
    };
    let sig = b.build().expect("lease signature");
    let rules = rules(&k, cfg);
    let prog = Program::new(sig, rules)
        .expect("lease rules")
        .with_policy(ResidencyPolicy { granted: k.granted, seek: k.seek, qcons: k.qcons })
        .with_policy(TenantPolicy { granted: k.granted, await_lease: k.await_lease });
    (prog, k)
}

fn rules(k: &LeaseKinds, cfg: &LeaseConfig) -> RuleTable {
    let mut t = RuleTable::new();
    let (pool, dispatch, seek, enq, matcher) = (k.pool, k.dispatch, k.seek, k.enq, k.matcher);
    let (free, fnil, qcons, qnil, granted) = (k.free, k.fnil, k.qcons, k.qnil, k.granted);
    let (demand, returned, await_lease, run_job, send, mcons) = (k.demand, k.returned, k.await_lease, k.run_job, k.send, k.mcons);
    let (done, failed) = (k.done, k.failed);
    let buggy = cfg.buggy_ignore_region;
    let max_attempts = cfg.max_attempts;
    let fits = move |d: &Value, region: &str| -> Result<bool, ValueError> { Ok(buggy || eligible(d, region)?) };
    let grant_payload = |l: &Value, d: &Value| -> Result<Value, ValueError> {
        Ok(tup![l.int(0)?, l.str(1)?, d.str(0)?, d.str(1)?])
    };

    // The pool takes the next mailbox message and keeps listening.
    t.def("pool_msg", mcons, pool, move |cx| {
        let [msg, rest] = cx.left_aux();
        let [fr, q] = cx.right_aux();
        let (fo, qo) = (cx.var(), cx.var());
        cx.agent(dispatch, (), msg, &[fr, q, fo, qo]);
        cx.agent(pool, (), rest, &[fo, qo]);
        Ok(())
    });
    t.def("dispatch_demand", dispatch, demand, move |cx| {
        let [fi, qi, fo, qo] = cx.left_aux();
        let [reply] = cx.right_aux();
        let d = cx.right().clone();
        cx.agent(seek, d, fi, &[reply, fo, qi, qo]);
        Ok(())
    });
    t.def("dispatch_returned", dispatch, returned, move |cx| {
        let [fi, qi, fo, qo] = cx.left_aux();
        let l = cx.right().clone();
        cx.agent(matcher, l, qi, &[qo, fi, fo]);
        Ok(())
    });
    // Scan the free list for the first lease the demand may use.
    t.def("seek_free", seek, free, move |cx| {
        let [reply, out, qi, qo] = cx.left_aux();
        let [next] = cx.right_aux();
        let (d, l) = (cx.left().clone(), cx.right().clone());
        if fits(&d, l.str(1)?)? {
            cx.agent(granted, grant_payload(&l, &d)?, reply, &[]);
            cx.link(out, next);
            cx.link(qo, qi);
        } else {
            let o2 = cx.var();
            cx.agent(free, l, out, &[o2]);
            cx.agent(seek, d, next, &[reply, o2, qi, qo]);
        }
        Ok(())
    });
    // No eligible lease: keep the free list, queue the demand.
    t.def("seek_nil", seek, fnil, move |cx| {
        let [reply, out, qi, qo] = cx.left_aux();
        let d = cx.left().clone();
        cx.agent(fnil, (), out, &[]);
        cx.agent(enq, d, qi, &[reply, qo]);
        Ok(())
    });
    t.def("enq_cons", enq, qcons, move |cx| {
        let [reply, out] = cx.left_aux();
        let [r2, next] = cx.right_aux();
        let (d, d2) = (cx.left().clone(), cx.right().clone());
        let o2 = cx.var();
        cx.agent(qcons, d2, out, &[r2, o2]);
        cx.agent(enq, d, next, &[reply, o2]);
        Ok(())
    });
    t.def("enq_nil", enq, qnil, move |cx| {
        let [reply, out] = cx.left_aux();
        let d = cx.left().clone();
        let n = cx.var();
        cx.agent(qcons, d, out, &[reply, n]);
        cx.agent(qnil, (), n, &[]);
        Ok(())
    });
    // Scan the wait queue for the first demand the returned lease fits.
    t.def("match_cons", matcher, qcons, move |cx| {
        let [out, fi, fo] = cx.left_aux();
        let [reply, next] = cx.right_aux();
        let (l, d) = (cx.left().clone(), cx.right().clone());
        if fits(&d, l.str(1)?)? {
            cx.agent(granted, grant_payload(&l, &d)?, reply, &[]);
            cx.link(out, next);
            cx.link(fo, fi);
        } else {
            let o2 = cx.var();
            cx.agent(qcons, d, out, &[reply, o2]);
            cx.agent(matcher, l, next, &[o2, fi, fo]);
        }
        Ok(())
    });
    // Nobody waiting for it: the lease goes back to the head of the free list.
    t.def("match_nil", matcher, qnil, move |cx| {
        let [out, fi, fo] = cx.left_aux();
        let l = cx.left().clone();
        cx.agent(qnil, (), out, &[]);
        cx.agent(free, l, fo, &[fi]);
        Ok(())
    });
    // The workspace receives its lease and starts the job (an effect).
    t.def("granted_await", granted, await_lease, move |cx| {
        let [st] = cx.right_aux();
        let (g, d) = (cx.left().clone(), cx.right().clone());
        let job = tup![g.int(0)?, g.str(1)?, d.str(0)?, d.str(1)?, d.get(2)?.clone(), d.int(3)?];
        cx.effect(run_job, job, &[st]);
        Ok(())
    })
    .emits(&[run_job]);
    t.def("job_done", k.job_done, run_job, move |cx| {
        let [st] = cx.right_aux();
        let j = cx.right().clone();
        let m = cx.var();
        cx.effect(send, tup![POOL], &[m]);
        cx.agent(returned, tup![j.int(0)?, j.str(1)?], m, &[]);
        cx.agent(done, tup![j.str(2)?, j.int(0)?], st, &[]);
        Ok(())
    })
    .emits(&[send]);
    t.def("job_failed", k.job_failed, run_job, move |cx| {
        let [st] = cx.right_aux();
        let j = cx.right().clone();
        let m = cx.var();
        cx.effect(send, tup![POOL], &[m]);
        cx.agent(returned, tup![j.int(0)?, j.str(1)?], m, &[]);
        let attempt = j.int(5)?;
        if attempt < max_attempts {
            let d2 = tup![j.str(2)?, j.str(3)?, j.get(4)?.clone(), attempt + 1];
            let (m2, reply) = (cx.var(), cx.var());
            cx.effect(send, tup![POOL], &[m2]);
            cx.agent(demand, d2.clone(), m2, &[reply]);
            cx.agent(await_lease, d2, reply, &[st]);
        } else {
            cx.agent(failed, tup![j.str(2)?, attempt], st, &[]);
        }
        Ok(())
    })
    .emits(&[send]);
    // Host routing: the injected `Route` splices the message into a mailbox.
    t.def("route_send", k.route, send, move |cx| {
        let [dest, next] = cx.left_aux();
        let [msg] = cx.right_aux();
        cx.agent(mcons, (), dest, &[msg, next]);
        Ok(())
    });
    t.def("done_ack", done, k.status_ack, |_| Ok(()));
    t.def("failed_ack", failed, k.status_ack, |_| Ok(()));
    t
}

/// Hard residency policy: every grant must be allowed by the demand it serves.
struct ResidencyPolicy {
    granted: KindId,
    seek: KindId,
    qcons: KindId,
}

impl Policy for ResidencyPolicy {
    fn name(&self) -> &str {
        "residency"
    }
    fn check(&self, v: &PatchView<'_>) -> Result<(), String> {
        for g in v.created_of(self.granted) {
            let region = g.str(1).map_err(|e| e.to_string())?;
            let d = v
                .consumed_of(self.seek)
                .chain(v.consumed_of(self.qcons))
                .next()
                .ok_or_else(|| "a lease was granted without a demand".to_string())?;
            if !eligible(d, region).map_err(|e| e.to_string())? {
                return Err(format!("lease in {region} granted to {} which allows {}", d.get(0).unwrap(), d.get(2).unwrap()));
            }
        }
        Ok(())
    }
}

/// Hard tenancy policy: a lease reaches only the request it was granted to.
struct TenantPolicy {
    granted: KindId,
    await_lease: KindId,
}

impl Policy for TenantPolicy {
    fn name(&self) -> &str {
        "tenancy"
    }
    fn check(&self, v: &PatchView<'_>) -> Result<(), String> {
        let (Some(g), Some(d)) = (v.consumed_of(self.granted).next(), v.consumed_of(self.await_lease).next()) else {
            return Ok(());
        };
        let pairs = [(g.str(2), d.str(0)), (g.str(3), d.str(1))];
        if pairs.iter().any(|(a, b)| a.as_ref().ok() != b.as_ref().ok()) {
            return Err(format!("grant {g} delivered to request {d}"));
        }
        Ok(())
    }
}

// ------------------------------------------------------------- host side
//
// Each host operation has a pure builder (`*_injection`) returning the
// `Injection` it performs — so a host can log it, retry it, or replay it —
// and a convenience wrapper that injects it.

/// Creates the pool with the given leases (in free-list order). Mints.
pub fn init_pool_injection(sig: &Signature, k: &LeaseKinds, leases: &[LeaseSpec]) -> Result<Injection, NetError> {
    let mut b = NetBuilder::new(sig);
    let (mb, f0, q0) = (b.var(), b.var(), b.var());
    b.agent(k.pool, (), mb, &[f0, q0]);
    let mut cur = f0;
    for l in leases {
        let next = b.var();
        b.agent(k.free, tup![l.id, l.region.as_str()], cur, &[next]);
        cur = next;
    }
    b.agent(k.fnil, (), cur, &[]);
    b.agent(k.qnil, (), q0, &[]);
    Ok(Injection::new(b.finish()?).new_hole(mb, Some(POOL)).mint().key("init-pool"))
}

pub fn init_pool(m: &mut Machine, k: &LeaseKinds, leases: &[LeaseSpec]) -> Result<InjectReceipt, InjectError> {
    let inj = init_pool_injection(&m.program().sig, k, leases)?;
    m.inject(inj)
}

fn demand_payload(r: &Request, attempt: i64) -> Value {
    tup![r.req.as_str(), r.tenant.as_str(), Value::list(r.regions.iter().map(Value::from)), attempt]
}

/// Admission: appends the request's demand to the pool mailbox and exposes
/// its status at hole `op:<req>`.
pub fn admit_injection(sig: &Signature, k: &LeaseKinds, r: &Request) -> Result<Injection, NetError> {
    let mut b = NetBuilder::new(sig);
    let (tail, msg, rest, reply, st) = (b.var(), b.var(), b.var(), b.var(), b.var());
    b.agent(k.mcons, (), tail, &[msg, rest]);
    b.agent(k.demand, demand_payload(r, 1), msg, &[reply]);
    b.agent(k.await_lease, demand_payload(r, 1), reply, &[st]);
    Ok(Injection::new(b.finish()?)
        .at_label(tail, POOL)
        .new_hole(rest, Some(POOL))
        .new_hole(st, Some(&format!("op:{}", r.req)))
        .key(format!("admit:{}", r.req)))
}

pub fn admit(m: &mut Machine, k: &LeaseKinds, r: &Request) -> Result<InjectReceipt, InjectError> {
    let inj = admit_injection(&m.program().sig, k, r)?;
    m.inject(inj)
}

/// Transport: delivers a pending `Send` effect into the mailbox it names.
pub fn route_injection(sig: &Signature, k: &LeaseKinds, send: &EffectView) -> Result<Injection, NetError> {
    let dest = send.payload.str(0).map_err(|e| NetError::Value(e.to_string()))?.to_string();
    let mut b = NetBuilder::new(sig);
    let (p, d, n) = (b.var(), b.var(), b.var());
    b.agent(k.route, (), p, &[d, n]);
    Ok(Injection::new(b.finish()?)
        .at_hole(p, send.hole)
        .at_label(d, &dest)
        .new_hole(n, Some(&dest))
        .key(format!("route:{}", send.hole)))
}

pub fn route(m: &mut Machine, k: &LeaseKinds, send: &EffectView) -> Result<InjectReceipt, InjectError> {
    let inj = route_injection(&m.program().sig, k, send)?;
    m.inject(inj)
}

/// Provider reply to a `RunJob` effect.
pub fn reply_injection(sig: &Signature, k: &LeaseKinds, job: &EffectView, ok: bool) -> Result<Injection, NetError> {
    let mut b = NetBuilder::new(sig);
    let p = b.var();
    b.agent(if ok { k.job_done } else { k.job_failed }, (), p, &[]);
    Ok(Injection::new(b.finish()?).at_hole(p, job.hole).key(format!("reply:{}", job.hole)))
}

pub fn reply_job(m: &mut Machine, k: &LeaseKinds, job: &EffectView, ok: bool) -> Result<InjectReceipt, InjectError> {
    let inj = reply_injection(&m.program().sig, k, job, ok)?;
    m.inject(inj)
}

/// Acknowledges (and removes) a finished request's status.
pub fn ack_injection(sig: &Signature, k: &LeaseKinds, req: &str) -> Result<Injection, NetError> {
    let mut b = NetBuilder::new(sig);
    let p = b.var();
    b.agent(k.status_ack, (), p, &[]);
    Ok(Injection::new(b.finish()?).at_label(p, &format!("op:{req}")).key(format!("ack:{req}")))
}

pub fn ack_status(m: &mut Machine, k: &LeaseKinds, req: &str) -> Result<InjectReceipt, InjectError> {
    let inj = ack_injection(&m.program().sig, k, req)?;
    m.inject(inj)
}

/// The request's outcome, if its status hole holds a final result.
pub fn status(m: &Machine, k: &LeaseKinds, req: &str) -> Option<Outcome> {
    let (_, port, a) = m.hole_peer(m.hole(&format!("op:{req}"))?)?;
    if port != 0 {
        return None;
    }
    if a.kind == k.done {
        Some(Outcome::Done { lease: a.payload.int(1).ok()? })
    } else if a.kind == k.failed {
        Some(Outcome::Failed { attempts: a.payload.int(1).ok()? })
    } else {
        None
    }
}

/// `(req, attempt, lease)` of a `RunJob` effect.
pub fn job_info(e: &EffectView) -> (String, i64, i64) {
    let p = &e.payload;
    (p.str(2).unwrap_or_default().to_string(), p.int(5).unwrap_or_default(), p.int(0).unwrap_or_default())
}

/// The message carried by a pending `Send` effect.
pub fn mail_of(m: &Machine, k: &LeaseKinds, send: &EffectView) -> MailMsg {
    let a = m.graph().agent(send.agent).expect("send agent");
    let Endpoint::Port(msg, 0) = a.ports[1] else { panic!("send carries no message") };
    let mv = m.graph().agent(msg).expect("message agent");
    let p = &mv.payload;
    if mv.kind == k.returned {
        MailMsg::Returned { lease: p.int(0).unwrap(), region: p.str(1).unwrap().to_string() }
    } else {
        MailMsg::Demand {
            req: p.str(0).unwrap().to_string(),
            tenant: p.str(1).unwrap().to_string(),
            regions: p.list_at(2).unwrap().iter().map(|v| v.as_str().unwrap().to_string()).collect(),
            attempt: p.int(3).unwrap(),
        }
    }
}

/// The reference model: the same pool as a plain sequential program.
#[derive(Clone, Debug)]
pub struct RefPool {
    free: Vec<(i64, String)>,
    queue: VecDeque<(String, Vec<String>, i64)>,
}

impl RefPool {
    pub fn new(leases: &[LeaseSpec]) -> Self {
        RefPool { free: leases.iter().map(|l| (l.id, l.region.clone())).collect(), queue: VecDeque::new() }
    }

    /// Processes one mailbox message; returns `(req, attempt, lease)` if it
    /// produced a grant.
    pub fn deliver(&mut self, msg: &MailMsg) -> Option<(String, i64, i64)> {
        match msg {
            MailMsg::Demand { req, regions, attempt, .. } => {
                match self.free.iter().position(|(_, r)| regions.contains(r)) {
                    Some(i) => Some((req.clone(), *attempt, self.free.remove(i).0)),
                    None => {
                        self.queue.push_back((req.clone(), regions.clone(), *attempt));
                        None
                    }
                }
            }
            MailMsg::Returned { lease, region } => match self.queue.iter().position(|(_, rs, _)| rs.contains(region)) {
                Some(i) => {
                    let (req, _, attempt) = self.queue.remove(i).unwrap();
                    Some((req, attempt, *lease))
                }
                None => {
                    self.free.insert(0, (*lease, region.clone()));
                    None
                }
            },
        }
    }
}
