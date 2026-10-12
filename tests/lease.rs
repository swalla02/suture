use std::collections::{BTreeMap, BTreeSet};

use suture::nets::lease::{self, LeaseConfig, LeaseKinds, LeaseSpec, MailMsg, Outcome, RefPool, Request};
use suture::*;

/// Drives a lease machine by hand, recording the pool's mailbox order and
/// every grant (as observed from emitted `RunJob` effects).
struct Drive {
    m: Machine,
    k: LeaseKinds,
    mailbox: Vec<MailMsg>,
    grants: BTreeMap<(String, i64), i64>,
    seen: BTreeSet<AgentId>,
}

impl Drive {
    fn new(cfg: LeaseConfig, leases: &[LeaseSpec]) -> Drive {
        let (prog, k) = lease::program(&cfg);
        let mut m = Machine::new(prog);
        lease::init_pool(&mut m, &k, leases).unwrap();
        Drive { m, k, mailbox: vec![], grants: BTreeMap::new(), seen: BTreeSet::new() }
    }

    fn settle(&mut self) {
        self.m.run(None);
        self.m.check_invariants().unwrap();
        for e in self.m.outbox() {
            if e.kind == self.k.run_job && self.seen.insert(e.agent) {
                let (req, attempt, lease) = lease::job_info(&e);
                assert!(self.grants.insert((req, attempt), lease).is_none());
            }
        }
    }

    fn admit(&mut self, req: &str, tenant: &str, regions: &[&str]) {
        let r = Request::new(req, tenant, regions);
        lease::admit(&mut self.m, &self.k, &r).unwrap();
        self.mailbox.push(MailMsg::Demand {
            req: req.into(),
            tenant: tenant.into(),
            regions: regions.iter().map(|s| s.to_string()).collect(),
            attempt: 1,
        });
        self.settle();
    }

    fn job(&self, req: &str) -> EffectView {
        self.m
            .outbox()
            .into_iter()
            .find(|e| e.kind == self.k.run_job && lease::job_info(e).0 == req)
            .unwrap_or_else(|| panic!("no job for {req}"))
    }

    /// Completes `req`'s job, then routes the resulting sends in order.
    fn finish(&mut self, req: &str, ok: bool) {
        let j = self.job(req);
        lease::reply_job(&mut self.m, &self.k, &j, ok).unwrap();
        self.settle();
        self.route_all();
    }

    fn route_all(&mut self) {
        loop {
            let sends: Vec<_> = self.m.outbox().into_iter().filter(|e| e.kind == self.k.send).collect();
            let Some(s) = sends.into_iter().min_by_key(|s| match lease::mail_of(&self.m, &self.k, s) {
                MailMsg::Returned { .. } => 0, // deterministic order: returns first
                MailMsg::Demand { .. } => 1,
            }) else {
                break;
            };
            self.mailbox.push(lease::mail_of(&self.m, &self.k, &s));
            lease::route(&mut self.m, &self.k, &s).unwrap();
            self.settle();
        }
    }

    fn running(&self) -> Vec<(String, i64)> {
        let mut v: Vec<_> = self
            .m
            .outbox()
            .iter()
            .filter(|e| e.kind == self.k.run_job)
            .map(|e| {
                let (req, _, lease) = lease::job_info(e);
                (req, lease)
            })
            .collect();
        v.sort();
        v
    }
}

fn eu() -> &'static [&'static str] {
    &["eu-west-1"]
}
fn us() -> &'static [&'static str] {
    &["us-east-1"]
}

#[test]
fn coverage_is_complete() {
    let (p, _) = lease::program(&LeaseConfig::default());
    let c = p.rules.coverage(&p.sig);
    assert!(c.missing.is_empty(), "{:?}", c.missing);
    assert!(c.never_fires.is_empty(), "{:?}", c.never_fires);
}

#[test]
fn single_request_lifecycle() {
    let mut d = Drive::new(LeaseConfig::default(), &[LeaseSpec::new(1, "eu-west-1")]);
    d.admit("r1", "acme", eu());
    assert_eq!(d.running(), vec![("r1".into(), 1)]);
    assert_eq!(lease::status(&d.m, &d.k, "r1"), None);
    d.finish("r1", true);
    assert_eq!(lease::status(&d.m, &d.k, "r1"), Some(Outcome::Done { lease: 1 }));
    assert_eq!(d.m.census("lease"), vec![Value::Int(1)]);
    lease::ack_status(&mut d.m, &d.k, "r1").unwrap();
    d.settle();
    let e = d.m.explain();
    assert_eq!(e.groups.len(), 1, "{}", d.m.render_explanation());
    assert!(matches!(&e.groups[0].terminal, Terminal::Hole { label: Some(l), .. } if l == "pool"));
    assert!(d.m.faults().is_empty());
}

