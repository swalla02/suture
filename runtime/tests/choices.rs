//! Slice 10: models choose slots' alternatives and sources, within a deadline (spec appendix K).

use std::sync::Arc;

use suture::Machine;
use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::kernel;
use suture_runtime::scenario::{run, Report};

const RETURNS: &str = "module returns version 1, currency EUR
kind Applicant   { name: Text }
kind Application { applicant: Applicant, paid: Bool = no }
kind Courier     { application: Application }
kind Collection  { application: Application }
kind Locker      { application: Application }
Application needs Return
Return is Courier or Collection or Locker
effect send Courier via couriers within 2 days
  sends    the application's applicant's name
  marks    sent
  reports  delivered | fails with lost
effect notify Collection via mailer within 1 hour
  sends    the application's applicant's name
  reports  notified
effect open Locker via lockers within 1 hour
  sends    the application's applicant's name
  reports  opened
rule prepaid: no Application has a Courier that is sent unless it is paid
prefer Return: Courier, then Collection after 3 days, then Locker
";

const CREDIT: &str = "module credit version 1, currency EUR
role staff
kind Customer { name: Text }
kind Order    { customer: Customer, total: Money, credit: Money }
kind Payment  { order: Order }
Order needs Payment
effect capture Payment via payments within 30 seconds
  sends    the order's total
  marks    sent
  reports  captured
source Order's credit via bureau within 2 seconds
  sends      the customer's name
  reports    found | fails with unknown
source Order's credit via backup within 2 seconds
  sends      the customer's name
  reports    found | fails with unknown
  otherwise  use 0 EUR
rule credit: no Order has a Payment that is sent unless its total is at most its credit
admit place a new Order by staff
";

