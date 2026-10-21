//! Slice 32: days in a row across entities (spec appendix AH).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const LEAVE: &str = "module leave version 1, currency EUR
role staff
kind Employee { name: Text, longest: Count = 5 }
kind Leave { employee: Employee, from: Time, to: Time, status: open | cancelled }
define: a Leave is held when it is open
rule run: for each Employee and day from their from to their to, the days in a row of their Leaves that are held is at most its longest
admit file a new Leave by staff
admit cancel Leave by staff
when a staff cancels a Leave: the Leave is cancelled
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

fn errors(src: &str) -> Vec<String> {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

#[test]
fn leaves_that_meet_make_one_run_and_a_gap_ends_it() {
    let r = passes(
        LEAVE,
        "  given  a Employee E with name Ann
  when   staff Sam files a new Leave L1 with employee E, from 2026-03-02, to 2026-03-03 and status open
  and    staff Sam files a new Leave L2 with employee E, from 2026-03-06, to 2026-03-07 and status open
  and    staff Sam files a new Leave L3 with employee E, from 2026-03-04, to 2026-03-05 and status open
           is refused by rule run
  and    staff Sam files a new Leave L4 with employee E, from 2026-03-04, to 2026-03-04 and status open
  and    staff Sam files a new Leave L5 with employee E, from 2026-03-09, to 2026-03-13 and status open
  expect L4 is open
  expect L5 is open
",
    );
    let t = r.host.trace.join("\n");
    assert!(t.contains("L3  refused staff Sam files it: E would break rule run"), "{t}");
}

#[test]
fn a_cancelled_leave_gives_back_its_days_and_splits_the_run() {
    passes(
        LEAVE,
        "  given  a Employee E with name Ann
  when   staff Sam files a new Leave L1 with employee E, from 2026-03-02, to 2026-03-04 and status open
  and    staff Sam files a new Leave L2 with employee E, from 2026-03-05, to 2026-03-06 and status open
  and    staff Sam files a new Leave L3 with employee E, from 2026-03-07, to 2026-03-07 and status open
           is refused by rule run
  when   staff Sam cancels L1
  and    staff Sam files a new Leave L4 with employee E, from 2026-03-07, to 2026-03-09 and status open
  expect L4 is open
",
    );
}

#[test]
fn days_in_a_row_are_read_in_a_rule_over_the_days_a_span_covers() {
    let p = suture_lang::compile(&[("t.st".to_string(), LEAVE.to_string())]).unwrap();
    let (formula, words) = (suture_lang::readback::formula(&p, &p.rules[0]), suture_lang::readback::paraphrase(&p, &p.rules[0]));
    assert_eq!(formula, "for each Employee e: run(count(l in Leave where l.employee = e and l is held, per day from l.from to l.to)) ≤ e.longest");
    assert_eq!(words, "each employee on its own: the days in a row its Leaves that are held span, through any one day is at most its longest");
    let bad = LEAVE.replace("for each Employee and day from their from to their to, the days", "for each Employee and month, the days");
    assert_eq!(
        errors(&bad),
        ["t.st:6: `the days in a row of its Leaves` counts the days a span covers: write it in `for each Employee and day from their ⟨time⟩ to their ⟨time⟩, …`"]
    );
}
