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

const SHOP: &str = "kind Country   { restricted: Bool = no }
kind Product   { name: Text }
kind Warehouse { country: Country }
kind Carrier   { countries: set of Country, fee: Money, express: Bool = no }
kind Stock     { warehouse: Warehouse, product: Product, units: Count }
kind Order     { country: Country, lines: list of Line }
kind Line      { order: Order, product: Product, quantity: Count }
kind Shipment  { order: Order }
Order needs Shipment
Shipment uses a Carrier where the Carrier's countries contain the Order's country
Shipment uses a Stock per line where the Stock's product is the line's product,
  taking the line's quantity
effect send Shipment via carriers within 7 days
  sends the Order's country, its Carrier's fee
  reports delivered | fails with lost
rule stock: the total quantity taken from each Stock is at most its units
";

#[test]
fn uses_binds_to_a_pool_key() {
    let p = prog(&format!("{SHOP}prefer Carrier: lowest fee, then those that are express\nprefer Stock for a line: those whose warehouse's country is the Order's country\n"));
    assert_eq!(p.candidates, 8);
    let carrier = &p.bindings[0];
    assert_eq!((carrier.subject.as_str(), carrier.target.as_str(), carrier.per.as_deref()), ("Shipment", "Carrier", None));
    assert_eq!((carrier.key_attr.as_str(), carrier.key_member), ("countries", true));
    assert_eq!(carrier.key, Num::Path(path(&[Step::Owner, attr("country")])));
    assert!(carrier.filter.is_empty());
    assert_eq!(
        carrier.prefer,
        vec![
            suture_lang::ir::Criterion::Lowest(Num::Path(path(&[Step::Cand, attr("fee")]))),
            suture_lang::ir::Criterion::Those(Cond::State { node: path(&[Step::Cand]), state: "express".into(), negated: false }),
        ]
    );
    let stock = &p.bindings[1];
    assert_eq!(stock.per.as_deref(), Some("lines"));
    assert_eq!((stock.key_attr.as_str(), stock.key_member), ("product", false));
    assert_eq!(stock.key, Num::Path(path(&[Step::Elem, attr("product")])));
    let t = stock.taking.as_ref().unwrap();
    assert_eq!((t.name.as_str(), t.ty), ("quantity", Ty::Count));
    assert_eq!(t.amount, Num::Path(path(&[Step::Elem, attr("quantity")])));
    let want = Cond::Cmp {
        left: Num::Path(path(&[Step::Cand, Step::Ref("warehouse".into()), attr("country")])),
        op: Op::Eq,
        right: Num::Path(path(&[Step::Owner, attr("country")])),
    };
    assert_eq!(stock.prefer, vec![suture_lang::ir::Criterion::Those(want)]);
    assert_eq!(p.effects[0].sends[1].1, path(&[Step::Bound("Carrier".into()), attr("fee")]));
    let rule = p.rules.iter().find(|r| r.name == "stock").unwrap();
    assert_eq!(rule.tier, "per-group");
    assert_eq!(rule.formula, "for each Stock s: taken_quantity(s) ≤ s.units");
    assert_eq!(rule.paraphrase, "each stock on its own: the total quantity taken from it is at most its units");
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    let p = prog(SHOP);
    assert!(p.warnings.iter().any(|w| w.contains("Carrier has no `prefer`; Carriers are chosen in the order they were created")), "{:?}", p.warnings);
}

#[test]
fn the_candidates_setting() {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR, candidates 3\n{SHOP}"))];
    assert_eq!(compile(&files).unwrap().candidates, 3);
    let files = vec![("t.st".to_string(), "module t version 1, currency EUR, candidates 0\n".to_string())];
    let e: Vec<String> = compile(&files).unwrap_err().iter().map(|e| e.to_string()).collect();
    assert!(e[0].contains("`candidates` must be at least 1"), "{e:?}");
}

#[test]
fn uses_mistakes_are_errors() {
    let e = errors(&SHOP.replace("where the Carrier's countries contain the Order's country", "where the Carrier's fee is at most 5 EUR"));
    assert!(e.iter().any(|e| e.contains("`Shipment uses a Carrier` needs a pool key")), "{e:?}");
    let e = errors(&SHOP.replace("per line", "per box"));
    assert!(e.iter().any(|e| e.contains("`per box`: neither a Shipment nor an owner of it has a list of boxs")), "{e:?}");
    let e = errors(&SHOP.replace("uses a Carrier", "uses a Line"));
    assert!(e.iter().any(|e| e.contains("Line is a part, so it is not shared")), "{e:?}");
    let e = errors(&SHOP.replace("taking the line's quantity", "taking the line's quantity + 1"));
    assert!(e.iter().any(|e| e.contains("name the attribute taken")), "{e:?}");
    let e = errors(&SHOP.replace("quantity taken", "units taken"));
    assert!(e.iter().any(|e| e.contains("no `uses a Stock` takes units")), "{e:?}");
    let e = errors(&format!("{SHOP}prefer Carrier: lowest countries\n"));
    assert!(e.iter().any(|e| e.contains("`its countries` is Set; `lowest` and `highest` rank")), "{e:?}");
    let e = errors(&format!("{SHOP}prefer Stock: lowest units\n"));
    assert!(e.iter().any(|e| e.contains("prefer Stock: it does not match how its `uses` binds; write `prefer Stock for a line`")), "{e:?}");
    let e = errors(&format!("{SHOP}prefer Product: lowest name\n"));
    assert!(e.iter().any(|e| e.contains("prefer Product: no kind `uses` a Product")), "{e:?}");
    let e = errors(&format!("{SHOP}prefer Carrier: Stock\n"));
    assert!(e.iter().any(|e| e.contains("prefer Carrier: rank the Carriers with `lowest …`")), "{e:?}");
    let e = errors(&format!("{SHOP}rule x: every Shipment's Carrier is cheap\n"));
    assert!(e.iter().any(|e| e.contains("`its Carrier` is a shared Carrier; compare its attributes, or a state: express")), "{e:?}");
}

