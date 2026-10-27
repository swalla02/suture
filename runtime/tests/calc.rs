//! Slice 64: calculations that decide. Conditionals, the larger or smaller
//! of two values, defined values, formulas known late, and 64 KiB units
//! (spec appendix BO).

use suture_runtime::host::Host;
use suture_runtime::state::Val;

fn compile(src: &str) -> suture_lang::ir::Program {
    suture_lang::compile(&[("calc.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{:?}", e.iter().map(|e| &e.msg).collect::<Vec<_>>()))
}

fn compile_err(src: &str) -> String {
    match suture_lang::compile(&[("calc.st".to_string(), src.to_string())]) {
        Ok(_) => panic!("compiled, but should not have"),
        Err(e) => e.iter().map(|e| e.msg.clone()).collect::<Vec<_>>().join("\n"),
    }
}

fn attrs(xs: &[(&str, Val)]) -> Vec<(String, Val)> {
    xs.iter().map(|(n, v)| (n.to_string(), v.clone())).collect()
}

fn attr(h: &Host, label: &str, name: &str) -> Option<Val> {
    h.unit(label).unwrap().root.attrs.get(name).cloned()
}

const RATES: &str = "module bags version 1, currency USD
role clerk
kind Rate { nth: Count, amount: Money }
kind Bag { nth: Count, fee: Money = its Base's Rate's amount }
kind Base { bag: Bag }
Bag needs Base
Base uses a Rate where the Rate's nth is the Bag's nth
admit add a new Rate or Bag by clerk
";

#[test]
fn a_formula_that_reads_a_grant_is_set_when_the_grant_comes() {
    let mut h = Host::new(compile(RATES));
    h.create("Rate", "R1", &attrs(&[("nth", Val::Count(1)), ("amount", Val::Money(3500))])).unwrap();
    h.create("Rate", "R2", &attrs(&[("nth", Val::Count(2)), ("amount", Val::Money(4500))])).unwrap();
    h.create("Bag", "B1", &attrs(&[("nth", Val::Count(1))])).unwrap();
    h.create("Bag", "B2", &attrs(&[("nth", Val::Count(2))])).unwrap();
    assert_eq!(attr(&h, "B1", "fee"), Some(Val::Money(3500)));
    assert_eq!(attr(&h, "B2", "fee"), Some(Val::Money(4500)));
    // A bag with no rate yet waits, and its fee is set once one comes.
    h.create("Bag", "B3", &attrs(&[("nth", Val::Count(3))])).unwrap();
    assert_eq!(attr(&h, "B3", "fee"), None);
    h.create("Rate", "R3", &attrs(&[("nth", Val::Count(3)), ("amount", Val::Money(15000))])).unwrap();
    assert_eq!(attr(&h, "B3", "fee"), Some(Val::Money(15000)));
}

const NOTES: &str = "module notes version 1, currency USD
role clerk
kind Note { text: Text }
admit add a new Note by clerk
";

#[test]
fn a_unit_holds_64_kib() {
    let mut h = Host::new(compile(NOTES));
    h.create("Note", "N1", &attrs(&[("text", Val::Text("x".repeat(10_000)))])).unwrap();
    let err = h.create("Note", "N2", &attrs(&[("text", Val::Text("x".repeat(70_000)))])).unwrap_err();
    assert!(err.contains("at most 65536"), "{err}");
}
