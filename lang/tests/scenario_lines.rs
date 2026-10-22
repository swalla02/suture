//! Scenario errors name the line they are on, and an `and` after an
//! `expect` is another expect (appendix AR.2.6).

use suture_lang::scenario::{parse_scenarios, ScenarioStep};

#[test]
fn an_error_names_its_own_line() {
    let src = "scenario \"x\":\n  given a Talk T1\n\n  expect T1 is\n";
    let e = parse_scenarios("t.scenario", src).unwrap_err();
    assert_eq!(e.line, 4, "{}", e.msg);
}

#[test]
fn and_after_an_expect_expects() {
    let src = "scenario \"x\":\n  given a Talk T1\n  expect T1 is accepted\n  and    T1 is not withdrawn\n  and    no alert for T1\n  and    payments answers paid for T1\n";
    let sc = &parse_scenarios("t.scenario", src).unwrap()[0];
    assert!(matches!(&sc.steps[1], ScenarioStep::Expect { state, negated: false, .. } if state == "accepted"));
    assert!(matches!(&sc.steps[2], ScenarioStep::Expect { state, negated: true, .. } if state == "withdrawn"));
    assert!(matches!(&sc.steps[3], ScenarioStep::Alert { negated: true, .. }));
    assert!(matches!(&sc.steps[4], ScenarioStep::Answer { .. }));
}

#[test]
fn a_range_creates_one_entity_per_name() {
    let src = "scenario \"x\":\n  given  3 Talks T1 to T3 with length 45\n  and    a Talk T4\n  when   member Ada books 2 new Bookings B1 to B2 with talk T1\n  and    3 days pass\n";
    let sc = &parse_scenarios("t.scenario", src).unwrap()[0];
    let names: Vec<_> = sc.steps.iter().filter_map(|s| match s { ScenarioStep::Create { name, given, .. } => Some((name.as_str(), *given)), _ => None }).collect();
    assert_eq!(names, [("T1", true), ("T2", true), ("T3", true), ("T4", true), ("B1", false), ("B2", false)]);
    assert!(matches!(&sc.steps[4], ScenarioStep::Create { kind, by: Some(b), .. } if kind == "Bookings" && b.name == "Ada"));
    assert!(matches!(sc.steps[6], ScenarioStep::Pass { .. }));
    let bad = parse_scenarios("t.scenario", "scenario \"x\":\n  given 3 Talks T1 to T4\n").unwrap_err();
    assert!(bad.msg.contains("numbers 3 apart"), "{}", bad.msg);
}
