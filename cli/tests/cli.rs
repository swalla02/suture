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
    assert!(out.contains("rule monthly: for each Customer and month"), "{out}");
    assert!(out.contains("choices:\n  Shipment uses a Carrier: pool key Carrier's countries contain its order's country"), "{out}");
    assert!(out.contains("each Refund adds to its customer's total amount of Refunds per month"), "{out}");
}

#[test]
fn run_passes_the_shop_scenarios() {
    let (ok, out) = suture(&["run", "examples/shop", "--scenario", "examples/shop/orders.scenario"]);
    assert!(ok, "{out}");
    assert!(out.contains("refused by rule declared"), "{out}");
    assert!(out.contains("payments: no answer within 30 seconds"), "{out}");
    assert!(out.contains("alert: not met within 3 days"), "{out}");
    assert!(out.contains("ignored a late answer for L1/Payment#1"), "{out}");
    assert!(out.contains("choice: Hermes for O1/Shipment#1@Carrier"), "{out}");
    assert_eq!(out.matches("  passed").count(), 9, "{out}");
}

#[test]
fn explain_shows_a_unit() {
    let (ok, out) = suture(&["explain", "examples/shop", "--scenario", "examples/shop/orders.scenario", "F1"]);
    assert!(ok, "{out}");
    assert!(out.contains("goal met: every Order that is not cancelled is paid and settled"), "{out}");
}

#[test]
fn bad_arguments_print_the_usage() {
    let (ok, out) = suture(&["frobnicate"]);
    assert!(!ok);
    assert!(out.contains("usage:"), "{out}");
}

#[test]
fn build_prints_warnings() {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("warn");
    std::fs::create_dir_all(&dir).unwrap();
    let src = "module w version 1, currency EUR\nkind O { }\nkind A { o: O }\nkind B { o: O }\nO needs D\nD is A or B\n";
    std::fs::write(dir.join("w.st"), src).unwrap();
    let (ok, out) = suture(&["build", dir.to_str().unwrap()]);
    assert!(ok, "{out}");
    assert!(out.contains("warning: D has no `prefer`"), "{out}");
}
