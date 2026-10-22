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
