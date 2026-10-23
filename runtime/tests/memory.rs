//! Slice 53: a served host keeps what it needs, not all it has done (spec
//! appendix BD).

use suture_runtime::http::{handle, Request};
use suture_runtime::service::Service;

const T0: u64 = 1_800_000_000;

const SHOP: &str = "module shop version 1, currency EUR
role staff
kind Item { name: Text }
admit add a new Item by staff
";

fn get(svc: &mut Service, target: &str) -> serde_json::Value {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    let req = Request { method: "GET".into(), path: path.into(), query: query.into(), headers: vec![], body: vec![] };
    handle(svc, &req, None, T0).1
}

#[test]
fn a_trimmed_trace_keeps_its_line_numbers() {
    let prog = suture_lang::compile(&[("t.st".to_string(), SHOP.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let mut svc = Service::in_memory(&prog, T0);
    let start = svc.host.trace.len();
    svc.host.trace.extend((0..50).map(|i| format!("line {}", start + i)));
    svc.host.gate.extend((0..50).map(|i| format!("gate {i}")));
    svc.host.trim(10);
    assert_eq!(svc.host.trace.len(), 10);
    assert_eq!(svc.host.trace_dropped, start + 40);
    assert_eq!(svc.host.gate.len(), 10);
    // A line keeps its number; lines no longer kept are skipped.
    let t = get(&mut svc, &format!("/trace?from={}", start + 45));
    assert_eq!((t["from"].as_u64(), t["lines"][0].as_str()), (Some(start as u64 + 45), Some(format!("line {}", start + 45).as_str())));
    let t = get(&mut svc, "/trace?from=0");
    assert_eq!((t["from"].as_u64(), t["next"].as_u64()), (Some(start as u64 + 40), Some(start as u64 + 50)));
    // Fewer than twice as many are left as they are.
    svc.host.trim(10);
    assert_eq!(svc.host.trace.len(), 10);
}

#[test]
fn a_served_machine_journals_nothing() {
    let prog = suture_lang::compile(&[("t.st".to_string(), SHOP.to_string())]).unwrap_or_else(|e| panic!("{e:?}"));
    let mut svc = Service::in_memory(&prog, T0);
    svc.host.create("Item", "I1", &[("name".into(), suture_runtime::state::Val::Text("cup".into()))]).unwrap();
    assert_eq!(svc.host.machine.journal_len(), 0);
}
