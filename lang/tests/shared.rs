//! Slice 5: shared entities (spec appendix F).

use suture_lang::compile;
use suture_lang::ir::{Cond, Num, Op, Path, Program, Step, Ty};

fn prog(src: &str) -> Program {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn errors(src: &str) -> Vec<String> {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

fn path(steps: &[Step]) -> Path {
    Path { steps: steps.to_vec() }
}

fn attr(a: &str) -> Step {
    Step::Attr(a.into())
}

const REFS: &str = "kind Country  { code: Text, restricted: Bool }
kind Customer { name: Text, country: Country }
kind Carrier  { name: Text, countries: set of Country }
kind Order    { customer: Customer, total: Money, country: Country = customer's country, cancelled: Bool = no }
kind Payment  { order: Order }
Order needs Payment while it is not cancelled
effect capture Payment via payments within 30 seconds
  sends the order's total, the order's customer's name
  reports captured
rule embargo: no Order whose country is restricted has a Payment
rule home:    every Order's country is the Customer's country
";

#[test]
fn references_sets_and_reading_through_them() {
    let p = prog(REFS);
    let order = p.kind("Order").unwrap();
    let customer = order.attr("customer").unwrap();
    assert_eq!((customer.ty, customer.of.as_deref()), (Ty::Ref, Some("Customer")));
    assert_eq!(order.attr("country").unwrap().formula, Some(Num::Path(path(&[Step::Ref("customer".into()), attr("country")]))));
    let carrier = p.kind("Carrier").unwrap().attr("countries").unwrap();
    assert_eq!((carrier.ty, carrier.of.as_deref()), (Ty::Set, Some("Country")));
    assert!(p.kind("Payment").unwrap().owner.is_some(), "a reference to the owner still names the owner");
    let embargo = p.rules.iter().find(|r| r.name == "embargo").unwrap();
    assert_eq!(embargo.filter, vec![Cond::State { node: path(&[Step::Ref("country".into())]), state: "restricted".into(), negated: false }]);
    assert_eq!(embargo.tier, "local");
    assert_eq!(embargo.paraphrase, "no order where its country is restricted ever has a payment");
    let home = p.rules.iter().find(|r| r.name == "home").unwrap();
    let want = Cond::Cmp {
        left: Num::Path(path(&[attr("country")])),
        op: Op::Eq,
        right: Num::Path(path(&[Step::Ref("customer".into()), attr("country")])),
    };
    assert_eq!(home.body, suture_lang::ir::Body::Every(want), "`the Customer` is the subject's unique Customer");
    assert_eq!(home.formula, "for each Order o: o.country = o.customer.country");
    assert_eq!(p.effects[0].sends[1].1, path(&[Step::Owner, Step::Ref("customer".into()), attr("name")]));
}

#[test]
fn contains_reads_a_set() {
    let p = prog(&format!("{REFS}kind Route {{ carrier: Carrier, to: Country }}\nrule served: every Route's carrier's countries contain its to\n"));
    let r = p.rules.iter().find(|r| r.name == "served").unwrap();
    let want = Cond::Contains {
        set: Num::Path(path(&[Step::Ref("carrier".into()), attr("countries")])),
        item: Num::Path(path(&[attr("to")])),
        negated: false,
    };
    assert_eq!(r.body, suture_lang::ir::Body::Every(want));
    assert_eq!(r.paraphrase, "each route on its own: its carrier's countries contains its to");
    assert_eq!(r.formula, "for each Route r: r.to ∈ r.carrier.countries");
    let p = prog(&format!("{REFS}kind Route {{ carrier: Carrier, to: Country }}\nrule served: every Route's carrier's countries do not contain its to\n"));
    assert!(!p.rules.is_empty());
}

#[test]
fn time_and_durations() {
    let p = prog(
        "kind Trial { length: Duration = 14 days, ends: Time = created + length }
rule short: every Trial's length is at most 30 days
rule order: every Trial's ends is at least its created + 1 day
",
    );
    let t = p.kind("Trial").unwrap();
    assert_eq!(t.attr("length").unwrap().default, Some(suture_lang::ir::Lit::Duration(14 * 86_400)));
    assert_eq!(t.attr("ends").unwrap().ty, Ty::Time);
    let short = p.rules.iter().find(|r| r.name == "short").unwrap();
    assert_eq!(short.paraphrase, "each trial on its own: its length is at most 30 days");
    let e = errors("kind T { a: Time, b: Time }\nrule x: every T's a is at most its a + its b\n");
    assert!(e.iter().any(|e| e.contains("cannot add a Time and a Time")), "{e:?}");
    let e = errors("kind T { a: Time }\nrule x: every T's a is at most 1 day\n");
    assert!(e.iter().any(|e| e.contains("cannot compare a Time with a Duration")), "{e:?}");
    let e = errors("kind T { created: Time }\n");
    assert!(e.iter().any(|e| e.contains("every entity has `created`")), "{e:?}");
}

#[test]
fn reference_mistakes_are_errors() {
    let e = errors("kind A { b: B }\n");
    assert!(e.iter().any(|e| e.contains("t.st:2: A.b: unknown kind B")), "{e:?}");
    let e = errors("kind O { x: Text }\nkind P { o: O }\nkind Q { p: P }\nO needs P\n");
    assert!(e.iter().any(|e| e.contains("Q.p: P is a part of O, so nothing outside an O can refer to it")), "{e:?}");
    let e = errors(&format!("{REFS}rule x: every Order's customer's name's length is at most 3\n"));
    assert!(e.iter().any(|e| e.contains("name is a value, so it has no `length`")), "{e:?}");
    let e = errors(&format!("{REFS}rule x: every Order's customer is restricted\n"));
    assert!(e.iter().any(|e| e.contains("`its customer` refers to a Customer; compare it with another reference to a Customer")), "{e:?}");
    let e = errors(&format!("{REFS}rule x: every Order's customer is its country\n"));
    assert!(e.iter().any(|e| e.contains("`its customer` refers to a Customer; `its country` does not")), "{e:?}");
    let e = errors(&format!("{REFS}rule x: every Customer's country's Payment is captured\n"));
    assert!(e.iter().any(|e| e.contains("a Country is shared, so only its attributes can be read")), "{e:?}");
    let e = errors(&format!("{REFS}rule x: every Order's total contains its country\n"));
    assert!(e.iter().any(|e| e.contains("`its total` is not a set")), "{e:?}");
    let e = errors(&format!("{REFS}kind Bad {{ c: Carrier }}\nrule x: every Bad's c's countries contain its c\n"));
    assert!(e.iter().any(|e| e.contains("holds references to Countries; `its c` is not one")), "{e:?}");
    let e = errors("kind C { x: Text }\nkind A { t: Text, c: C = t }\n");
    assert!(e.iter().any(|e| e.contains("A.c refers to a C, so its formula must name one")), "{e:?}");
    let e = errors(&format!("{REFS}when a Payment is captured: its order's customer's name becomes its order's customer's name\n"));
    assert!(e.iter().any(|e| e.contains("is a shared Customer; a reaction changes only its own unit")), "{e:?}");
}
