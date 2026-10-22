//! Slice 38: the days between two dates (spec appendix AN).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const LEAVE: &str = "module leave version 1, currency EUR
role staff
kind Employee { name: Text, allowed: Count }
kind Leave { employee: Employee, from: Time, to: Time, days: Count = the days from its from to its to }
rule enough: the total days of each Employee's Leaves is at most its allowed
rule ordered: every Leave's days is more than 0
admit file a new Leave by staff
";

fn compiled(src: &str) -> suture_lang::ir::Program {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn runs(src: &str, body: &str) -> Report {
    let p = compiled(src);
    let text = format!("scenario \"s\":\n{body}");
    let sc = &parse_scenarios("t.scenario", &text).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&p, sc)
}

fn errors(src: &str) -> Vec<String> {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

#[test]
fn the_days_count_both_ends_by_calendar_date() {
    let r = runs(
        LEAVE,
        "  given  a Employee E with name Ann and allowed 10
  when   staff Sam files a new Leave L1 with employee E, from 2027-01-05 and to 2027-01-08
  and    staff Sam files a new Leave L2 with employee E, from 2025-12-31 and to 2026-01-01
  and    staff Sam files a new Leave L3 with employee E, from 2027-03-01 18:00 and to 2027-03-02 09:00
  and    staff Sam files a new Leave L4 with employee E, from 2028-02-28 and to 2028-03-01
  expect L1's days is 4
  expect L2's days is 2
  expect L3's days is 2
  expect L4's days is 3
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn a_count_of_days_is_held_to_the_rules() {
    let r = runs(
        LEAVE,
        "  given  a Employee E with name Ann and allowed 5
  when   staff Sam files a new Leave L1 with employee E, from 2027-01-05 and to 2027-01-08
  and    staff Sam files a new Leave L2 with employee E, from 2027-02-01 and to 2027-02-02
  expect the total days of E's Leaves is 4
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    assert!(r.host.trace.iter().any(|l| l.contains("refused L2") && l.ends_with("rule enough")), "{}", r.host.trace.join("\n"));
    // A `to` before the `from` counts no days or fewer, which a rule refuses.
    let r = runs(
        LEAVE,
        "  given  a Employee E with name Ann and allowed 5
  when   staff Sam files a new Leave L1 with employee E, from 2027-01-08 and to 2027-01-07
           is refused by rule ordered
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn the_days_read_back_as_written() {
    let src = LEAVE.replace("rule ordered: every Leave's days is more than 0", "rule short: every Leave's days is at least the days from its from to its to - 1");
    let p = compiled(&src);
    let r = p.rules.iter().find(|r| r.name == "short").unwrap();
    let text = suture_lang::readback::formula(&p, r);
    assert!(text.contains("the days from"), "{text}");
    assert!(text.ends_with("the days from l.from to l.to − 1"), "{text}");
    // A calculated end is in brackets, both written and read back.
    let src = LEAVE.replace("rule ordered: every Leave's days is more than 0", "rule short: every Leave's days is at most the days from its from to (its to + 1 day)");
    let p = compiled(&src);
    let text = suture_lang::readback::formula(&p, p.rules.iter().find(|r| r.name == "short").unwrap());
    assert!(text.ends_with("the days from l.from to (l.to + 1 day)"), "{text}");
}

#[test]
fn the_days_are_between_two_times() {
    let with = |f: &str| errors(&LEAVE.replace("the days from its from to its to", f));
    assert_eq!(with("the days from its employee's allowed to its to"), ["t.st:4: `the days from its employee's allowed to its to` counts the days between two times, and `its employee's allowed` is a Count"]);
    let e = with("the days from its from");
    assert_eq!(e.len(), 1, "{e:?}");
    assert!(e[0].contains("expected `to`"), "{e:?}");
}
