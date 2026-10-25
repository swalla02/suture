//! Slice 61: the time a calendar works (spec appendix BL).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const LEAVE: &str = "module leave version 1, currency EUR
role staff
calendar work: Monday to Friday, 9:00 to 17:00, except 2026-04-06 and 2026-12-25
kind Employee { name: Text, allowed: Duration }
kind Leave { employee: Employee, from: Time, to: Time, length: Duration = the work time from its from to its to }
rule enough: the total length of each Employee's Leaves is at most its allowed
rule some: every Leave's length is more than 0 seconds
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
fn whole_days_half_days_weekends_and_days_off() {
    let r = runs(
        LEAVE,
        "  given  a Employee E with name Ann and allowed 4000 hours
  when   staff Sam files a new Leave L1 with employee E, from 2026-03-02 and to 2026-03-06
  and    staff Sam files a new Leave L2 with employee E, from 2026-03-09 9:00 and to 2026-03-09 13:00
  and    staff Sam files a new Leave L3 with employee E, from 2026-04-06 and to 2026-04-10
  and    staff Sam files a new Leave L4 with employee E, from 2026-03-12 15:00 and to 2026-03-13 11:00
  and    staff Sam files a new Leave L5 with employee E, from 2026-01-01 and to 2026-12-31
  expect L1's length is 40 hours
  and    L2's length is 4 hours
  and    L3's length is 32 hours
  and    L4's length is 4 hours
  and    L5's length is 2072 hours
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn working_time_is_held_to_the_rules() {
    let r = runs(
        LEAVE,
        "  given  a Employee E with name Ann and allowed 44 hours
  when   staff Sam files a new Leave L1 with employee E, from 2026-03-02 and to 2026-03-06
  and    staff Sam files a new Leave L2 with employee E, from 2026-03-07 and to 2026-03-08
         is refused by rule some
  and    staff Sam files a new Leave L3 with employee E, from 2026-03-09 and to 2026-03-09
  and    staff Sam files a new Leave L4 with employee E, from 2026-03-10 9:00 and to 2026-03-10 13:00
  expect the total length of E's Leaves is 44 hours
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let refused = |l: &str, rule: &str| r.host.trace.iter().any(|t| t.contains(&format!("refused {l}")) && t.ends_with(&format!("rule {rule}")));
    assert!(refused("L3", "enough"), "a whole day is over what is left\n{}", r.host.trace.join("\n"));
}

#[test]
fn a_calendar_reads_back_and_its_mistakes_are_named() {
    let p = compiled(LEAVE);
    let c = &p.calendars[0];
    assert_eq!((c.days, c.start, c.end, c.holidays.len()), ([true, true, true, true, true, false, false], 9 * 3_600, 17 * 3_600, 2));
    let wrap = compiled("module m version 1, currency EUR\ncalendar shop: Sunday to Thursday\ncalendar odd: Monday, Wednesday and Friday, 0:00 to 24:00\n");
    assert_eq!(wrap.calendars[0].days, [true, true, true, true, false, false, true]);
    assert_eq!((wrap.calendars[1].days, wrap.calendars[1].end), ([true, false, true, false, true, false, false], 86_400));
    let head = "module m version 1, currency EUR\n";
    for (src, msg) in [
        ("kind T { a: Time, b: Time, c: Duration = the shop time from its a to its b }\n", "there is no calendar shop; none is declared"),
        ("calendar work: every day\nkind T { a: Time, b: Count, c: Duration = the work time from its a to its b }\n", "measures the time between two times, and `its b` is a Count"),
        ("calendar work: Monday to Friday, except 2026-12-25 12:00\n", "is not a date; a day off is a whole day"),
        ("calendar work: Monday to Friday, 17:00 to 9:00\n", "a working day ends after it starts"),
        ("calendar work: every day\ncalendar work: every day\n", "calendar work is declared twice"),
        ("calendar work: Mon to Fri\n", "`Mon` is not a day of the week"),
        ("calendar work: every day, 9:00 to 24:30\n", "`24:30` is not a time of day"),
    ] {
        let e = errors(&format!("{head}{src}"));
        assert!(e.iter().any(|e| e.contains(msg)), "{src}: {e:?}");
    }
}

#[test]
fn a_long_span_counts_as_fast_as_a_short_one() {
    let p = compiled(LEAVE);
    let c = &p.calendars[0];
    let day = 86_400;
    // 2026 has 261 weekdays, two of them days off.
    assert_eq!(suture_runtime::interp::working(c, 0, 364 * day), 259 * 8 * 3_600);
    // A hundred years, counted by weeks, not by days.
    let century = suture_runtime::interp::working(c, 0, 36_524 * day);
    assert!(century > 26_000 * 8 * 3_600 && century < 26_100 * 8 * 3_600, "{century}");
    assert_eq!(suture_runtime::interp::working(c, 10 * day, 3 * day), 0, "an end before the start");
}

const ROTA: &str = "module rota version 1, currency EUR
role staff
calendar work: Monday to Friday, 9:00 to 17:00, except 2026-04-06
kind Employee { name: Text }
kind Leave { employee: Employee, from: Time, to: Time }
rule apart: for each Employee and day from their from to their to, the total work time of their Leaves is at most 8 hours
admit file a new Leave by staff
";

#[test]
fn a_span_rule_totals_the_working_time_of_each_day() {
    let r = runs(
        ROTA,
        "  given  a Employee E with name Ann
  when   staff Sam files a new Leave L1 with employee E, from 2026-03-02 and to 2026-03-04 13:00
  and    staff Sam files a new Leave L2 with employee E, from 2026-03-04 13:00 and to 2026-03-06
  and    staff Sam files a new Leave L3 with employee E, from 2026-03-06 12:00 and to 2026-03-06 14:00
  and    staff Sam files a new Leave L4 with employee E, from 2026-04-03 13:00 and to 2026-04-07 13:00
  and    staff Sam files a new Leave L5 with employee E, from 2026-04-06 and to 2026-04-06
",
    );
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    // Friday is full, so L3 waits for room; L4 takes Friday and Tuesday
    // afternoons, around Easter Monday, which L5 then has to itself.
    let t = r.host.trace.join("\n");
    assert!(r.host.trace.iter().any(|l| l.contains("refused L3") && l.contains("rule apart on 2026-03-06")), "{t}");
    for l in ["L4", "L5"] {
        assert!(!r.host.trace.iter().any(|x| x.contains(&format!("refused {l}"))), "{l}\n{t}");
    }
    let errs = errors("module m version 1, currency EUR\ncalendar work: every day\nkind E { name: Text }\nkind L { e: E, from: Time, to: Time }\nrule r: the total work time of each E's Ls is at most 8 hours\n");
    assert!(errs.iter().any(|e| e.contains("is read only in a rule `for each … and day from their … to their …`")), "{errs:?}");
    let errs = errors("module m version 1, currency EUR\nkind E { name: Text }\nkind L { e: E, from: Time, to: Time }\nrule r: for each E and day from their from to their to, the total shop time of their Ls is at most 8 hours\n");
    assert!(errs.iter().any(|e| e.contains("there is no calendar shop")), "{errs:?}");
}
