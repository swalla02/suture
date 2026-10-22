//! Slice 34: a model picks which model makes a choice (spec appendix AJ).

use std::path::PathBuf;

use serde_json::json;
use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::choice::{Choice, Option_};
use suture_runtime::host::Host;
use suture_runtime::scenario::{run, Report};
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
decide Carrier with model \"cheapest@1\" or model \"dearest@1\" or prefer within 2 seconds, picked by model \"router@1\" within 100 milliseconds
";

const CLAIM: &str = "module claims version 1, currency EUR
role staff
kind Claim    { note: Text }
kind Repair   { claim: Claim }
kind Writeoff { claim: Claim }
Claim needs Outcome
Outcome is Repair or Writeoff
prefer Outcome: Repair, then Writeoff
decide Outcome with model \"fast@1\" or model \"careful@1\", picked by model \"router@1\"
admit file a new Claim by staff
";

fn prog(src: &str) -> Program {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn errors(src: &str) -> Vec<String> {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

fn passes(p: &Program, body: &str) -> Report {
    let src = format!("scenario \"s\":\n{body}");
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    let r = run(p, sc);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    r
}

fn has(r: &Report, line: &str) -> bool {
    r.host.trace.iter().any(|l| l.contains(line))
}

const WORLD: &str = "  given  a Product K with name Kettle
  and    a Carrier Cheap with name Cheap, zone EU and fee 5 EUR
  and    a Carrier Fast with name Fast, zone EU and fee 9 EUR
";

#[test]
fn the_router_picks_the_model_and_that_model_picks_the_carrier() {
    let p = prog(SHOP);
    let r = passes(
        &p,
        &format!(
            "{WORLD}  when   model \"router@1\" picks \"dearest@1\"
  and    model \"dearest@1\" picks Fast
  and    staff Sam places a new Order O1 with product K and zone EU
"
        ),
    );
    assert!(has(&r, "model for O1/Shipment#1@Carrier: dearest@1 by model \"router@1\" (of cheapest@1, dearest@1, prefer)"), "{}", r.host.trace.join("\n"));
    assert!(has(&r, "choice: Fast for O1/Shipment#1@Carrier by model \"dearest@1\" (of Cheap, Fast)"), "{}", r.host.trace.join("\n"));
}

#[test]
fn the_router_may_leave_the_choice_to_prefer_and_falls_back_to_the_first_candidate() {
    let p = prog(SHOP);
    let r = passes(
        &p,
        &format!(
            "{WORLD}  when   model \"router@1\" picks prefer
  and    staff Sam places a new Order O1 with product K and zone EU
"
        ),
    );
    assert!(has(&r, "choice: Cheap for O1/Shipment#1@Carrier by prefer (as model \"router@1\" picked)"), "{}", r.host.trace.join("\n"));
    // A pick it was not offered, or no answer, goes to the first candidate.
    let r = passes(
        &p,
        &format!(
            "{WORLD}  when   model \"router@1\" picks \"other@1\"
  and    model \"cheapest@1\" picks Cheap
  and    staff Sam places a new Order O1 with product K and zone EU
"
        ),
    );
    assert!(has(&r, "model for O1/Shipment#1@Carrier: cheapest@1 by prefer (model \"router@1\" picked other@1, which it was not offered)"), "{}", r.host.trace.join("\n"));
    assert!(has(&r, "choice: Cheap for O1/Shipment#1@Carrier by model \"cheapest@1\""), "{}", r.host.trace.join("\n"));
}

#[test]
fn a_slot_is_routed_too() {
    let p = prog(CLAIM);
    let r = passes(
        &p,
        "  when   model \"router@1\" picks \"careful@1\"
  and    model \"careful@1\" picks Writeoff
  and    staff Sam files a new Claim C1 with note \"roof\"
  expect C1 has a Writeoff
",
    );
    assert!(has(&r, "model for C1?Outcome#1: careful@1 by model \"router@1\" (of fast@1, careful@1)"), "{}", r.host.trace.join("\n"));
    let text = suture_lang::readback::handlers_for(&p, "Outcome", "Claim", "otherwise `prefer Outcome`".into()).join("; ");
    assert_eq!(text, "model \"fast@1\" or model \"careful@1\", picked by model \"router@1\"; otherwise `prefer Outcome`");
}

fn register(h: &mut Host) {
    let fee = |o: &Option_| match o.attrs.iter().find(|(k, _)| k == "fee") {
        Some((_, Val::Money(x))) => *x,
        _ => 0,
    };
    h.models.register("router@1", Box::new(|c: &Choice| c.options.iter().find(|o| o.label == "dearest@1").map(|o| o.label.clone())));
    h.models.register("dearest@1", Box::new(move |c: &Choice| c.options.iter().max_by_key(|o| fee(o)).map(|o| o.label.clone())));
}

#[test]
fn served_the_router_and_the_model_are_asked_after_the_step_and_both_answers_replay() {
    let p = prog(SHOP);
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("routing");
    let _ = std::fs::remove_dir_all(&dir);
    let mut svc = Service::open(&p, &dir, T0, register).unwrap();
    let world = |kind: &str, label: &str, attrs: serde_json::Value| Record::World {
        kind: kind.into(),
        label: label.into(),
        attrs: service::attrs_json(&p, kind, Some(&attrs)).unwrap(),
        items: vec![],
    };
    for r in [
        world("Product", "Kettle", json!({ "name": "Kettle" })),
        world("Carrier", "Cheap", json!({ "name": "Cheap", "zone": "EU", "fee": "5 EUR" })),
        world("Carrier", "Fast", json!({ "name": "Fast", "zone": "EU", "fee": "9 EUR" })),
    ] {
        assert_eq!(svc.input(r, T0).unwrap().refused, None);
    }
    let attrs = service::attrs_json(&p, "Order", Some(&json!({ "product": "Kettle", "zone": "EU" }))).unwrap();
    let new = Record::New { by: Who { role: "staff".into(), name: "Sam".into() }, verb: "places".into(), kind: "Order".into(), label: "O1".into(), attrs, items: vec![] };
    let applied = svc.input(new, T0).unwrap();
    assert!(applied.trace.iter().any(|l| l.contains("choice for O1/Shipment#1@Carrier?model: model \"router@1\" is asked")), "{:?}", applied.trace);
    let ticked = svc.tick(T0).unwrap();
    assert!(ticked.iter().any(|l| l.contains("model for O1/Shipment#1@Carrier: dearest@1 by model \"router@1\"")), "{ticked:?}");
    assert!(ticked.iter().any(|l| l.contains("choice: Fast for O1/Shipment#1@Carrier by model \"dearest@1\" (of Cheap, Fast)")), "{ticked:?}");
    let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap();
    assert_eq!(log.matches("\"decided\"").count(), 2, "{log}");
    let unit = svc.host.unit("O1");
    drop(svc);
    let again = Service::open(&p, &dir, T0 + 1, |_| {}).unwrap();
    assert_eq!(again.host.unit("O1"), unit);
    assert!(again.host.model_asks().is_empty());
}

#[test]
fn a_late_router_leaves_the_choice_to_the_first_candidate() {
    let p = prog(SHOP);
    let r = passes(
        &p,
        &format!(
            "{WORLD}  when   model \"router@1\" is late
  and    model \"cheapest@1\" picks Cheap
  and    staff Sam places a new Order O1 with product K and zone EU
"
        ),
    );
    assert!(has(&r, "model for O1/Shipment#1@Carrier: cheapest@1 by prefer (model \"router@1\" did not answer within 100 ms)"), "{}", r.host.trace.join("\n"));
}

#[test]
fn a_routed_decide_names_two_or_more_models_once_and_ends_with_its_router() {
    let line = |d: &str| CLAIM.replace("decide Outcome with model \"fast@1\" or model \"careful@1\", picked by model \"router@1\"", d);
    assert_eq!(
        errors(&line("decide Outcome with model \"fast@1\" or model \"careful@1\"")),
        ["t.st:9: a choice among models ends with the model that picks one: `, picked by model \"router@1\"`"]
    );
    assert_eq!(
        errors(&line("decide Outcome with model \"fast@1\", picked by model \"router@1\"")),
        ["t.st:9: `picked by` picks among models: name two or more, as in `with model \"fast@1\" or model \"careful@1\", picked by model \"router@1\"`"]
    );
    assert_eq!(
        errors(&line("decide Outcome with model \"fast@1\" or model \"fast@1\", picked by model \"router@1\"")),
        ["t.st:9: `decide`: fast@1 is named twice among the models to pick from"]
    );
    assert_eq!(
        errors(&line("decide Outcome with model \"fast@1\" or model \"careful@1\", picked by model \"router\"")),
        ["t.st:9: a model is named `name@version`, as in \"router@1\""]
    );
}
