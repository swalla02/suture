//! Slice 33: totals kept for readers and scenarios (spec appendix AI).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::{run, Report};

const LEAVE: &str = "module leave version 1, currency EUR
role staff
kind Employee { name: Text }
kind Leave { employee: Employee, days: Count, status: open | approved | cancelled }
keep the total days of each Employee's Leaves that are approved
keep the number of each Employee's Leaves that are open
admit file a new Leave by staff
admit approve Leave by staff
when a staff approves a Leave: the Leave is approved
";

fn scenario(src: &str, body: &str) -> Report {
    let p = suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let src = format!("scenario \"s\":\n{body}");
    let sc = &parse_scenarios("t.scenario", &src).unwrap_or_else(|e| panic!("{e}"))[0];
    run(&p, sc)
}

fn errors(src: &str) -> Vec<String> {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

const STEPS: &str = "  given  a Employee E with name Ann
  when   staff Sam files a new Leave L1 with employee E, days 3 and status open
  and    staff Sam files a new Leave L2 with employee E, days 2 and status open
  and    staff Sam approves L1
  expect the total days of E's Leaves that are approved is 3
  expect the number of E's Leaves that are open is 1
";

#[test]
fn a_kept_total_is_kept_with_no_rule_and_read_by_a_scenario() {
    let r = scenario(LEAVE, STEPS);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let p = suture_lang::compile(&[("t.st".to_string(), LEAVE.to_string())]).unwrap();
    let lines = suture_runtime::explain::explain(&p, &r.host.unit("E").unwrap(), |_| None);
    assert!(lines.iter().any(|l| l == "  its total days of Leaves that are approved: 3"), "{}", lines.join("\n"));
    assert!(lines.iter().any(|l| l == "  its number of Leaves that are open: 1"), "{}", lines.join("\n"));
}

#[test]
fn without_keep_a_scenario_cannot_read_the_total() {
    let r = scenario(&LEAVE.replace("keep the total days of each Employee's Leaves that are approved\n", ""), STEPS);
    assert!(!r.passed());
    assert!(format!("{:?}", r.failures).contains("the program keeps no total days of Leaves that are approved of each Employee"), "{:?}", r.failures);
}

#[test]
fn keep_names_a_total_of_each_entity_across_units() {
    let with = |line: &str| LEAVE.replace("keep the number of each Employee's Leaves that are open", line);
    let form = "t.st:6: `keep` names a total of each entity, as in `keep the total days of each Allocation's Applications` or `keep the number of each Allocation's Applications that are open`";
    assert_eq!(errors(&with("keep the total days of all Leaves")), [form]);
    assert_eq!(errors(&with("keep the name of each Employee")), [form]);
    assert_eq!(errors(&with("keep the number of each Manager's Leaves")), ["t.st:6: unknown kind Manager"]);
}
