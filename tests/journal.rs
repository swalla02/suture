mod common;

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};

use common::*;
use suture::*;

fn tmp(name: &str) -> PathBuf {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("journal-{}-{name}.jsonl", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p
}

#[test]
fn replay_equals_original() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 2, 3, "a")).unwrap();
    m.inject(start_go(&m0, "out")).unwrap();
    m.run(None);
    m.commit().unwrap();
    let (d, ob, injects, reductions) = (m.digest(), m.outbox(), m.stats().injects, m.stats().reductions);
    let r = Machine::recover(m0.prog.clone(), m.into_journal()).unwrap();
    assert_eq!((r.digest(), r.outbox(), r.stats().injects, r.stats().reductions), (d, ob, injects, reductions));
    r.check_invariants().unwrap();
}

#[test]
fn replay_partial_run() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 2, 3, "a")).unwrap();
    assert_eq!(m.run(Some(2)), RunOutcome::Budget { reductions: 2 }); // 2 + 3 takes three reductions
    m.commit().unwrap();
    let mid = m.digest();
    let mut r = Machine::recover(m0.prog.clone(), m.into_journal()).unwrap();
    assert_eq!(r.digest(), mid);
    assert_eq!(r.ready_count(), 1);
    r.check_invariants().unwrap();
    r.run(None);
    let mut control = Machine::new(m0.prog.clone());
    control.inject(sum(&m0, 2, 3, "a")).unwrap();
    control.run(None);
    assert_eq!(r.digest(), control.digest());
}

#[test]
fn crash_drops_uncommitted_and_rerun_converges() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 1, 1, "a").key("k1")).unwrap();
    m.run(None);
    m.commit().unwrap();
    let committed = m.digest();
    m.inject(sum(&m0, 2, 2, "b").key("k2")).unwrap();
    m.run(None);
    let mut j = m.into_journal();
    j.crash();
    let mut r = Machine::recover(m0.prog.clone(), j).unwrap();
    assert_eq!(r.digest(), committed);
    assert!(!r.inject(sum(&m0, 2, 2, "b").key("k2")).unwrap().duplicate, "k2 was never committed");
    r.run(None);
    let mut c = Machine::new(m0.prog.clone());
    c.inject(sum(&m0, 1, 1, "a").key("k1")).unwrap();
    c.run(None);
    c.inject(sum(&m0, 2, 2, "b").key("k2")).unwrap();
    c.run(None);
    assert_eq!(r.digest(), c.digest());
}

#[test]
fn file_journal_torn_tail() {
    let m0 = mini();
    let path = tmp("torn");
    let d1 = {
        let mut m = Machine::with_journal(m0.prog.clone(), Box::new(FileJournal::open(&path).unwrap())).unwrap();
        m.inject(sum(&m0, 2, 1, "a")).unwrap();
        m.run(None);
        m.commit().unwrap();
        m.digest()
    };
    // A complete but uncommitted record, then a torn partial line.
    let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
    let stray = Record::Fault { left: AgentId(1), right: AgentId(2), rule: None, error: "stray".into() };
    writeln!(f, "{}", serde_json::to_string(&stray).unwrap()).unwrap();
    f.write_all(br#"{"t":"Inject","seq":9,"#).unwrap();
    drop(f);
    let mut r = Machine::recover(m0.prog.clone(), Box::new(FileJournal::open(&path).unwrap())).unwrap();
    assert_eq!(r.digest(), d1);
    assert!(r.faults().is_empty(), "uncommitted records are not durable");
    r.inject(sum(&m0, 1, 1, "b")).unwrap();
    r.run(None);
    r.commit().unwrap();
    let d2 = r.digest();
    drop(r);
    let r2 = Machine::recover(m0.prog.clone(), Box::new(FileJournal::open(&path).unwrap())).unwrap();
    assert_eq!(r2.digest(), d2);
    assert_eq!(read_nat(&r2, &m0, r2.hole("b").unwrap()), Some(2));
}

#[test]
fn with_journal_requires_an_empty_journal() {
    let m0 = mini();
    let path = tmp("nonempty");
    let mut m = Machine::with_journal(m0.prog.clone(), Box::new(FileJournal::open(&path).unwrap())).unwrap();
    m.inject(sum(&m0, 1, 1, "a")).unwrap();
    m.commit().unwrap();
    drop(m);
    assert!(Machine::with_journal(m0.prog.clone(), Box::new(FileJournal::open(&path).unwrap())).is_err());
}

fn committed_journal(m0: &Mini) -> Box<dyn Journal> {
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(m0, 3, 2, "a")).unwrap();
    m.run(None);
    m.commit().unwrap();
    m.into_journal()
}

