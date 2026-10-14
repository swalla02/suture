use suture_lang::ir::Program;
use suture_runtime::explain::explain;
use suture_runtime::host::Host;
use suture_runtime::state::Val;

fn shop() -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop");
    suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"))
}

#[test]
fn explain_says_why_an_order_waits() {
    let p = shop();
    let mut h = Host::new(p.clone());
    let given = [("country".to_string(), Val::Text("France".into())), ("total".to_string(), Val::Money(12000))];
    h.create("Order", "O1", &given).unwrap();
    let text = explain(&p, &h.unit("O1").unwrap(), |_| None).join("\n");
    assert!(text.starts_with("Order O1 ("), "{text}");
    assert!(
        text.contains("goal not met yet: every Order that is not cancelled is paid and settled (not yet paid, settled)"),
        "{text}"
    );
    assert!(text.contains("  Shipment: waiting: rule hold"), "{text}");
    assert!(text.contains("    Declaration: requested from forms"), "{text}");
}

#[test]
fn explain_shows_deadlines_and_alerts() {
    let p = shop();
    let mut h = Host::new(p.clone());
    let given = [("country".to_string(), Val::Text("Japan".into())), ("total".to_string(), Val::Money(8000))];
    h.create("Order", "J1", &given).unwrap();
    h.pass(30).unwrap();
    let show = |h: &Host| explain(&p, &h.unit("J1").unwrap(), |k| h.deadline(k).map(|d| d - h.now)).join("\n");
    let text = show(&h);
    assert!(text.contains("  Payment: requested from payments, send 2 of 3, answer due in 30 seconds"), "{text}");
    h.pass(4 * 86_400).unwrap();
    let text = show(&h);
    assert!(text.contains("  Payment: failed (timed out)"), "{text}");
    assert!(text.contains("  goal late, alert raised: every Order that is not cancelled is paid and settled"), "{text}");
}
