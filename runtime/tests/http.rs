//! Slice 11: the HTTP API (spec appendix L.2.5–L.2.6).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::{json, Value};
use suture_lang::ir::Program;
use suture_runtime::http::{handle, Request};
use suture_runtime::service::Service;

const T0: u64 = 1_800_000_000;
const TOKEN: &str = "op-secret";

fn tickets() -> Program {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/tickets");
    suture_lang::compile_dir(&dir).unwrap_or_else(|e| panic!("{e:?}"))
}

fn req(method: &str, target: &str, headers: &[(&str, &str)], body: Value) -> Request {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Request {
        method: method.into(),
        path: path.into(),
        query: query.into(),
        headers: headers.iter().map(|(k, v)| (k.to_ascii_lowercase(), v.to_string())).collect(),
        body: if body.is_null() { vec![] } else { body.to_string().into_bytes() },
    }
}

fn op(svc: &mut Service, path: &str, body: Value) -> (u16, Value) {
    handle(svc, &req("POST", path, &[("Authorization", &format!("Bearer {TOKEN}"))], body), Some(TOKEN), T0)
}

fn setup(svc: &mut Service) {
    for b in [
        json!({ "kind": "Fan", "label": "Ada", "attrs": { "name": "Ada" } }),
        json!({ "kind": "Show", "label": "Hamlet", "attrs": { "name": "Hamlet", "price": "40 EUR" } }),
        json!({ "kind": "Session", "label": "Fri", "attrs": { "show": "Hamlet", "day": "Friday", "seats": 100 } }),
    ] {
        let (status, body) = op(svc, "/world", b);
        assert_eq!(status, 200, "{body}");
    }
}

fn book(svc: &mut Service, caller: &str, label: &str) -> (u16, Value) {
    let b = json!({ "new": { "verb": "books", "kind": "Booking", "label": label, "attrs": { "fan": "Ada", "show": "Hamlet", "quantity": 2 } } });
    handle(svc, &req("POST", "/inputs", &[("Suture-Caller", caller)], b), Some(TOKEN), T0)
}

#[test]
fn a_booking_goes_through_the_api() {
    let mut svc = Service::in_memory(&tickets(), T0);
    setup(&mut svc);
    let (status, body) = book(&mut svc, "fan Ada", "B1");
    assert_eq!(status, 200, "{body}");
    assert!(body["trace"].as_array().unwrap().iter().any(|l| l == "B1  needs Payment"), "{body}");
    let (_, pending) = handle(&mut svc, &req("GET", "/pending", &[], Value::Null), None, T0);
    assert_eq!(pending["pending"][0], json!({ "key": "B1/Payment#1", "adapter": "payments", "verb": "charge", "kind": "Payment", "unit": "B1", "sends": { "amount": "80 EUR" } }));
    let (status, body) = op(&mut svc, "/answers", json!({ "adapter": "payments", "outcome": "charged", "key": "B1/Payment#1" }));
    assert_eq!(status, 200, "{body}");
    let (status, body) = op(&mut svc, "/answers", json!({ "adapter": "mailer", "outcome": "issued", "target": "B1" }));
    assert_eq!(status, 200, "{body}");
    let (status, unit) = handle(&mut svc, &req("GET", "/units/B1", &[], Value::Null), None, T0);
    assert_eq!(status, 200);
    assert_eq!(unit["kind"], "Booking");
    assert!(unit["gate"][0].as_str().unwrap().starts_with("admitted: fan Ada books a new Booking B1"), "{unit}");
    let (_, trace) = handle(&mut svc, &req("GET", "/trace?from=3", &[], Value::Null), None, T0);
    assert_eq!(trace["from"], 3);
    assert_eq!(trace["next"], svc.host.trace.len());
}

#[test]
fn the_gate_refuses_and_bad_requests_say_why() {
    let mut svc = Service::in_memory(&tickets(), T0);
    setup(&mut svc);
    let (status, body) = book(&mut svc, "fan Bo", "B1");
    assert_eq!(status, 409, "{body}");
    assert!(body["refused"].as_str().unwrap().starts_with("refused at the gate"), "{body}");
    let b = json!({ "new": { "verb": "books", "kind": "Booking", "label": "B1" } });
    let (status, body) = handle(&mut svc, &req("POST", "/inputs", &[], b), Some(TOKEN), T0);
    assert_eq!((status, body["error"].as_str().unwrap()), (401, "an input needs the header `Suture-Caller: <role> <name>`, which the gateway sets"));
    let b = json!({ "new": { "verb": "books", "kind": "Booking", "label": "B1", "attrs": { "quantity": "two" } } });
    let (status, body) = handle(&mut svc, &req("POST", "/inputs", &[("Suture-Caller", "fan Ada")], b), Some(TOKEN), T0);
    assert_eq!((status, body["error"].as_str().unwrap()), (400, "quantity: \"two\" is not a whole number"));
    let (status, _) = handle(&mut svc, &req("POST", "/world", &[], json!({})), None, T0);
    assert_eq!(status, 403, "no token configured: operator requests are refused");
    let (status, _) = handle(&mut svc, &req("POST", "/world", &[("Authorization", "Bearer wrong")], json!({})), Some(TOKEN), T0);
    assert_eq!(status, 401);
    assert_eq!(handle(&mut svc, &req("GET", "/units/Nobody", &[], Value::Null), None, T0).0, 404);
    assert_eq!(handle(&mut svc, &req("DELETE", "/inputs", &[], Value::Null), None, T0).0, 405);
    assert_eq!(handle(&mut svc, &req("GET", "/nowhere", &[], Value::Null), None, T0).0, 404);
}

