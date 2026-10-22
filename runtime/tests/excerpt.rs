//! A failing scenario's trace, cut to the steps around each failure
//! (appendix AT).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::run;

const PROGRAM: &str = "module cfp version 1, currency EUR
role organiser
kind Program { name: Text }
kind Talk { program: Program, length: Count, accepted: Bool = no }
rule minutes: the total length of each Program's Talks that are accepted is at most 100
admit propose a new Talk by organiser
";

#[test]
fn the_excerpt_keeps_each_failure_its_steps_and_the_refusal_behind_it() {
    let prog = suture_lang::compile(&[("t.st".to_string(), PROGRAM.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let sc = parse_scenarios("t.scenario", "scenario \"x\":
  given  a Program P1 with name \"main\"
  when   organiser Olga proposes a new Talk T1 with program P1, length 90 and accepted yes
  and    organiser Olga proposes a new Talk T2 with program P1, length 30 and accepted yes
         is refused by rule minutes
  and    organiser Olga proposes 12 new Talks T3 to T14 with program P1 and length 5
  expect T2 is accepted
").unwrap();
    let r = run(&prog, &sc[0]);
    assert_eq!(r.failures.len(), 1);
    let ex = r.excerpt();
    assert!(ex.len() < r.host.trace.len() / 2, "{ex:#?}");
    assert!(ex.last().unwrap().starts_with("✗ expect T2 is accepted"), "{ex:#?}");
    assert!(ex.iter().any(|l| l.starts_with("T2  refused") && l.contains("minutes")), "{ex:#?}");
    assert!(ex.iter().any(|l| l == "…"), "{ex:#?}");
    // A passing scenario has no excerpt.
    let ok = parse_scenarios("t.scenario", "scenario \"y\":\n  given  a Program P1 with name \"main\"\n").unwrap();
    assert!(run(&prog, &ok[0]).excerpt().is_empty());
}
