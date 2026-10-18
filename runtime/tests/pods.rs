//! Slice 14: pods over one log (spec appendix O).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use serde_json::{json, Value};
use suture_lang::ir::Program;
use suture_runtime::backend::Transport;
use suture_runtime::service::{self, Record, Service, Who};

const T0: u64 = 1_800_000_000;

fn tickets() -> Program {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/tickets");
    suture_lang::compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"))
}

fn state(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("pods").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn world(p: &Program, kind: &str, label: &str, attrs: Value) -> Record {
    Record::World { kind: kind.into(), label: label.into(), attrs: service::attrs_json(p, kind, Some(&attrs)).unwrap(), items: vec![] }
}

fn booking(p: &Program, label: &str, by: &str) -> Record {
    let attrs = json!({ "fan": by, "show": "Hamlet", "quantity": 1 });
    let by = Who { role: "fan".into(), name: by.into() };
    Record::New { by, verb: "books".into(), kind: "Booking".into(), label: label.into(), attrs: service::attrs_json(p, "Booking", Some(&attrs)).unwrap(), items: vec![] }
}

fn open(p: &Program, dir: &Path, pod: &str, now: u64) -> Service {
    let mut s = Service::open(p, dir, now, |_| {}).unwrap();
    s.join(pod, now).unwrap();
    s
}

fn setup(p: &Program, s: &mut Service) {
    for r in [
        world(p, "Fan", "Ada", json!({ "name": "Ada" })),
        world(p, "Fan", "Bo", json!({ "name": "Bo" })),
        world(p, "Show", "Hamlet", json!({ "name": "Hamlet", "price": "40 EUR" })),
        world(p, "Session", "Fri", json!({ "show": "Hamlet", "day": "Friday", "seats": 100 })),
    ] {
        assert_eq!(s.input(r, T0).unwrap().refused, None);
    }
}

/// Keeps the keys of the requests posted, and answers that it will answer later.
#[derive(Clone, Default)]
struct Fake(Rc<RefCell<Vec<String>>>);

impl Transport for Fake {
    fn post(&mut self, _: &str, _: Option<&str>, body: &Value, _: Duration) -> Result<Value, String> {
        self.0.borrow_mut().push(body["key"].as_str().unwrap_or_default().to_string());
        Ok(json!({}))
    }
}

fn connect(s: &mut Service) -> Fake {
    let fake = Fake::default();
    let adapters = service::adapters(r#"{"payments": {"url": "http://pay.local/charge"}}"#).unwrap();
    s.connect(adapters, Box::new(fake.clone()));
    fake
}

#[test]
fn two_pods_take_inputs_in_turn_and_come_to_the_same_state() {
    let p = tickets();
    let dir = state("turns");
    let mut a = open(&p, &dir, "a", T0);
    setup(&p, &mut a);
    let mut b = open(&p, &dir, "b", T0);
    assert!(b.host.unit("Fri").is_some(), "b replays what a took");
    assert_eq!(a.input(booking(&p, "B1", "Ada"), T0 + 1).unwrap().refused, None);
    b.refresh().unwrap();
    assert!(b.host.unit("B1").is_some(), "a read on b sees a's input");
    assert_eq!(b.input(booking(&p, "B2", "Bo"), T0 + 2).unwrap().refused, None);
    assert_eq!(a.input(booking(&p, "B3", "Ada"), T0 + 3).unwrap().refused, None);
    b.refresh().unwrap();
    let c = Service::open(&p, &dir, T0 + 3, |_| {}).unwrap();
    for l in ["B1", "B2", "B3", "Fri"] {
        assert_eq!(a.host.unit(l), c.host.unit(l), "{l}");
        assert_eq!(b.host.unit(l), c.host.unit(l), "{l}");
    }
    assert_eq!(a.host.now, 3);
    assert_eq!(b.host.now, 3);
}

#[test]
fn each_request_is_posted_by_its_owner_and_moves_when_a_pod_stops() {
    let p = tickets();
    let dir = state("owners");
    let mut a = open(&p, &dir, "a", T0);
    let mut b = open(&p, &dir, "b", T0);
    let (fa, fb) = (connect(&mut a), connect(&mut b));
    setup(&p, &mut a);
    let labels: Vec<String> = (1..=8).map(|i| format!("B{i}")).collect();
    for l in &labels {
        assert_eq!(a.input(booking(&p, l, "Ada"), T0 + 1).unwrap().refused, None);
    }
    b.tick(T0 + 1).unwrap();
    let pods = a.pods(T0 + 1);
    assert_eq!(pods, ["a", "b"]);
    let (sa, sb): (BTreeSet<String>, BTreeSet<String>) = (fa.0.borrow().iter().cloned().collect(), fb.0.borrow().iter().cloned().collect());
    assert!(!sa.is_empty() && !sb.is_empty(), "{sa:?} {sb:?}");
    assert!(sa.is_disjoint(&sb), "{sa:?} {sb:?}");
    let all: BTreeSet<String> = labels.iter().map(|l| format!("{l}/Payment#1")).collect();
    assert_eq!(sa.union(&sb).cloned().collect::<BTreeSet<_>>(), all);
    for k in &sa {
        assert_eq!(service::owner(k.split('/').next().unwrap(), &pods).map(String::as_str), Some("a"));
    }
    // b stops beating: after BEAT_SECS, its units are a's, and a posts them.
    let later = T0 + service::BEAT_SECS + 1;
    a.tick(later).unwrap();
    assert_eq!(a.pods(later), ["a"]);
    let again: BTreeSet<String> = fa.0.borrow().iter().cloned().collect();
    assert_eq!(again, all, "a posts b's waiting requests");
    // b comes back and takes its units again, then stops: a posts them once more.
    let back = later + 1;
    b.tick(back).unwrap();
    a.tick(back).unwrap();
    fa.0.borrow_mut().clear();
    let gone = back + service::BEAT_SECS + 1;
    a.tick(gone).unwrap();
    let theirs: BTreeSet<String> = all.iter().filter(|k| service::owner(k.split('/').next().unwrap(), &pods).map(String::as_str) == Some("b")).cloned().collect();
    assert_eq!(fa.0.borrow().iter().cloned().collect::<BTreeSet<_>>(), theirs, "a forgot what it posted for units it gave up");
}

#[test]
fn an_input_is_read_against_what_other_pods_made() {
    let p = tickets();
    let dir = state("http");
    let mut a = open(&p, &dir, "a", T0);
    let mut b = open(&p, &dir, "b", T0);
    setup(&p, &mut a);
    let sat = world(&p, "Session", "Sat", json!({ "show": "Hamlet", "day": "Saturday", "seats": 100 }));
    assert_eq!(a.input(sat, T0 + 1).unwrap().refused, None);
    let req = suture_runtime::http::Request {
        method: "POST".into(),
        path: "/inputs".into(),
        query: String::new(),
        headers: vec![("suture-caller".into(), "box Kim".into())],
        body: br#"{"change": {"target": "Sat", "attr": "seats", "value": 90}}"#.to_vec(),
    };
    let (status, body) = suture_runtime::http::handle(&mut b, &req, None, T0 + 2);
    assert_eq!(status, 200, "{body}");
    assert_eq!(b.host.unit("Sat").unwrap().root.attrs.get("seats"), Some(&suture_runtime::state::Val::Count(90)));
}

#[test]
fn a_timer_fires_once_whichever_pods_tick() {
    let p = tickets();
    let dir = state("timers");
    let mut a = open(&p, &dir, "a", T0);
    let mut b = open(&p, &dir, "b", T0);
    setup(&p, &mut a);
    assert_eq!(a.input(booking(&p, "B1", "Ada"), T0 + 1).unwrap().refused, None);
    let fired_a = a.tick(T0 + 40).unwrap();
    let fired_b = b.tick(T0 + 40).unwrap();
    assert!(fired_a.iter().any(|l| l.contains("no answer within")), "{fired_a:?}");
    assert!(!fired_b.iter().any(|l| l.contains("no answer within")), "{fired_b:?}");
    assert_eq!(a.host.unit("B1"), b.host.unit("B1"));
    let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap();
    assert_eq!(log.matches(r#"{"pass":39}"#).count(), 1, "{log}");
}

#[test]
fn a_line_a_crashed_pod_left_short_is_cut_off_by_the_next() {
    let p = tickets();
    let dir = state("crash");
    let mut a = open(&p, &dir, "a", T0);
    setup(&p, &mut a);
    let mut b = open(&p, &dir, "b", T0);
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new().append(true).open(dir.join("log.jsonl")).unwrap();
        f.write_all(br#"{"new":{"zz"#).unwrap();
    }
    b.refresh().unwrap();
    assert_eq!(b.input(booking(&p, "B1", "Ada"), T0 + 1).unwrap().refused, None);
    let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap();
    assert!(!log.contains("zz"), "{log}");
    a.refresh().unwrap();
    assert!(a.host.unit("B1").is_some());
    let c = Service::open(&p, &dir, T0 + 1, |_| {}).unwrap();
    assert_eq!(c.host.unit("B1"), a.host.unit("B1"));
}

#[test]
fn a_secret_given_to_one_pod_is_known_to_the_other() {
    let src = "module vault version 1, currency EUR
kind Order   { card: Text secret, total: Money }
kind Payment { order: Order, amount: Money = order's total }
Order needs Payment
effect charge Payment via payments within 30 seconds
  sends    its amount, the order's card
  reports  charged
allow payments to receive Order's card
";
    let p = suture_lang::compile(&[("v.st".to_string(), src.to_string())]).unwrap();
    let dir = state("secrets");
    let mut a = open(&p, &dir, "a", T0);
    let mut b = open(&p, &dir, "b", T0);
    let attrs = service::attrs_json(&p, "Order", Some(&json!({ "card": "4111 1111", "total": "80 EUR" }))).unwrap();
    a.input(Record::World { kind: "Order".into(), label: "O1".into(), attrs, items: vec![] }, T0).unwrap();
    b.refresh().unwrap();
    let ask = b.host.pending().into_iter().find(|x| x.adapter == "payments").unwrap().clone();
    assert_eq!(service::request_json(&b.host, &ask, true)["sends"]["card"], "4111 1111");
    // b seals the next secret after a's, not over it.
    let attrs = service::attrs_json(&p, "Order", Some(&json!({ "card": "5500 0000", "total": "10 EUR" }))).unwrap();
    b.input(Record::World { kind: "Order".into(), label: "O2".into(), attrs, items: vec![] }, T0 + 1).unwrap();
    a.refresh().unwrap();
    assert_eq!(a.host.secrets.len(), 2);
    assert_eq!(a.host.secrets, b.host.secrets);
}
