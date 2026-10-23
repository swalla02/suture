//! Slice 51: hardening from the security review (spec appendix BB).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde_json::Value;
use suture_lang::scenario::parse_scenarios;
use suture_runtime::http::{handle, Request, TRACE_PAGE};
use suture_runtime::scenario::run;
use suture_runtime::service::Service;

const T0: u64 = 1_800_000_000;

const CFP: &str = "module cfp version 1, currency EUR
role staff
kind Person (plural People) { name: Text, email: Text personal, photo: Media }
rule emails: no two People have the same email, checked globally
admit add a new Person by staff
";

fn compiled(src: &str) -> suture_lang::ir::Program {
    suture_lang::compile(&[("t.st".to_string(), src.to_string())]).unwrap_or_else(|e| panic!("{e:?}"))
}

fn req(method: &str, target: &str, headers: &[(&str, &str)], body: &[u8]) -> Request {
    let (path, query) = target.split_once('?').unwrap_or((target, ""));
    Request { method: method.into(), path: path.into(), query: query.into(), headers: headers.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(), body: body.to_vec() }
}

/// Sends raw bytes and returns the status and the JSON body.
fn send(addr: &str, raw: &str) -> (u16, Value) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    (out[9..12].parse().unwrap(), out.split_once("\r\n\r\n").map(|(_, b)| serde_json::from_str(b).unwrap()).unwrap())
}

#[test]
fn behind_a_gateway_only_its_requests_are_served() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let stop = Arc::new(AtomicBool::new(false));
    let p = compiled(CFP);
    let server = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut svc = Service::in_memory(&p, T0);
            suture_runtime::http::serve(&mut svc, listener, None, Some("g-secret"), &stop, || T0, &mut vec![]).unwrap();
        })
    };
    assert_eq!(send(&addr, "GET /health HTTP/1.1\r\n\r\n").0, 200, "health needs no token");
    let (status, body) = send(&addr, "GET /units/P1 HTTP/1.1\r\n\r\n");
    assert_eq!((status, body["error"].as_str().unwrap()), (401, "this service is reached only through its gateway"));
    assert_eq!(send(&addr, "GET /units/P1 HTTP/1.1\r\nSuture-Gateway: wrong\r\n\r\n").0, 401);
    assert_eq!(send(&addr, "GET /units/P1 HTTP/1.1\r\nSuture-Gateway: g-secret\r\n\r\n").0, 404);
    // A folded line, which a gateway may read as part of the header above.
    let (status, body) = send(&addr, "POST /inputs HTTP/1.1\r\nX-Note: a\r\n Suture-Caller: staff Sam\r\nSuture-Gateway: g-secret\r\n\r\n");
    assert_eq!((status, body["error"].as_str().unwrap()), (400, "a header line may not start with a space"));
    stop.store(true, Ordering::Relaxed);
    server.join().unwrap();
}

#[test]
fn the_trace_is_read_a_page_at_a_time() {
    let mut svc = Service::in_memory(&compiled(CFP), T0);
    let start = svc.host.trace.len();
    svc.host.trace.extend((0..TRACE_PAGE + 5).map(|i| format!("line {i}")));
    let (_, first) = handle(&mut svc, &req("GET", &format!("/trace?from={start}"), &[], b""), None, T0);
    assert_eq!(first["lines"].as_array().unwrap().len(), TRACE_PAGE);
    let next = first["next"].as_u64().unwrap();
    assert_eq!(next as usize, start + TRACE_PAGE);
    let (_, rest) = handle(&mut svc, &req("GET", &format!("/trace?from={next}"), &[], b""), None, T0);
    assert_eq!(rest["lines"].as_array().unwrap().len(), 5);
    let (_, past) = handle(&mut svc, &req("GET", "/trace?from=999999", &[], b""), None, T0);
    assert_eq!((past["lines"].as_array().unwrap().len(), past["next"].as_u64()), (0, Some(999999)));
}

#[test]
fn only_a_role_of_the_module_uploads_media() {
    let mut svc = Service::in_memory(&compiled(CFP), T0);
    let mime = ("content-type", "image/png");
    let (status, body) = handle(&mut svc, &req("POST", "/media", &[("suture-caller", "nobody Eve"), mime], b"png"), None, T0);
    assert_eq!((status, body["error"].as_str().unwrap()), (403, "there is no role nobody"));
    assert_eq!(handle(&mut svc, &req("POST", "/media", &[("suture-caller", "staff Sam"), mime], b"png"), None, T0).0, 200);
}

#[test]
fn a_scenario_reads_files_only_inside_its_module() {
    let p = compiled(CFP);
    let base = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("hardening");
    let (module, outside) = (base.join("module"), base.join("outside.png"));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&module).unwrap();
    std::fs::write(&outside, b"secret").unwrap();
    std::fs::write(module.join("ok.png"), b"png").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, module.join("link.png")).unwrap();
    let mut cases = vec![("ok.png", None), ("../outside.png", Some("without `..` or a leading `/`")), (outside.to_str().unwrap(), Some("without `..`"))];
    if cfg!(unix) {
        cases.push(("link.png", Some("is not a file in the module's directory")));
    }
    for (file, want) in cases {
        let src = format!("scenario \"s\":\n  when   staff Sam adds a new Person P1 with name \"P\", email \"p@x.org\" and photo file \"{file}\"\n  expect P1's name is \"P\"\n");
        let sc = &parse_scenarios(module.join("s.scenario").to_str().unwrap(), &src).unwrap()[0];
        let r = run(&p, sc);
        match want {
            None => assert!(r.passed(), "{file}: {:?}", r.failures),
            Some(w) => assert!(r.failures.iter().any(|f| f.contains(w)), "{file}: {:?}", r.failures),
        }
    }
}

#[test]
fn a_personal_value_that_must_be_unique_is_never_shown() {
    let p = compiled(&CFP.replace(", photo: Media", ""));
    let text = "scenario \"s\":
  when   staff Sam adds a new Person A with name \"A\" and email \"ann@x.org\"
  and    staff Sam adds a new Person B with name \"B\" and email \"ann@x.org\"
         is refused by rule emails
";
    let sc = &parse_scenarios("t.scenario", text).unwrap()[0];
    let r = run(&p, sc);
    assert!(r.passed(), "{:?}\n{}", r.failures, r.host.trace.join("\n"));
    let all = format!("{}\n{}", r.host.trace.join("\n"), r.host.gate.join("\n"));
    assert!(!all.contains("ann@x.org"), "{all}");
    assert!(all.contains("(personal "), "the group is named by a hash: {all}");
}
