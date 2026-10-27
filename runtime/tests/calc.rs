//! Slice 64: calculations that decide. Conditionals, the larger or smaller
//! of two values, defined values, formulas known late, and 64 KiB units
//! (spec appendix BO).

use suture_runtime::host::Host;
use suture_runtime::state::Val;

fn compile(src: &str) -> suture_lang::ir::Program {
    suture_lang::compile(&[("calc.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{:?}", e.iter().map(|e| &e.msg).collect::<Vec<_>>()))
}

fn compile_err(src: &str) -> String {
    match suture_lang::compile(&[("calc.st".to_string(), src.to_string())]) {
        Ok(_) => panic!("compiled, but should not have"),
        Err(e) => e.iter().map(|e| e.msg.clone()).collect::<Vec<_>>().join("\n"),
    }
}

fn attrs(xs: &[(&str, Val)]) -> Vec<(String, Val)> {
    xs.iter().map(|(n, v)| (n.to_string(), v.clone())).collect()
}

fn attr(h: &Host, label: &str, name: &str) -> Option<Val> {
    h.unit(label).unwrap().root.attrs.get(name).cloned()
}

const RATES: &str = "module bags version 1, currency USD
role clerk
kind Rate { nth: Count, amount: Money }
kind Bag { nth: Count, fee: Money = its Base's Rate's amount }
kind Base { bag: Bag }
Bag needs Base
Base uses a Rate where the Rate's nth is the Bag's nth
admit add a new Rate or Bag by clerk
";

#[test]
fn a_formula_that_reads_a_grant_is_set_when_the_grant_comes() {
    let mut h = Host::new(compile(RATES));
    h.create("Rate", "R1", &attrs(&[("nth", Val::Count(1)), ("amount", Val::Money(3500))])).unwrap();
    h.create("Rate", "R2", &attrs(&[("nth", Val::Count(2)), ("amount", Val::Money(4500))])).unwrap();
    h.create("Bag", "B1", &attrs(&[("nth", Val::Count(1))])).unwrap();
    h.create("Bag", "B2", &attrs(&[("nth", Val::Count(2))])).unwrap();
    assert_eq!(attr(&h, "B1", "fee"), Some(Val::Money(3500)));
    assert_eq!(attr(&h, "B2", "fee"), Some(Val::Money(4500)));
    // A bag with no rate yet waits, and its fee is set once one comes.
    h.create("Bag", "B3", &attrs(&[("nth", Val::Count(3))])).unwrap();
    assert_eq!(attr(&h, "B3", "fee"), None);
    h.create("Rate", "R3", &attrs(&[("nth", Val::Count(3)), ("amount", Val::Money(15000))])).unwrap();
    assert_eq!(attr(&h, "B3", "fee"), Some(Val::Money(15000)));
}

const NOTES: &str = "module notes version 1, currency USD
role clerk
kind Note { text: Text }
admit add a new Note by clerk
";

#[test]
fn a_unit_holds_64_kib() {
    let mut h = Host::new(compile(NOTES));
    h.create("Note", "N1", &attrs(&[("text", Val::Text("x".repeat(10_000)))])).unwrap();
    let err = h.create("Note", "N2", &attrs(&[("text", Val::Text("x".repeat(70_000)))])).unwrap_err();
    assert!(err.contains("at most 65536"), "{err}");
}

const FEES: &str = "module fees version 1, currency USD
role clerk
kind Bag {
  size: Count,
  weight: Count,
  oversize: Money = 0 USD if its size is at most 62, otherwise 30 USD if its size is at most 65, otherwise 200 USD,
  overweight: Money = 0 USD if its weight is at most 50, otherwise (100 USD if its weight is at most 70, otherwise 200 USD),
  extra: Money = the larger of its oversize and its overweight,
  least: Money = the smaller of its oversize and its overweight,
  far: Bool = yes if its size is more than 100 or its weight is more than 90, otherwise no
}
admit add a new Bag by clerk
";

fn bag(size: i64, weight: i64) -> Host {
    let mut h = Host::new(compile(FEES));
    h.create("Bag", "B", &attrs(&[("size", Val::Count(size)), ("weight", Val::Count(weight))])).unwrap();
    h
}

#[test]
fn a_chained_conditional_takes_the_first_branch_that_holds() {
    for (size, fee) in [(40, 0), (62, 0), (63, 3000), (65, 3000), (66, 20000)] {
        assert_eq!(attr(&bag(size, 10), "B", "oversize"), Some(Val::Money(fee)), "size {size}");
    }
    for (weight, fee) in [(50, 0), (51, 10000), (70, 10000), (71, 20000)] {
        assert_eq!(attr(&bag(10, weight), "B", "overweight"), Some(Val::Money(fee)), "weight {weight}");
    }
    assert_eq!(attr(&bag(120, 10), "B", "far"), Some(Val::Bool(true)));
    assert_eq!(attr(&bag(10, 10), "B", "far"), Some(Val::Bool(false)));
}

#[test]
fn larger_and_smaller_pick_one_of_two() {
    let h = bag(63, 60);
    assert_eq!(attr(&h, "B", "extra"), Some(Val::Money(10000)));
    assert_eq!(attr(&h, "B", "least"), Some(Val::Money(3000)));
}

#[test]
fn an_unknown_condition_leaves_the_value_unknown() {
    let src = RATES.replace("fee: Money = its Base's Rate's amount", "fee: Money = 1 USD if its Base's Rate's amount is more than 40 USD, otherwise 2 USD");
    let mut h = Host::new(compile(&src));
    h.create("Bag", "B1", &attrs(&[("nth", Val::Count(1))])).unwrap();
    assert_eq!(attr(&h, "B1", "fee"), None, "no rate yet, so the condition is unknown");
    h.create("Rate", "R1", &attrs(&[("nth", Val::Count(1)), ("amount", Val::Money(4500))])).unwrap();
    assert_eq!(attr(&h, "B1", "fee"), Some(Val::Money(100)));
}

#[test]
fn branches_and_sides_must_agree_and_a_conditional_needs_otherwise() {
    let kind = |formula: &str| format!("module m version 1, currency USD\nkind Bag {{ size: Count, fee: Money = {formula} }}\n");
    let e = compile_err(&kind("1 USD if its size is at most 3, otherwise 2"));
    assert!(e.contains("Money") && e.contains("Count"), "{e}");
    let e = compile_err(&kind("1 USD if its size is at most 3"));
    assert!(e.contains("otherwise"), "{e}");
    let e = compile_err(&kind("the larger of its size and 3 USD"));
    assert!(e.contains("Money") && e.contains("Count"), "{e}");
    let e = compile_err(&kind("the larger of \"a\" and \"b\""));
    assert!(e.contains("amounts, counts, durations or times"), "{e}");
}

#[test]
fn a_formula_follows_no_state_and_a_cycle_through_a_condition_is_found() {
    let e = compile_err("module m version 1, currency USD
kind Bag { size: Count, fee: Money = 1 USD if it is done, otherwise 2 USD }
");
    assert!(e.contains("whether it is done, which changes") && e.contains("define: a Bag's x is"), "{e}");
    let e = compile_err("module m version 1, currency USD\nkind Bag { a: Count = 1 if its b is 2, otherwise 0, b: Count = its a }\n");
    assert!(e.contains("depend on each other"), "{e}");
}

#[test]
fn a_rule_reads_a_conditional_and_says_it_back() {
    let src = "module m version 1, currency USD
role clerk
kind Bag { size: Count, fee: Money }
rule capped: every Bag's fee is at most (100 USD if its size is at most 62, otherwise the larger of 150 USD and (its size * 2 USD))
admit add a new Bag by clerk
";
    let prog = compile(src);
    let rule = prog.rules.iter().find(|r| r.name == "capped").unwrap();
    let said = suture_lang::readback::paraphrase(&prog, rule);
    assert!(said.contains("100 USD if its size is at most 62, otherwise the larger of 150 USD and (its size × 2 USD)"), "{said}");
    let formula = suture_lang::readback::formula(&prog, rule);
    assert!(formula.contains("max(150 USD, "), "{formula}");
    let mut h = Host::new(prog);
    h.create("Bag", "B1", &attrs(&[("size", Val::Count(60)), ("fee", Val::Money(10000))])).unwrap();
    h.create("Bag", "B2", &attrs(&[("size", Val::Count(100)), ("fee", Val::Money(20000))])).unwrap();
    assert!(h.create("Bag", "B3", &attrs(&[("size", Val::Count(60)), ("fee", Val::Money(10001))])).is_err());
    assert!(h.create("Bag", "B4", &attrs(&[("size", Val::Count(100)), ("fee", Val::Money(20001))])).is_err());
}

const LIVE: &str = "module live version 1, currency USD
role clerk
kind Rate { nth: Count, amount: Money }
kind Bag { nth: Count, weight: Count }
kind Base { bag: Bag }
Bag needs Base
Base uses a Rate where the Rate's nth is the Bag's nth
define: a Bag is heavy when its weight is more than 50
define: a Bag's surcharge is 100 USD if it is heavy, otherwise 0 USD
define: a Bag's fee is its Base's Rate's amount + its surcharge
define: a Bag's charge is its weight * 2 USD
rule light: every Bag's charge is at most 200 USD
admit add a new Rate or Bag by clerk
admit change Bag's weight by clerk
";

#[test]
fn a_defined_value_follows_a_grant_and_a_state() {
    let mut h = Host::new(compile(LIVE));
    h.create("Bag", "B1", &attrs(&[("nth", Val::Count(1)), ("weight", Val::Count(40))])).unwrap();
    assert_eq!(h.value("B1", &[], "surcharge"), Some(Val::Money(0)));
    assert_eq!(h.value("B1", &[], "fee"), None, "no rate yet");
    h.create("Rate", "R1", &attrs(&[("nth", Val::Count(1)), ("amount", Val::Money(3500))])).unwrap();
    assert_eq!(h.value("B1", &[], "fee"), Some(Val::Money(3500)));
    let clerk = suture_lang::scenario::Caller { role: "clerk".into(), name: "Cy".into(), verb: "changes".into() };
    h.input_change(Some(&clerk), "B1", "weight", Val::Count(60)).unwrap();
    assert_eq!(h.value("B1", &[], "fee"), Some(Val::Money(13500)), "heavy now, so the value follows");
    assert_eq!(h.value("B1", &[], "nothing"), None);
    assert!(!h.unit("B1").unwrap().root.attrs.contains_key("fee"), "a defined value is never stored");
}

#[test]
fn a_rule_reads_a_defined_value() {
    let mut h = Host::new(compile(LIVE));
    h.create("Bag", "B1", &attrs(&[("nth", Val::Count(1)), ("weight", Val::Count(100))])).unwrap();
    assert!(h.create("Bag", "B2", &attrs(&[("nth", Val::Count(1)), ("weight", Val::Count(101))])).is_err());
}

#[test]
fn a_defined_value_is_read_in_its_own_unit_and_only_from_above() {
    let e = compile_err(&format!("{LIVE}kind Tag {{ bag: Bag }}\nrule tagged: every Tag's bag's charge is at most 3 USD\n"));
    assert!(e.contains("is a defined value, computed in the Bag's own unit"), "{e}");
    let e = compile_err("module m version 1, currency USD\nkind Bag { size: Count }\ndefine: a Bag's a is its b + 1\ndefine: a Bag's b is its size\n");
    assert!(e.contains("`b`"), "{e}");
    let e = compile_err("module m version 1, currency USD\nkind Bag { size: Count }\ndefine: a Bag's size is 3\n");
    assert!(e.contains("already has an attribute, a part or a value called `size`"), "{e}");
}

#[test]
fn a_scenario_expects_a_defined_value() {
    let src = "scenario \"s\":
  given a Rate R1 with nth 1 and amount 35 USD
  given a Bag B1 with nth 1 and weight 60
  expect B1's fee is 135 USD
  expect B1's surcharge is more than 50 USD
";
    let sc = &suture_lang::scenario::parse_scenarios("s.scenario", src).unwrap_or_else(|e| panic!("{e}"))[0];
    let r = suture_runtime::scenario::run(&compile(LIVE), sc);
    assert!(r.passed(), "{:?}", r.failures);
}

#[test]
fn a_defined_value_and_a_conditional_are_read_in_a_total_over_a_list() {
    let src = "module m version 1, currency USD
kind Order { vip: Bool, lines: list of Line, cap: Money }
kind Line { order: Order, price: Money }
define: an Order's rate is 2 if its vip is yes, otherwise 1
rule capped: every Order's cap is at least the total of the line's price * its rate over its lines
rule plain: every Order's cap is at least the total of (the line's price if its vip is yes, otherwise 0 USD) over its lines
";
    compile(src);
}

#[test]
fn a_pool_key_that_reads_the_candidate_in_a_conditional_is_no_key() {
    let src = RATES.replace(
        "Base uses a Rate where the Rate's nth is the Bag's nth",
        "Base uses a Rate where the Rate's nth is (1 if the Rate's amount is more than 50 USD, otherwise 2) and the Rate's nth is the Bag's nth",
    );
    let mut h = Host::new(compile(&src));
    h.create("Rate", "R1", &attrs(&[("nth", Val::Count(2)), ("amount", Val::Money(3500))])).unwrap();
    h.create("Bag", "B1", &attrs(&[("nth", Val::Count(2))])).unwrap();
    assert_eq!(attr(&h, "B1", "fee"), Some(Val::Money(3500)));
}

#[test]
fn a_formula_reads_no_part_that_may_be_withdrawn_or_replaced() {
    let e = compile_err("module m version 1, currency USD
kind Order { big: Bool, cost: Money = its Shipment's price }
kind Shipment { order: Order, price: Money }
Order needs Shipment while it is big
");
    assert!(e.contains("Shipment") && e.contains("define"), "{e}");
}

#[test]
fn a_formula_that_would_break_a_rule_stays_unknown() {
    let src = format!("{RATES}rule cheap: every Bag's fee is at most 100 USD\n");
    let mut h = Host::new(compile(&src));
    h.create("Rate", "R1", &attrs(&[("nth", Val::Count(1)), ("amount", Val::Money(15000))])).unwrap();
    h.create("Bag", "B1", &attrs(&[("nth", Val::Count(1))])).unwrap();
    assert!(h.machine.faults().is_empty(), "{:?}", h.machine.faults());
    assert_eq!(attr(&h, "B1", "fee"), None);
}

#[test]
fn a_value_at_no_part_is_none() {
    let mut h = Host::new(compile(LIVE));
    h.create("Bag", "B1", &attrs(&[("nth", Val::Count(1)), ("weight", Val::Count(40))])).unwrap();
    assert_eq!(h.value("B1", &[5], "surcharge"), None);
}

#[test]
fn a_value_where_none_is_read_says_why() {
    let e = compile_err(&LIVE.replace("rule light:", "prefer Bag: highest fee\nrule light:").replace("kind Base { bag: Bag }", "kind Base { bag: Bag }\nkind Tote { trip: Count }\nkind Pick { tote: Tote }\nTote needs Pick\nPick uses a Bag where the Bag's nth is the Tote's trip"));
    assert!(e.contains("a Bag's fee is a defined value, read only in its own unit"), "{e}");
    let e = compile_err("module m version 1, currency USD\nkind Bag { size: Count, twice: Count = its double }\ndefine: a Bag's double is its size * 2\n");
    assert!(e.contains("a Bag's double is a defined value"), "{e}");
}
