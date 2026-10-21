//! Slice 29: an attribute with no default must be given (spec appendix AE).

use suture_lang::ir::Program;
use suture_lang::scenario::Caller;
use suture_runtime::host::Host;
use suture_runtime::state::Val;

fn prog(src: &str) -> Program {
    suture_lang::compile(&[("app.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

const LEAVE: &str = "module leave version 1, currency EUR
role staff
kind Approver { user: Text }
kind Leave { approver: Approver, days: Count, note: Text = \"\", half: Bool = no,
             status: open | approved, items: list of Line, total: Count = days + 1 }
kind Line { leave: Leave, day: Count }
admit file a new Leave by staff
";

fn staff() -> Caller {
    Caller { role: "staff".into(), name: "Sam".into(), verb: "files".into() }
}

#[test]
fn an_admitted_entity_without_a_required_attribute_is_refused_at_the_gate() {
    let mut h = Host::new(prog(LEAVE));
    h.create("Approver", "A", &[("user".into(), Val::Text("a".into()))]).unwrap();
    let e = h.input_new(Some(&staff()), "Leave", "L1", &[("days".into(), Val::Count(2))], &[]).unwrap_err();
    assert_eq!(e, "refused at the gate: a new Leave needs its approver: it has no default");
    assert!(h.unit("L1").is_none());
    assert!(h.gate.iter().any(|l| l.contains("staff Sam files a new Leave L1: a new Leave needs its approver")), "{:?}", h.gate);
    let e = h.input_new(Some(&staff()), "Leave", "L2", &[], &[]).unwrap_err();
    assert_eq!(e, "refused at the gate: a new Leave needs its approver and days: they have no default");
}

#[test]
fn defaults_states_formulas_and_an_elements_owner_are_not_required() {
    let mut h = Host::new(prog(LEAVE));
    h.create("Approver", "A", &[("user".into(), Val::Text("a".into()))]).unwrap();
    let given = [("approver".into(), Val::Ref("A".into())), ("days".into(), Val::Count(2))];
    h.input_new(Some(&staff()), "Leave", "L1", &given, &[("items".into(), vec![("day".into(), Val::Count(1))])]).unwrap();
    assert!(h.unit("L1").is_some());
    let e = h.input_new(Some(&staff()), "Leave", "L2", &given, &[("items".into(), vec![])]).unwrap_err();
    assert_eq!(e, "refused at the gate: a new Line needs its day: it has no default");
}

#[test]
fn a_given_entity_without_a_required_attribute_is_an_error() {
    let mut h = Host::new(prog(LEAVE));
    let e = h.create("Approver", "A", &[]).unwrap_err();
    assert_eq!(e, "a new Approver needs its user: it has no default");
}

#[test]
fn an_attribute_an_effects_answer_sets_is_not_required() {
    let src = "module quotes version 1, currency EUR
role staff
kind Quote { amount: Money }
effect price Quote via pricing within 1 hour
  sends    its amount
  reports  priced with offer: Money
admit ask a new Quote by staff
";
    let mut h = Host::new(prog(src));
    let ask = Caller { role: "staff".into(), name: "Sam".into(), verb: "asks".into() };
    h.input_new(Some(&ask), "Quote", "Q1", &[("amount".into(), Val::Money(500))], &[]).unwrap_or_else(|e| panic!("{e}"));
    assert!(h.unit("Q1").is_some());
}
