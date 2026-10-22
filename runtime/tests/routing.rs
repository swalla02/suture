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
    // A later `decide` with a bad router deadline is an error, not a crash.
    let two = CLAIM.replace(
        "admit file",
        "decide Outcome for Claims with model \"fast@1\" or model \"careful@1\", picked by model \"router@1\" within 0 milliseconds\nadmit file",
    );
    assert_eq!(errors(&two), ["t.st:10: a model's `within` is from 1 millisecond to 10 minutes"]);
}

#[test]
fn a_bound_in_the_program_holds_for_the_candidates_and_the_router() {
    // Slice 35: `at least … sure` on a `decide` (spec appendix AK).
    let src = SHOP.replace(
        "or prefer within 2 seconds, picked by model \"router@1\" within 100 milliseconds",
        "or prefer within 2 seconds at least 0.8 sure, picked by model \"router@1\" within 100 milliseconds at least 0.7 sure",
    );
    let p = prog(&src);
    assert_eq!(
        p.decisions[0].handler(),
        "model \"cheapest@1\" or model \"dearest@1\" or `prefer` within 2000 ms at least 0.8 sure, picked by model \"router@1\" within 100 ms at least 0.7 sure"
    );
    let r = passes(
        &p,
        &format!(
            "{WORLD}  when   model \"router@1\" picks \"dearest@1\" at 0.9
  and    model \"dearest@1\" picks Fast at 0.95
  and    staff Sam places a new Order O1 with product K and zone EU
"
        ),
    );
    assert!(has(&r, "choice: Fast for O1/Shipment#1@Carrier by model \"dearest@1\""), "{}", r.host.trace.join("\n"));
    // An unsure router leaves the choice to the first candidate, and an
    // unsure candidate to `prefer`.
    let r = passes(
        &p,
        &format!(
            "{WORLD}  when   model \"router@1\" picks \"dearest@1\" at 0.6
  and    model \"cheapest@1\" picks Fast at 0.5
  and    staff Sam places a new Order O1 with product K and zone EU
"
        ),
    );
    assert!(has(&r, "model for O1/Shipment#1@Carrier: cheapest@1 by prefer (model \"router@1\" was unsure, at 0.60)"), "{}", r.host.trace.join("\n"));
    assert!(has(&r, "choice: Cheap for O1/Shipment#1@Carrier by prefer (model \"cheapest@1\" was unsure, at 0.50)"), "{}", r.host.trace.join("\n"));
    // With a bound, a pick that gives no confidence is not used.
    let r = passes(
        &p,
        &format!(
            "{WORLD}  when   model \"router@1\" picks \"dearest@1\" at 0.9
  and    model \"dearest@1\" picks Fast
  and    staff Sam places a new Order O1 with product K and zone EU
"
        ),
    );
    assert!(
        has(&r, "choice: Cheap for O1/Shipment#1@Carrier by prefer (model \"dearest@1\" failed: it gave no confidence, and the program needs at least 0.8 sure)"),
        "{}",
        r.host.trace.join("\n")
    );
}

#[test]
fn a_single_model_and_a_slot_take_a_bound_too() {
    let src = CLAIM.replace(
        "decide Outcome with model \"fast@1\" or model \"careful@1\", picked by model \"router@1\"",
        "decide Outcome with model \"careful@1\" at least 0.9 sure",
    );
    let p = prog(&src);
    let r = passes(
        &p,
        "  when   model \"careful@1\" picks Writeoff at 0.8
  and    staff Sam files a new Claim C1 with note \"roof\"
  expect C1 has a Repair
",
    );
    assert!(has(&r, "choice: Repair for C1?Outcome#1 by prefer (model \"careful@1\" was unsure, at 0.80)"), "{}", r.host.trace.join("\n"));
    let prefer = CLAIM.replace(
        "decide Outcome with model \"fast@1\" or model \"careful@1\", picked by model \"router@1\"",
        "decide Outcome with prefer at least 0.9 sure",
    );
    assert_eq!(errors(&prefer), ["t.st:9: `with prefer` answers at once; `at least … sure` bounds a model's answer"]);
}


