//! Slice 9: models at choice points (spec appendix J).

use std::sync::Arc;

use suture::Machine;
use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::kernel;
use suture_runtime::scenario::{run, Report};

const SHOP: &str = "module shop version 1, currency EUR
role staff
kind Product  { name: Text }
kind Stock    { product: Product, units: Count }
kind Carrier  { name: Text, zone: Text, fee: Money }
kind Order    { product: Product, zone: Text, quantity: Count, total: Money }
kind Shipment { order: Order }
Order needs Shipment
Shipment uses a Stock where the Stock's product is the Order's product, taking the Order's quantity
Shipment uses a Carrier where the Carrier's zone is the Order's zone
rule stock: the total quantity taken from each Stock is at most its units
prefer Carrier: lowest fee
prefer Stock: highest units
admit place a new Order by staff
";

const WORLD: &str = "  given  a Product Kettle with name Kettle
  and    a Stock Small with product Kettle and units 1
  and    a Stock Big with product Kettle and units 9
  and    a Carrier Cheap with name Cheap, zone EU and fee 5 EUR
  and    a Carrier Fast with name Fast, zone EU and fee 9 EUR
";

fn prog(decides: &str) -> Program {
    let files = vec![("shop.st".to_string(), format!("{SHOP}{decides}"))];
    suture_lang::compile(&files).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn scenario(p: &Program, steps: &str) -> Report {
    let src = format!("scenario \"s\":\n{WORLD}{steps}");
    let sc = &parse_scenarios("s.scenario", &src).unwrap()[0];
    run(p, sc)
}

fn trace(p: &Program, steps: &str) -> String {
    let r = scenario(p, steps);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    r.host.trace.join("\n")
}

const ORDER: &str = "  when   staff Sam places a new Order O1 with product Kettle, zone EU, quantity 1 and total 50 EUR\n";
const BIG_ORDER: &str = "  when   staff Sam places a new Order O1 with product Kettle, zone EU, quantity 1 and total 5000 EUR\n";

fn carrier(t: &str) -> &str {
    t.lines().find(|l| l.contains("for O1/Shipment#1@Carrier")).unwrap_or("no carrier choice").trim()
}

#[test]
fn without_a_decide_prefer_picks() {
    let t = trace(&prog(""), ORDER);
    assert_eq!(carrier(&t), "choice: Cheap for O1/Shipment#1@Carrier");
}

#[test]
fn the_app_level_model_picks_among_the_options() {
    let p = prog("decide with model \"planner@3\"\n");
    let t = trace(&p, &format!("  when   model \"planner@3\" picks Fast, then Big\n{ORDER}  expect O1's Shipment's Carrier's name is \"Fast\"\n"));
    assert_eq!(carrier(&t), "choice: Fast for O1/Shipment#1@Carrier by model \"planner@3\" (of Cheap, Fast)");
    assert!(t.contains("choice: Big for O1/Shipment#1@Stock by model \"planner@3\" (of Big, Small)"), "{t}");
}

#[test]
fn a_decision_level_model_beats_the_app_level_one() {
    let p = prog("decide with model \"planner@3\"\ndecide Carrier with model \"carriers@1\"\n");
    let t = trace(&p, &format!("  when   model \"carriers@1\" picks Fast\n  and    model \"planner@3\" picks Small\n{ORDER}"));
    assert_eq!(carrier(&t), "choice: Fast for O1/Shipment#1@Carrier by model \"carriers@1\" (of Cheap, Fast)");
    assert!(t.contains("choice: Small for O1/Shipment#1@Stock by model \"planner@3\""), "{t}");
}

#[test]
fn a_step_level_model_beats_the_workflow_level_one() {
    let p = prog("decide for Orders with model \"orders@2\"\ndecide for Shipments with model \"shipping@1\"\n");
    let t = trace(&p, &format!("  when   model \"shipping@1\" picks Fast, then Big\n  and    model \"orders@2\" picks Cheap\n{ORDER}"));
    assert_eq!(carrier(&t), "choice: Fast for O1/Shipment#1@Carrier by model \"shipping@1\" (of Cheap, Fast)");
}

#[test]
fn a_condition_narrows_a_scope() {
    let p = prog(
        "decide Carrier with model \"carriers@1\"
decide Carrier for Orders whose total is at least 1000 EUR with model \"careful@1\"
",
    );
    let t = trace(&p, &format!("  when   model \"careful@1\" picks Fast\n  and    model \"carriers@1\" picks Cheap\n{BIG_ORDER}"));
    assert_eq!(carrier(&t), "choice: Fast for O1/Shipment#1@Carrier by model \"careful@1\" (of Cheap, Fast)");
    let t = trace(&p, &format!("  when   model \"careful@1\" picks Fast\n  and    model \"carriers@1\" picks Cheap\n{ORDER}"));
    assert_eq!(carrier(&t), "choice: Cheap for O1/Shipment#1@Carrier by model \"carriers@1\" (of Cheap, Fast)");
}

#[test]
fn with_prefer_takes_a_scope_back() {
    let p = prog("decide with model \"planner@3\"\ndecide Carrier for Shipments with prefer\n");
    let t = trace(&p, &format!("  when   model \"planner@3\" picks Fast, then Big\n{ORDER}"));
    assert_eq!(carrier(&t), "choice: Cheap for O1/Shipment#1@Carrier");
}

#[test]
fn a_model_that_declines_strays_or_is_missing_falls_back_to_prefer() {
    let p = prog("decide Carrier with model \"carriers@1\"\n");
    let t = trace(&p, ORDER);
    assert_eq!(carrier(&t), "choice: Cheap for O1/Shipment#1@Carrier by prefer (model \"carriers@1\" is not available)");
    let t = trace(&p, &format!("  when   model \"carriers@1\" picks Slow\n{ORDER}"));
    assert_eq!(carrier(&t), "choice: Cheap for O1/Shipment#1@Carrier by prefer (model \"carriers@1\" picked Slow, which it was not offered)");
    let t = trace(&p, &format!("  when   model \"carriers@1\" picks Fast\n{ORDER}  and    staff Sam places a new Order O2 with product Kettle, zone EU, quantity 1 and total 50 EUR\n"));
    assert!(t.contains("choice: Cheap for O2/Shipment#1@Carrier by prefer (model \"carriers@1\" declined)"), "{t}");
}

#[test]
fn a_pick_that_breaks_a_rule_is_refused_and_the_model_asked_again() {
    let p = prog("decide Stock with model \"stock@4\"\n");
    let t = trace(
        &p,
        "  when   model \"stock@4\" picks Small, then Big
  and    staff Sam places a new Order O1 with product Kettle, zone EU, quantity 2 and total 50 EUR
  expect the total quantity taken from Small is 0
  expect the total quantity taken from Big is 2
",
    );
    assert!(t.contains("choice: Small for O1/Shipment#1@Stock by model \"stock@4\" (of Big, Small)"), "{t}");
    assert!(t.contains("choice: Big for O1/Shipment#1@Stock by model \"stock@4\" (of Big)"), "{t}");
}

#[test]
fn a_registered_function_is_a_model() {
    let p = prog("decide Carrier with model \"dearest@1\"\n");
    let sc = &parse_scenarios("s.scenario", &format!("scenario \"s\":\n{WORLD}")).unwrap()[0];
    let mut r = run(&p, sc);
    r.host.models.register(
        "dearest@1",
        Box::new(|c: &suture_runtime::choice::Choice| {
            let fee = |o: &suture_runtime::choice::Option_| match o.attrs.iter().find(|(k, _)| k == "fee") {
                Some((_, suture_runtime::state::Val::Money(x))) => *x,
                _ => 0,
            };
            c.options.iter().max_by_key(|o| fee(o)).map(|o| o.label.clone())
        }),
    );
    let by = suture_lang::scenario::Caller { role: "staff".into(), name: "Sam".into(), verb: "places".into() };
    r.host
        .input_new(
            Some(&by),
            "Order",
            "O1",
            &[
                ("product".into(), suture_runtime::state::Val::Ref("Kettle".into())),
                ("zone".into(), suture_runtime::state::Val::Text("EU".into())),
                ("quantity".into(), suture_runtime::state::Val::Count(1)),
                ("total".into(), suture_runtime::state::Val::Money(5000)),
            ],
            &[],
        )
        .unwrap();
    assert_eq!(carrier(&r.host.trace.join("\n")), "choice: Fast for O1/Shipment#1@Carrier by model \"dearest@1\" (of Cheap, Fast)");
}

#[test]
fn picks_replay_without_the_model_and_only_under_the_same_model() {
    let p = prog("decide with model \"planner@3\"\n");
    let r = scenario(&p, &format!("  when   model \"planner@3\" picks Fast, then Big\n{ORDER}"));
    assert!(r.passed(), "{:?}", r.failures);
    let digest = r.host.machine.digest();
    let (kernel_program, _) = kernel::program(Arc::new(p.clone()));
    let replayed = Machine::recover(kernel_program, r.host.machine.into_journal()).unwrap();
    assert_eq!(replayed.digest(), digest);
    let again = scenario(&p, &format!("  when   model \"planner@3\" picks Fast, then Big\n{ORDER}"));
    let other = prog("decide with model \"planner@4\"\n");
    let (kernel_program, _) = kernel::program(Arc::new(other));
    assert!(matches!(Machine::recover(kernel_program, again.host.machine.into_journal()).err(), Some(suture::RecoveryError::Version { .. })));
}