/// Sends one raw HTTP request and returns the status and the JSON body.
fn send(addr: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> (u16, Value) {
    let mut s = TcpStream::connect(addr).unwrap();
    let mut head = format!("{method} {path} HTTP/1.1\r\nHost: test\r\nContent-Length: {}\r\n", body.len());
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    s.write_all(format!("{head}\r\n{body}").as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let status = out[9..12].parse().unwrap();
    let json = out.split_once("\r\n\r\n").map(|(_, b)| serde_json::from_str(b).unwrap()).unwrap();
    (status, json)
}

#[test]
fn the_server_answers_over_tcp() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let p = tickets();
    let server = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut svc = Service::in_memory(&p, T0);
            let mut out = vec![];
            suture_runtime::http::serve(&mut svc, listener, Some(TOKEN), None, &stop, || T0, &mut out).unwrap();
            String::from_utf8(out).unwrap()
        })
    };
    let auth = format!("Bearer {TOKEN}");
    assert_eq!(send(&addr, "GET", "/health", &[], ""), (200, json!({ "ok": true, "module": "tickets", "version": 1, "pod": "solo", "pods": ["solo"] })));
    let (status, _) = send(&addr, "POST", "/world", &[("Authorization", &auth)], r#"{"kind": "Fan", "label": "Ada", "attrs": {"name": "Ada"}}"#);
    assert_eq!(status, 200);
    let (status, body) = send(&addr, "POST", "/inputs", &[("Suture-Caller", "fan Ada")], "not json");
    assert_eq!(status, 400);
    assert!(body["error"].as_str().unwrap().starts_with("the body is not JSON"), "{body}");
    let (status, body) = send(&addr, "POST", "/inputs", &[("Suture-Caller", "fan Ada"), ("Suture-Caller", "box Sam")], "{}");
    assert_eq!((status, body["error"].as_str().unwrap()), (400, "more than one `suture-caller` header"));
    let (status, body) = send(&addr, "POST", "/inputs", &[("Transfer-Encoding", "chunked")], "");
    assert_eq!((status, body["error"].as_str().unwrap()), (400, "a body must be sent with a Content-Length, not a Transfer-Encoding"));
    let long = "x".repeat(20_000);
    let (status, body) = send(&addr, "GET", "/health", &[("X-Long", &long)], "");
    assert_eq!((status, body["error"].as_str().unwrap()), (400, "the headers are too large"));
    let (status, body) = send(&addr, "GET", "/units/No%20one", &[], "");
    assert_eq!((status, body["error"].as_str().unwrap()), (404, "there is no unit No one"));
    stop.store(true, Ordering::Relaxed);
    let out = server.join().unwrap();
    assert!(out.contains("Ada placed: name \"Ada\""), "{out}");
}

/// An adapter that answers only once `open` is set.
#[derive(Clone)]
struct Held {
    open: Arc<AtomicBool>,
}

impl suture_runtime::backend::Transport for Held {
    fn post(&mut self, _: &str, _: Option<&str>, _: &Value, _: std::time::Duration) -> Result<Value, String> {
        let start = std::time::Instant::now();
        while !self.open.load(Ordering::Relaxed) && start.elapsed() < std::time::Duration::from_secs(20) {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        Ok(json!({ "outcome": "charged" }))
    }

    fn on_another_thread(&self) -> Option<Box<dyn suture_runtime::backend::Transport + Send>> {
        Some(Box::new(self.clone()))
    }
}

#[test]
fn a_slow_adapter_holds_up_no_input() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let (stop, open) = (Arc::new(AtomicBool::new(false)), Arc::new(AtomicBool::new(false)));
    let p = tickets();
    let server = {
        let (stop, open) = (stop.clone(), open.clone());
        std::thread::spawn(move || {
            let mut svc = Service::in_memory(&p, T0);
            setup(&mut svc);
            let adapters = suture_runtime::service::adapters(r#"{"payments": {"url": "http://pay.local/charge"}}"#).unwrap();
            svc.connect(adapters, Box::new(Held { open }));
            suture_runtime::http::serve(&mut svc, listener, Some(TOKEN), None, &stop, || T0, &mut vec![]).unwrap();
        })
    };
    let booking = r#"{"new": {"verb": "books", "kind": "Booking", "label": "B1", "attrs": {"fan": "Ada", "show": "Hamlet", "quantity": 2}}}"#;
    // The payment is posted while the adapter holds it; the inputs go on.
    assert_eq!(send(&addr, "POST", "/inputs", &[("Suture-Caller", "fan Ada")], booking).0, 200);
    let b2 = booking.replace("B1", "B2");
    assert_eq!(send(&addr, "POST", "/inputs", &[("Suture-Caller", "fan Ada")], &b2).0, 200);
    let charged = |addr: &str| send(addr, "GET", "/trace?from=0", &[], "").1["lines"].as_array().unwrap().iter().any(|l| l.as_str().unwrap().contains("B1  Payment charged"));
    assert!(!charged(&addr), "the adapter has not answered yet");
    open.store(true, Ordering::Relaxed);
    let start = std::time::Instant::now();
    while !charged(&addr) {
        assert!(start.elapsed() < std::time::Duration::from_secs(10), "its answer is applied when it comes");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
}
