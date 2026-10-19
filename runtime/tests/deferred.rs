//! Slice 18: a model asked after the step that needs it (spec appendix S.2.2).

use std::path::PathBuf;

use serde_json::json;
use suture_lang::ir::Program;
use suture_lang::scenario::Caller;
use suture_runtime::choice::{Choice, Option_};
use suture_runtime::host::Host;
use suture_runtime::service::{self, Record, Service, Who};
use suture_runtime::state::Val;

const T0: u64 = 1_800_000_000;

const SHOP: &str = "module shop version 1, currency EUR
role staff
kind Product  { name: Text }
kind Carrier  { name: Text, zone: Text, fee: Money }
kind Order    { product: Product, zone: Text }
kind Shipment { order: Order }
Order needs Shipment
Shipment uses a Carrier where the Carrier's zone is the Order's zone
prefer Carrier: lowest fee
admit place a new Order by staff
decide Carrier with model \"dearest@1\" within 2 seconds
";

fn prog() -> Program {
    suture_lang::compile(&[("shop.st".to_string(), SHOP.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn dearest(h: &mut Host) {
    h.models.register(
        "dearest@1",
        Box::new(|c: &Choice| {
            let fee = |o: &Option_| match o.attrs.iter().find(|(k, _)| k == "fee") {
                Some((_, Val::Money(x))) => *x,
                _ => 0,
            };
            c.options.iter().max_by_key(|o| fee(o)).map(|o| o.label.clone())
        }),
    );
}

fn world(p: &Program, kind: &str, label: &str, attrs: serde_json::Value) -> Record {
    Record::World { kind: kind.into(), label: label.into(), attrs: service::attrs_json(p, kind, Some(&attrs)).unwrap(), items: vec![] }
}

fn carrier(t: &[String]) -> String {
    t.iter().find(|l| l.contains("choice: ") && l.contains("@Carrier")).cloned().unwrap_or_default()
}

#[test]
fn the_input_returns_before_the_model_answers_and_the_log_keeps_the_answer() {
    let p = prog();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("deferred");
    let _ = std::fs::remove_dir_all(&dir);
    let mut svc = Service::open(&p, &dir, T0, dearest).unwrap();
    for r in [
        world(&p, "Product", "Kettle", json!({ "name": "Kettle" })),
        world(&p, "Carrier", "Cheap", json!({ "name": "Cheap", "zone": "EU", "fee": "5 EUR" })),
        world(&p, "Carrier", "Fast", json!({ "name": "Fast", "zone": "EU", "fee": "9 EUR" })),
    ] {
        assert_eq!(svc.input(r, T0).unwrap().refused, None);
    }
    let attrs = service::attrs_json(&p, "Order", Some(&json!({ "product": "Kettle", "zone": "EU" }))).unwrap();
    let new = Record::New { by: Who { role: "staff".into(), name: "Sam".into() }, verb: "places".into(), kind: "Order".into(), label: "O1".into(), attrs, items: vec![] };
    let applied = svc.input(new, T0).unwrap();
    assert!(applied.trace.iter().any(|l| l.contains("choice for O1/Shipment#1@Carrier: model \"dearest@1\" is asked")), "{:?}", applied.trace);
    assert_eq!(carrier(&applied.trace), "", "nothing is picked in the input's step");
    let ticked = svc.tick(T0).unwrap();
    assert_eq!(carrier(&ticked).trim(), "choice: Fast for O1/Shipment#1@Carrier by model \"dearest@1\" (of Cheap, Fast)", "{ticked:?}");
    let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap();
    assert_eq!(log.matches("\"decided\"").count(), 1, "{log}");
    // The answer replays from the log, with no model registered.
    let unit = svc.host.unit("O1");
    drop(svc);
    let again = Service::open(&p, &dir, T0 + 1, |_| {}).unwrap();
    assert_eq!(again.host.unit("O1"), unit);
    assert!(again.host.model_asks().is_empty());
}

#[test]
fn a_late_model_leaves_the_pick_to_prefer_and_its_answer_is_ignored() {
    let p = prog();
    let mut h = Host::new(p.clone());
    h.defer_models = true;
    for (k, l, a) in [
        ("Product", "Kettle", vec![("name", Val::Text("Kettle".into()))]),
        ("Carrier", "Cheap", vec![("name", Val::Text("Cheap".into())), ("zone", Val::Text("EU".into())), ("fee", Val::Money(500))]),
        ("Carrier", "Fast", vec![("name", Val::Text("Fast".into())), ("zone", Val::Text("EU".into())), ("fee", Val::Money(900))]),
    ] {
        let a: Vec<(String, Val)> = a.into_iter().map(|(n, v)| (n.to_string(), v)).collect();
        h.create_with(k, l, &a, &[]).unwrap();
    }
    let by = Caller { role: "staff".into(), name: "Sam".into(), verb: "places".into() };
    h.input_new(Some(&by), "Order", "O1", &[("product".into(), Val::Ref("Kettle".into())), ("zone".into(), Val::Text("EU".into()))], &[]).unwrap();
    assert_eq!(h.model_asks().len(), 1);
    h.pass(2).unwrap();
    assert!(h.model_asks().is_empty());
    let t = h.trace.join("\n");
    assert!(t.contains("choice: Cheap for O1/Shipment#1@Carrier by prefer (model \"dearest@1\" did not answer within 2000 ms)"), "{t}");
    h.model_answer("O1/Shipment#1@Carrier", &suture_runtime::choice::Answer::Picked("Fast".into())).unwrap();
    assert!(h.trace.last().unwrap().contains("ignored: nothing waits for it"), "{:?}", h.trace.last());
}