const REFUNDS: &str = "kind Country  { restricted: Bool = no }
kind Customer { country: Country }
kind Order    { customer: Customer, total: Money, country: Country = customer's country }
kind Shipment { order: Order }
kind Refund   { order: Order, customer: Customer = order's customer, amount: Money = order's total }
Delivery is Shipment or Refund
Order needs Delivery
prefer Delivery: Shipment, then Refund after 2 days
effect send Shipment via carriers within 7 days
  sends the Order's country
  reports delivered
effect pay Refund via bank within 1 hour
  sends its amount
  reports paid
rule embargo: no Order whose country is restricted has a Shipment
";

#[test]
fn relations_across_units_periods_and_global_rules() {
    let p = prog(&format!(
        "{REFUNDS}rule cap: for each Customer and month, the total amount of their Refunds is at most 100 EUR
rule few: the number of each Customer's Refunds is at most 3
rule budget: the total amount of all Refunds is at most 1000 EUR, checked globally
"
    ));
    let b = p.bindings.iter().find(|b| b.id == "Customer(amount of Refunds per month)").expect("an implicit binding");
    assert_eq!((b.subject.as_str(), b.target.as_str(), b.key_attr.as_str()), ("Refund", "Customer", ""));
    assert_eq!(b.key, Num::Path(path(&[attr("customer")])));
    assert_eq!(b.taking.as_ref().unwrap().period.as_deref(), Some("month"));
    assert!(p.bindings.iter().any(|b| b.id == "Customer(number of Refunds)" && b.subject == "Refund"));
    assert!(p.bindings.iter().any(|b| b.id == "all Refunds(amount of Refunds)" && b.target == "all Refunds"));
    assert_eq!(p.globals, ["all Refunds"]);
    let rule = |n: &str| p.rules.iter().find(|r| r.name == n).unwrap();
    assert_eq!(rule("cap").tier, "per-group");
    assert_eq!(rule("cap").period.as_deref(), Some("month"));
    assert_eq!(rule("few").tier, "per-group");
    assert_eq!(rule("budget").tier, "global (bottleneck)");
    assert!(rule("embargo").fixed && !rule("cap").fixed);
    assert!(rule("cap").formula.contains("where r.customer = c, per month of r.created: r.amount"), "{}", rule("cap").formula);
    assert!(rule("cap").paraphrase.contains("the total amount of its Refunds in a month"), "{}", rule("cap").paraphrase);
    assert!(rule("budget").paraphrase.contains("all Refunds"), "{}", rule("budget").paraphrase);
}

#[test]
fn relation_and_period_mistakes_are_errors() {
    let e = errors(&format!("{REFUNDS}rule budget: the total amount of all Refunds is at most 1000 EUR\n"));
    assert!(e[0].contains("`all Refunds` has no group key") && e[0].contains("checked globally"), "{e:?}");
    let e = errors(&format!("{REFUNDS}rule small: every Order's total is at most 5 EUR, checked globally\n"));
    assert!(e[0].contains("only a rule about `all …` is checked globally"), "{e:?}");
    let e = errors(&format!("{REFUNDS}rule cap: for each Customer and fortnight, the number of their Refunds is at most 2\n"));
    assert!(e[0].contains("a period is a day, week, month or year"), "{e:?}");
    let e = errors(&format!("{REFUNDS}rule cap: for each Order and month, its total is at most 5 EUR\n"));
    assert!(e[0].contains("a total per period adds up a relation across units"), "{e:?}");
    let e = errors(&format!("{REFUNDS}rule cap: the number of each Customer's Refunds that are paid is at most 2\n"));
    assert!(e[0].contains("cannot be filtered yet"), "{e:?}");
    let e = errors(&format!("{REFUNDS}rule cap: the total amount of all Pigs is at most 2 EUR, checked globally\n"));
    assert!(e[0].contains("no kind is called Pigs"), "{e:?}");
}