#[test]
fn otherwise_names_the_option_taken_when_the_model_is_no_help() {
    // Slice 36: `otherwise …` on a `decide` (spec appendix AL).
    let line = "decide Outcome with model \"fast@1\" or model \"careful@1\", picked by model \"router@1\"";
    let p = prog(&CLAIM.replace(line, "decide Outcome with model \"careful@1\" at least 0.9 sure otherwise Writeoff"));
    assert_eq!(p.decisions[0].handler(), "model \"careful@1\" at least 0.9 sure otherwise Writeoff");
    let r = passes(
        &p,
        "  when   model \"careful@1\" picks Repair at 0.8
  and    staff Sam files a new Claim C1 with note \"roof\"
  expect C1 has a Writeoff
",
    );
    assert!(has(&r, "choice: Writeoff for C1?Outcome#1 by otherwise (model \"careful@1\" was unsure, at 0.80)"), "{}", r.host.trace.join("\n"));
    // A sure pick is used as before.
    let r = passes(
        &p,
        "  when   model \"careful@1\" picks Repair at 0.95
  and    staff Sam files a new Claim C1 with note \"roof\"
  expect C1 has a Repair
",
    );
    assert!(r.passed(), "{:?}", r.failures);
    // A router falls back to the candidate it names.
    let p = prog(&CLAIM.replace(line, &format!("{line} at least 0.7 sure otherwise model \"careful@1\"")));
    let r = passes(
        &p,
        "  when   model \"router@1\" picks \"fast@1\" at 0.5
  and    model \"careful@1\" picks Writeoff
  and    staff Sam files a new Claim C1 with note \"roof\"
  expect C1 has a Writeoff
",
    );
    assert!(has(&r, "model for C1?Outcome#1: careful@1 by otherwise (model \"router@1\" was unsure, at 0.50)"), "{}", r.host.trace.join("\n"));
}

#[test]
fn otherwise_is_one_of_the_choices_options() {
    let line = "decide Outcome with model \"fast@1\" or model \"careful@1\", picked by model \"router@1\"";
    let with = |d: &str| errors(&CLAIM.replace(line, d));
    assert_eq!(with("decide Outcome with model \"careful@1\" otherwise Refund"), ["t.st:9: `otherwise Refund`: Outcome is one of Repair, Writeoff"]);
    assert_eq!(with("decide Outcome with prefer otherwise Writeoff"), ["t.st:9: `with prefer` answers at once; `otherwise` is for when a model is no help"]);
    assert_eq!(
        with(&format!("{line} otherwise model \"slow@1\"")),
        ["t.st:9: `otherwise slow@1`: a router falls back to one of its candidates, fast@1, careful@1"]
    );
    assert!(with("decide Outcome with model \"careful@1\" otherwise prefer").is_empty());
}

#[test]
fn a_pooled_choice_falls_back_to_prefer_only() {
    let src = SHOP.replace(
        "model \"cheapest@1\" or model \"dearest@1\" or prefer within 2 seconds, picked by model \"router@1\" within 100 milliseconds",
        "model \"cheapest@1\" otherwise Cheap",
    );
    let e = errors(&src);
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].ends_with("`otherwise Cheap`: Carrier is chosen among entities that exist only as the program runs, so its fallback is `prefer`"), "{e:?}");
}

#[test]
fn the_router_sees_the_options_and_picks_by_them() {
    // Appendix AY: dearer carriers go to the careful model.
    let p = prog(SHOP);
    let sc = &parse_scenarios("t.scenario", &format!("scenario \"s\":\n{WORLD}  when   staff Sam places a new Order O1 with product K and zone EU\n")).unwrap()[0];
    let r = suture_runtime::scenario::run_with(&p, sc, |h| {
        h.models.register(
            "router@1",
            Box::new(|c: &Choice| {
                let dear = c.among.iter().any(|o| o.attrs.iter().any(|(k, v)| k == "fee" && matches!(v, Val::Money(x) if *x > 800)));
                Some(if dear { "dearest@1" } else { "cheapest@1" }.to_string())
            }),
        );
        h.models.register("dearest@1", Box::new(|c: &Choice| c.options.last().map(|o| o.label.clone())));
        h.models.register("cheapest@1", Box::new(|c: &Choice| c.options.first().map(|o| o.label.clone())));
    });
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(has(&r, "model for O1/Shipment#1@Carrier: dearest@1 by model \"router@1\""), "{}", r.host.trace.join("\n"));
}
