use suture_lang::ir::Program;
use suture_runtime::interp::{in_state, new_unit, step};
use suture_runtime::state::{Ask, Msg, Unit, Val};

fn shop() -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop");
    suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"))
}

fn run(p: &Program, u: &Unit, m: Msg) -> (Unit, Vec<Ask>) {
    let out = step(p, u, &m);
    (out.unit, out.asks)
}

fn order(p: &Program, label: &str, country: &str, euros: i64) -> (Unit, Vec<Ask>) {
    let given = [("country".to_string(), Val::Text(country.into())), ("total".to_string(), Val::Money(euros * 100))];
    let u = new_unit(p, label, "Order", &given).unwrap();
    run(p, &u, Msg::Create)
}

fn answer(p: &Program, u: &Unit, ask: &Ask, outcome: &str, data: &[(&str, Val)]) -> (Unit, Vec<Ask>) {
    let data = data.iter().map(|(k, v)| (k.to_string(), v.clone())).collect();
    run(p, u, Msg::Answer { key: ask.key.clone(), outcome: outcome.into(), data })
}

fn ask<'a>(asks: &'a [Ask], adapter: &str) -> &'a Ask {
    asks.iter().find(|a| a.adapter == adapter).unwrap_or_else(|| panic!("no request for {adapter} in {asks:?}"))
}

fn has_note(u: &Unit, text: &str) -> bool {
    u.notes.iter().any(|n| n.contains(text))
}

fn has_part(u: &Unit, kind: &str) -> bool {
    u.root.parts.iter().any(|n| n.kind == kind)
}

fn root_is(p: &Program, u: &Unit, state: &str) -> bool {
    in_state(p, &u.root, &[], state)
}

#[test]
fn a_french_order_grows_a_customs_form() {
    let p = shop();
    let (u, asks) = order(&p, "O1", "France", 120);
    assert!(has_part(&u, "CustomsForm"));
    assert!(has_note(&u, "needs Declaration"), "{:?}", u.notes);
    assert!(has_note(&u, "Shipment waits: rule hold"), "{:?}", u.notes);
    assert_eq!(asks.len(), 2, "{asks:?}");
    assert_eq!(ask(&asks, "forms").key, "O1/CustomsForm/Declaration#1");
    assert_eq!(ask(&asks, "payments").sends, vec![("total".to_string(), Val::Money(12000))]);
}

#[test]
fn a_japanese_order_has_no_form() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    assert!(!has_part(&u, "CustomsForm"));
    assert_eq!(asks.len(), 1, "{asks:?}");
    assert!(has_note(&u, "Shipment waits: rule paid"), "{:?}", u.notes);
}

#[test]
fn an_over_declared_value_is_refused_and_asked_again() {
    let p = shop();
    let (u, asks) = order(&p, "O1", "France", 120);
    let form = ask(&asks, "forms").clone();
    let data = [("value", Val::Money(15000)), ("contents", Val::Text("books".into()))];
    let (u2, again) = answer(&p, &u, &form, "filled", &data);
    assert!(has_note(&u2, "refused by rule declared"), "{:?}", u2.notes);
    assert_eq!(u2.root, u.root, "a refused answer changes nothing");
    assert_eq!(again, vec![form]);
}

#[test]
fn alcohol_opens_the_excise_section_and_the_form_releases_shipping() {
    let p = shop();
    let (u, asks) = order(&p, "O1", "France", 120);
    let (u, _) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    assert!(has_note(&u, "Payment captured"), "{:?}", u.notes);
    let data = [("value", Val::Money(10000)), ("contents", Val::Text("alcohol".into()))];
    let (u, asks) = answer(&p, &u, ask(&asks, "forms"), "filled", &data);
    let excise = ask(&asks, "forms").clone();
    assert_eq!(excise.key, "O1/CustomsForm/Excise#1");
    let (u, again) = answer(&p, &u, &excise, "filled", &[("licence", Val::Text(String::new()))]);
    assert!(has_note(&u, "refused by rule licence"), "{:?}", u.notes);
    assert_eq!(again, vec![excise.clone()]);
    let (u, asks) = answer(&p, &u, &excise, "filled", &[("licence", Val::Text("L-77".into()))]);
    assert!(has_note(&u, "CustomsForm done"), "{:?}", u.notes);
    let ship = ask(&asks, "carriers");
    assert_eq!(ship.sends, vec![("country".to_string(), Val::Text("France".into()))]);
    let (u, _) = answer(&p, &u, ship, "delivered", &[]);
    assert!(root_is(&p, &u, "paid") && root_is(&p, &u, "settled"));
    assert!(has_note(&u, "goal met"), "{:?}", u.notes);
}

#[test]
fn a_declined_payment_cancels_the_order_and_withdraws_the_shipment() {
    let p = shop();
    let (u, asks) = order(&p, "D1", "Japan", 50);
    let (u, more) = answer(&p, &u, ask(&asks, "payments"), "declined", &[]);
    assert!(root_is(&p, &u, "cancelled"));
    assert!(!root_is(&p, &u, "paid"));
    assert!(!has_part(&u, "Shipment"), "the idle shipment is withdrawn");
    assert!(has_note(&u, "Shipment withdrawn"), "{:?}", u.notes);
    assert!(more.is_empty());
}

#[test]
fn a_late_answer_is_ignored() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    let pay = ask(&asks, "payments").clone();
    let (u, _) = answer(&p, &u, &pay, "captured", &[]);
    let (u2, again) = answer(&p, &u, &pay, "captured", &[]);
    assert_eq!(u2.root, u.root);
    assert!(again.is_empty());
    assert_eq!(u2.notes, vec![format!("ignored an answer for {}: no open request", pay.key)]);
}

#[test]
fn wrongly_typed_answer_data_is_refused() {
    let p = shop();
    let (u, asks) = order(&p, "O1", "France", 120);
    let form = ask(&asks, "forms").clone();
    let data = [("value", Val::Text("abc".into())), ("contents", Val::Text("books".into()))];
    let (u2, again) = answer(&p, &u, &form, "filled", &data);
    assert_eq!(u2.root, u.root);
    assert!(has_note(&u2, "value must be Money"), "{:?}", u2.notes);
    assert_eq!(again, vec![form]);
}

#[test]
fn a_lost_parcel_is_shipped_again() {
    let p = shop();
    let (u, asks) = order(&p, "J1", "Japan", 80);
    let (u, asks) = answer(&p, &u, ask(&asks, "payments"), "captured", &[]);
    let first = ask(&asks, "carriers").clone();
    assert_eq!(first.key, "J1/Shipment#1");
    let (u, asks) = answer(&p, &u, &first, "lost", &[]);
    assert!(has_note(&u, "Shipment lost: attempt 2"), "{:?}", u.notes);
    assert_eq!(ask(&asks, "carriers").key, "J1/Shipment#2");
}
