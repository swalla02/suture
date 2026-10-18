//! Slice 9: models at choice points (spec appendix J).

use suture_lang::compile;
use suture_lang::ir::Program;

fn prog(src: &str) -> Program {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).unwrap_or_else(|e| panic!("{}", e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n")))
}

fn errors(src: &str) -> Vec<String> {
    let files = vec![("t.st".to_string(), format!("module t version 1, currency EUR\n{src}"))];
    compile(&files).err().unwrap_or_default().iter().map(|e| e.to_string()).collect()
}

const SHOP: &str = "kind Product  { name: Text }
kind Stock    { product: Product, units: Count }
kind Carrier  { name: Text, zone: Text, fee: Money }
kind Order    { product: Product, zone: Text, quantity: Count, total: Money, local: Bool = no }
kind Shipment { order: Order }
Order needs Shipment
Shipment uses a Stock where the Stock's product is the Order's product, taking the Order's quantity
Shipment uses a Carrier where the Carrier's zone is the Order's zone
effect send Shipment via carriers within 1 day
  sends   its Carrier's name
  reports delivered
rule stock: the total quantity taken from each Stock is at most its units
prefer Carrier: lowest fee
";

#[test]
fn every_level_compiles() {
    let p = prog(&format!(
        "{SHOP}decide with model \"planner@3\"
decide for Orders with model \"orders@2\"
decide for Shipments with model \"shipping@1\"
decide Carrier with model \"carriers@1\"
decide Stock for Shipments with model \"stock@4\"
decide Carrier for Orders whose total is at least 1000 EUR with model \"careful@1\"
decide Carrier for Shipments whose Order is local with prefer
"
    ));
    type Row<'a> = (Option<&'a str>, Option<&'a str>, bool, Option<&'a str>);
    let d: Vec<Row> =
        p.decisions.iter().map(|d| (d.point.as_deref(), d.scope.as_deref(), d.cond.is_some(), d.model.as_deref())).collect();
    assert_eq!(
        d,
        [
            (None, None, false, Some("planner@3")),
            (None, Some("Order"), false, Some("orders@2")),
            (None, Some("Shipment"), false, Some("shipping@1")),
            (Some("Carrier"), None, false, Some("carriers@1")),
            (Some("Stock"), Some("Shipment"), false, Some("stock@4")),
            (Some("Carrier"), Some("Order"), true, Some("careful@1")),
            (Some("Carrier"), Some("Shipment"), true, None),
        ]
    );
}

#[test]
fn build_lists_the_handlers_of_each_choice_point() {
    let p = prog(&format!(
        "{SHOP}decide with model \"planner@3\"
decide Carrier with model \"carriers@1\"
decide Carrier for Orders whose total is at least 1000 EUR with model \"careful@1\"
"
    ));
    let carrier = p.bindings.iter().find(|b| b.target == "Carrier").unwrap();
    assert_eq!(
        suture_lang::readback::handlers(&p, carrier),
        [
            "for Orders whose total is at least 1000 EUR: model \"careful@1\"",
            "model \"carriers@1\"",
            "otherwise `prefer Carrier`",
        ]
    );
    let stock = p.bindings.iter().find(|b| b.target == "Stock").unwrap();
    assert_eq!(suture_lang::readback::handlers(&p, stock), ["model \"planner@3\"", "otherwise the order they were created"]);
}

#[test]
fn misuses_are_errors() {
    let slot = SHOP.replace("Order needs Shipment", "Order needs Delivery\nDelivery is Shipment or Refund\nkind Refund { order: Order }");
    let cases = [
        (format!("{SHOP}decide Product with model \"m@1\"\n"), "t.st:15: no `uses`, slot or source picks a Product, so there is no choice to decide"),
        (
            format!("{slot}decide Delivery with model \"a@1\"\ndecide Delivery with model \"b@1\"\n"),
            "t.st:18: `decide Delivery with model \"b@1\"` and `decide Delivery with model \"a@1\"` (line 17) can both decide an Order's Delivery, and neither is more specific",
        ),
        (format!("{SHOP}decide for Parcels with model \"m@1\"\n"), "t.st:15: unknown kind `Parcels`"),
        (format!("{SHOP}decide Stock for Carriers with model \"m@1\"\n"), "t.st:15: no Stock is chosen inside a Carrier"),
        (format!("{SHOP}decide Carrier with model \"carriers\"\n"), "t.st:15: a model is named `name@version`, as in \"carriers@1\""),
        (
            format!("{SHOP}decide Carrier with model \"a@1\"\ndecide Carrier with model \"b@1\"\n"),
            "t.st:16: `decide Carrier with model \"b@1\"` and `decide Carrier with model \"a@1\"` (line 15) can both decide a Shipment's Carrier, and neither is more specific",
        ),
        (
            format!("{SHOP}decide for Orders whose total is at least 5 EUR with model \"a@1\"\ndecide for Orders whose product's name is \"Kettle\" with model \"b@1\"\n"),
            "t.st:16: `decide for Orders whose product's name is \"Kettle\" with model \"b@1\"` and `decide for Orders whose total is at least 5 EUR with model \"a@1\"` (line 15) can both decide a Shipment's Stock, and neither is more specific",
        ),
    ];
    for (src, want) in cases {
        let e = errors(&src);
        assert!(e.iter().any(|x| x == want), "want {want}\ngot {e:#?}");
    }
}

