//! Slice 31: formulas over values a source reads once (spec appendix AG).

use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const CLAIM: &str = "module claims version 1, currency EUR
role staff
kind Policy { name: Text }
kind Claim  { policy: Policy, note: Text, amount: Money, payout: Money = amount - 100 EUR }
kind Payout { claim: Claim }
Claim needs Payout
effect pay Payout via bank within 1 minute
  sends    the claim's payout
  marks    sent
  reports  paid
source Claim's amount via model \"reader@1\" within 5 seconds
  sends      its note
  otherwise  ask again after 1 hour
rule yearly: for each Policy and year, the total payout of their Claims is at most 2000 EUR
define: a Claim is paid when its Payout is done
desired within 14 days: every Claim is paid
admit file a new Claim by staff
";

fn prog(src: &str) -> Program {
    let files = vec![("claims.st".to_string(), src.to_string())];
    suture_lang::compile(&files).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn errors(src: &str) -> Vec<String> {
    let files = vec![("claims.st".to_string(), src.to_string())];
    suture_lang::compile(&files).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

fn scenario(p: &Program, steps: &str) -> Report {
    let src = format!("scenario \"s\":\n{steps}");
    let sc = &parse_scenarios("s.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(p, sc)
}

fn passes(p: &Program, steps: &str) -> Report {
    let r = scenario(p, steps);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    r
}

fn has(trace: &[String], line: &str) -> bool {
    trace.iter().any(|l| l.contains(line))
}

#[test]
fn a_formula_over_a_read_value_is_computed_once_it_is_read_and_counted() {
    let r = passes(
        &prog(CLAIM),
        "  given  a Policy P1 with name Ann
  when   model \"reader@1\" reads 1200 EUR, then 1500 EUR
  and    staff Sam files a new Claim C1 with policy P1 and note \"door\"
  expect C1's payout is 1100 EUR
  expect C1's Payout is sent
  when   staff Sam files a new Claim C2 with policy P1 and note \"roof\"
  expect C2's payout is 1400 EUR
  expect C2's Payout is not sent
",
    );
    let t = &r.host.trace;
    assert!(has(t, "C1  amount: for a Policy's total payout of Claims per year, asking model \"reader@1\" to read it (note \"door\")"), "{}", t.join("\n"));
    assert!(has(t, "P1  granted C1/@Policy(payout of Claims per year): 1100 EUR"), "{}", t.join("\n"));
    assert!(has(t, "C2  Claim waits for room in a Policy's total payout of Claims per year"), "{}", t.join("\n"));
}

#[test]
fn a_value_not_read_yet_is_named_and_asked_for_again_without_end() {
    let p = prog(CLAIM);
    // Fifteen days of hourly asks: the unit keeps one timer for the value,
    // not one per ask, and stays within its size.
    let r = passes(
        &p,
        "  given  a Policy P1 with name Ann
  when   model \"reader@1\" reads nothing
  and    staff Sam files a new Claim C1 with policy P1 and note \"door\"
  and    15 days pass
  expect C1's Payout is not sent
  expect an alert for C1
",
    );
    let t = &r.host.trace;
    assert!(has(t, "C1    because Claim waits for its amount, to count toward a Policy's total payout of Claims per year"), "{}", t.join("\n"));
    let u = r.host.unit("C1").unwrap();
    assert_eq!(u.timers.iter().filter(|(id, _)| id.contains("~amount#")).count(), 1, "{:?}", u.timers);
    let lines = suture_runtime::explain::explain(&p, &u, |_| None);
    assert!(lines.iter().any(|l| l == "  Claim waits for its amount, to count toward a Policy's total payout of Claims per year"), "{}", lines.join("\n"));
}

#[test]
fn a_rule_over_a_value_read_once_moves_the_slot_on_once_it_is_read() {
    let src = "module claims version 1, currency EUR
role staff
kind Claim    { note: Text, amount: Money, payout: Money = amount - 100 EUR }
kind Writeoff { claim: Claim }
kind Repair   { claim: Claim }
Claim needs Outcome
Outcome is Writeoff or Repair
effect pay Writeoff via bank within 1 minute
  sends    the claim's payout
  marks    sent
  reports  paid
effect book Repair via garages within 1 minute
  sends    the claim's note
  marks    sent
  reports  booked
source Claim's amount via model \"reader@1\" within 5 seconds
  sends      its note
  otherwise  ask again after 1 hour
rule limit: no Claim has a Writeoff that is sent unless its payout is at most 3000 EUR
admit file a new Claim by staff
";
    let p = prog(src);
    assert!(p.rules[0].fixed, "a value read once is a fixed fact");
    // Not read yet, the rule cannot refuse for good: the Writeoff waits.
    let r = passes(
        &p,
        "  when   model \"reader@1\" reads nothing, then 4300 EUR
  and    staff Sam files a new Claim C1 with note \"roof\"
  expect C1's Writeoff is not sent
  expect C1 has no Repair
  when   1 hour passes
  expect C1's payout is 4200 EUR
  expect C1 has no Writeoff
  expect C1's Repair is sent
",
    );
    assert!(has(&r.host.trace, "C1  Writeoff waits: rule limit"), "{}", r.host.trace.join("\n"));
    passes(
        &p,
        "  when   model \"reader@1\" reads 2000 EUR
  and    staff Sam files a new Claim C1 with note \"roof\"
  expect C1's Writeoff is sent
",
    );
}

#[test]
fn a_formula_reads_only_a_value_read_once() {
    let stale = CLAIM.replace("  otherwise  ask again after 1 hour", "  good for   1 day\n  otherwise  ask again after 1 hour");
    assert_eq!(
        errors(&stale),
        ["claims.st:4: Claim.payout reads amount, which its source reads again every 1 day; a formula is computed once, so it reads only a source with no `good for`"]
    );
    let fresh = CLAIM.replace(
        "rule yearly:",
        "rule small: no Claim has a Payout that is sent unless its amount is at most 5000 EUR, with amount at most 1 day old\nrule yearly:",
    );
    assert_eq!(
        errors(&fresh),
        ["claims.st:14: rule small: `with amount at most 1 day old`: the formula of Claim's payout reads amount once, so it is not read again"]
    );
}

#[test]
fn a_filtered_total_keeps_its_entity_waiting_until_the_amount_is_read() {
    let src = CLAIM
        .replace("note: Text, amount", "note: Text, open: Bool = yes, amount")
        .replace("the total payout of their Claims is at most", "the total payout of their Claims that are open is at most");
    passes(
        &prog(&src),
        "  given  a Policy P1 with name Ann
  when   model \"reader@1\" reads 1500 EUR
  and    staff Sam files a new Claim C1 with policy P1 and note \"door\"
  expect C1's Payout is sent
  when   model \"reader@1\" is late
  and    staff Sam files a new Claim C2 with policy P1 and note \"roof\"
  expect C2's Payout is not sent
  when   model \"reader@1\" reads 1500 EUR
  and    1 hour passes
  expect C2's payout is 1400 EUR
  expect C2's Payout is not sent
",
    );
}
