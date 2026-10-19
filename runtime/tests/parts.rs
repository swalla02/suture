//! Slice 17: goals about parts, and events sent to parts (spec appendix R).

use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::run;

const SRC: &str = "module t version 1, currency EUR
role customer is a Customer
role courier
kind Customer { name: Text }
kind Order    { customer: Customer, express: Bool = no, cancelled: Bool = no }
kind Shipment { order: Order, delivered: Bool = no, note: Text = \"\" }
Order needs Shipment while it is not cancelled
admit deliver Shipment by courier with remark: Text where it is not delivered
admit cancel Order by customer where the Order's customer is the caller
when a courier delivers a Shipment: it is delivered and its note becomes the remark
when a customer cancels an Order: it is cancelled
desired within 2 days: every Shipment whose order is express is delivered
rule late: no Order that is cancelled has a Shipment that is delivered
";

fn prog() -> Program {
    suture_lang::compile(&[("t.st".to_string(), SRC.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn scenario(steps: &str) -> suture_runtime::scenario::Report {
    let src = format!(
        "scenario \"t\":\n  given  a Customer Ada with name Ada\n  and    an Order O1 with customer Ada and express yes\n  and    an Order O2 with customer Ada\n{steps}"
    );
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&prog(), sc)
}

#[test]
fn an_event_goes_to_the_part_and_meets_a_goal_about_parts() {
    let r = scenario("  when   courier Cy delivers O1's Shipment with remark \"left at the door\"\n  and    3 days pass\n  expect no alert for O1\n  expect no alert for O2\n");
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let trace = r.host.trace.join("\n");
    assert!(trace.contains("O1  courier Cy delivers its Shipment with remark \"left at the door\""), "{trace}");
    assert!(trace.contains("O1  goal met: every Shipment whose order is express is delivered"), "{trace}");
    assert!(!trace.contains("O2  goal met"), "the goal does not apply to O2: {trace}");
    let u = r.host.unit("O1").unwrap();
    assert_eq!(u.root.parts[0].attrs["note"], suture_runtime::state::Val::Text("left at the door".into()));
}

#[test]
fn a_goal_about_parts_alerts_when_a_part_is_late() {
    let r = scenario("  when   3 days pass\n  expect an alert for O1\n  expect no alert for O2\n");
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let explained = suture_runtime::explain::explain(&prog(), &r.host.unit("O1").unwrap(), |_| None).join("\n");
    assert!(explained.contains("goal late, alert raised: every Shipment whose order is express is delivered (not yet delivered)"), "{explained}");
}

#[test]
fn the_gate_reads_the_part_and_a_rule_still_refuses() {
    let r = scenario(
        "  when   courier Cy delivers O1's Shipment with remark a\n\
         \x20 and    courier Cy delivers O1's Shipment with remark b\n         is refused at the gate\n\
         \x20 and    courier Cy delivers O1 with remark c\n         is refused at the gate\n\
         \x20 and    customer Ada cancels O2\n\
         \x20 and    courier Cy delivers O2's Shipment with remark d\n         is refused by rule late\n",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let gate = r.host.gate.join("\n");
    assert!(gate.contains("courier Cy delivers O1's Shipment: the condition of `admit deliver Shipment"), "{gate}");
}
