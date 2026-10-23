//! Slice 56: snapshots, so a restart replays only the log after one (spec
//! appendix BG).

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use suture_lang::ir::Program;
use suture_runtime::service::{self, Record, Service, Who};
use suture_runtime::state::Work;

const T0: u64 = 1_800_000_000;

fn tickets() -> Program {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/tickets");
    suture_lang::compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"))
}

fn state(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("snapshot").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn world(p: &Program, kind: &str, label: &str, attrs: Value) -> Record {
    Record::World { kind: kind.into(), label: label.into(), attrs: service::attrs_json(p, kind, Some(&attrs)).unwrap(), items: vec![] }
}

fn booking(p: &Program, label: &str, quantity: i64) -> Record {
    let attrs = json!({ "fan": "Ada", "show": "Hamlet", "quantity": quantity });
    let by = Who { role: "fan".into(), name: "Ada".into() };
    Record::New { by, verb: "books".into(), kind: "Booking".into(), label: label.into(), attrs: service::attrs_json(p, "Booking", Some(&attrs)).unwrap(), items: vec![] }
}

fn charged(label: &str) -> Record {
    Record::Answer { adapter: "payments".into(), outcome: "charged".into(), key: None, target: Some(label.into()), data: vec![] }
}

/// Books `n` from `from` on, a second apart, and charges every other one.
fn bookings(p: &Program, s: &mut Service, from: usize, n: usize) {
    for i in from..from + n {
        let t = T0 + 10 + i as u64;
        s.input(booking(p, &format!("B{i}"), 1), t).unwrap();
        if i % 2 == 0 {
            s.input(charged(&format!("B{i}")), t).unwrap();
        }
    }
}

fn setup(p: &Program, s: &mut Service) {
    for r in [
        world(p, "Fan", "Ada", json!({ "name": "Ada" })),
        world(p, "Show", "Hamlet", json!({ "name": "Hamlet", "price": "40 EUR" })),
        world(p, "Session", "Fri", json!({ "show": "Hamlet", "day": "Friday", "seats": 1000 })),
    ] {
        assert_eq!(s.input(r, T0).unwrap().refused, None);
    }
}

/// The host's state as JSON, but for its trace: a restart's own lines differ.
fn image(s: &Service) -> Value {
    let mut v = serde_json::to_value(s.host.image()).unwrap();
    v.as_object_mut().unwrap().remove("trace");
    v
}

fn copy(from: &Path, to: &Path) {
    let _ = std::fs::remove_dir_all(to);
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_file() {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

#[test]
fn a_restart_from_a_snapshot_comes_to_the_state_a_full_replay_does() {
    let p = tickets();
    let dir = state("same");
    {
        let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
        setup(&p, &mut s);
        bookings(&p, &mut s, 0, 20);
        s.snapshot().unwrap();
        bookings(&p, &mut s, 20, 20);
        // The clock passes deadlines, which fire after the snapshot.
        s.tick(T0 + 3600).unwrap();
    }
    let full = state("same-full");
    copy(&dir, &full);
    std::fs::remove_file(full.join("snapshot.json")).unwrap();
    let a = Service::open(&p, &dir, T0 + 4000, |_| {}).unwrap();
    let b = Service::open(&p, &full, T0 + 4000, |_| {}).unwrap();
    assert_eq!(image(&a), image(&b));
    assert!(!a.host.trace.iter().any(|l| l.starts_with("snapshot:")), "{:?}", a.host.trace);
    // And they go on alike.
    let (mut a, mut b) = (a, b);
    for s in [&mut a, &mut b] {
        bookings(&p, s, 40, 3);
        s.tick(T0 + 9000).unwrap();
    }
    assert_eq!(image(&a), image(&b));
}

#[test]
fn a_restart_replays_only_the_log_after_the_snapshot() {
    let p = tickets();
    let dir = state("cost");
    {
        let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
        setup(&p, &mut s);
        bookings(&p, &mut s, 0, 200);
        s.snapshot().unwrap();
        bookings(&p, &mut s, 200, 2);
    }
    let full = state("cost-full");
    copy(&dir, &full);
    std::fs::remove_file(full.join("snapshot.json")).unwrap();
    let work = |d: &Path| {
        let before = Work::now();
        Service::open(&p, d, T0 + 1000, |_| {}).unwrap();
        let after = Work::now();
        (after.0 - before.0) + (after.1 - before.1)
    };
    let (from_snapshot, from_start) = (work(&dir), work(&full));
    assert!(from_snapshot * 20 < from_start, "a restart from the snapshot did {from_snapshot} units of work, a full replay {from_start}");
}

#[test]
fn a_snapshot_of_another_log_is_set_aside() {
    let p = tickets();
    let dir = state("other");
    {
        let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
        setup(&p, &mut s);
        bookings(&p, &mut s, 0, 4);
        s.snapshot().unwrap();
    }
    // The log is cut back, and written on otherwise.
    let log = dir.join("log.jsonl");
    let text = std::fs::read_to_string(&log).unwrap();
    let kept: Vec<&str> = text.lines().take(4).collect();
    std::fs::write(&log, kept.join("\n") + "\n").unwrap();
    {
        let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
        bookings(&p, &mut s, 10, 6);
    }
    let s = Service::open(&p, &dir, T0 + 100, |_| {}).unwrap();
    assert!(s.host.trace.iter().any(|l| l == "snapshot: not used, since it was taken of another log; the whole log replays"), "{:?}", s.host.trace);
    assert!(s.host.unit("B10").is_some() && s.host.unit("B3").is_none());
    // A snapshot that is not one is set aside too.
    std::fs::write(dir.join("snapshot.json"), "{").unwrap();
    let s = Service::open(&p, &dir, T0 + 100, |_| {}).unwrap();
    assert!(s.host.trace.iter().any(|l| l.starts_with("snapshot: not used, since it is not one")), "{:?}", s.host.trace);
}

#[test]
fn a_snapshot_keeps_no_secret_and_is_this_users_alone() {
    let vault = "module vault version 1, currency EUR
role customer is a Customer
kind Customer { name: Text }
kind Order    { customer: Customer, card: Text secret, total: Money }
kind Payment  { order: Order, amount: Money = order's total }
Order needs Payment
effect charge Payment via payments within 30 seconds
  sends    its amount, the order's card
  reports  charged
allow payments to receive Order's card
admit place a new Order by customer where the Order's customer is the caller
";
    let p = suture_lang::compile(&[("vault.st".to_string(), vault.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let dir = state("secret");
    let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
    s.input(world(&p, "Customer", "Ada", json!({ "name": "Ada" })), T0).unwrap();
    let attrs = service::attrs_json(&p, "Order", Some(&json!({ "customer": "Ada", "card": "4111 1111", "total": "80 EUR" }))).unwrap();
    let by = Who { role: "customer".into(), name: "Ada".into() };
    assert_eq!(s.input(Record::New { by, verb: "places".into(), kind: "Order".into(), label: "O1".into(), attrs, items: vec![] }, T0).unwrap().refused, None);
    s.snapshot().unwrap();
    let path = dir.join("snapshot.json");
    assert!(!std::fs::read_to_string(&path).unwrap().contains("4111"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    drop(s);
    let s = Service::open(&p, &dir, T0, |_| {}).unwrap();
    let pay = s.host.pending()[0].clone();
    assert_eq!(service::request_json(&s.host, &pay, true)["sends"]["card"], "4111 1111", "the secret comes back from the secret store");
}