#[test]
fn fifo_queue_and_return_handoff() {
    let mut d = Drive::new(LeaseConfig::default(), &[LeaseSpec::new(1, "eu-west-1")]);
    for r in ["r1", "r2", "r3"] {
        d.admit(r, "acme", eu());
    }
    assert_eq!(d.running(), vec![("r1".into(), 1)]);
    d.finish("r1", true);
    assert_eq!(d.running(), vec![("r2".into(), 1)], "the head of the queue gets the returned lease");
    d.finish("r2", true);
    assert_eq!(d.running(), vec![("r3".into(), 1)]);
}

#[test]
fn eligibility_skips_ineligible_leases_and_demands() {
    let mut d = Drive::new(LeaseConfig::default(), &[LeaseSpec::new(1, "us-east-1"), LeaseSpec::new(2, "eu-west-1")]);
    d.admit("r1", "acme", eu());
    assert_eq!(d.running(), vec![("r1".into(), 2)], "skips the US lease at the head");
    d.admit("r2", "globex", us());
    d.admit("r3", "acme", eu());
    assert_eq!(d.running(), vec![("r1".into(), 2), ("r2".into(), 1)]);
    d.finish("r2", true);
    assert_eq!(d.running(), vec![("r1".into(), 2)], "a returned US lease does not serve the queued EU demand");
    d.finish("r1", true);
    assert_eq!(d.running(), vec![("r3".into(), 2)]);
    assert_eq!(d.m.census("lease"), vec![Value::Int(1), Value::Int(2)]);
}

#[test]
fn retry_after_failure_requeues_at_back() {
    let mut d = Drive::new(LeaseConfig { max_attempts: 2, ..LeaseConfig::default() }, &[LeaseSpec::new(1, "eu-west-1")]);
    d.admit("r1", "acme", eu());
    d.admit("r2", "acme", eu());
    d.finish("r1", false); // returns the lease (to r2) and re-demands at the back
    assert_eq!(d.running(), vec![("r2".into(), 1)]);
    d.finish("r2", true);
    assert_eq!(d.running(), vec![("r1".into(), 1)]);
    assert_eq!(d.grants.get(&("r1".to_string(), 2)), Some(&1));
    d.finish("r1", false);
    assert_eq!(lease::status(&d.m, &d.k, "r1"), Some(Outcome::Failed { attempts: 2 }));
    assert_eq!(lease::status(&d.m, &d.k, "r2"), Some(Outcome::Done { lease: 1 }));
    assert!(d.running().is_empty());
    assert_eq!(d.m.census("lease"), vec![Value::Int(1)]);
}

#[test]
fn buggy_rule_is_stopped_by_residency_policy() {
    let cfg = LeaseConfig { buggy_ignore_region: true, ..LeaseConfig::default() };
    let mut d = Drive::new(cfg, &[LeaseSpec::new(1, "us-east-1")]);
    d.admit("r1", "acme", eu());
    let faults = d.m.faults();
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].rule.as_deref(), Some("seek_free"));
    assert!(faults[0].error.contains("residency"), "{}", faults[0].error);
    assert!(d.running().is_empty());
    assert_eq!(d.m.census("lease"), vec![Value::Int(1)]);
    assert_eq!(d.m.explain().stuck().len(), 1);
}

#[test]
fn reference_model_agrees_on_scripted_sequence() {
    let leases = [LeaseSpec::new(1, "eu-west-1"), LeaseSpec::new(2, "us-east-1"), LeaseSpec::new(3, "eu-west-1")];
    let mut d = Drive::new(LeaseConfig { max_attempts: 3, ..LeaseConfig::default() }, &leases);
    d.admit("r1", "acme", eu());
    d.admit("r2", "globex", us());
    d.admit("r3", "initech", &["eu-west-1", "us-east-1"]);
    d.admit("r4", "acme", eu());
    d.admit("r5", "globex", us());
    d.finish("r2", false);
    d.finish("r1", true);
    d.admit("r6", "umbrella", &["us-east-1", "eu-west-1"]);
    d.finish("r3", true);
    while let Some(next) = d.running().first().cloned() {
        let ok = next.0 != "r5";
        d.finish(&next.0, ok || d.grants.contains_key(&(next.0.clone(), 2)));
    }
    let mut model = RefPool::new(&leases);
    let mut expected = BTreeMap::new();
    for msg in &d.mailbox {
        if let Some((req, attempt, lease)) = model.deliver(msg) {
            expected.insert((req, attempt), lease);
        }
    }
    assert_eq!(d.grants, expected);
    assert_eq!(d.grants.len(), 8, "six requests plus two retries: {:?}", d.grants);
    assert_eq!(d.m.census("lease"), vec![Value::Int(1), Value::Int(2), Value::Int(3)]);
    assert!(d.m.faults().is_empty());
}
