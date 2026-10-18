//! Slice 8: sourcing (spec appendix I).

use std::sync::Arc;

use suture::Machine;
use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::kernel;
use suture_runtime::scenario::{run, Report};

const CREDIT: &str = "module credit version 1, currency EUR
role staff
kind Customer { name: Text }
kind Order    { customer: Customer, total: Money, credit: Money, approved: Bool = no, cancelled: Bool = no }
kind Payment  { order: Order }
Order needs Payment while it is not cancelled
effect capture Payment via payments within 30 seconds
  sends    the order's total
  marks    sent
  reports  captured
source Order's credit via bureau within 2 seconds
  sends      the customer's name
  reports    found | fails with unknown
  good for   1 day
  otherwise  use 0 EUR
rule credit:   no Order has a Payment that is sent unless its total is at most its credit,
               with credit at most 1 hour old
rule approved: no Order has a Payment that is sent unless it is approved
desired within 1 day: every Order that is not cancelled is paid
define: an Order is paid when its Payment is captured
admit place a new Order by staff
admit change Order's approved by staff
";

fn prog(src: &str) -> Program {
    let files = vec![("credit.st".to_string(), src.to_string())];
    suture_lang::compile(&files).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn scenario(p: &Program, steps: &str) -> Report {
    let src = format!("scenario \"s\":\n  given  a Customer Ada with name Ada\n{steps}");
    let sc = &parse_scenarios("s.scenario", &src).unwrap()[0];
    run(p, sc)
}

fn passes(p: &Program, steps: &str) -> Vec<String> {
    let r = scenario(p, steps);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    r.host.trace.clone()
}

fn has(trace: &[String], line: &str) -> bool {
    trace.iter().any(|l| l.contains(line))
}

#[test]
fn credit_is_fetched_only_when_a_step_needs_it() {
    let p = prog(CREDIT);
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  expect O1's Payment is not sent
  when   staff Sam changes O1's approved to yes
  and    bureau answers found with credit 100 EUR
  expect O1's Payment is sent
  expect O1's credit is 100 EUR
",
    );
    // The credit rule comes first, so the payment waits on it and fetches.
    assert!(has(&t, "O1  credit: asking bureau (name \"Ada\")"), "{}", t.join("\n"));
    assert!(has(&t, "O1  credit: 100 EUR from bureau, trusted for 1 day"), "{}", t.join("\n"));
    assert!(has(&t, "O1  Payment → capture via payments"), "{}", t.join("\n"));
}

#[test]
fn nothing_is_fetched_for_a_rule_that_holds_nothing_back() {
    let unordered = CREDIT.replace(
        "rule credit:   no Order has a Payment that is sent unless its total is at most its credit,
               with credit at most 1 hour old
rule approved: no Order has a Payment that is sent unless it is approved",
        "rule approved: no Order has a Payment that is sent unless it is approved
rule credit:   no Order has a Payment that is sent unless its total is at most its credit,
               with credit at most 1 hour old",
    );
    let t = passes(&prog(&unordered), "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR\n");
    assert!(has(&t, "Payment waits: rule approved"));
    assert!(!has(&t, "asking bureau"), "{}", t.join("\n"));
}

#[test]
fn a_low_credit_holds_the_payment() {
    let p = prog(CREDIT);
    passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 500 EUR
  and    staff Sam changes O1's approved to yes
  and    bureau answers found with credit 100 EUR
  expect O1's Payment is not sent
",
    );
}

#[test]
fn a_credit_too_old_for_the_rule_is_fetched_again() {
    let p = prog(CREDIT);
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    bureau answers found with credit 100 EUR
  expect O1's Payment is not sent
  when   2 hours pass
  and    staff Sam changes O1's approved to yes
  expect O1's Payment is not sent
  when   bureau answers found with credit 80 EUR
  expect O1's Payment is sent
",
    );
    assert!(has(&t, "O1  credit: older than 1 hour for rule credit: asking bureau again (name \"Ada\")"), "{}", t.join("\n"));
}

#[test]
fn an_expired_credit_is_fetched_again_while_a_step_waits_on_it() {
    let p = prog(CREDIT);
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 500 EUR
  and    staff Sam changes O1's approved to yes
  and    bureau answers found with credit 100 EUR
  and    1 day passes
  and    bureau answers found with credit 900 EUR
  expect O1's Payment is sent
",
    );
    assert!(has(&t, "asking bureau again"), "{}", t.join("\n"));
}

#[test]
fn a_failed_fetch_falls_back() {
    let p = prog(CREDIT);
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    bureau answers unknown
  expect O1's credit is 0 EUR
  expect O1's Payment is not sent
",
    );
    assert!(has(&t, "O1  credit: bureau failed (unknown): using 0 EUR instead"), "{}", t.join("\n"));
}

#[test]
fn a_silent_bureau_is_asked_again_then_fails() {
    let p = prog(CREDIT);
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    6 seconds pass
  expect O1's credit is 0 EUR
",
    );
    assert!(has(&t, "O1  credit: no answer within 2 seconds: asking again (send 2 of 3)"), "{}", t.join("\n"));
    assert!(has(&t, "O1  credit: bureau failed (timed out after 3 sends): using 0 EUR instead"), "{}", t.join("\n"));
}

