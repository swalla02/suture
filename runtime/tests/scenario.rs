use suture_lang::ir::Program;
use suture_lang::scenario::{parse_scenarios, Scenario};
use suture_runtime::scenario::run;

fn shop() -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop");
    suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"))
}

fn scenarios() -> Vec<Scenario> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop/orders.scenario");
    parse_scenarios("orders.scenario", &std::fs::read_to_string(path).unwrap()).unwrap()
}

#[test]
fn the_shop_scenarios_pass() {
    let p = shop();
    let all = scenarios();
    assert_eq!(all.len(), 8);
    for sc in all {
        let r = run(&p, &sc);
        assert!(r.passed(), "{}: {:?}\n{}", r.title, r.failures, r.host.trace.join("\n"));
    }
}

#[test]
fn the_japanese_order_never_sees_a_form() {
    let p = shop();
    let sc = scenarios().into_iter().find(|s| s.title.contains("Japanese")).unwrap();
    let r = run(&p, &sc);
    let unit = r.host.unit("J1").unwrap();
    assert!(unit.root.parts.iter().all(|n| n.kind != "CustomsForm"));
    assert!(r.host.trace.iter().all(|l| !l.contains("forms")), "{:?}", r.host.trace);
}

#[test]
fn impossible_scenario_steps_fail_the_report() {
    let p = shop();
    let src = "scenario \"broken\":\n  when   payments answers captured\n  expect X1 is paid\n  and    a new Payment P1\n";
    let sc = &parse_scenarios("b.scenario", src).unwrap()[0];
    let r = run(&p, sc);
    assert_eq!(r.failures.len(), 3, "{:?}", r.failures);
    assert!(r.failures[0].contains("payments has no pending request"), "{:?}", r.failures);
    assert!(r.failures[1].contains("there is no unit X1"), "{:?}", r.failures);
    assert!(r.failures[2].contains("is a part of Order"), "{:?}", r.failures);
}

#[test]
fn expectations_name_real_states() {
    let p = shop();
    let src = "scenario \"typos\":\n  when   a new Order D1 with country Japan and total 50 EUR\n  expect D1 is not payed\n  expect D1 is setled\n";
    let sc = &parse_scenarios("t.scenario", src).unwrap()[0];
    let r = run(&p, sc);
    assert_eq!(r.failures.len(), 2, "{:?}", r.failures);
    assert!(r.failures[0].contains("Order has no state payed"), "{:?}", r.failures);
    assert!(r.failures[1].contains("Order has no state setled"), "{:?}", r.failures);
}

#[test]
fn time_passes_in_scenarios() {
    let p = shop();
    let src = "scenario \"silent payments\":\n  when   a new Order J1 with country Japan and total 80 EUR\n  and    90 seconds pass\n  expect J1's Payment is timed out\n  expect no alert for J1\n  when   3 days pass\n  expect an alert for J1\n  expect J1 is not settled\n";
    let sc = &parse_scenarios("t.scenario", src).unwrap()[0];
    let r = run(&p, sc);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn expectations_take_any_condition() {
    let p = shop();
    let src = "scenario \"counts\":\n  when   a new Order P1 with country Japan and total 90 EUR\n  and    payments answers captured\n  and    carriers answers lost\n  and    carriers answers lost\n  expect the number of P1's Shipments is 3\n  expect the number of P1's Shipments that are lost is 2\n  expect P1's total is at most 90 EUR\n  expect P1's total is more than 90 EUR\n";
    let sc = &parse_scenarios("t.scenario", src).unwrap()[0];
    let r = run(&p, sc);
    assert_eq!(r.failures.len(), 1, "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(r.failures[0].contains("expected P1's total is more than 90 EUR"), "{:?}", r.failures);
}

#[test]
fn the_workspace_scenarios_pass() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/workspaces");
    let p = suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"));
    let src = std::fs::read_to_string(format!("{dir}/workspaces.scenario")).unwrap();
    let all = parse_scenarios("workspaces.scenario", &src).unwrap();
    assert_eq!(all.len(), 3);
    for sc in all {
        let r = run(&p, &sc);
        assert!(r.passed(), "{}: {:?}\n{}", r.title, r.failures, r.host.trace.join("\n"));
    }
}