#[test]
fn replay_detects_rule_drift() {
    let j = committed_journal(&mini());
    let drifted = mini_with(1, 1); // same name and version, different add_s output
    let err = Machine::recover(drifted.prog.clone(), j).err().expect("must refuse");
    assert!(matches!(err, RecoveryError::Divergence { ref rule, .. } if rule == "add_s"), "{err}");
}

#[test]
fn replay_refuses_version_change() {
    let j = committed_journal(&mini());
    let bumped = mini_with(0, 2);
    let err = Machine::recover(bumped.prog.clone(), j).err().expect("must refuse");
    assert!(matches!(err, RecoveryError::RuleVersion { journal: 1, program: 2, .. }), "{err}");
}

#[test]
fn header_mismatch_is_refused() {
    let j = committed_journal(&mini());
    let (other, _) = suture::nets::arith::program();
    assert!(matches!(Machine::recover(other, j), Err(RecoveryError::Header(_))));
}

static ORACLE: AtomicI64 = AtomicI64::new(100);

/// `Start >< Go` produces `Result(n)` where `n` is read from a global: a
/// stand-in for a learned, non-reproducible law.
fn oracle_program() -> (Program, KindId, KindId) {
    let mut b = SigBuilder::new("oracle", 1);
    let ping = b.polarized("Ping");
    let out = b.polarized("Out");
    let start = b.kind("Start", ping.pos()).done();
    let go = b.kind("Go", ping.neg()).aux("out", out.pos()).done();
    let result = b.kind("Result", out.pos()).payload(ValueType::Int).done();
    let mut t = RuleTable::new();
    t.def("oracle", start, go, move |cx| {
        let [o] = cx.right_aux();
        cx.agent(result, ORACLE.load(Ordering::SeqCst), o, &[]);
        Ok(())
    })
    .recorded();
    (Program::new(b.build().unwrap(), t).unwrap(), start, go)
}

#[test]
fn recorded_rule_replays_without_rerunning() {
    let (prog, start, go) = oracle_program();
    let mut m = Machine::new(prog.clone());
    let mut b = NetBuilder::new(&prog.sig);
    let (p, o) = (b.var(), b.var());
    b.agent(start, (), p, &[]);
    b.agent(go, (), p, &[o]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(o, Some("out"))).unwrap();
    m.run(None);
    m.commit().unwrap();
    let d = m.digest();
    ORACLE.store(999, Ordering::SeqCst);
    let r = Machine::recover(prog, m.into_journal()).unwrap();
    assert_eq!(r.digest(), d);
    let (_, _, ag) = r.hole_peer(r.hole("out").unwrap()).unwrap();
    assert_eq!(ag.payload, Value::Int(100));
}

#[test]
fn idempotency_survives_recovery() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 1, 1, "a").key("once")).unwrap();
    m.commit().unwrap();
    let mut r = Machine::recover(m0.prog.clone(), m.into_journal()).unwrap();
    assert!(r.inject(sum(&m0, 1, 1, "a").key("once")).unwrap().duplicate);
}

#[test]
fn empty_journal_recovers_to_a_fresh_machine() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 1, 1, "a")).unwrap(); // never committed
    let mut j = m.into_journal();
    j.crash();
    let mut r = Machine::recover(m0.prog.clone(), j).unwrap();
    assert_eq!(r.graph().agent_count(), 0);
    r.inject(sum(&m0, 1, 1, "a")).unwrap();
    r.commit().unwrap();
    let r2 = Machine::recover(m0.prog.clone(), r.into_journal()).unwrap();
    assert_eq!(r2.stats().injects, 1);
}
