//! Slice 8: sourcing (spec appendix I).

use suture_lang::compile;
use suture_lang::ir::{Lit, Otherwise, Program, Ty};

fn prog(src: &str) -> Program {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn errors(src: &str) -> Vec<String> {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

const CREDIT: &str = "kind Customer { name: Text }
kind Order   { customer: Customer, total: Money, credit: Money, cancelled: Bool = no }
kind Payment { order: Order }
Order needs Payment while it is not cancelled
effect capture Payment via payments within 30 seconds
  sends    the order's total
  marks    sent
  reports  captured
source Order's credit via bureau within 2 seconds
  sends      the customer's name
  reports    found | fails with unknown
  good for   1 day
  otherwise  use 0 EUR
define: an Order is covered when its total is at most its credit
rule credit: no Order has a Payment that is sent unless its total is at most its credit,
             with credit at most 1 hour old
rule covered: no Order has a Payment that is sent unless it is covered
";

#[test]
fn a_source_compiles() {
    let p = prog(CREDIT);
    assert_eq!(p.sources.len(), 1);
    let s = &p.sources[0];
    assert_eq!((s.kind.as_str(), s.attr.as_str(), s.ty, s.adapter.as_str(), s.within_secs), ("Order", "credit", Ty::Money, "bureau", 2));
    assert_eq!(s.sends.len(), 1);
    assert_eq!(s.sends[0].0, "name");
    assert_eq!((s.success.as_str(), s.failures.clone()), ("found", vec!["unknown".to_string()]));
    assert_eq!(s.good_for, Some(86_400));
    assert_eq!(s.otherwise, Otherwise::Use(Lit::Money(0)));
    assert_eq!(p.request_within("Order~credit"), 2);
    assert_eq!(p.request_within("Payment"), 30);
}

#[test]
fn rules_know_the_sourced_facts_they_read() {
    let p = prog(CREDIT);
    let credit = p.rules.iter().find(|r| r.name == "credit").unwrap();
    assert_eq!(credit.sources, [("Order".to_string(), "credit".to_string())]);
    assert_eq!(credit.fresh, [("credit".to_string(), 3_600)]);
    // Through a definition too.
    let covered = p.rules.iter().find(|r| r.name == "covered").unwrap();
    assert_eq!(covered.sources, [("Order".to_string(), "credit".to_string())]);
    assert!(covered.fresh.is_empty());
}

#[test]
fn the_three_recoveries_compile() {
    let again = CREDIT.replace("otherwise  use 0 EUR", "otherwise  ask again after 1 hour");
    assert_eq!(prog(&again).sources[0].otherwise, Otherwise::Again(3_600));
    let act = CREDIT.replace("otherwise  use 0 EUR", "otherwise  it is cancelled");
    match &prog(&act).sources[0].otherwise {
        Otherwise::Act(actions) => assert_eq!((actions.len(), actions[0].attr.as_str(), actions[0].value), (1, "cancelled", true)),
        o => panic!("{o:?}"),
    }
    let none = CREDIT.replace("  otherwise  use 0 EUR\n", "").replace("  good for   1 day\n", "");
    let s = &prog(&none).sources[0];
    assert_eq!((s.good_for, &s.otherwise), (None, &Otherwise::Hold));
}

#[test]
fn misuses_are_errors() {
    let cases = [
        (CREDIT.replace("source Order's credit", "source Order's limit"), "t.st:10: an Order has no attribute `limit` to source"),
        (CREDIT.replace("otherwise  use 0 EUR", "otherwise  use \"none\""), "t.st:10: Order.credit is Money, but `otherwise use` gives Text"),
        (CREDIT.replace("credit: Money, cancelled", "credit: Money = total, cancelled"), "t.st:10: Order.credit is calculated by its formula, so it cannot be sourced"),
        (CREDIT.replace("credit: Money, cancelled", "credit: Money = 5 EUR, cancelled"), "t.st:10: Order.credit has a default, so it cannot be sourced"),
        (format!("{CREDIT}source Order's credit via other within 1 second\n  sends its total\n  reports found\n"), "t.st:19: Order.credit already has a source"),
        (CREDIT.replace("  reports    found | fails with unknown\n", ""), "t.st:10: a source needs a `reports` line"),
        (CREDIT.replace("  reports    found | fails with unknown", "  reports    found with x: Text"), "t.st:10: a source's answer carries only credit; drop `with x`"),
        (CREDIT.replace("with credit at most 1 hour old", "with total at most 1 hour old"), "t.st:16: rule credit: `with total at most 1 hour old`: the rule reads no sourced `total`"),
        (format!("{CREDIT}when a Payment is captured: its Order's credit becomes 5 EUR\n"), "t.st:19: Order.credit comes from its source; a reaction cannot set it"),
        (format!("{CREDIT}role staff\nadmit change Order's credit by staff\n"), "t.st:20: `admit change Order's credit by staff`: `credit` comes from its source, so it cannot be changed"),
        (format!("{CREDIT}kind Note {{ order: Order }}\n"), "t.st:10: Order is shared (Note refers to it), so its credit cannot be sourced in v0"),
        (CREDIT.replace("credit: Money, cancelled", "credit: Money, blocked: Bool, cancelled").replace("source Order's credit", "source Order's blocked").replace("  otherwise  use 0 EUR\n", ""), "t.st:10: Order.blocked is a state; v0 sources values, not states: source a Text or Count and compare it"),
        (CREDIT.replace("kind Payment { order: Order }", "kind Payment { order: Order, room: Money = order's credit }"), "t.st:4: Payment.room reads credit, which comes from its source and goes stale; a formula is computed when the entity is created"),
        (CREDIT.replace("cancelled: Bool = no }", "cancelled: Bool = no, kept: Money = 0 EUR }") + "when a Payment is captured: its Order's kept becomes its Order's credit\n", "t.st:19: `kept becomes …` reads credit, which comes from its source and goes stale; read it in a rule instead"),
        (CREDIT.replace("otherwise  use 0 EUR", "otherwise  its credit becomes 5 EUR"), "t.st:10: `credit` comes from a source; `otherwise` cannot set it"),
    ];
    for (src, want) in cases {
        let e = errors(&src);
        assert!(e.iter().any(|x| x == want), "want {want}\ngot {e:#?}");
    }
}
