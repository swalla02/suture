//! Slice 41: when a time passes (spec appendix AQ).

use std::sync::Arc;

use suture::Machine;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::kernel;
use suture_runtime::scenario::{run, Report};

const LIBRARY: &str = "module library version 1, currency EUR
role librarian
kind Member { name: Text }
kind Loan { member: Member, due: Time = created + 14 days, returned: Bool = no, overdue: Bool = no }
kind Hold { member: Member, until: Time, expired: Bool = no }
define: a Loan is late when it is overdue and it is not returned
when a Loan's due passes: it is overdue
when a Hold's until passes: it is expired
when a librarian returns a Loan: the Loan is returned
admit lend a new Loan by librarian
admit return Loan by librarian
admit place a new Hold by librarian
admit change Hold's until by librarian
";

fn compiled(src: &str) -> suture_lang::ir::Program {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn runs(src: &str, body: &str) -> Report {
    let p = compiled(src);
    let text = format!("scenario \"s\":\n  given  a Member Ann with name \"Ann\"\n{body}");
    let sc = &parse_scenarios("t.scenario", &text).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&p, sc)
}

fn errors(src: &str) -> Vec<String> {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

#[test]
fn a_loan_becomes_overdue_when_its_due_passes_and_not_before() {
    let r = runs(
        LIBRARY,
        "  when   librarian Lu lends a new Loan L1 with member Ann
  and    librarian Lu lends a new Loan L2 with member Ann
  and    librarian Lu returns L2
  and    13 days pass
  expect L1 is not overdue
  when   1 day passes
  expect L1 is late
  expect L2 is overdue
  expect L2 is not late
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(r.host.trace.iter().any(|l| l.contains("L1  its due passes (2026-01-15 00:00)")), "{}", r.host.trace.join("\n"));
}

#[test]
fn a_time_already_past_passes_when_the_entity_is_made() {
    let r = runs(
        LIBRARY,
        "  when   librarian Lu places a new Hold H1 with member Ann and until 2025-12-01
  expect H1 is expired
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn a_change_moves_the_time_and_the_old_one_does_nothing() {
    let r = runs(
        LIBRARY,
        "  when   librarian Lu places a new Hold H1 with member Ann and until 2026-01-10
  and    librarian Lu places a new Hold H2 with member Ann and until 2026-01-10
  and    5 days pass
  and    librarian Lu changes H1's until to 2026-01-20
  and    librarian Lu changes H2's until to 2026-01-02
  expect H2 is expired
  when   10 days pass
  expect H1 is not expired
  when   10 days pass
  expect H1 is expired
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert_eq!(r.host.trace.iter().filter(|l| l.starts_with("H1  its until pass")).count(), 1, "{}", r.host.trace.join("\n"));
}

#[test]
fn a_passing_that_would_break_a_rule_is_held_and_happens_once() {
    let src = LIBRARY.replace("admit lend", "rule kept: no Hold is expired unless its member is the Hold's member and 1 is 2\nadmit lend")
        .replace("kind Hold { member: Member, until: Time, expired: Bool = no }", "kind Hold { member: Member, until: Time, expired: Bool = no, count: Count = 0 }")
        .replace("when a Hold's until passes: it is expired", "when a Hold's until passes: it is expired\nwhen a Hold's until passes: its count becomes its count + 1");
    let r = runs(
        &src,
        "  when   librarian Lu places a new Hold H1 with member Ann and until 2025-12-01
  and    librarian Lu places a new Hold H2 with member Ann and until 2026-01-05
  and    10 days pass
  and    librarian Lu changes H1's until to 2025-12-01
  expect H1 is not expired
  expect H2 is not expired
  expect H1's count is 1
  expect H2's count is 1
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(r.host.trace.iter().any(|l| l.starts_with("H2  reaction held: rule kept")), "{}", r.host.trace.join("\n"));
}

#[test]
fn a_replay_passes_at_the_same_times() {
    let p = compiled(LIBRARY);
    let r = runs(
        LIBRARY,
        "  when   librarian Lu lends a new Loan L1 with member Ann
  and    20 days pass
  expect L1 is late
",
    );
    assert!(r.passed(), "{:?}", r.failures);
    let digest = r.host.machine.digest();
    let (kernel_program, _) = kernel::program(Arc::new(p));
    let replayed = Machine::recover(kernel_program, r.host.machine.into_journal()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(replayed.digest(), digest);
}

#[test]
fn the_checker_names_what_cannot_pass() {
    let first = |extra: &str| errors(&format!("{LIBRARY}{extra}\n")).first().cloned().unwrap_or_default();
    assert!(first("when a Loan's mood passes: it is overdue").contains("`mood` is not a Time attribute of a Loan"));
    assert!(first("when a Member's name passes: it is overdue").contains("`name` is not a Time attribute"));
    assert!(first("when a Loan's due ends: it is overdue").contains("expected `passes`"));
    let reset = LIBRARY.replace("when a librarian returns a Loan: the Loan is returned", "when a librarian returns a Loan: the Loan is returned\nwhen a librarian renews a Hold: the Hold's until becomes the Hold's until + 7 days\nadmit renew Hold by librarian");
    assert!(errors(&reset).iter().any(|e| e.contains("only an admitted change may move a time that passes")), "{:?}", errors(&reset));
    let part = "module m version 1, currency EUR
kind Order { name: Text }
kind Step { order: Order, due: Time, missed: Bool = no }
Order needs Step
when a Step's due passes: it is missed
";
    assert!(errors(part).iter().any(|e| e.contains("a Step is a part")), "{:?}", errors(part));
}
