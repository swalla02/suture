use suture_lang::ir::Program;
use suture_runtime::host::Host;
use suture_runtime::interp::in_state;
use suture_runtime::state::Val;

fn shop() -> Program {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../examples/shop");
    suture_lang::compile_dir(std::path::Path::new(dir)).unwrap_or_else(|e| panic!("{e:?}"))
}

fn french(h: &mut Host) {
    let given = [("country".to_string(), Val::Text("France".into())), ("total".to_string(), Val::Money(12000))];
    h.create("Order", "O1", &given).unwrap();
}

fn data(pairs: &[(&str, Val)]) -> Vec<(String, Val)> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect()
}

#[test]
fn a_french_order_runs_end_to_end_on_the_kernel() {
    let mut h = Host::new(shop());
    french(&mut h);
    h.answer("payments", "captured", &[], None).unwrap();
    let over = data(&[("value", Val::Money(15000)), ("contents", Val::Text("alcohol".into()))]);
    h.answer("forms", "filled", &over, None).unwrap();
    let right = data(&[("value", Val::Money(10000)), ("contents", Val::Text("alcohol".into()))]);
    h.answer("forms", "filled", &right, None).unwrap();
    h.answer("forms", "filled", &data(&[("licence", Val::Text("L-77".into()))]), None).unwrap();
    h.answer("carriers", "delivered", &[], None).unwrap();
    let u = h.unit("O1").unwrap();
    assert!(in_state(&h.prog, &u.root, &[], "settled"));
    let trace = h.trace.join("\n");
    assert!(trace.contains("refused by rule declared"), "{trace}");
    assert!(trace.contains("goal met"), "{trace}");
    assert!(h.machine.faults().is_empty(), "{:?}", h.machine.faults());
    assert!(h.pending().is_empty());
}

#[test]
fn an_oversized_unit_is_reported_not_panicked() {
    let mut h = Host::new(shop());
    french(&mut h);
    let big = data(&[("value", Val::Money(100)), ("contents", Val::Text("x".repeat(3_600)))]);
    let err = h.answer("forms", "filled", &big, None).unwrap_err();
    assert!(err.contains("bytes, limit 4096"), "{err}");
}

#[test]
fn answers_with_no_pending_request_are_errors_not_panics() {
    let mut h = Host::new(shop());
    let err = h.answer("payments", "captured", &[], None).unwrap_err();
    assert!(err.contains("payments has no pending request"), "{err}");
    let err = h.create("Payment", "P1", &[]).unwrap_err();
    assert!(err.contains("is a part of Order"), "{err}");
}
