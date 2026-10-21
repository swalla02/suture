//! Slice 28: totals per period over the periods an entity spans, and
//! admitted new entities refused by a filtered total (spec appendix AD).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const LEAVE: &str = "module leave version 1, currency EUR
role staff
kind Employee { name: Text }
kind Room { name: Text }
kind Leave { employee: Employee, from: Time, to: Time, status: open | approved | cancelled }
kind Booking { room: Room, from: Time, to: Time }
define: a Leave is held when it is not cancelled
define: a Leave is taken when it is approved
rule apart: for each Employee and day from their from to their to, the number of their Leaves that are held is at most 1
rule seats: for each Room and week from its from to its to, the number of their Bookings is at most 2
admit file a new Leave by staff
admit cancel Leave by staff
admit approve Leave by staff
admit book a new Booking by staff
when a staff cancels a Leave: the Leave is cancelled
when a staff approves a Leave: the Leave is approved
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
fn a_leave_that_shares_a_day_with_another_of_its_employee_is_refused() {
    let r = passes(
        LEAVE,
        "  given  a Employee E with name E
  and    a Employee F with name F
  when   staff S files a new Leave L1 with employee E, from 2026-01-05 and to 2026-01-08
  and    staff S files a new Leave L2 with employee E, from 2026-01-08 and to 2026-01-09
           is refused by rule apart
  and    staff S files a new Leave L3 with employee E, from 2026-01-01 and to 2026-01-04
  and    staff S files a new Leave L4 with employee E, from 2026-01-09 and to 2026-01-09
  and    staff S files a new Leave L5 with employee F, from 2026-01-05 and to 2026-01-08
  expect L3 is held
  expect L4 is held
  expect L5 is held
",
    );
    let refused = r.host.trace.iter().find(|l| l.contains("refused staff S files it")).expect("a refusal");
    assert!(refused.contains("E would break rule apart"), "{refused}");
    assert!(r.host.trace.iter().any(|l| l.contains("held per day from its from to its to, 2026-01-05 to 2026-01-08 taken")), "{}", r.host.trace.join("\n"));
}

#[test]
fn a_cancelled_leave_gives_back_every_day_it_spanned() {
    passes(
        LEAVE,
        "  given  a Employee E with name E
  when   staff S files a new Leave L1 with employee E, from 2026-01-05 and to 2026-01-08
  and    staff S cancels L1
  and    staff S files a new Leave L2 with employee E, from 2026-01-06 and to 2026-01-07
  and    staff S files a new Leave L3 with employee E, from 2026-01-08 and to 2026-01-10
  expect L2 is held
  expect L3 is held
",
    );
}

#[test]
fn an_entity_counts_toward_each_week_it_spans() {
    // 2026-01-05 and 2026-01-12 are Mondays. B1 takes a seat in both weeks,
    // B2 fills the first and B3 the second, so B4 waits.
    let r = passes(
        LEAVE,
        "  given  a Room R with name R
  when   staff S books a new Booking B1 with room R, from 2026-01-09 and to 2026-01-13
  and    staff S books a new Booking B2 with room R, from 2026-01-05 and to 2026-01-05
  and    staff S books a new Booking B3 with room R, from 2026-01-14 and to 2026-01-14
  and    staff S books a new Booking B4 with room R, from 2026-01-06 and to 2026-01-06
",
    );
    let granted: Vec<&String> = r.host.trace.iter().filter(|l| l.starts_with("R   granted")).collect();
    assert_eq!(granted.len(), 3, "{}", r.host.trace.join("\n"));
    assert!(r.host.trace.iter().any(|l| l.contains("refused B4/") && l.contains("rule seats")), "{}", r.host.trace.join("\n"));
}

#[test]
fn a_new_entity_whose_filter_holds_is_refused_and_an_unfiltered_total_still_waits() {
    let src = LEAVE.replace("rule seats: for each Room and week from its from to its to, the number of their Bookings is at most 2",
                            "rule seats: for each Room and day from its from to its to, the number of their Bookings is at most 1");
    let r = passes(
        &src,
        "  given  a Room R with name R
  when   staff S books a new Booking B1 with room R, from 2026-01-05 and to 2026-01-06
  and    staff S books a new Booking B2 with room R, from 2026-01-06 and to 2026-01-06
",
    );
    // Bookings have no filter: B2 is made, and its seat waits.
    assert!(r.host.trace.iter().any(|l| l.contains("refused B2/") && l.contains("rule seats")), "{}", r.host.trace.join("\n"));
    assert!(!r.host.trace.iter().any(|l| l.contains("refused staff S books it")), "{}", r.host.trace.join("\n"));
}

#[test]
fn approving_into_a_taken_day_is_refused_by_the_rule() {
    let src = LEAVE.replace("the number of their Leaves that are held is at most 1", "the number of their Leaves that are taken is at most 1");
    passes(
        &src,
        "  given  a Employee E with name E
  when   staff S files a new Leave L1 with employee E, from 2026-01-05 and to 2026-01-08
  and    staff S files a new Leave L2 with employee E, from 2026-01-07 and to 2026-01-07
  and    staff S approves L1
  and    staff S approves L2
           is refused by rule apart
  expect L2 is open
",
    );
}