#[test]
fn a_failed_fetch_can_cancel_and_cascade() {
    let p = prog(&CREDIT.replace("otherwise  use 0 EUR", "otherwise  it is cancelled"));
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    bureau answers unknown
  expect O1 is cancelled
  expect O1 has no Payment
",
    );
    assert!(has(&t, "O1  Order is cancelled"), "{}", t.join("\n"));
    assert!(has(&t, "O1  Payment withdrawn"), "{}", t.join("\n"));
}

#[test]
fn a_failed_fetch_can_ask_again_later() {
    let p = prog(&CREDIT.replace("otherwise  use 0 EUR", "otherwise  ask again after 1 hour"));
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    staff Sam changes O1's approved to yes
  and    bureau answers unknown
  expect O1's Payment is not sent
  when   1 hour passes
  and    bureau answers found with credit 100 EUR
  expect O1's Payment is sent
",
    );
    assert!(has(&t, "O1  credit: bureau failed (unknown): asking again in 1 hour"), "{}", t.join("\n"));
}

#[test]
fn with_no_recovery_the_steps_wait_and_the_goal_says_why() {
    let p = prog(&CREDIT.replace("  otherwise  use 0 EUR\n", ""));
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    staff Sam changes O1's approved to yes
  and    bureau answers unknown
  and    1 day passes
  expect O1's Payment is not sent
  expect an alert for O1
",
    );
    assert!(has(&t, "O1  credit: bureau failed (unknown); the steps its rules guard wait"), "{}", t.join("\n"));
    assert!(has(&t, "because credit could not be sourced from bureau"), "{}", t.join("\n"));
}

#[test]
fn bad_and_late_answers_change_nothing() {
    let p = prog(CREDIT);
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    bureau answers found with credit \"lots\"
  and    bureau answers found with credit 100 EUR
  expect O1's credit is 100 EUR
",
    );
    assert!(has(&t, "refused an answer for O1~credit#1: credit must be Money"), "{}", t.join("\n"));
}

#[test]
fn a_sourced_value_is_never_given() {
    let p = prog(CREDIT);
    let r = scenario(&p, "  when   staff Sam places a new Order O1 with customer Ada, total 50 EUR and credit 900 EUR\n");
    assert!(r.failures.iter().any(|f| f.contains("Order.credit comes from its source; it is not given")), "{:?}", r.failures);
}

#[test]
fn sourcing_replays_to_the_same_state() {
    let p = prog(CREDIT);
    let r = scenario(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    bureau answers found with credit 100 EUR
  and    2 hours pass
  and    staff Sam changes O1's approved to yes
  and    bureau answers found with credit 80 EUR
",
    );
    assert!(r.passed(), "{:?}", r.failures);
    let digest = r.host.machine.digest();
    let (kernel_program, _) = kernel::program(Arc::new(p.clone()));
    let replayed = Machine::recover(kernel_program, r.host.machine.into_journal()).unwrap();
    assert_eq!(replayed.digest(), digest);
}

#[test]
fn explain_shows_where_a_sourced_value_stands() {
    let p = prog(CREDIT);
    let show = |steps: &str| {
        let r = scenario(&p, steps);
        assert!(r.passed(), "{:?}", r.failures);
        let now = r.host.now as i64;
        suture_runtime::explain::explain_at(&p, &r.host.unit("O1").unwrap(), Some(now), |_| None).join("\n")
    };
    let placed = "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR\n";
    assert!(show(placed).contains("  credit: asking bureau, send 1 of 3"));
    let fetched = format!("{placed}  and    bureau answers found with credit 100 EUR\n  and    2 hours pass\n");
    assert!(show(&fetched).contains("  credit: 100 EUR from bureau, 2 hours old, trusted 22 hours more"), "{}", show(&fetched));
    let fallback = format!("{placed}  and    bureau answers unknown\n");
    assert!(show(&fallback).contains("  credit: 0 EUR, the fallback after bureau failed"), "{}", show(&fallback));
}

#[test]
fn a_late_answer_after_the_last_time_out_is_ignored() {
    let p = prog(CREDIT);
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    6 seconds pass
  and    bureau answers found with credit 900 EUR
  expect O1's credit is 0 EUR
",
    );
    assert!(has(&t, "ignored a late answer for O1~credit#1: it timed out"), "{}", t.join("\n"));
}

#[test]
fn a_rule_reading_a_source_through_a_definition_is_strict() {
    let p = prog(&CREDIT.replace(
        "rule credit:   no Order has a Payment that is sent unless its total is at most its credit,
               with credit at most 1 hour old",
        "define: an Order is covered when its total is at most its credit
rule credit:   no Order has a Payment that is sent unless it is covered",
    ));
    let t = passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    staff Sam changes O1's approved to yes
  expect O1's Payment is not sent
  when   bureau answers found with credit 100 EUR
  expect O1's Payment is sent
",
    );
    assert!(has(&t, "O1  credit: asking bureau"), "{}", t.join("\n"));
}

#[test]
fn two_recovery_actions_both_apply() {
    let p = prog(&CREDIT.replace("otherwise  use 0 EUR", "otherwise  it is cancelled and it is not approved"));
    passes(
        &p,
        "  when   staff Sam places a new Order O1 with customer Ada and total 50 EUR
  and    staff Sam changes O1's approved to yes
  and    bureau answers unknown
  expect O1 is cancelled
  expect O1 is not approved
  expect O1 has no Payment
",
    );
}
