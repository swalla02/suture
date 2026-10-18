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
    let d: Vec<(Option<&str>, Option<&str>, bool, Option<&str>)> =
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
        (format!("{SHOP}decide Product with model \"m@1\"\n"), "t.st:15: no `uses` picks a Product, so there is no choice to decide"),
        (format!("{slot}decide Delivery with model \"m@1\"\n"), "t.st:17: Delivery is a slot; v0 picks a slot's alternative by `prefer` and patience, not by a model"),
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
