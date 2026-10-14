use suture_lang::ir::{Cond, Lit, Op, Operand, Path, Program, Step};
use suture_lang::{compile, compile_dir};

fn shop() -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop");
    compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"))
}

fn path(steps: &[Step]) -> Path {
    Path { steps: steps.to_vec() }
}

fn errors(src: &str) -> Vec<String> {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

#[test]
fn compiles_the_shop_example() {
    let p = shop();
    assert_eq!((p.module.as_str(), p.version, p.currency.as_str()), ("shop", 1, "EUR"));
    let form = p.kind("CustomsForm").unwrap();
    assert_eq!(form.owner.as_ref().map(|o| (o.attr.as_str(), o.kind.as_str())), Some(("order", "Order")));
    assert!(p.kind("Declaration").unwrap().attr("contents").is_some(), "answers add attributes");
    let names: Vec<&str> = p.rules.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["declared", "licence", "hold", "paid", "late"]);
    assert!(p.rules.iter().all(|r| r.tier == "local"));
}

#[test]
fn read_backs_are_fixed_text() {
    let p = shop();
    let r = |n: &str| p.rules.iter().find(|r| r.name == n).unwrap().clone();
    assert_eq!(r("declared").formula, "for each Declaration d: d.value ≤ d.form.order.total");
    assert_eq!(r("declared").paraphrase, "each declaration on its own: its value is at most its form's order's total");
    assert_eq!(
        r("hold").formula,
        "for each Order o: o.country = \"France\" and exists(s in o.Shipment where s is sent) implies o.CustomsForm is done"
    );
    assert_eq!(
        r("hold").paraphrase,
        "no order where its country is \"France\" ever has a shipment that is sent unless its customs form is done"
    );
    assert_eq!(r("late").formula, "for each Order o: not (o is cancelled and exists(s in o.Shipment where s is sent))");
    assert_eq!(r("late").paraphrase, "no order where it is cancelled ever has a shipment that is sent");
}

#[test]
fn bare_words_are_text_or_states() {
    let p = shop();
    let form = p.needs.iter().find(|n| n.part == "CustomsForm").unwrap();
    let country = path(&[Step::Attr("country".into())]);
    assert_eq!(form.cond, Some(Cond::Cmp { left: country, op: Op::Eq, right: Operand::Lit(Lit::Text("France".into())) }));
    let pay = p.needs.iter().find(|n| n.part == "Payment").unwrap();
    assert_eq!(pay.cond, Some(Cond::State { node: path(&[]), state: "cancelled".into(), negated: true }));
}

#[test]
fn ambiguous_names_are_errors() {
    let e = errors("kind A { paid: Bool }\ndefine: an A is paid when it is done\n");
    assert!(e.iter().any(|m| m.contains("already has a state or attribute called `paid`")), "{e:?}");
    let e = errors("kind A { }\nkind B { }\nkind P { a: A, b: B }\nA needs P\nB needs P\n");
    assert!(e.iter().any(|m| m.contains("already a part of A")), "{e:?}");
}

#[test]
fn unknown_names_and_types_are_errors() {
    let e = errors("kind A { n: Count }\nrule r: every A's colour is red\n");
    assert!(e.iter().any(|m| m.contains("has no attribute, part or owner called `colour`")), "{e:?}");
    let e = errors("kind A { n: Count, t: Text }\nrule r: every A's n is at most its t\n");
    assert!(e.iter().any(|m| m.contains("cannot compare a Count with a Text")), "{e:?}");
    let e = errors("kind A { m: Money }\nrule r: every A's m is at most 5 USD\n");
    assert!(e.iter().any(|m| m.contains("not in this module's currency")), "{e:?}");
}

#[test]
fn states_with_the_same_name_are_errors() {
    let base = "kind O { }\nkind P { o: O, n: Count, sent: Bool }\nO needs P\n";
    let e = errors(&format!("{base}effect send P via c within 1 days\n  sends its n\n  marks sent\n  reports done\n"));
    assert!(e.iter().any(|m| m.contains("P has an attribute called sent")), "{e:?}");
    assert!(e.iter().any(|m| m.contains("`done` is a state of every kind")), "{e:?}");
    let e = errors("kind O { }\nkind P { o: O, n: Count }\nO needs P\neffect send P via c within 1 days\n  sends its n\n  marks captured\n  reports captured | fails with captured\n");
    assert!(e.iter().any(|m| m.contains("P's effect uses the name captured twice")), "{e:?}");
    let e = errors("kind O { failed: Bool }\n");
    assert!(e.iter().any(|m| m.contains("`failed` is a state of every kind")), "{e:?}");
}

#[test]
fn an_ownership_loop_is_reported_not_followed_forever() {
    let e = errors("kind A { b: B, n: Count }\nkind B { a: A }\nkind Zed { n: Count }\nA needs B\nB needs A\nrule r: every A's n is at most the Zed's n\n");
    assert!(e.iter().any(|m| m.contains("needs loop")), "{e:?}");
}

#[test]
fn a_part_needed_twice_is_an_error() {
    let e = errors("kind O { c: Bool = no }\nkind P { o: O }\nO needs P\nO needs P while it is c\n");
    assert!(e.iter().any(|m| m.contains("O needs P twice")), "{e:?}");
}
