//! One admit for several events (appendix AV).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::run;

const LIB: &str = "module library version 1, currency EUR
role member is a Member
role staff
kind Member { name: Text }
kind Loan { member: Member, returned: Bool = no, renewed: Bool = no, lost: Bool = no }
admit lend a new Loan by staff
admit return, renew or lose Loan by member, staff where the Loan is not returned
when a member or staff returns a Loan: the Loan is returned
when a member or staff renews a Loan: the Loan is renewed
when a member or staff loses a Loan: the Loan is lost
";

fn compile(src: &str) -> Result<suture_lang::ir::Program, String> {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).map_err(|e| e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))
}

#[test]
fn one_admit_lets_each_of_its_events_through() {
    let prog = compile(LIB).unwrap();
    assert_eq!(prog.admits.len(), 4);
    let sc = parse_scenarios("t.scenario", "scenario \"x\":
  given  a Member Ada with name \"Ada\"
  when   staff Sam lends a new Loan L1 with member Ada
  and    member Ada renews L1
  and    member Ada returns L1
  and    member Ada loses L1
         is refused at the gate
  expect L1 is renewed
  expect L1 is returned
  expect L1 is not lost
").unwrap();
    let r = run(&prog, &sc[0]);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn change_keeps_its_own_admit() {
    let e = compile(&LIB.replace("admit lend a new Loan by staff", "admit lend a new Loan by staff\nadmit return or change Loan by staff")).unwrap_err();
    assert!(e.contains("`change` takes its own admit"), "{e}");
}
