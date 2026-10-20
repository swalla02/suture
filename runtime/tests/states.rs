//! Slice 25: state attributes (spec appendix Z).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const LIBRARY: &str = "module library version 1, currency EUR
role staff
kind Shelf { name: Text, access: open | closed }
kind Copy  { shelf: Shelf, condition: shelved | lent | lost | mending }
kind Loan  { copy: Copy, shelf: Shelf, state: open | closed, back: Bool = no }
define: a Copy is out when it is lent or it is lost
admit lend Copy by staff where the Copy is shelved
admit lose Copy by staff where the Copy is lent
admit return Copy by staff where the Copy is out
admit change Copy's condition by staff
admit change Shelf's access by staff
admit add a new Shelf, Copy or Loan by staff
when a staff lends a Copy: the Copy is lent
when a staff loses a Copy: the Copy is lost
when a staff returns a Copy: the Copy is shelved
rule reachable: every Loan's shelf is open
";

fn scenario(body: &str) -> Report {
    let p = suture_lang::compile(&[("t.st".to_string(), LIBRARY.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let src = format!("scenario \"s\":\n  given  a Shelf S1 with name Main\n{body}");
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&p, sc)
}

fn passes(body: &str) {
    let r = scenario(body);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn a_reaction_moves_a_state_attribute_from_one_state_to_the_next() {
    passes(
        "  when   staff Sam adds a new Copy C1 with shelf S1
  expect C1 is shelved
  expect C1 is not out
  when   staff Sam lends C1
  expect C1 is lent
  expect C1 is not shelved
  expect C1 is out
  expect C1's condition is \"lent\"
  when   staff Sam loses C1
  expect C1 is lost
  expect C1 is not lent
  when   staff Sam lends C1
           is refused at the gate
  when   staff Sam returns C1
  expect C1 is shelved
",
    );
}

#[test]
fn a_value_that_is_not_a_state_is_refused() {
    passes(
        "  when   staff Sam adds a new Copy C1 with shelf S1 and condition mending
  expect C1 is mending
  when   staff Sam changes C1's condition to shelved
  expect C1 is shelved
",
    );
    for bad in ["  when   staff Sam adds a new Copy C1 with shelf S1 and condition burnt\n", "  given  a Copy C1 with shelf S1\n  when   staff Sam changes C1's condition to burnt\n"] {
        let r = scenario(bad);
        assert!(r.failures.iter().any(|f| f.contains("Copy.condition is one of shelved, lent, lost, mending, not burnt")), "{:?}", r.failures);
    }
}

#[test]
fn a_shared_entitys_state_is_read_through_a_reference() {
    passes(
        "  given  a Copy C1 with shelf S1
  when   staff Sam adds a new Loan L1 with copy C1 and shelf S1
  expect L1 is open
  when   staff Sam changes S1's access to closed
           is refused by rule reachable
  when   staff Sam adds a new Shelf S2 with name Back and access closed
  and    staff Sam adds a new Loan L2 with copy C1 and shelf S2
           is refused by rule reachable
",
    );
}

#[test]
fn a_rule_that_reads_a_state_another_unit_changes_by_reaction_is_an_error() {
    let src = format!("{LIBRARY}rule kept: every Loan's copy is not lost\n");
    let e = suture_lang::compile(&[("t.st".to_string(), src)]).err().unwrap_or_default();
    assert!(e.iter().any(|e| e.msg.contains("rule kept: it reads `condition` from another unit")), "{e:?}");
}
