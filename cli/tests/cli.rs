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
    assert!(out.contains("roles: customer (a Customer), support, staff\nadmitted:\n  place a new Order by customer where the Order's customer is the caller"), "{out}");
    assert!(out.contains("watches:\n  Stock's units, changed by staff: carried by Shipments bound to a Stock; each change is checked there first, then refreshed"), "{out}");
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
    assert_eq!(out.matches("  passed").count(), 14, "{out}");
    assert!(out.contains("✓ C1 is refused at the gate"), "{out}");
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

const ROUTED: &str = "module r version 1, currency EUR
kind Applicant   { name: Text }
kind Application { applicant: Applicant }
kind Courier     { application: Application }
kind Locker      { application: Application }
Application needs Return
Return is Courier or Locker
effect send Courier via couriers within 2 days
  sends    the application's applicant's name
  reports  delivered
effect open Locker via lockers within 1 hour
  sends    the application's applicant's name
  reports  opened
prefer Return: Courier, then Locker
decide Return with model \"routing@1\" within 500 milliseconds
";

fn routed(name: &str, models: Option<&str>) -> std::path::PathBuf {
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("r.st"), ROUTED).unwrap();
    std::fs::write(dir.join("r.scenario"), "scenario \"s\":\n  given  an Applicant Ada with name Ada\n  when   a new Application A1 with applicant Ada\n").unwrap();
    if let Some(m) = models {
        std::fs::write(dir.join("models.json"), m).unwrap();
    }
    dir
}

#[test]
fn build_lists_who_decides_a_slot() {
    let dir = routed("routed-build", None);
    let (ok, out) = suture(&["build", dir.to_str().unwrap()]);
    assert!(ok, "{out}");
    assert!(out.contains("slots:\n  Application's Return: Courier, then Locker\n    decided by: model \"routing@1\" within 500 ms; otherwise `prefer Return`"), "{out}");
}

#[test]
fn run_with_models_asks_the_configured_backend_and_falls_back_when_it_fails() {
    // A closed local port: the backend fails at once, and `prefer` picks.
    let dir = routed("routed-run", Some(r#"{"routing@1": {"backend": "custom", "url": "http://127.0.0.1:9/", "pick": "/pick"}}"#));
    let scenario = dir.join("r.scenario");
    let (ok, out) = suture(&["run", dir.to_str().unwrap(), "--scenario", scenario.to_str().unwrap(), "--models"]);
    assert!(ok, "{out}");
    assert!(out.contains("choice: Courier for A1?Return#1 by prefer (model \"routing@1\" failed: "), "{out}");
    let (ok, out) = suture(&["run", dir.to_str().unwrap(), "--scenario", scenario.to_str().unwrap()]);
    assert!(ok, "{out}");
    assert!(out.contains("choice: Courier for A1?Return#1 by prefer (model \"routing@1\" is not available)"), "{out}");
    let bad = routed("routed-bad", Some(r#"{"routing@1": {"backend": "magic"}}"#));
    let (ok, out) = suture(&["run", bad.to_str().unwrap(), "--scenario", scenario.to_str().unwrap(), "--models"]);
    assert!(!ok);
    assert!(out.contains("models.json: routing@1: unknown backend `magic`"), "{out}");
}

#[test]
fn serve_runs_a_program_and_a_restart_keeps_its_state() {
    use std::io::{BufRead, BufReader, Read, Write};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let state = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("serve-state");
    let _ = std::fs::remove_dir_all(&state);
    let start = || {
        let mut child = Command::new(env!("CARGO_BIN_EXE_suture"))
            .args(["serve", "examples/tickets", "--listen", "127.0.0.1:0", "--state", state.to_str().unwrap()])
            .env("SUTURE_OPERATOR_TOKEN", "t")
            .current_dir(&root)
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut first = String::new();
        BufReader::new(child.stdout.as_mut().unwrap()).read_line(&mut first).unwrap();
        let addr = first.split("http://").nth(1).and_then(|r| r.split(',').next()).unwrap_or_else(|| panic!("{first}")).to_string();
        (child, addr)
    };
    let post = |addr: &str, path: &str, headers: &str, body: &str| {
        let mut s = std::net::TcpStream::connect(addr).unwrap();
        write!(s, "POST {path} HTTP/1.1\r\nContent-Length: {}\r\n{headers}\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let (mut child, addr) = start();
    let out = post(&addr, "/world", "Authorization: Bearer t\r\n", r#"{"kind": "Fan", "label": "Ada", "attrs": {"name": "Ada"}}"#);
    assert!(out.starts_with("HTTP/1.1 200"), "{out}");
    child.kill().unwrap();
    child.wait().unwrap();
    let (mut child, addr) = start();
    let out = post(&addr, "/world", "Authorization: Bearer t\r\n", r#"{"kind": "Fan", "label": "Ada", "attrs": {"name": "Ada"}}"#);
    child.kill().unwrap();
    child.wait().unwrap();
    assert!(out.starts_with("HTTP/1.1 409") && out.contains("Ada already exists"), "the log brought Ada back: {out}");
}
