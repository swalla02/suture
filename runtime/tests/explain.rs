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
    let text = explain(&p, &h.unit("O1").unwrap()).join("\n");
    assert!(text.starts_with("Order O1 ("), "{text}");
    assert!(
        text.contains("goal not met yet: every Order that is not cancelled is paid and settled (not yet paid, settled)"),
        "{text}"
    );
    assert!(text.contains("  Shipment: waiting: rule hold"), "{text}");
    assert!(text.contains("    Declaration: requested from forms"), "{text}");
}
