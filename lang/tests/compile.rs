use suture_lang::ir::{Cond, Lit, Num, Op, Path, Program, Step};
use suture_lang::{compile, compile_dir};

fn shop() -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../fixtures/shop-slice4");
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
    assert_eq!(names, ["declared", "licence", "hold", "paid", "refund", "late", "cap"]);
    assert!(p.rules.iter().all(|r| r.tier == if r.name == "cap" { "per-group" } else { "local" }));
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
    assert_eq!(form.cond, Some(Cond::Cmp { left: Num::Path(country), op: Op::Eq, right: Num::Lit(Lit::Text("France".into())) }));
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

#[test]
fn timed_out_is_a_state_of_every_effect() {
    let p = shop();
    assert!(p.states("Payment").contains(&"timed out".to_string()));
    assert!(!p.states("Order").contains(&"timed out".to_string()));
    let e = errors("kind O { c: Bool = no }\nkind P { o: O }\nO needs P\neffect pay P via payments within 30 seconds\n  sends its o's c\n  reports paid\nwhen a P is timed out: its O is c\ndefine: an O is stuck when its P is timed out\n");
    assert!(e.is_empty(), "{e:?}");
}

#[test]
fn slots_compile_with_their_prefer_order_and_patience() {
    let p = shop();
    let s = p.slot("Delivery").expect("the Delivery slot");
    assert_eq!((s.owner.as_str(), s.alternatives.clone()), ("Order", vec!["Shipment".to_string(), "Refund".to_string()]));
    assert_eq!(s.patience, vec![Some(2 * 86_400), None]);
    assert!(p.warnings.is_empty(), "{:?}", p.warnings);
    assert_eq!(p.states("Delivery"), vec!["done".to_string(), "failed".to_string()]);
    let settled = p.defines.iter().find(|d| d.state == "settled").unwrap();
    assert_eq!(
        settled.cond,
        Cond::State { node: path(&[Step::Slot("Delivery".into())]), state: "done".into(), negated: false }
    );
}

#[test]
fn a_slot_without_prefer_warns() {
    let files = vec![(
        "t.st".to_string(),
        "module t version 1, currency EUR\nkind O { }\nkind A { o: O }\nkind B { o: O }\nO needs D\nD is A or B\n".to_string(),
    )];
    let p = compile(&files).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(p.slot("D").unwrap().alternatives, vec!["A".to_string(), "B".to_string()]);
    assert!(p.warnings.iter().any(|w| w.contains("D has no `prefer`")), "{:?}", p.warnings);
}

#[test]
fn slot_mistakes_are_errors() {
    let base = "kind O { n: Count }\nkind A { o: O }\nkind B { o: O }\n";
    let e = errors(&format!("{base}O needs D\nD is A or C\n"));
    assert!(e.iter().any(|m| m.contains("unknown kind C")), "{e:?}");
    let e = errors(&format!("{base}O needs D\nD is A or B\nprefer D: A\n"));
    assert!(e.iter().any(|m| m.contains("prefer D must list each of A, B once")), "{e:?}");
    let e = errors(&format!("{base}O needs D, A\nD is A or B\n"));
    assert!(e.iter().any(|m| m.contains("A is an alternative of D")), "{e:?}");
    let e = errors(&format!("{base}D is A or B\n"));
    assert!(e.iter().any(|m| m.contains("no kind needs the slot D")), "{e:?}");
    let e = errors(&format!("{base}O needs D\nD is A or B\nrule r: every O's D's n is at most 3\n"));
    assert!(e.iter().any(|m| m.contains("D is a slot")), "{e:?}");
    let e = errors(&format!("{base}O needs D\nD is A or B\nprefer D: A, then B after 1 day, then A\n"));
    assert!(e.iter().any(|m| m.contains("prefer D must list each of A, B once")), "{e:?}");
    let e = errors(&format!("{base}O needs D\nD is A or B\nprefer D: lowest n\n"));
    assert!(e.iter().any(|m| m.contains("prefer D: D is a slot; list its alternatives in order: A, B")), "{e:?}");
}

#[test]
fn has_a_slot_is_an_error() {
    let e = errors("kind O { }\nkind A { o: O }\nkind B { o: O }\nO needs D\nD is A or B\nprefer D: A, then B\ndefine: an O is going when it has a D\n");
    assert!(e.iter().any(|m| m.contains("D is a slot; name an alternative")), "{e:?}");
}

const NUMS: &str = "kind Order { total: Money, fee: Money = 0 EUR, items: Count = 1 }
kind Refund { order: Order }
kind Shipment { order: Order }
Order needs Refund, Shipment
effect pay Refund via payments within 30 seconds
  sends    the order's total
  reports  refunded with amount: Money | fails with bounced
