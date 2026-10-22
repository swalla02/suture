//! A scenario line that creates a range of entities (appendix AS).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::run;

const PROGRAM: &str = "module cfp version 1, currency EUR
role organiser
kind Program { name: Text }
kind Talk { program: Program, length: Count, accepted: Bool = no }
rule minutes: the total length of each Program's Talks that are accepted is at most 600
admit propose a new Talk by organiser
";

#[test]
fn a_range_fills_a_program_up_to_its_limit() {
    let prog = suture_lang::compile(&[("t.st".to_string(), PROGRAM.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let sc = parse_scenarios("t.scenario", "scenario \"full\":
  given  a Program P1 with name \"main\"
  and    13 Talks T1 to T13 with program P1, length 45 and accepted yes
  when   organiser Olga proposes 2 new Talks T14 to T15 with program P1 and length 30
  and    organiser Olga proposes a new Talk T16 with program P1, length 30 and accepted yes
         is refused by rule minutes
  expect T15 is not accepted
").unwrap();
    let r = run(&prog, &sc[0]);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn a_range_keeps_leading_zeros() {
    let sc = suture_lang::scenario::parse_scenarios("t.scenario", "scenario \"x\":\n  given 3 Talks T09 to T11\n").unwrap();
    let names: Vec<_> = sc[0].steps.iter().map(|s| match s {
        suture_lang::scenario::ScenarioStep::Create { name, .. } => name.clone(),
        _ => String::new(),
    }).collect();
    assert_eq!(names, ["T09", "T10", "T11"]);
}
