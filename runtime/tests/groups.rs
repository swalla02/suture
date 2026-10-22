//! `and` with `or` in one sentence, with parentheses (appendix AU).

use suture_lang::scenario::parse_scenarios;
use suture_runtime::scenario::run;

const SHOP: &str = "module shop version 1, currency EUR
role staff
kind Order { total: Money, paid: Bool = no, free: Bool = no, cancelled: Bool = no, shipped: Bool = no }
rule ship: no Order is shipped unless it is not cancelled and (it is paid or it is free)
rule cap: every Order's total is at most (100 EUR + 20 EUR) * 2
define: a Order is open when (it is paid or it is free) and it is not cancelled
admit place a new Order by staff
admit ship Order by staff
when a staff ships an Order: the Order is shipped
";

fn compile(src: &str) -> Result<suture_lang::ir::Program, String> {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).map_err(|e| e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))
}

#[test]
fn a_group_mixes_and_with_or() {
    let prog = compile(SHOP).unwrap();
    let sc = parse_scenarios("t.scenario", "scenario \"x\":
  when   staff Sam places a new Order O1 with total 50 EUR and free yes
  and    staff Sam places a new Order O2 with total 50 EUR
  and    staff Sam places a new Order O3 with total 50 EUR, paid yes and cancelled yes
  and    staff Sam ships O1
  and    staff Sam ships O2
         is refused by rule ship
  and    staff Sam ships O3
         is refused by rule ship
  and    staff Sam places a new Order O4 with total 250 EUR
         is refused by rule cap
  expect O1 is open
  expect O2 is not open
  expect O3 is not open
").unwrap();
    let r = run(&prog, &sc[0]);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
}

#[test]
fn the_read_back_keeps_the_group() {
    let prog = compile(SHOP).unwrap();
    let rule = prog.rules.iter().find(|r| r.name == "ship").unwrap();
    let words = suture_lang::readback::paraphrase(&prog, rule);
    assert!(words.contains("it is not cancelled and (it is paid or it is free)"), "{words}");
}

#[test]
fn without_parentheses_it_is_still_an_error() {
    let e = compile(&SHOP.replace("and (it is paid or it is free)", "and it is paid or it is free")).unwrap_err();
    assert!(e.contains("without parentheses"), "{e}");
}
