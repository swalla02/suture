//! Slice 12: versions and deploys (spec appendix M).

use std::path::PathBuf;

use serde_json::json;
use suture_lang::ir::Program;
use suture_runtime::host::Host;
use suture_runtime::service::{self, Check, Record, Service, Who};
use suture_runtime::state::Val;

const T0: u64 = 1_800_000_000;

const TICKETS: &str = include_str!("../../examples/tickets/tickets.st");

fn compile(text: &str) -> Program {
    suture_lang::compile(&[("tickets.st".to_string(), text.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

/// The tickets program as version `v`, with `edits` made to its text.
fn tickets(v: u32, edits: &[(&str, &str)]) -> Program {
    let mut text = TICKETS.replace("module tickets version 1", &format!("module tickets version {v}"));
    for (from, to) in edits {
        assert!(text.contains(from), "{from}");
        text = text.replace(from, to);
    }
    compile(&text)
}

fn given(pairs: &[(&str, Val)]) -> Vec<(String, Val)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

/// v1 with Ada's booking B1 of two seats, its payment waiting.
fn live() -> Host {
    let mut h = Host::new(tickets(1, &[]));
    h.create("Fan", "Ada", &given(&[("name", Val::Text("Ada".into()))])).unwrap();
    h.create("Show", "Hamlet", &given(&[("name", Val::Text("Hamlet".into())), ("price", Val::Money(4000))])).unwrap();
    let session = given(&[("show", Val::Ref("Hamlet".into())), ("day", Val::Text("Friday".into())), ("seats", Val::Count(100))]);
    h.create("Session", "Fri", &session).unwrap();
    book(&mut h, "B1").unwrap();
    h
}

fn book(h: &mut Host, label: &str) -> Result<(), String> {
    h.create("Booking", label, &given(&[("fan", Val::Ref("Ada".into())), ("show", Val::Ref("Hamlet".into())), ("quantity", Val::Count(2))]))
}

const PAIRS: (&str, &str) = ("rule early:", "rule pairs: every Booking's quantity is at most 1\nrule early:");

#[test]
fn a_rule_live_units_break_refuses_the_deploy_until_it_is_applied_from_a_version() {
    let mut h = live();
    let breaks = h.deploy_check(&tickets(2, &[PAIRS])).unwrap();
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert!(breaks[0].starts_with("rule pairs would break"), "{}", breaks[0]);
    assert!(breaks[0].contains("live in B1"), "{}", breaks[0]);
    assert!(breaks[0].contains("`apply rule pairs to Bookings created from version 2`"), "{}", breaks[0]);
    let err = h.deploy(tickets(2, &[PAIRS])).unwrap_err();
    assert!(err.starts_with("the deploy is refused"), "{err}");
    assert_eq!(h.prog.version, 1, "nothing changed");

    let apply = ("admit book", "apply rule pairs to Bookings created from version 2\nadmit book");
    h.deploy(tickets(2, &[PAIRS, apply])).unwrap();
    assert_eq!(h.prog.version, 2);
    assert!(h.trace.iter().any(|l| l == "deployed tickets v2 over v1"), "{:?}", h.trace);
    assert!(h.trace.iter().any(|l| l == "B1  now under version 2"), "{:?}", h.trace);
    assert_eq!(h.unit("B1").unwrap().version, 1);
    let err = book(&mut h, "B2").unwrap_err();
    assert!(err.contains("pairs"), "a new booking is checked: {err}");
    // The old booking goes on as before.
    h.answer("payments", "charged", &[], None).unwrap();
    assert!(h.trace.iter().any(|l| l.starts_with("B1  Payment charged")), "{:?}", h.trace);
    assert!(h.machine.faults().is_empty(), "{:?}", h.machine.faults());
}

#[test]
fn removed_kinds_changed_types_and_attributes_without_defaults_are_breaks() {
    let h = live();
    let vip = ("cancelled: Bool = no }", "cancelled: Bool = no, vip: Bool }");
    let breaks = h.deploy_check(&tickets(2, &[vip])).unwrap();
    assert_eq!(breaks.len(), 1, "{breaks:?}");
    assert!(breaks[0].starts_with("Booking's vip is added without a default\n    live in B1\n    resolve: give it a default"), "{}", breaks[0]);
    let day = ("day: Text", "day: Count");
    let breaks = h.deploy_check(&tickets(2, &[day])).unwrap();
    assert!(breaks[0].starts_with("Session's day changes from Text to Count\n    live in Fri"), "{breaks:?}");
    let no_ticket = [
        ("Booking needs Payment, Ticket while", "Booking needs Payment while"),
        ("kind Ticket  { booking: Booking }\n", ""),
        ("Ticket  uses  a Session where the Session's show is the Booking's show,\n              taking the Booking's quantity\n", ""),
        ("effect issue Ticket via mailer within 10 minutes\n  sends    the booking's fan's name\n  marks    sent\n  reports  issued\n", ""),
        ("define: a Booking is issued when its Ticket is issued\n", ""),
        ("rule early: no Booking has a Ticket that is sent unless it is paid\n", ""),
        (" is paid and issued", " is paid"),
        ("prefer Session: highest seats\n", ""),
        ("rule seats: the total quantity taken from each Session is at most its seats\n", ""),
    ];
    let breaks = h.deploy_check(&tickets(2, &no_ticket)).unwrap();
    assert!(breaks.iter().any(|b| b.starts_with("kind Ticket is removed\n    live in B1")), "{breaks:?}");
    assert!(h.deploy_check(&tickets(1, &[])).unwrap_err().contains("needs a higher version than 1"));
}

#[test]
fn a_deploy_fills_defaults_drops_removed_attributes_and_creates_new_parts() {
    let mut h = live();
    let edits = [
        ("cancelled: Bool = no }", "cancelled: Bool = no, vip: Bool = no }"),
        ("kind Ticket  { booking: Booking }", "kind Ticket  { booking: Booking }\nkind Receipt { booking: Booking }"),
        ("Booking needs Payment, Ticket while", "Booking needs Payment, Ticket, Receipt while"),
        ("define: a Booking is paid", "effect send Receipt via mailer within 10 minutes\n  sends    the booking's fan's name\n  reports  sent\n\ndefine: a Booking is paid"),
    ];
    h.deploy(tickets(2, &edits)).unwrap();
    let u = h.unit("B1").unwrap();
    assert_eq!(u.root.attrs.get("vip"), Some(&Val::Bool(false)));
    assert!(u.root.parts.iter().any(|p| p.kind == "Receipt"), "{:?}", h.trace);
    assert!(h.pending().iter().any(|a| a.kind == "Receipt"), "{:?}", h.trace);
    assert!(h.pending().iter().any(|a| a.kind == "Payment"), "the payment request is still waiting");
    h.answer("payments", "charged", &[], None).unwrap();
    h.answer("mailer", "sent", &[], None).unwrap();

    let mut h = live();
    h.deploy(tickets(2, &[(", day: Text", "")])).unwrap_or_else(|e| panic!("{e}"));
    assert!(!h.unit("Fri").unwrap().root.attrs.contains_key("day"));
    assert!(h.trace.iter().any(|l| l == "Fri now under version 2, dropped Session's day"), "{:?}", h.trace);
}

#[test]
fn apply_is_checked() {
    let bad = |line: &str| {
        let text = TICKETS.replace("module tickets version 1", "module tickets version 2").replace("admit book", &format!("{line}\nadmit book"));
        let errs = suture_lang::compile(&[("t.st".into(), text)]).unwrap_err();
        errs.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")
    };
    assert!(bad("apply rule nope to Bookings created from version 2").contains("there is no rule nope"));
    assert!(bad("apply rule seats to Sessions created from version 2").contains("only a rule about one entity at a time"));
    assert!(bad("apply rule early to Bookings created from version 3").contains("version 3 is not from 1 to this module's version, 2"));
    assert!(bad("apply rule early to Tickets created from version 2").contains("not to Tickets"));
    assert!(bad("apply rule early to Bookings created from version 2\napply rule early to Bookings created from version 1").contains("already applies"));
    assert!(bad("apply rule early to Bookings from version 2").contains("expected `created`"));
}

fn state(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("deploy").join(name);
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn world(p: &Program, kind: &str, label: &str, attrs: serde_json::Value) -> Record {
    Record::World { kind: kind.into(), label: label.into(), attrs: service::attrs_json(p, kind, Some(&attrs)).unwrap(), items: vec![] }
}

fn served(p: &Program, dir: &std::path::Path) {
    let mut s = Service::open(p, dir, T0, |_| {}).unwrap();
    for r in [
        world(p, "Fan", "Ada", json!({ "name": "Ada" })),
        world(p, "Show", "Hamlet", json!({ "name": "Hamlet", "price": "40 EUR" })),
        world(p, "Session", "Fri", json!({ "show": "Hamlet", "day": "Friday", "seats": 100 })),
    ] {
        s.input(r, T0).unwrap();
    }
    let attrs = service::attrs_json(p, "Booking", Some(&json!({ "fan": "Ada", "show": "Hamlet", "quantity": 2 }))).unwrap();
    let by = Who { role: "fan".into(), name: "Ada".into() };
    let a = s.input(Record::New { by, verb: "books".into(), kind: "Booking".into(), label: "B1".into(), attrs, items: vec![] }, T0 + 5).unwrap();
    assert_eq!(a.refused, None, "{:?}", a.trace);
}

#[test]
fn a_service_deploys_a_new_version_and_a_restart_replays_across_it() {
    let dir = state("across");
    let v1 = tickets(1, &[]);
    served(&v1, &dir);
    let v2 = tickets(2, &[PAIRS]);
    // The check reads the state and never writes it.
    let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap();
    let Check::Refused(breaks) = Service::check(&v2, &dir, T0 + 10).unwrap() else { panic!("expected breaks") };
    assert!(breaks[0].starts_with("rule pairs would break"), "{breaks:?}");
    let e = Service::open(&v2, &dir, T0 + 10, |_| {}).err().unwrap();
    assert!(e.contains("the deploy of tickets v2 is refused; these breaks need a resolution"), "{e}");
    assert_eq!(std::fs::read_to_string(dir.join("log.jsonl")).unwrap(), log, "a refused deploy writes nothing");
    assert!(!dir.join("program-v2.json").exists());

    let v2 = tickets(2, &[PAIRS, ("admit book", "apply rule pairs to Bookings created from version 2\nadmit book")]);
    let Check::Deploys(lines) = Service::check(&v2, &dir, T0 + 10).unwrap() else { panic!("expected a deploy") };
    assert!(lines.contains(&"deployed tickets v2 over v1".to_string()), "{lines:?}");
    assert_eq!(std::fs::read_to_string(dir.join("log.jsonl")).unwrap(), log);
    let (trace, b1) = {
        let mut s = Service::open(&v2, &dir, T0 + 10, |_| {}).unwrap();
        assert_eq!(s.host.prog.version, 2);
        let a = s.input(Record::Answer { adapter: "payments".into(), outcome: "charged".into(), key: None, target: Some("B1".into()), data: vec![] }, T0 + 12).unwrap();
        assert_eq!(a.refused, None, "{:?}", a.trace);
        (s.host.trace.clone(), s.host.unit("B1"))
    };
    assert!(dir.join("program-v2.json").exists());
    assert!(std::fs::read_to_string(dir.join("log.jsonl")).unwrap().contains(r#"{"deploy":{"version":2}}"#));
    let s = Service::open(&v2, &dir, T0 + 20, |_| {}).unwrap();
    assert_eq!(s.host.trace, trace);
    assert_eq!(s.host.unit("B1"), b1);
    assert_eq!(Service::check(&v2, &dir, T0 + 20).unwrap(), Check::Same);
    let e = Service::open(&v1, &dir, T0 + 20, |_| {}).err().unwrap();
    assert!(e.ends_with("the state runs tickets v2; going back to v1 is not supported"), "{e}");
    let mut other = v2.clone();
    other.goals.clear();
    let e = Service::open(&other, &dir, T0 + 20, |_| {}).err().unwrap();
    assert!(e.ends_with("this program differs under the same version; give it a new version to deploy it"), "{e}");
}

#[test]
fn the_check_reads_carried_facts_as_the_new_version_has_them() {
    let h = live();
    let limit = [
        ("kind Show    { name: Text, price: Money }", "kind Show    { name: Text, price: Money, limit: Count = 1 }"),
        ("rule early:", "rule limit: every Booking's quantity is at most its show's limit\nrule early:"),
    ];
    let breaks = h.deploy_check(&tickets(2, &limit)).unwrap();
    assert!(breaks.iter().any(|b| b.starts_with("rule limit would break") && b.contains("live in B1")), "{breaks:?}");
}

#[test]
fn an_added_formula_is_computed_and_pools_follow_new_keys() {
    let mut h = live();
    let edits = [
        ("cancelled: Bool = no }", "cancelled: Bool = no, seats: Count = quantity * 2, zone: Text = \"A\" }"),
        ("kind Session { show: Show, day: Text, seats: Count }", "kind Session { show: Show, day: Text, seats: Count, zone: Text = \"A\" }"),
        ("where the Session's show is the Booking's show,", "where the Session's zone is the Booking's zone,"),
    ];
    h.deploy(tickets(2, &edits)).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(h.unit("B1").unwrap().root.attrs.get("seats"), Some(&Val::Count(4)), "{:?}", h.trace);
    book(&mut h, "B2").unwrap();
    assert!(h.trace.iter().any(|l| l == "B2  Ticket gets Session Fri"), "the pool is keyed by zone now: {:?}", h.trace);
}