fn prog(src: &str, decides: &str) -> Program {
    let files = vec![("t.st".to_string(), format!("{src}{decides}"))];
    suture_lang::compile(&files).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn scenario(p: &Program, steps: &str) -> Report {
    let given = if p.kind("Applicant").is_some() { "an Applicant Ada with name Ada" } else { "a Customer Bo with name Bo" };
    let src = format!("scenario \"s\":\n  given  {given}\n{steps}");
    let sc = &parse_scenarios("s.scenario", &src).unwrap_or_else(|e| panic!("{e:?}"))[0];
    run(p, sc)
}

fn trace(p: &Program, steps: &str) -> String {
    let r = scenario(p, steps);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    r.host.trace.join("\n")
}

fn replays(p: &Program, steps: &str) {
    let r = scenario(p, steps);
    assert!(r.passed(), "{:?}", r.failures);
    let digest = r.host.machine.digest();
    let (kernel_program, _) = kernel::program(Arc::new(p.clone()));
    let replayed = Machine::recover(kernel_program, r.host.machine.into_journal()).unwrap();
    assert_eq!(replayed.digest(), digest);
}

const APPLY: &str = "  when   a new Application A1 with applicant Ada\n";
const ROUTING: &str = "decide Return with model \"routing@1\"\n";

#[test]
fn a_model_picks_a_slots_first_alternative() {
    let p = prog(RETURNS, ROUTING);
    let t = trace(
        &p,
        "  when   model \"routing@1\" picks Locker\n  and    a new Application A1 with applicant Ada\n  and    lockers answers opened\n  expect A1's Locker is opened\n  expect the number of A1's Couriers is 0\n",
    );
    assert!(t.contains("A1  Return: asking model \"routing@1\""), "{t}");
    assert!(t.contains("      choice: Locker for A1?Return#1 by model \"routing@1\" (of Courier, Collection, Locker)"), "{t}");
    assert!(t.contains("A1  Return: trying Locker"), "{t}");
}

#[test]
fn a_model_picks_the_next_alternative_after_patience_and_after_lost_couriers() {
    // Patience: the courier waits unpaid for 3 days, then the model picks among the rest.
    let p = prog(RETURNS, ROUTING);
    let t = trace(
        &p,
        &format!("  when   model \"routing@1\" picks Courier, then Locker\n{APPLY}  and    3 days pass\n  and    lockers answers opened\n  expect A1's Locker is opened\n"),
    );
    assert!(t.contains("A1  Return: no Courier after 3 days, asking model \"routing@1\""), "{t}");
    assert!(t.contains("      choice: Locker for A1?Return#2 by model \"routing@1\" (of Collection, Locker)"), "{t}");

    // Failure: a paid application's courier is lost three times.
    let paid = "  when   a new Application A1 with applicant Ada and paid yes\n";
    let t = trace(
        &p,
        &format!(
            "  when   model \"routing@1\" picks Courier, then Collection\n{paid}  and    couriers answers lost\n  and    couriers answers lost\n  and    couriers answers lost\n  and    mailer answers notified\n  expect A1's Collection is notified\n"
        ),
    );
    assert!(t.contains("A1  Return: Courier failed, asking model \"routing@1\""), "{t}");
    assert!(t.contains("      choice: Collection for A1?Return#2 by model \"routing@1\" (of Collection, Locker)"), "{t}");
}

#[test]
fn one_alternative_left_is_no_choice() {
    let p = prog(RETURNS, ROUTING);
    let t = trace(
        &p,
        &format!(
            "  when   model \"routing@1\" picks Collection, then Courier, then Collection\n{APPLY}  and    3 hours pass\n  and    3 days pass\n  and    lockers answers opened\n  expect A1's Locker is opened\n"
        ),
    );
    assert!(t.contains("A1  Collection timed out after 3 sends\nA1  Return: Collection failed, asking model \"routing@1\""), "{t}");
    assert!(t.contains("choice: Courier for A1?Return#2 by model \"routing@1\" (of Courier, Locker)"), "{t}");
    // Only the locker is left: it is taken, and the model is not asked.
    assert!(t.contains("A1  Return: no Courier after 3 days, trying Locker"), "{t}");
    assert!(!t.contains("A1?Return#3"), "{t}");
}

#[test]
fn a_declining_late_or_missing_model_leaves_the_slot_to_prefer() {
    let p = prog(RETURNS, ROUTING);
    let first = |t: &str| t.lines().find(|l| l.contains("for A1?Return#1")).unwrap_or("no choice").trim().to_string();
    let t = trace(&p, &format!("  when   model \"routing@1\" declines\n{APPLY}"));
    assert_eq!(first(&t), "choice: Courier for A1?Return#1 by prefer (model \"routing@1\" declined)");
    let t = trace(&p, &format!("  when   model \"routing@1\" is late\n{APPLY}"));
    assert_eq!(first(&t), "choice: Courier for A1?Return#1 by prefer (model \"routing@1\" did not answer within 1000 ms)");
    let p = prog(RETURNS, "decide Return with model \"routing@1\" within 250 milliseconds\n");
    let t = trace(&p, &format!("  when   model \"routing@1\" is late\n{APPLY}"));
    assert_eq!(first(&t), "choice: Courier for A1?Return#1 by prefer (model \"routing@1\" did not answer within 250 ms)");
    let t = trace(&p, APPLY);
    assert_eq!(first(&t), "choice: Courier for A1?Return#1 by prefer (model \"routing@1\" is not available)");
    let t = trace(&p, &format!("  when   model \"routing@1\" picks Parcel\n{APPLY}"));
    assert_eq!(first(&t), "choice: Courier for A1?Return#1 by prefer (model \"routing@1\" picked Parcel, which it was not offered)");
}

#[test]
fn an_app_level_model_also_decides_slots_and_with_prefer_takes_them_back() {
    let p = prog(RETURNS, "decide with model \"planner@1\"\n");
    let t = trace(&p, &format!("  when   model \"planner@1\" picks Collection\n{APPLY}"));
    assert!(t.contains("choice: Collection for A1?Return#1 by model \"planner@1\""), "{t}");
    let p = prog(RETURNS, "decide with model \"planner@1\"\ndecide Return with prefer\n");
    let t = trace(&p, APPLY);
    assert!(!t.contains("asking model"), "{t}");
    assert!(t.contains("A1  Return: trying Courier"), "{t}");
}

#[test]
fn a_registered_slow_model_is_late() {
    let p = prog(RETURNS, "decide Return with model \"slow@1\" within 20 milliseconds\n");
    let sc = &parse_scenarios("s.scenario", "scenario \"s\":\n  given  an Applicant Ada with name Ada\n").unwrap()[0];
    let mut r = run(&p, sc);
    r.host.models.register(
        "slow@1",
        Box::new(|_| {
            std::thread::sleep(std::time::Duration::from_millis(60));
            Some("Locker".into())
        }),
    );
    r.host.input_new(None, "Application", "A1", &[("applicant".into(), suture_runtime::state::Val::Ref("Ada".into()))], &[]).unwrap();
    let t = r.host.trace.join("\n");
    assert!(t.contains("choice: Courier for A1?Return#1 by prefer (model \"slow@1\" did not answer within 20 ms)"), "{t}");
}

#[test]
fn slot_choices_replay_without_the_model() {
    let p = prog(RETURNS, ROUTING);
    replays(&p, &format!("  when   model \"routing@1\" picks Courier, then Locker\n{APPLY}  and    3 days pass\n  and    lockers answers opened\n"));
}

const ORDER: &str = "  when   staff Sam places a new Order O1 with customer Bo and total 50 EUR\n";
const SOURCES: &str = "decide Order's credit with model \"sources@1\"\n";

#[test]
fn several_sources_are_tried_in_prefer_order() {
    let p = prog(CREDIT, "");
    let t = trace(
        &p,
        &format!("{ORDER}  and    bureau answers unknown\n  and    backup answers found with credit 80 EUR\n  and    payments answers captured\n  expect O1's credit is 80 EUR\n"),
    );
    assert!(t.contains("O1  credit: asking bureau (name \"Bo\")"), "{t}");
    assert!(t.contains("O1  credit: bureau failed (unknown), asking backup (name \"Bo\")"), "{t}");
    assert!(t.contains("O1  credit: 80 EUR from backup"), "{t}");
}

#[test]
fn a_model_picks_the_source_and_the_otherwise_applies_when_all_fail() {
    let p = prog(CREDIT, SOURCES);
    let t = trace(
        &p,
        &format!(
            "  when   model \"sources@1\" picks backup\n{ORDER}  and    backup answers unknown\n  and    bureau answers unknown\n  expect O1's credit is 0 EUR\n  expect O1's Payment is not sent\n"
        ),
    );
    assert!(t.contains("O1  credit: asking model \"sources@1\" for a source"), "{t}");
    assert!(t.contains("      choice: backup for O1~credit#1?source by model \"sources@1\" (of bureau, backup)"), "{t}");
    assert!(t.contains("O1  credit: asking backup (name \"Bo\")"), "{t}");
    // One source left: no model is asked.
    assert!(t.contains("O1  credit: backup failed (unknown), asking bureau (name \"Bo\")"), "{t}");
    assert!(t.contains("O1  credit: bureau failed (unknown): using 0 EUR instead"), "{t}");
}

#[test]
fn a_late_answer_from_a_source_that_was_left_is_ignored() {
    let p = prog(CREDIT, "");
    let r = scenario(&p, &format!("{ORDER}  and    3 seconds pass\n  and    3 seconds pass\n  and    3 seconds pass\n  and    backup answers found with credit 70 EUR\n  expect O1's credit is 70 EUR\n"));
    let t = r.host.trace.join("\n");
    assert!(r.passed(), "{:?}\n{t}", r.failures);
    assert!(t.contains("credit: bureau failed (timed out after 3 sends), asking backup"), "{t}");
}

#[test]
fn source_choices_replay_without_the_model() {
    let p = prog(CREDIT, SOURCES);
    replays(&p, &format!("  when   model \"sources@1\" picks backup\n{ORDER}  and    backup answers found with credit 90 EUR\n  and    payments answers captured\n"));
}
