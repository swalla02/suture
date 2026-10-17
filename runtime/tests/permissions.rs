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