effect send Shipment via carriers within 1 day
  sends    the order's total
  reports  delivered | fails with lost
";

fn prog(src: &str) -> Program {
    compile(&[("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn rule<'a>(p: &'a Program, name: &str) -> &'a suture_lang::ir::Rule {
    p.rules.iter().find(|r| r.name == name).unwrap()
}

#[test]
fn totals_counts_and_math_read_back() {
    let p = prog(&format!(
        "{NUMS}rule cap: the total amount of each Order's Refunds is at most its total
rule few: the number of each Order's Shipments that are lost is at most 2
rule fees: every Order's fee is at most its total / 10 + 1 EUR
rule big: no Order whose total is at least 1000 EUR has a Shipment or a Refund unless its items is more than 2 * (3 - 1)
"
    ));
    let cap = rule(&p, "cap");
    assert_eq!(cap.formula, "for each Order o: sum(r in o.refunds: r.amount) ≤ o.total");
    assert_eq!(cap.paraphrase, "each order on its own: the total amount of its refunds is at most its total");
    assert_eq!(cap.tier, "per-group");
    assert_eq!(rule(&p, "few").formula, "for each Order o: count(s in o.shipments where s is lost) ≤ 2");
    assert_eq!(rule(&p, "fees").formula, "for each Order o: o.fee ≤ o.total / 10 + 1 EUR");
    assert_eq!(rule(&p, "fees").tier, "local");
    assert_eq!(
        rule(&p, "big").formula,
        "for each Order o: o.total ≥ 1000 EUR and (exists(s in o.Shipment) or exists(r in o.Refund)) implies o.items > 2 × (3 − 1)"
    );
}

#[test]
fn number_mistakes_are_errors() {
    let e = errors(&format!("{NUMS}rule r: every Order's total is at most its total * its fee\n"));
    assert!(e.iter().any(|m| m.contains("cannot multiply a Money by a Money")), "{e:?}");
    let e = errors(&format!("{NUMS}rule r: every Order's items is at most its total + 1\n"));
    assert!(e.iter().any(|m| m.contains("cannot add a Money and a Count")), "{e:?}");
    let e = errors(&format!("{NUMS}rule r: every Order's items is at most its Shipments\n"));
    assert!(e.iter().any(|m| m.contains("`its Shipments` is a relation")), "{e:?}");
    let e = errors(&format!("{NUMS}rule r: the total amount of each Order's Payments is at most 3 EUR\n"));
    assert!(e.iter().any(|m| m.contains("an Order has no Payments")), "{e:?}");
    let e = errors(&format!("{NUMS}rule r: the total amount of each Order's Refunds is at most 3\n"));
    assert!(e.iter().any(|m| m.contains("cannot compare a Money with a Count")), "{e:?}");
}

#[test]
fn plurals_and_the_attempts_setting() {
    let p = prog("kind Team { }\nkind Person (plural People) { team: Team }\nkind Box { team: Team, n: Count }\nTeam needs Person, Box\nrule r: the number of each Team's People is at most 5\nrule s: the total n of each Team's Boxes is at most 9\n");
    assert_eq!(rule(&p, "r").formula, "for each Team t: count(p in t.people) ≤ 5");
    assert_eq!(p.attempts, 3);
    let five = compile(&[("t.st".to_string(), "module t version 1, currency EUR, attempts 5\nkind A { }\n".to_string())]).unwrap();
    assert_eq!(five.attempts, 5);
}

#[test]
fn formulas_lists_and_becomes_compile() {
    let p = compile_dir(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/workspaces"))).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(rule(&p, "seats").formula, "for each Workspace w: count(m in w.members) ≤ w.seats");
    let price = p.kind("Workspace").unwrap().attr("price").unwrap();
    assert!(price.formula.is_some() && price.default.is_none());
    assert!(p.lists.iter().any(|l| l.owner == "Workspace" && l.name == "members" && l.element == "Member"));
    let q = prog("kind Order { lines: list of Line, credit: Money = 0 EUR,
                   total: Money = the total of the line's price * the line's quantity over its lines }
kind Line { order: Order, price: Money, quantity: Count }
kind Payment { order: Order }
Order needs Payment
effect capture Payment via payments within 30 seconds
  sends    the order's total
  reports  captured | fails with declined
when a Payment is captured: its order's credit becomes its order's credit + its order's total
");
    let total = q.kind("Order").unwrap().attr("total").unwrap();
    assert!(total.formula.is_some());
    assert!(q.reactions[0].actions[0].becomes.is_some());
}

#[test]
fn formula_list_and_becomes_mistakes_are_errors() {
    let base = "kind Order { lines: list of Line, n: Count = 0 }\nkind Line { order: Order, price: Money }\n";
    let e = errors(&format!("{base}kind Box {{ order: Order, x: Count = order's lines }}\nOrder needs Box\n"));
    assert!(e.iter().any(|m| m.contains("`its order's lines` is a relation")), "{e:?}");
    let e = errors("kind Order { total: Money = n + 1, n: Count = 0 }\n");
    assert!(e.iter().any(|m| m.contains("Order.total is Money, but its formula gives a Count")), "{e:?}");
    let e = errors(&format!("{base}kind Ping {{ line: Line }}\nLine needs Ping\n"));
    assert!(e.iter().any(|m| m.contains("Line is an element of a list")), "{e:?}");
    let e = errors(&format!("{base}kind Payment {{ order: Order }}\nOrder needs Payment\neffect capture Payment via payments within 30 seconds\n  sends    the order's n\n  reports  captured\nwhen a Payment is captured: its order's n becomes its order's n + 1 EUR\n"));
    assert!(e.iter().any(|m| m.contains("cannot add a Count and a Money")), "{e:?}");
    let e = errors(&format!("{base}kind Payment {{ order: Order }}\nOrder needs Payment\neffect capture Payment via payments within 30 seconds\n  sends    the order's n\n  reports  captured\nwhen a Payment is captured: its order's n becomes 3 EUR\n"));
    assert!(e.iter().any(|m| m.contains("Order.n is Count, but it would become a Money")), "{e:?}");
}

#[test]
fn review_two_lists_of_one_kind_are_an_error() {
    let e = errors("kind Team { admins: list of Member, guests: list of Member }\nkind Member { team: Team }\n");
    assert!(e.iter().any(|m| m.contains("Team has two lists of Member")), "{e:?}");
}

#[test]
fn review_formulas_that_cannot_be_known_at_creation_are_errors() {
    let e = errors("kind O { a: Count = b + 1, b: Count = a + 1 }\n");
    assert!(e.iter().any(|m| m.contains("the formulas of O depend on each other: a, b")), "{e:?}");
    let e = errors("kind O { }\nkind P { o: O, code: Text = receipt }\nO needs P\neffect take P via bank within 1 day\n  sends    its code\n  reports  taken with receipt: Text\n");
    assert!(e.iter().any(|m| m.contains("P.code reads receipt, which an answer sets later")), "{e:?}");
    let e = errors("kind O { n: Count = the number of its Ps }\nkind P { o: O }\nO needs P\n");
    assert!(e.iter().any(|m| m.contains("O.n reads its Ps, which do not exist yet when an O is created")), "{e:?}");
}

#[test]
fn review_totals_over_a_list_are_typed_and_read_back_plainly() {
    let base = "kind O { lines: list of Line, discount: Money = 0 EUR, n: Count = 3, k: Count = 2 }\nkind Line { o: O, name: Text, price: Money, quantity: Count }\n";
    let e = errors(&format!("{base}rule r: every O's n is at most the total of the line's name * 2 over its lines\n"));
    assert!(e.iter().any(|m| m.contains("cannot multiply a Text by a Count")), "{e:?}");
    let p = prog(&format!("{base}rule r: every O's discount is at most the total of (the line's price - its discount) * the line's quantity over its lines\nrule s: every O's discount is at most 10 EUR * (its n / its k)\nrule t: no O has a Line that is done unless its n is at least the number of its lines that are done and its k is 2\n"));
    assert_eq!(rule(&p, "r").paraphrase, "each o on its own: its discount is at most the total of (the line's price − its discount) × the line's quantity over its lines");
    assert_eq!(rule(&p, "s").formula, "for each O o: o.discount ≤ 10 EUR × (o.n / o.k)");
    assert_eq!(rule(&p, "t").formula, "for each O o: exists(l in o.Line where l is done) implies o.n ≥ count(l in o.lines where l is done) and o.k = 2");
}

#[test]
fn review_list_elements_have_ordinary_singulars() {
    let p = prog("kind Ledger { entries: list of Entry, total: Money = the total of the entry's amount over its entries }\nkind Entry { ledger: Ledger, amount: Money }\nkind Box { boxes: list of Case }\nkind Case { box: Box, n: Count }\n");
    let names = |l: &str| p.lists.iter().find(|x| x.name == l).unwrap().singulars();
    assert!(names("entries").contains(&"entry".to_string()));
    assert!(names("boxes").contains(&"box".to_string()) && names("boxes").contains(&"case".to_string()));
}
