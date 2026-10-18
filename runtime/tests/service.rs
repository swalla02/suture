//! Slice 11: a program as a service (spec appendix L.2): the log, restart,
//! the clock, adapters and values in JSON.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use serde_json::{json, Value};
use suture_lang::ir::{Program, Ty};
use suture_runtime::backend::Transport;
use suture_runtime::choice::Answer;
use suture_runtime::service::{self, Adapter, Record, Service, Who};
use suture_runtime::state::Val;

const T0: u64 = 1_800_000_000;

fn tickets() -> Program {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/tickets");
    suture_lang::compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"))
}

/// A fresh state directory for one test.
fn state(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("service").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn world(p: &Program, kind: &str, label: &str, attrs: Value) -> Record {
    Record::World { kind: kind.into(), label: label.into(), attrs: service::attrs_json(p, kind, Some(&attrs)).unwrap(), items: vec![] }
}

fn fan(name: &str) -> Who {
    Who { role: "fan".into(), name: name.into() }
}

fn booking(p: &Program, label: &str, by: &str, quantity: i64) -> Record {
    let attrs = json!({ "fan": by, "show": "Hamlet", "quantity": quantity });
    Record::New { by: fan(by), verb: "books".into(), kind: "Booking".into(), label: label.into(), attrs: service::attrs_json(p, "Booking", Some(&attrs)).unwrap(), items: vec![] }
}

fn answer(adapter: &str, outcome: &str, target: &str) -> Record {
    Record::Answer { adapter: adapter.into(), outcome: outcome.into(), key: None, target: Some(target.into()), data: vec![] }
}

fn setup(p: &Program, s: &mut Service) {
    for r in [
        world(p, "Fan", "Ada", json!({ "name": "Ada" })),
        world(p, "Fan", "Bo", json!({ "name": "Bo" })),
        world(p, "Show", "Hamlet", json!({ "name": "Hamlet", "price": "40 EUR" })),
        world(p, "Session", "Fri", json!({ "show": "Hamlet", "day": "Friday", "seats": 100 })),
    ] {
        let a = s.input(r, T0).unwrap();
        assert_eq!(a.refused, None, "{:?}", a.trace);
    }
}

#[test]
fn inputs_apply_and_are_refused_with_the_trace() {
    let p = tickets();
    let mut s = Service::in_memory(&p, T0);
    setup(&p, &mut s);
    let a = s.input(booking(&p, "B1", "Ada", 2), T0 + 5).unwrap();
    assert_eq!(a.refused, None);
    assert!(a.trace.iter().any(|l| l.contains("B1  Payment → charge via payments (amount 80 EUR)")), "{:?}", a.trace);
    let a = s.input(booking(&p, "B2", "Bo", 1).with_by(fan("Ada")), T0 + 6).unwrap();
    assert!(a.refused.as_deref().is_some_and(|r| r.starts_with("refused at the gate")), "{a:?}");
    let a = s.input(answer("payments", "charged", "B1"), T0 + 7).unwrap();
    assert!(a.trace.iter().any(|l| l.contains("B1  Ticket → issue via mailer")), "{:?}", a.trace);
    assert_eq!(s.host.now, 7, "the clock follows the inputs' times");
}

trait WithBy {
    fn with_by(self, by: Who) -> Record;
}

impl WithBy for Record {
    fn with_by(self, who: Who) -> Record {
        match self {
            Record::New { verb, kind, label, attrs, items, .. } => Record::New { by: who, verb, kind, label, attrs, items },
            r => r,
        }
    }
}

#[test]
fn a_restart_replays_the_log_into_the_same_state() {
    let p = tickets();
    let dir = state("restart");
    let (trace, units, pending) = {
        let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
        setup(&p, &mut s);
        s.input(booking(&p, "B1", "Ada", 2), T0 + 5).unwrap();
        s.input(booking(&p, "B2", "Bo", 1).with_by(fan("Ada")), T0 + 6).unwrap();
        s.input(booking(&p, "B3", "Bo", 3), T0 + 8).unwrap();
        s.input(answer("payments", "charged", "B1"), T0 + 9).unwrap();
        let units: Vec<_> = ["B1", "B3", "Fri"].iter().map(|l| s.host.unit(l)).collect();
        let pending: Vec<String> = s.host.pending().iter().map(|a| a.key.clone()).collect();
        (s.host.trace.clone(), units, pending)
    };
    let s = Service::open(&p, &dir, T0 + 100, |_| {}).unwrap();
    assert_eq!(s.started, T0);
    assert_eq!(s.host.trace, trace);
    assert_eq!(["B1", "B3", "Fri"].iter().map(|l| s.host.unit(l)).collect::<Vec<_>>(), units);
    assert_eq!(s.host.pending().iter().map(|a| a.key.clone()).collect::<Vec<_>>(), pending);
    assert_eq!(s.host.now, 9, "the clock stands where the log left it, until the next tick");
}

#[test]
fn a_log_written_under_another_program_is_refused() {
    let p = tickets();
    let dir = state("changed");
    let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
    setup(&p, &mut s);
    drop(s);
    let mut q = p.clone();
    q.rules.pop();
    let e = Service::open(&q, &dir, T0, |_| {}).err().unwrap();
    assert!(e.ends_with("the log was written under tickets v1, and this program differs; a deploy with live units waits for versioning, so start with a fresh state directory"), "{e}");
}

#[test]
fn a_last_line_cut_short_is_dropped() {
    let p = tickets();
    let dir = state("cut");
    let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
    setup(&p, &mut s);
    drop(s);
    let log = dir.join("log.jsonl");
    let mut text = std::fs::read_to_string(&log).unwrap();
    text.push_str("{\"new\": {\"by\": ");
    std::fs::write(&log, text).unwrap();
    let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
    assert!(s.host.unit("Fri").is_some());
    assert!(std::fs::read_to_string(&log).unwrap().ends_with("}\n"), "the cut line is cut off the file");
    // The next record starts on a line of its own, so a second restart reads it.
    s.input(booking(&p, "B1", "Ada", 2), T0 + 1).unwrap();
    drop(s);
    let s = Service::open(&p, &dir, T0, |_| {}).unwrap();
    assert!(s.host.unit("B1").is_some());
}

#[test]
fn a_model_asked_while_the_clock_catches_up_is_logged_too() {
    let p = suture_lang::compile_dir(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/returns")).unwrap_or_else(|e| panic!("{e:?}"));
    let dir = state("catch-up");
    let asked = Rc::new(RefCell::new(vec![]));
    let model = |asked: Rc<RefCell<Vec<String>>>, picks: Vec<&'static str>| {
        move |h: &mut suture_runtime::host::Host| {
            let mut picks = picks.into_iter();
            h.models.register("routing@1", Box::new(move |c| {
                asked.borrow_mut().push(c.key.clone());
                picks.next().map(String::from)
            }))
        }
    };
    let mut s = Service::open(&p, &dir, T0, model(asked.clone(), vec!["Courier", "Locker"])).unwrap();
    s.input(world(&p, "Applicant", "Ada", json!({ "name": "Ada" })), T0).unwrap();
    s.input(world(&p, "Application", "A1", json!({ "applicant": "Ada" })), T0).unwrap();
    // Three days on, the courier's patience runs out as the clock catches up
    // before this input, and the model picks the slot's next alternative.
    s.input(world(&p, "Applicant", "Bo", json!({ "name": "Bo" })), T0 + 3 * 86_400).unwrap();
    assert_eq!(asked.borrow().len(), 2);
    let unit = s.host.unit("A1");
    drop(s);
    let s = Service::open(&p, &dir, T0, model(asked.clone(), vec!["Collection", "Collection"])).unwrap();
    assert_eq!(asked.borrow().len(), 2, "replay does not ask the model again");
    assert_eq!(s.host.unit("A1"), unit);
}

#[test]
fn a_clock_that_steps_back_waits() {
    let p = tickets();
    let mut s = Service::in_memory(&p, T0);
    setup(&p, &mut s);
    s.input(booking(&p, "B1", "Ada", 2), T0 + 40).unwrap();
    assert_eq!(s.tick(T0 + 10).unwrap(), Vec::<String>::new());
    assert_eq!(s.host.now, 40);
}

const SHOP: &str = "module shop version 1, currency EUR
kind Carrier  { name: Text, zone: Text, fee: Money }
kind Order    { zone: Text }
kind Shipment { order: Order }
Order needs Shipment
Shipment uses a Carrier where the Carrier's zone is the Order's zone
prefer Carrier: lowest fee
decide Carrier with model \"carriers@1\"
";

#[test]
fn model_answers_come_from_the_log_on_replay() {
    let p = suture_lang::compile(&[("shop.st".to_string(), SHOP.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let dir = state("models");
    let asked = Rc::new(RefCell::new(0));
    let model = |asked: Rc<RefCell<i32>>| {
        move |h: &mut suture_runtime::host::Host| {
            h.models.register("carriers@1", Box::new(move |_| {
                *asked.borrow_mut() += 1;
                Some("Fast".into())
            }))
        }
    };
    let mut s = Service::open(&p, &dir, T0, model(asked.clone())).unwrap();
    for (l, fee) in [("Cheap", "5 EUR"), ("Fast", "9 EUR")] {
        s.input(world(&p, "Carrier", l, json!({ "name": l, "zone": "EU", "fee": fee })), T0).unwrap();
    }
    s.input(world(&p, "Order", "O1", json!({ "zone": "EU" })), T0).unwrap();
    assert_eq!(*asked.borrow(), 1);
    let trace = s.host.trace.clone();
    drop(s);
    let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap();
    assert!(log.contains("{\"model\":{\"key\":\"O1/Shipment#1@Carrier\",\"answer\":{\"Picked\":\"Fast\"}}}"), "{log}");
    let s = Service::open(&p, &dir, T0, model(asked.clone())).unwrap();
    assert_eq!(*asked.borrow(), 1, "replay does not ask the model again");
    assert_eq!(s.host.trace, trace);
    assert_eq!(s.host.model_answers, [("O1/Shipment#1@Carrier".to_string(), Answer::Picked("Fast".into()))]);
}

#[test]
fn the_clock_fires_deadlines_and_downtime_fires_what_it_covered() {
    let p = tickets();
    let dir = state("clock");
    let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
    setup(&p, &mut s);
    s.input(booking(&p, "B1", "Ada", 2), T0).unwrap();
    assert_eq!(s.tick(T0 + 29).unwrap(), Vec::<String>::new(), "nothing is due yet");
    let fired = s.tick(T0 + 31).unwrap();
    assert!(fired.iter().any(|l| l.contains("payments: no answer within 30 seconds")), "{fired:?}");
    drop(s);
    // Down for an hour: the last two sends' deadlines ran out meanwhile.
    let mut s = Service::open(&p, &dir, T0 + 3600, |_| {}).unwrap();
    assert_eq!(s.host.now, 31);
    let fired = s.tick(T0 + 3600).unwrap();
    assert!(fired.iter().any(|l| l.contains("B1  Payment timed out after 3 sends")), "{fired:?}");
    assert_eq!(s.host.now, 3600);
}

/// Answers each post with `reply`, and keeps the posts.
#[derive(Clone)]
struct Fake {
    reply: Rc<RefCell<Result<Value, String>>>,
    sent: Rc<RefCell<Vec<(String, Value)>>>,
}

impl Transport for Fake {
    fn post(&mut self, url: &str, _: Option<&str>, body: &Value, _: Duration) -> Result<Value, String> {
        self.sent.borrow_mut().push((url.into(), body.clone()));
        self.reply.borrow().clone()
    }
}

fn adapters() -> BTreeMap<String, Adapter> {
    service::adapters(r#"{"payments": {"url": "http://pay.local/charge"}, "mailer": {"url": "http://mail.local/send"}}"#).unwrap()
}

#[test]
fn requests_are_posted_and_an_immediate_reply_is_the_answer() {
    let p = tickets();
    let fake = Fake { reply: Rc::new(RefCell::new(Ok(json!({ "outcome": "charged" })))), sent: Rc::default() };
    let mut s = Service::in_memory(&p, T0);
    s.connect(adapters(), Box::new(fake.clone()));
    setup(&p, &mut s);
    *fake.reply.borrow_mut() = Ok(json!({ "outcome": "charged" }));
    let a = s.input(booking(&p, "B1", "Ada", 2), T0).unwrap();
    let sent = fake.sent.borrow().clone();
    assert_eq!(sent[0], ("http://pay.local/charge".into(), json!({ "key": "B1/Payment#1", "adapter": "payments", "verb": "charge", "kind": "Payment", "unit": "B1", "sends": { "amount": "80 EUR" } })));
    // The mailer's reply is read the same way; `charged` is not one of its outcomes.
    assert!(a.trace.iter().any(|l| l.contains("B1  Payment charged")), "{:?}", a.trace);
    assert_eq!(sent[1].0, "http://mail.local/send");
}

#[test]
fn a_later_answer_arrives_by_key_and_a_restart_posts_again() {
    let p = tickets();
    let dir = state("adapters");
    let fake = Fake { reply: Rc::new(RefCell::new(Ok(json!({ "accepted": true })))), sent: Rc::default() };
    let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
    s.connect(adapters(), Box::new(fake.clone()));
    setup(&p, &mut s);
    let a = s.input(booking(&p, "B1", "Ada", 2), T0).unwrap();
    assert!(a.trace.iter().any(|l| l.contains("payments: posted; it answers later   (B1/Payment#1)")), "{:?}", a.trace);
    drop(s);
    let mut s = Service::open(&p, &dir, T0 + 2, |_| {}).unwrap();
    s.connect(adapters(), Box::new(fake.clone()));
    s.post_new(T0 + 2).unwrap();
    let keys: Vec<Value> = fake.sent.borrow().iter().map(|(_, b)| b["key"].clone()).collect();
    assert_eq!(keys, [json!("B1/Payment#1"), json!("B1/Payment#1")], "posted again, with the same key");
    let r = Record::Answer { adapter: "payments".into(), outcome: "charged".into(), key: Some("B1/Payment#1".into()), target: None, data: vec![] };
    let a = s.input(r, T0 + 3).unwrap();
    assert_eq!(a.refused, None);
    assert!(a.trace.iter().any(|l| l.contains("B1  Payment charged")), "{:?}", a.trace);
}

#[test]
fn a_failed_post_is_tried_again() {
    let p = tickets();
    let fake = Fake { reply: Rc::new(RefCell::new(Err("connection refused".into()))), sent: Rc::default() };
    let mut s = Service::in_memory(&p, T0);
    s.connect(adapters(), Box::new(fake.clone()));
    setup(&p, &mut s);
    let a = s.input(booking(&p, "B1", "Ada", 2), T0).unwrap();
    assert!(a.trace.iter().any(|l| l.contains("payments: could not be posted: connection refused; trying again in 5 seconds   (B1/Payment#1)")), "{:?}", a.trace);
    *fake.reply.borrow_mut() = Ok(json!({ "outcome": "charged" }));
    s.tick(T0 + 4).unwrap();
    assert_eq!(fake.sent.borrow().len(), 1, "not before 5 seconds");
    let t = s.tick(T0 + 5).unwrap();
    assert!(t.iter().any(|l| l.contains("B1  Payment charged")), "{t:?}");
}

#[test]
fn values_follow_the_attribute_type() {
    let v = |x: Value, t: Option<Ty>| service::from_json(&x, t, "EUR");
    assert_eq!(v(json!("12.50 EUR"), Some(Ty::Money)), Ok(Val::Money(1250)));
    assert_eq!(v(json!("80 EUR"), None), Ok(Val::Money(8000)));
    assert_eq!(v(json!("80 USD"), Some(Ty::Money)), Err("\"80 USD\" is not in this module's currency, EUR".into()));
    assert_eq!(v(json!(80), Some(Ty::Money)), Err("an amount is text such as \"80 EUR\", not 80".into()));
    assert_eq!(v(json!("1.234 EUR"), Some(Ty::Money)), Err("\"1.234 EUR\" is not an amount such as \"80 EUR\"".into()));
    assert_eq!(v(json!(3), Some(Ty::Count)), Ok(Val::Count(3)));
    assert_eq!(v(json!(3600), Some(Ty::Duration)), Ok(Val::Duration(3600)));
    assert_eq!(v(json!("x"), Some(Ty::Count)), Err("\"x\" is not a whole number".into()));
    assert_eq!(v(json!(["B", "A", "B"]), Some(Ty::Set)), Ok(Val::Set(vec!["A".into(), "B".into()])));
    assert_eq!(v(json!("hello"), None), Ok(Val::Text("hello".into())));
    let e = service::adapters(r#"{"payments": {"url": "u", "token": "x"}}"#).err().unwrap();
    assert_eq!(e, "adapters.json: payments: unknown field `token`; use \"url\" and \"key_env\"");
}
