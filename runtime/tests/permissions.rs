//! Slice 6: permissions (spec appendix G).

use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::run;

const SRC: &str = "module t version 1, currency EUR
role customer is a Customer
role staff
kind Country  { code: Text }
kind Customer { name: Text, country: Country }
kind Order    { customer: Customer, total: Money }
admit place a new Order by customer where the Order's customer is the caller
admit add a new Customer by staff where its country's code is \"DE\"
rule small: every Order's total is at most 100 EUR
";

fn prog() -> Program {
    suture_lang::compile(&[("t.st".to_string(), SRC.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

const WORLD: &str = "  given  a Country DE with code DE\n  and    a Country FR with code FR\n  and    a Customer Ada with name Ada and country DE\n  and    a Customer Bob with name Bob and country FR\n";

fn scenario(steps: &str) -> suture_runtime::scenario::Report {
    let src = format!("scenario \"t\":\n{WORLD}{steps}");
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&prog(), sc)
}

#[test]
fn a_caller_creates_what_it_is_admitted_to() {
    let r = scenario("  when   customer Ada places a new Order O1 with customer Ada and total 50 EUR\n  and    staff Tom adds a new Customer Cy with name Cy and country DE\n");
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(r.host.unit("O1").is_some() && r.host.unit("Cy").is_some());
    assert_eq!(r.host.gate, ["admitted: customer Ada places a new Order O1", "admitted: staff Tom adds a new Customer Cy"]);
}

#[test]
fn the_gate_refuses_and_changes_nothing() {
    let r = scenario(
        "  when   customer Bob places a new Order O1 with customer Ada and total 50 EUR\n         is refused at the gate\n\
         \x20 and    customer Bob adds a new Customer Cy with name Cy and country DE\n         is refused at the gate\n\
         \x20 and    staff Tom adds a new Customer Cy with name Cy and country FR\n         is refused at the gate\n\
         \x20 and    customer Eve places a new Order O2 with customer Ada and total 50 EUR\n         is refused at the gate\n\
         \x20 and    a new Order O3 with customer Ada and total 50 EUR\n         is refused at the gate\n\
         \x20 and    guest Gus places a new Order O4 with customer Ada and total 50 EUR\n         is refused at the gate\n",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    for l in ["O1", "Cy", "O2", "O3", "O4"] {
        assert!(r.host.unit(l).is_none(), "{l}");
    }
    let g = &r.host.gate;
    assert!(g[0].contains("customer Bob places a new Order O1: the condition of `admit place a new Order by customer where the Order's customer is the caller` does not hold"), "{g:?}");
    assert!(g[1].contains("customer Bob adds a new Customer Cy: no `admit` lets a customer do that"), "{g:?}");
    assert!(g[3].contains("there is no Customer Eve"), "{g:?}");
    assert!(g[4].ends_with("a new Order O3: an input needs a caller"), "{g:?}");
    assert!(g[5].contains("there is no role guest"), "{g:?}");
}

#[test]
fn the_gate_comes_before_the_rules() {
    let r = scenario("  when   customer Bob places a new Order O1 with customer Ada and total 500 EUR\n         is refused at the gate\n  and    customer Ada places a new Order O2 with customer Ada and total 500 EUR\n         is refused by rule small\n");
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert_eq!(r.host.gate.len(), 2);
}

#[test]
fn expecting_a_refusal_that_does_not_happen_fails() {
    let r = scenario("  when   customer Ada places a new Order O1 with customer Ada and total 50 EUR\n         is refused at the gate\n");
    assert_eq!(r.failures.len(), 1);
    assert!(r.failures[0].contains("expected O1 to be refused at the gate, but it was created"), "{:?}", r.failures);
}

const EVENTS: &str = "module t version 1, currency EUR
role customer is a Customer
role support
kind Customer { name: Text }
kind Order    { customer: Customer, total: Money, cancelled: Bool = no }
kind Payment  { order: Order }
Order needs Payment while it is not cancelled
effect capture Payment via payments within 30 seconds
  sends the order's total
  reports captured
admit cancel Order by customer where the Order's customer is the caller
admit cancel Order by support
when a customer or support cancels an Order: the Order is cancelled
rule kept: no cancelled Order has a Payment that is captured
";

fn events(steps: &str) -> suture_runtime::scenario::Report {
    let p = suture_lang::compile(&[("t.st".to_string(), EVENTS.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let src = format!("scenario \"t\":\n  given  a Customer Ada with name Ada\n  and    a Customer Bob with name Bob\n  and    a new Order O1 with customer Ada and total 50 EUR\n{steps}");
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&p, sc)
}

#[test]
fn an_admitted_event_runs_its_reaction() {
    let r = events("  when   customer Ada cancels O1\n  expect O1 is cancelled\n  and    payments answers captured\n  expect O1 is cancelled\n");
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(r.host.trace.iter().any(|l| l.contains("customer Ada cancels it")), "{}", r.host.trace.join("\n"));
}

#[test]
fn an_event_from_the_wrong_caller_is_refused_at_the_gate() {
    let r = events("  when   customer Bob cancels O1\n         is refused at the gate\n  expect O1 is not cancelled\n  when   support Sam cancels O1\n  expect O1 is cancelled\n");
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn an_event_that_would_break_a_rule_is_refused_by_it() {
    let r = events("  when   payments answers captured\n  and    support Sam cancels O1\n         is refused by rule kept\n  expect O1 is not cancelled\n  expect O1's Payment is captured\n");
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let r = events("  when   support Sam pays O1\n");
    assert!(r.failures[0].contains("no `admit` lets a support do that"), "{:?}", r.failures);
    let r = events("  when   support Sam cancels O9\n");
    assert!(r.failures[0].contains("there is no unit O9"), "{:?}", r.failures);
}

const CHANGES: &str = "module t version 1, currency EUR
role staff
role customer
kind Country  { code: Text, restricted: Bool = no }
kind Product  { name: Text }
kind Stock    { product: Product, units: Count }
kind Order    { country: Country, product: Product, quantity: Count }
kind Shipment { order: Order }
Order needs Shipment
Shipment uses a Stock where the Stock's product is the Order's product, taking the Order's quantity
prefer Stock: highest units
effect send Shipment via carriers within 7 days
  sends the order's quantity
  reports delivered
rule stock: the total quantity taken from each Stock is at most its units
rule embargo: no Order whose country is restricted has a Shipment that is delivered
admit place a new Order by customer
admit change Stock's units by staff
admit change Country's restricted by staff where its code is not \"DE\"
";

fn changes(steps: &str) -> suture_runtime::scenario::Report {
    let p = suture_lang::compile(&[("t.st".to_string(), CHANGES.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let src = format!(
        "scenario \"t\":\n  given  a Country DE with code DE\n  and    a Country FR with code FR\n  and    a Product Kettle with name Kettle\n  and    a Stock S1 with product Kettle and units 3\n{steps}"
    );
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&p, sc)
}

#[test]
fn a_change_is_checked_by_the_changed_units_rules() {
    let r = changes(
        "  when   customer Ada places a new Order O1 with country FR, product Kettle and quantity 2\n\
         \x20 and    staff Tom changes S1's units to 1\n         is refused by rule stock\n\
         \x20 expect S1's units is 3\n\
         \x20 when   staff Tom changes S1's units to 2\n\
         \x20 expect S1's units is 2\n\
         \x20 when   customer Bo changes S1's units to 9\n         is refused at the gate\n",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn raising_a_shared_entity_wakes_what_waits_for_it() {
    let r = changes(
        "  when   customer Ada places a new Order O1 with country FR, product Kettle and quantity 5\n\
         \x20 expect O1's Shipment is not delivered\n\
         \x20 when   staff Tom changes S1's units to 5\n\
         \x20 and    carriers answers delivered\n\
         \x20 expect O1's Shipment is delivered\n\
         \x20 expect the total quantity taken from S1 is 5\n",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(r.host.trace.iter().any(|l| l.contains("waits for")), "{}", r.host.trace.join("\n"));
}

#[test]
fn a_change_its_readers_cannot_take_is_refused() {
    let r = changes(
        "  when   customer Ada places a new Order O1 with country FR, product Kettle and quantity 1\n\
         \x20 and    carriers answers delivered\n\
         \x20 and    staff Tom changes FR's restricted to yes\n         is refused by rule embargo\n\
         \x20 expect FR is not restricted\n\
         \x20 when   staff Tom changes DE's restricted to yes\n         is refused at the gate\n",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(r.host.trace.iter().any(|l| l.contains("O1 would break rule embargo")), "{}", r.host.trace.join("\n"));
}

#[test]
fn readers_see_a_change_they_can_take() {
    let r = changes(
        "  when   customer Ada places a new Order O1 with country FR, product Kettle and quantity 1\n\
         \x20 and    staff Tom changes FR's restricted to yes\n\
         \x20 expect O1's country's restricted is yes\n\
         \x20 when   carriers answers delivered\n\
         \x20 expect O1's Shipment is not delivered\n",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn an_event_runs_only_the_reactions_for_the_callers_role() {
    let src = EVENTS.replace(
        "when a customer or support cancels an Order: the Order is cancelled",
        "when a customer cancels an Order: the Order is cancelled\nwhen a support cancels an Order: the Order is cancelled and the Order is flagged",
    )
    .replace("cancelled: Bool = no }", "cancelled: Bool = no, flagged: Bool = no }");
    let p = suture_lang::compile(&[("t.st".to_string(), src)]).unwrap_or_else(|e| panic!("{e:?}"));
    let steps = "scenario \"t\":\n  given  a Customer Ada with name Ada\n  and    a new Order O1 with customer Ada and total 50 EUR\n  and    a new Order O2 with customer Ada and total 50 EUR\n  when   customer Ada cancels O1\n  and    support Sam cancels O2\n  expect O1 is not flagged\n  expect O2 is flagged\n";
    let r = run(&p, &parse_scenarios("t.scenario", steps).unwrap()[0]);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn a_pool_key_never_changes_even_without_admits() {
    let mut h = suture_runtime::host::Host::new(
        suture_lang::compile(&[("t.st".to_string(), "module t version 1, currency EUR\nkind Zone { name: Text }\nkind Depot { zone: Text }\nkind Site { zone: Text }\nkind Pick { site: Site }\nSite needs Pick\nPick uses a Depot where the Depot's zone is the Site's zone\nprefer Depot: highest 1\n".to_string())]).unwrap(),
    );
    h.create("Depot", "D1", &[("zone".to_string(), suture_runtime::state::Val::Text("EU".into()))]).unwrap();
    let e = h.input_change(None, "D1", "zone", suture_runtime::state::Val::Text("US".into())).unwrap_err();
    assert!(e.contains("keys the pool"), "{e}");
}

#[test]
fn a_refusal_names_exactly_its_rule() {
    let r = events("  when   payments answers captured\n  and    support Sam cancels O1\n         is refused by rule kep\n");
    assert_eq!(r.failures.len(), 1, "rule `kep` is not rule `kept`: {:?}", r.failures);
}