#[test]
fn a_model_is_part_of_the_program() {
    let a = prog(&format!("{SHOP}decide Carrier with model \"carriers@1\"\n"));
    let b = prog(&format!("{SHOP}decide Carrier with model \"carriers@2\"\n"));
    assert_ne!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
}

const CREDIT: &str = "kind Customer { name: Text }
kind Order   { customer: Customer, total: Money, credit: Money }
kind Payment { order: Order }
Order needs Payment
effect capture Payment via payments within 30 seconds
  sends    the order's total
  marks    sent
  reports  captured
rule credit: no Order has a Payment that is sent unless its total is at most its credit
";

#[test]
fn slots_and_sources_are_choice_points() {
    let slot = SHOP.replace("Order needs Shipment", "Order needs Delivery\nDelivery is Shipment or Refund\nkind Refund { order: Order }");
    let p = prog(&format!("{slot}decide Delivery with model \"routing@1\" within 300 milliseconds\n"));
    assert_eq!(p.decisions[0].point.as_deref(), Some("Delivery"));
    assert_eq!(p.decisions[0].within_ms, 300);
    let p = prog(&format!(
        "{CREDIT}source Order's credit via bureau within 5 seconds
  sends    the customer's name
  reports  found
source Order's credit via backup within 2 seconds
  sends    the customer's name
  reports  found
  otherwise  use 0 EUR
decide Order's credit with model \"sources@1\" within 2 seconds
"
    ));
    assert_eq!(p.sources_of("Order", "credit").len(), 2);
    assert_eq!(p.decisions[0].point.as_deref(), Some("Order's credit"));
    assert_eq!(p.decisions[0].within_ms, 2000);
    assert!(p.choice_points().iter().any(|c| c.name == "Order's credit" && c.chooser == "Order"));
    let p = prog(&format!("{SHOP}decide Carrier with model \"carriers@1\"\n"));
    assert_eq!(p.decisions[0].within_ms, 1000, "a model has 1 second by default");
    for within in ["0 milliseconds", "11 minutes"] {
        let e = errors(&format!("{SHOP}decide Carrier with model \"carriers@1\" within {within}\n"));
        assert_eq!(e, ["t.st:15: a model's `within` is from 1 millisecond to 10 minutes"], "{within}");
    }
}

#[test]
fn source_choice_misuses_are_errors() {
    let src = |extra: &str| {
        format!(
            "{CREDIT}source Order's credit via bureau within 5 seconds
  sends    the customer's name
  reports  found
  otherwise  use 0 EUR
{extra}"
        )
    };
    let cases = [
        (src("decide Order's credit with model \"m@1\"\n"), "t.st:15: Order's credit has one source, so there is no choice to decide"),
        (src("decide Order's total with model \"m@1\"\n"), "t.st:15: Order's total has no sources, so there is no choice to decide"),
        (
            src("source Order's credit via bureau within 1 second\n  sends the customer's name\n  reports found\n"),
            "t.st:15: Order.credit already has a source via bureau",
        ),
        (
            src("source Order's credit via backup within 1 second\n  sends the customer's name\n  reports found\n  otherwise use 1 EUR\n"),
            "t.st:15: Order.credit already has an `otherwise`; it applies when every source has failed, so give it once",
        ),
        (src("decide Order's credit with prefer within 1 second\n"), "t.st:15:"),
    ];
    for (src, want) in cases {
        let e = errors(&src);
        assert!(e.iter().any(|x| x.starts_with(want)), "want {want}\ngot {e:#?}");
    }
}

#[test]
fn a_rule_reads_an_attribute_with_two_sources_once() {
    let p = prog(&format!(
        "{CREDIT}source Order's credit via bureau within 5 seconds
  sends    the customer's name
  reports  found
source Order's credit via backup within 2 seconds
  sends    the customer's name
  reports  found
"
    ));
    let r = p.rules.iter().find(|r| r.name == "credit").unwrap();
    assert_eq!(r.sources, [("Order".to_string(), "credit".to_string())]);
    assert_eq!(suture_lang::readback::rule_sources(&p, r).as_deref(), Some("credit from bureau (fetched once) or backup (fetched once)"));
    let line = suture_lang::readback::source_line(&p, &p.sources[0]);
    assert!(line.contains("if it fails, the next source is asked; if every source fails, the steps its rules guard wait"), "{line}");
}
