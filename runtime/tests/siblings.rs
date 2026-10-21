//! Slice 26: filtered relations across units, and units that read the
//! totals of the unit they refer to (spec appendix AA).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const RETURNS: &str = "module returns version 1, currency EUR
role staff
kind Unit { number: Text }
kind Item { unit: Unit, seq: Count = the number of its unit's Items,
            reception: awaiting | received | lost, paid: Bool = no }
define: an Item is back when it is not awaiting
define: an Item is superseded when the number of its unit's Items is more than its seq + 1
define: an Item is clean when the number of its unit's Items that are paid is 0 or it is paid
rule once: the number of each Unit's Items that are back is at most 1
admit add a new Item by staff
admit receive Item by staff where the Item is awaiting and it is not superseded
admit lose Item by staff where the Item is awaiting
admit pay Item by staff where the Item is clean
admit unpay Item by staff where the Item is paid
when a staff receives an Item: the Item is received
when a staff loses an Item: the Item is lost
when a staff pays an Item: the Item is paid
when a staff unpays an Item: the Item is not paid
";

fn scenario(src: &str, body: &str) -> Report {
    let p = suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let src = format!("scenario \"s\":\n{body}");
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&p, sc)
}

fn passes(src: &str, body: &str) -> Report {
    let r = scenario(src, body);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    r
}

#[test]
fn a_new_item_reads_how_many_came_before_it_and_supersedes_them() {
    passes(
        RETURNS,
        "  given  a Unit U1 with number \"1\"
  when   staff Sam adds a new Item I1 with unit U1
  and    staff Sam adds a new Item I2 with unit U1
  expect I1's seq is 0
  expect I2's seq is 1
  expect I1 is superseded
  expect I2 is not superseded
  when   staff Sam receives I1
           is refused at the gate
  when   staff Sam receives I2
  expect I2 is received
",
    );
}

#[test]
fn an_event_that_would_overfill_a_filtered_total_is_refused_by_its_rule() {
    let r = passes(
        RETURNS,
        "  given  a Unit U1 with number \"1\"
  and    a Unit U2 with number \"2\"
  and    a Item I1 with unit U1
  and    a Item I2 with unit U1
  and    a Item I3 with unit U2
  when   staff Sam loses I1
  and    staff Sam receives I2
           is refused by rule once
  expect I2 is awaiting
  when   staff Sam receives I3
  expect I3 is received
",
    );
    let line = r.host.trace.iter().find(|l| l.contains("would break rule once")).expect("the refusal is traced");
    assert!(line.contains("refused staff Sam receives it: U1 would break rule once with its total number of Items that are back"), "{line}");
}

#[test]
fn a_filtered_total_is_given_back_when_its_filter_stops_holding_and_readers_are_refreshed() {
    let r = passes(
        RETURNS,
        "  given  a Unit U1 with number \"1\"
  and    a Item I1 with unit U1
  and    a Item I2 with unit U1
  expect I2 is clean
  when   staff Sam pays I1
  expect I1 is clean
  expect I2 is not clean
  when   staff Sam pays I2
           is refused at the gate
  when   staff Sam unpays I1
  expect I2 is clean
  when   staff Sam pays I2
  expect I2 is paid
  expect I1 is not clean
",
    );
    let t = r.host.trace.join("\n");
    assert!(t.contains("I1  Item no longer counts toward U1's total number of Items that are paid"), "{t}");
    assert!(t.contains("U1  I1 gave back 1 number of Items that are paid"), "{t}");
    assert!(t.contains("I2  refreshed U1"), "{t}");
}

#[test]
fn a_part_counts_toward_a_filtered_total_once_it_is_done_and_an_answer_waits_for_room() {
    let src = "module refunds version 1, currency EUR
kind Customer { name: Text }
kind Order    { customer: Customer }
kind Refund   { order: Order, customer: Customer = order's customer }
Order needs Refund
effect pay Refund via bank within 1 hour
  sends its customer's name
  reports paid
rule cap: the number of each Customer's Refunds that are paid is at most 1
";
    let r = passes(
        src,
        "  given  a Customer C1 with name Ada
  when   a new Order O1 with customer C1
  and    a new Order O2 with customer C1
  and    bank answers paid for O1
  and    bank answers paid for O2
  expect O2's Refund is paid
",
    );
    let t = r.host.trace.join("\n");
    assert!(t.contains("O1  Refund counts toward C1's total number of Refunds that are paid"), "{t}");
    assert!(t.contains("C1  refused O2/Refund#1@Customer(number of Refunds that are paid): rule cap"), "{t}");
    assert!(t.contains("Refund waits for room in a Customer's total number of Refunds that are paid"), "{t}");
}

#[test]
fn an_expectation_about_a_total_the_program_does_not_keep_fails() {
    let r = scenario(
        RETURNS,
        "  given  a Unit U with number \"1\"
  when   staff S adds a new Item I1 with unit U
  expect the number of U's Items that are lost is 0
",
    );
    assert!(!r.passed());
    assert!(
        r.failures.iter().any(|f| f.contains("the program keeps no total number of Items that are lost of each Unit")),
        "{:?}",
        r.failures
    );
}
