//! Slice 5: shared entities (spec appendix F).

use suture_lang::ir::Program;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::host::Host;
use suture_runtime::interp::holds;
use suture_runtime::scenario::run;
use suture_runtime::state::Val;

const SRC: &str = "module t version 1, currency EUR
kind Country  { restricted: Bool = no }
kind Customer { name: Text, country: Country }
kind Carrier  { countries: set of Country }
kind Order    { customer: Customer, carrier: Carrier, total: Money, country: Country = customer's country }
kind Payment  { order: Order }
Order needs Payment
effect capture Payment via payments within 30 seconds
  sends the order's total, the order's customer's name
  reports captured
rule embargo: no Order whose country is restricted has a Payment
rule served:  every Order's carrier's countries contain its country
";

fn prog() -> Program {
    suture_lang::compile(&[("t.st".to_string(), SRC.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn data(pairs: &[(&str, Val)]) -> Vec<(String, Val)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

fn world(h: &mut Host) {
    h.create("Country", "DE", &[]).unwrap();
    h.create("Country", "FR", &[]).unwrap();
    h.create("Country", "IR", &data(&[("restricted", Val::Bool(true))])).unwrap();
    h.create("Carrier", "K1", &data(&[("countries", Val::Set(vec!["DE".into(), "IR".into()]))])).unwrap();
    h.create("Customer", "C1", &data(&[("name", Val::Text("Ada".into())), ("country", Val::Text("DE".into()))])).unwrap();
    h.create("Customer", "C2", &data(&[("name", Val::Text("Bo".into())), ("country", Val::Text("IR".into()))])).unwrap();
    h.create("Customer", "C3", &data(&[("name", Val::Text("Cy".into())), ("country", Val::Text("FR".into()))])).unwrap();
}

fn order(h: &mut Host, label: &str, customer: &str) -> Result<(), String> {
    let given = data(&[("customer", Val::Text(customer.into())), ("carrier", Val::Text("K1".into())), ("total", Val::Money(5_000))]);
    h.create("Order", label, &given)
}

#[test]
fn a_unit_reads_the_shared_entities_it_refers_to() {
    let mut h = Host::new(prog());
    world(&mut h);
    order(&mut h, "O1", "C1").unwrap();
    let u = h.unit("O1").unwrap();
    assert_eq!(u.root.attrs["customer"], Val::Ref("C1".into()));
    assert_eq!(u.root.attrs["country"], Val::Ref("DE".into()), "a formula reads through the reference");
    assert_eq!(u.facts.keys().collect::<Vec<_>>(), ["C1", "DE", "IR", "K1"], "facts are carried transitively");
    let ask = h.pending()[0].clone();
    assert_eq!(ask.sends[1], ("name".to_string(), Val::Text("Ada".into())));
    assert!(h.machine.faults().is_empty(), "{:?}", h.machine.faults());
}

#[test]
fn rules_read_carried_facts() {
    let mut h = Host::new(prog());
    world(&mut h);
    order(&mut h, "O2", "C2").unwrap();
    let u = h.unit("O2").unwrap();
    assert!(u.root.parts.is_empty(), "the embargo keeps the Payment out:\n{}", h.trace.join("\n"));
    assert!(h.trace.join("\n").contains("Payment waits: rule embargo"), "{}", h.trace.join("\n"));
    let e = order(&mut h, "O3", "C3").unwrap_err();
    assert!(e.contains("refused by rule served"), "{e}");
}

#[test]
fn references_name_units_of_the_right_kind() {
    let mut h = Host::new(prog());
    world(&mut h);
    let e = order(&mut h, "O4", "DE").unwrap_err();
    assert_eq!(e, "Order.customer refers to a Customer, but DE is a Country");
    let e = order(&mut h, "O4", "C9").unwrap_err();
    assert_eq!(e, "Order.customer: there is no Customer C9");
    let e = h.create("Order", "O4", &data(&[("total", Val::Count(3))])).unwrap_err();
    assert_eq!(e, "Order.total is Money, not 3");
}

#[test]
fn entities_know_when_they_were_created() {
    let src = "module t version 1, currency EUR
kind Trial { length: Duration = 14 days, ends: Time = created + length }
";
    let p = suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap();
    let mut h = Host::new(p);
    h.pass(2 * 86_400 + 3_600).unwrap();
    h.create("Trial", "T1", &[]).unwrap();
    let u = h.unit("T1").unwrap();
    assert_eq!(u.root.created, 2 * 86_400 + 3_600);
    assert_eq!(u.root.attrs["ends"].show("EUR"), "2026-01-17 01:00");
}

#[test]
fn scenarios_give_shared_entities() {
    let src = "scenario \"carried\":
  given  a Country DE
  and    a Country IR with restricted yes
  and    a Carrier K1 with countries {DE, IR}
  and    a Customer C1 with name Ada and country DE
  and    a Customer C2 with name Bo and country IR
  when   a new Order O1 with customer C1 and carrier K1 and total 50 EUR
  and    a new Order O2 with customer C2 and carrier K1 and total 50 EUR
  expect O1 has a Payment
  expect O2 has no Payment
";
    let sc = &parse_scenarios("t.scenario", src).unwrap()[0];
    let r = run(&prog(), sc);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let u = r.host.unit("O1").unwrap();
    let c = suture_lang::ir::Cond::Contains {
        set: suture_lang::ir::Num::Path(suture_lang::ir::Path { steps: vec![suture_lang::ir::Step::Ref("carrier".into()), suture_lang::ir::Step::Attr("countries".into())] }),
        item: suture_lang::ir::Num::Path(suture_lang::ir::Path { steps: vec![suture_lang::ir::Step::Attr("country".into())] }),
        negated: false,
    };
    assert!(holds(&r.host.prog, &u, &[], &c));
}
