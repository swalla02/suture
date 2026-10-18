//! Slice 13: data marks and the manifest (spec appendix N).

use std::path::PathBuf;

use serde_json::json;
use suture_lang::ir::Program;
use suture_runtime::host::Host;
use suture_runtime::service::{self, Record, Service, Who};
use suture_runtime::state::Val;

const T0: u64 = 1_800_000_000;

const VAULT: &str = "module vault version 1, currency EUR

role customer is a Customer

kind Customer { name: Text, address: Text personal }
kind Order    { customer: Customer, card: Text secret, total: Money }
kind Payment  { order: Order, amount: Money = order's total, receipt: Text secret }
kind Shipment { order: Order }

Order needs Payment, Shipment

effect charge Payment via payments within 30 seconds
  sends    its amount, the order's card
  reports  charged with receipt: Text
effect ship Shipment via carriers within 1 day
  sends    the order's customer's address
  reports  delivered

allow payments to receive Order's card
allow carriers to receive Customer's address

admit place a new Order by customer where the Order's customer is the caller
";

fn compile(text: &str) -> Result<Program, String> {
    suture_lang::compile(&[("vault.st".to_string(), text.to_string())]).map_err(|e| e.iter().map(|e| e.to_string()).collect::<Vec<_>>().join("\n"))
}

fn vault() -> Program {
    compile(VAULT).unwrap_or_else(|e| panic!("{e}"))
}

fn text(s: &str) -> Val {
    Val::Text(s.into())
}

#[test]
fn secrets_are_never_compared_and_marked_fields_need_a_grant() {
    let bad = |from: &str, to: &str| {
        assert!(VAULT.contains(from), "{from}");
        compile(&VAULT.replace(from, to)).unwrap_err()
    };
    let e = bad("admit place", "rule visa: every Order's card is not \"\"\nadmit place");
    assert!(e.contains("Order's card is secret; secrets can be sent to an adapter, never compared or computed with"), "{e}");
    let e = bad("total: Money }", "total: Money, copy: Text = card }");
    assert!(e.contains("card is secret"), "{e}");
    assert!(compile(&VAULT.replace("total: Money }", "total: Money, copy: Text secret = card }")).is_ok(), "a secret may copy a secret");
    let e = bad("total: Money }", "total: Money secret }");
    assert!(e.contains("Order.total is Money; only a Text attribute can be secret"), "{e}");
    let e = bad("card: Text secret,", "card: Text secret = \"x\",");
    assert!(e.contains("Order.card is secret, so it has no default"), "{e}");
    let e = bad("allow payments to receive Order's card\n", "");
    assert!(e.contains("Order's card is secret, and payments is not allowed to receive it; write `allow payments to receive Order's card`"), "{e}");
    let e = bad("allow carriers to receive Customer's address\n", "allow payments to receive Customer's address\n");
    assert!(e.contains("Customer's address is personal, and carriers is not allowed to receive it"), "{e}");
    let e = bad("allow carriers to receive Customer's address", "allow carriers to receive Customer's address, Customer's name");
    assert!(e.contains("Customer's name is not marked secret or personal, so it needs no `allow`"), "{e}");
    let e = bad("allow carriers", "allow couriers");
    assert!(e.contains("no effect or source is via couriers"), "{e}");
    let p = compile(&VAULT.replace("allow payments to receive Order's card", "allow payments to receive Order's card, Customer's address")).unwrap();
    assert!(p.warnings.iter().any(|w| w.contains("no effect or source via payments sends it")), "{:?}", p.warnings);
}

fn live() -> Host {
    let mut h = Host::new(vault());
    h.create("Customer", "Ada", &[("name".into(), text("Ada")), ("address".into(), text("1 Rue de Rivoli"))]).unwrap();
    h.create("Order", "O1", &[("customer".into(), Val::Ref("Ada".into())), ("card".into(), text("4111 1111")), ("total".into(), Val::Money(8000))]).unwrap();
    h
}

#[test]
fn a_secret_is_held_by_reference_and_marked_values_are_masked() {
    let h = live();
    let o1 = h.unit("O1").unwrap();
    assert_eq!(o1.root.attrs.get("card"), Some(&Val::Secret(1)));
    assert_eq!(h.reveal(&Val::Secret(1)), text("4111 1111"));
    let trace = h.trace.join("\n");
    assert!(!trace.contains("4111") && !trace.contains("Rivoli"), "{trace}");
    assert!(trace.contains("card (secret)") && trace.contains("address (personal)"), "{trace}");
    let explain = suture_runtime::explain::explain(&h.prog, &o1, |_| None).join("\n");
    assert!(!explain.contains("4111"), "{explain}");
    let pay = h.pending().into_iter().find(|a| a.adapter == "payments").unwrap().clone();
    let body = service::request_json(&h, &pay, true);
    assert_eq!(body["sends"], json!({ "amount": "80 EUR", "card": "4111 1111" }));
    assert_eq!(service::request_json(&h, &pay, false)["sends"]["card"], "(secret)");
    let ship = h.pending().into_iter().find(|a| a.adapter == "carriers").unwrap().clone();
    assert_eq!(service::request_json(&h, &ship, true)["sends"]["address"], "1 Rue de Rivoli");
    assert_eq!(service::request_json(&h, &ship, false)["sends"]["address"], "(personal)");
}

#[test]
fn a_served_secret_stays_out_of_the_log_and_comes_back_from_the_store() {
    let p = vault();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("marks").join("served");
    let _ = std::fs::remove_dir_all(&dir);
    let unit = {
        let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
        let ada = service::attrs_json(&p, "Customer", Some(&json!({ "name": "Ada", "address": "1 Rue de Rivoli" }))).unwrap();
        s.input(Record::World { kind: "Customer".into(), label: "Ada".into(), attrs: ada, items: vec![] }, T0).unwrap();
        let attrs = service::attrs_json(&p, "Order", Some(&json!({ "customer": "Ada", "card": "4111 1111", "total": "80 EUR" }))).unwrap();
        let by = Who { role: "customer".into(), name: "Ada".into() };
        let a = s.input(Record::New { by, verb: "places".into(), kind: "Order".into(), label: "O1".into(), attrs, items: vec![] }, T0 + 1).unwrap();
        assert_eq!(a.refused, None, "{:?}", a.trace);
        s.host.unit("O1")
    };
    let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap();
    assert!(!log.contains("4111"), "{log}");
    assert!(log.contains(r#"{"Secret":1}"#), "{log}");
    let store = std::fs::read_to_string(dir.join("secrets.json")).unwrap();
    assert_eq!(store, r#"{"1":"4111 1111"}"#);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(dir.join("secrets.json")).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let s = Service::open(&p, &dir, T0 + 2, |_| {}).unwrap();
    assert_eq!(s.host.unit("O1"), unit);
    let pay = s.host.pending().into_iter().find(|a| a.adapter == "payments").unwrap().clone();
    assert_eq!(service::request_json(&s.host, &pay, true)["sends"]["card"], "4111 1111");
    drop(s);
    std::fs::remove_file(dir.join("secrets.json")).unwrap();
    let e = Service::open(&p, &dir, T0 + 3, |_| {}).err().unwrap();
    assert!(e.contains("secret 1 is not in the secret store"), "{e}");
}

#[test]
fn the_manifest_lists_what_each_adapter_receives() {
    let m = suture_lang::manifest::manifest(&vault());
    assert!(m.contains("payments   receives  Payment.amount, Order.card (secret)"), "{m}");
    assert!(m.contains("carriers   receives  Customer.address (personal)"), "{m}");
    assert!(m.contains("answers   charge Payment: charged"), "{m}");
    assert!(m.contains("marked:   Customer.address (personal), Order.card (secret)"), "{m}");
    assert!(m.contains("roles     customer (a Customer)"), "{m}");
}

#[test]
fn a_stray_answer_keeps_its_secret_out_of_the_log() {
    let p = vault();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("marks").join("stray");
    let _ = std::fs::remove_dir_all(&dir);
    let mut s = Service::open(&p, &dir, T0, |_| {}).unwrap();
    let data = vec![("receipt".to_string(), text("rcpt-0042"))];
    let _ = s.input(Record::Answer { adapter: "payments".into(), outcome: "charged".into(), key: Some("O9/Payment".into()), target: None, data }, T0);
    let log = std::fs::read_to_string(dir.join("log.jsonl")).unwrap_or_default();
    assert!(!log.contains("rcpt-0042"), "{log}");
}

#[test]
fn a_changed_secret_mark_is_a_deploy_break() {
    let h = live();
    let v2 = compile(&VAULT.replace("version 1", "version 2").replace("name: Text,", "name: Text secret,")).unwrap();
    let breaks = h.deploy_check(&v2).unwrap();
    assert!(breaks.iter().any(|b| b.contains("Customer's name becomes secret")), "{breaks:?}");
    let v2 = compile(&VAULT.replace("version 1", "version 2").replace("card: Text secret", "card: Text").replace("allow payments to receive Order's card\n", "")).unwrap();
    let breaks = h.deploy_check(&v2).unwrap();
    assert!(breaks.iter().any(|b| b.contains("Order's card stops being secret")), "{breaks:?}");
}
