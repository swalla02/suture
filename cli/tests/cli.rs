use std::process::Command;

fn suture(args: &[&str]) -> (bool, String) {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/..");
    let out = Command::new(env!("CARGO_BIN_EXE_suture")).args(args).current_dir(root).output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    (out.status.success(), text)
}

#[test]
fn build_prints_read_backs() {
    let (ok, out) = suture(&["build", "examples/shop"]);
    assert!(ok, "{out}");
    assert!(out.contains("shop v1 → examples/shop/shop-v1.sut"), "{out}");
    assert!(out.contains("rule declared: every Declaration's value is at most its form's order's total"), "{out}");
    assert!(out.contains("in other words:  each declaration on its own"), "{out}");
}

#[test]
fn run_passes_the_shop_scenarios() {
    let (ok, out) = suture(&["run", "examples/shop", "--scenario", "examples/shop/orders.scenario"]);
    assert!(ok, "{out}");
    assert!(out.contains("refused by rule declared"), "{out}");
    assert_eq!(out.matches("  passed").count(), 3, "{out}");
}

#[test]
fn explain_shows_a_unit() {
    let (ok, out) = suture(&["explain", "examples/shop", "--scenario", "examples/shop/orders.scenario", "O1"]);
    assert!(ok, "{out}");
    assert!(out.contains("goal met: every Order that is not cancelled is paid and settled"), "{out}");
}

#[test]
fn bad_arguments_print_the_usage() {
    let (ok, out) = suture(&["frobnicate"]);
    assert!(!ok);
    assert!(out.contains("usage:"), "{out}");
}
