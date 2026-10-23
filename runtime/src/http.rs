//! The service's HTTP API (appendix L.2.5–L.2.6): a small HTTP/1.1 server
//! on the standard library, one request at a time, so inputs are applied in
//! the order they arrive.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::explain::explain_at;
use crate::media::MAX_MEDIA_BYTES;
use crate::state::Val;
use crate::service::{self, Record, Service, Who};
use suture_lang::ir::{third_person, Admitted};

/// The most a request's headers and body may hold, and how long it may take
/// to arrive: the service answers one request at a time.
const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 256 * 1024;
const DEADLINE: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Default)]
pub struct Request {
    pub method: String,
    /// The path, without the query.
    pub path: String,
    pub query: String,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }
}

/// Compares in time that does not depend on where the texts differ.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// `%20` and the like, decoded; a bad escape is kept as written.
fn decoded(path: &str) -> String {
    let b = path.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).and_then(|c| hex(*c)), b.get(i + 2).and_then(|c| hex(*c))) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The trace lines and gate decisions a served host keeps in memory; the
/// rest went to its standard output.
pub const TRACE_KEEP: usize = 100_000;

/// The most trace lines one `GET /trace` returns.
pub const TRACE_PAGE: usize = 1000;

/// Whether a request may be served: it carries the gateway's token, if
/// there is one, or it is `GET /health`.
pub fn from_gateway(req: &Request, gateway: Option<&str>) -> bool {
    let Some(t) = gateway else { return true };
    (req.method == "GET" && req.path == "/health") || req.header("suture-gateway").is_some_and(|g| same(t, g))
}

/// Answers one request at Unix time `now`. `operator` is the token operator
/// requests must carry; without one, they are refused.
pub fn handle(svc: &mut Service, req: &Request, operator: Option<&str>, now: u64) -> (u16, Value) {
    let error = |status: u16, msg: String| (status, json!({ "error": msg }));
    // A read first catches up with what other pods appended (appendix O.2.2).
    // Any request first catches up with what other pods appended, so an
    // input is read against the units and the program they made (appendix O.2.2).
    if let Err(e) = svc.refresh() {
        return error(503, e);
    }
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/health") => (200, json!({ "ok": true, "module": svc.host.prog.module, "version": svc.host.prog.version, "pod": svc.pod, "pods": svc.pods(now) })),
        ("GET", "/pending") => {
            // With the operator token, an adapter polling for its requests
            // receives the marked fields it was granted (appendix N.2.5).
            let reveal = matches!((operator, req.header("authorization").and_then(|a| a.strip_prefix("Bearer "))), (Some(t), Some(given)) if same(t, given));
            let list: Vec<Value> = svc.host.pending().into_iter().map(|a| service::request_json(&svc.host, a, reveal)).collect();
            (200, json!({ "pending": list }))
        }
        ("GET", "/trace") => {
            let from = req.query.split('&').find_map(|kv| kv.strip_prefix("from=")).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
            // At most TRACE_PAGE lines at a time; `next` reads on.
            // Lines no longer kept are skipped (appendix BD.2.2).
            let from = from.max(svc.host.trace_dropped);
            let lines = svc.host.trace.get(from - svc.host.trace_dropped..).unwrap_or_default();
            let lines = &lines[..lines.len().min(TRACE_PAGE)];
            (200, json!({ "from": from, "next": from + lines.len(), "lines": lines }))
        }
        ("GET", p) if p.starts_with("/units/") => {
            let label = decoded(&p["/units/".len()..]);
            let Some(unit) = svc.host.unit(&label) else { return error(404, format!("there is no unit {label}")) };
            // Read at the current time, which the runtime clock may not have reached.
            let at = svc.elapsed(now).max(svc.host.now);
            let host = &svc.host;
            let explain = explain_at(&host.prog, &unit, Some(at as i64), |k| host.deadline(k).map(|d| d.saturating_sub(at)));
            let gate: Vec<&String> = host.gate_for(&label);
            (200, json!({ "label": label, "kind": unit.root.kind, "explain": explain, "gate": gate }))
        }
        ("POST", "/inputs") => {
            let Some((role, name)) = req.header("suture-caller").and_then(|c| c.trim().split_once(' ')) else {
                return error(401, "an input needs the header `Suture-Caller: <role> <name>`, which the gateway sets".into());
            };
            let by = Who { role: role.to_string(), name: name.trim().to_string() };
            match body(req).and_then(|b| input(svc, by, &b)) {
                Ok(r) => apply(svc, r, now),
                Err(e) => error(400, e),
            }
        }
        ("POST", "/media") => {
            let Some((role, _)) = req.header("suture-caller").and_then(|c| c.trim().split_once(' ')) else {
                return error(401, "media needs the header `Suture-Caller: <role> <name>`, which the gateway sets".into());
            };
            if !svc.host.prog.roles.iter().any(|r| r.name == role) {
                return error(403, format!("there is no role {role}"));
            }
            let Some(mime) = req.header("content-type").map(|m| m.split(';').next().unwrap_or_default().trim().to_ascii_lowercase()) else {
                return error(400, "media needs a `Content-Type` such as image/png".into());
            };
            match svc.host.media.put(&mime, req.body.clone()) {
                Ok(Val::Media { sha256, mime, bytes }) => (200, json!({ "sha256": sha256, "mime": mime, "bytes": bytes })),
                Ok(_) => error(500, "the store gave no media".into()),
                Err(e) => error(400, e),
            }
        }
        ("POST", "/world" | "/answers") => {
            match (operator, req.header("authorization").and_then(|a| a.strip_prefix("Bearer "))) {
                (None, _) => return error(403, "operator requests are refused: SUTURE_OPERATOR_TOKEN is not set".into()),
                (Some(t), Some(given)) if same(t, given) => {}
                _ => return error(401, "an operator request needs `Authorization: Bearer <SUTURE_OPERATOR_TOKEN>`".into()),
            }
            let r = body(req).and_then(|b| if req.path == "/world" { world(svc, &b) } else { answer(svc, &b) });
            match r {
                Ok(r) => apply(svc, r, now),
                Err(e) => error(400, e),
            }
        }
        (_, "/health" | "/pending" | "/trace" | "/inputs" | "/world" | "/answers" | "/media") => error(405, format!("{} is not allowed on {}", req.method, req.path)),
        (_, p) if p.starts_with("/units/") => error(405, format!("{} is not allowed on {}", req.method, req.path)),
        (_, p) => error(404, format!("no such path: {p}")),
    }
}

fn apply(svc: &mut Service, r: Record, now: u64) -> (u16, Value) {
    match svc.input(r, now) {
        Ok(a) => match a.refused {
            None => (200, json!({ "trace": a.trace })),
            Some(why) => (409, json!({ "refused": why, "trace": a.trace })),
        },
        Err(e) => (500, json!({ "error": e })),
    }
}

fn body(req: &Request) -> Result<Value, String> {
    serde_json::from_slice(&req.body).map_err(|e| format!("the body is not JSON: {e}"))
}

fn text(v: &Value, field: &str) -> Result<String, String> {
    v.get(field).and_then(Value::as_str).map(String::from).ok_or_else(|| format!("`{field}` is missing, or is not text"))
}

fn input(svc: &Service, by: Who, b: &Value) -> Result<Record, String> {
    let prog = &svc.host.prog;
    let one = |k: &str| b.get(k).filter(|_| b.as_object().is_some_and(|o| o.len() == 1));
    if let Some(n) = one("new") {
        let kind = text(n, "kind")?;
        let (attrs, items) = (service::attrs_json(prog, &kind, n.get("attrs"))?, service::items_json(prog, &kind, n.get("items"))?);
        return Ok(Record::New { by, verb: text(n, "verb")?, label: text(n, "label")?, kind, attrs, items });
    }
    if let Some(e) = one("event") {
        let (verb, target) = (text(e, "verb")?, text(e, "target")?);
        let part = match e.get("part") {
            None => None,
            Some(p) => Some(p.as_str().map(String::from).ok_or("an event's `part` is the name of a part kind")?),
        };
        let root = svc.host.unit(&target).map(|u| u.root.kind).ok_or_else(|| format!("there is no unit {target}"))?;
        let kind = part.clone().unwrap_or(root);
        // The data is typed by the event's `admit … with` (appendix Q.2.1).
        let want = prog.admits.iter().find_map(|a| match &a.what {
            Admitted::Event { verb: v, kind: k } if third_person(v) == verb && *k == kind => Some(a.data.as_slice()),
            _ => None,
        });
        let mut data = vec![];
        if let Some(d) = e.get("data") {
            for (n, v) in d.as_object().ok_or("an event's `data` is an object")? {
                let ty = want.and_then(|w| w.iter().find(|(x, _)| x == n)).map(|(_, t)| *t);
                data.push((n.clone(), service::from_json(v, ty, &prog.currency).map_err(|e| format!("{n}: {e}"))?));
            }
        }
        return Ok(Record::Event { by, verb, target, part, data });
    }
    if let Some(c) = one("change") {
        let (target, attr) = (text(c, "target")?, text(c, "attr")?);
        let kind = svc.host.unit(&target).map(|u| u.root.kind).ok_or_else(|| format!("there is no unit {target}"))?;
        let ty = prog.kind(&kind).and_then(|k| k.attr(&attr)).map(|a| a.ty);
        let value = service::from_json(c.get("value").ok_or("`value` is missing")?, ty, &prog.currency).map_err(|e| format!("{attr}: {e}"))?;
        return Ok(Record::Change { by, target, attr, value });
    }
    Err("an input is one of {\"new\": …}, {\"event\": …} or {\"change\": …}".into())
}

fn world(svc: &Service, b: &Value) -> Result<Record, String> {
    let prog = &svc.host.prog;
    let kind = text(b, "kind")?;
    let (attrs, items) = (service::attrs_json(prog, &kind, b.get("attrs"))?, service::items_json(prog, &kind, b.get("items"))?);
    Ok(Record::World { label: text(b, "label")?, kind, attrs, items })
}

fn answer(svc: &Service, b: &Value) -> Result<Record, String> {
    let (adapter, outcome) = (text(b, "adapter")?, text(b, "outcome")?);
    let key = b.get("key").and_then(Value::as_str).map(String::from);
    let target = b.get("target").and_then(Value::as_str).map(String::from);
    if key.is_none() && target.is_none() {
        return Err("an answer names its request's `key`, or the `target` unit".into());
    }
    // The data is typed by the attributes of the request it answers.
    let ask = svc.host.pending().into_iter().find(|a| a.adapter == adapter && key.as_ref().map_or(target.as_ref() == Some(&a.label), |k| *k == a.key));
    let data = match (b.get("data"), ask) {
        (None, _) => vec![],
        (Some(d), Some(a)) => service::answer_data(&svc.host.prog, a, d)?,
        (Some(d), None) => service::attrs_json(&svc.host.prog, "", Some(d))?,
    };
    Ok(Record::Answer { adapter, outcome, key, target, data })
}

/// Reads one request, within the size limits.
pub fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let start = Instant::now();
    let left = |s: &TcpStream| -> Result<(), String> {
        let rest = DEADLINE.checked_sub(start.elapsed()).filter(|d| !d.is_zero()).ok_or("the request took too long to arrive")?;
        s.set_read_timeout(Some(rest)).map_err(|e| e.to_string())
    };
    let mut r = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?.take((MAX_HEADERS + MAX_MEDIA_BYTES) as u64));
    let mut head = 0;
    let mut line = vec![];
    let mut req = Request::default();
    loop {
        line.clear();
        left(stream)?;
        // At most what is left of the headers' allowance, so a line with no end stops there.
        let n = (&mut r).take((MAX_HEADERS - head) as u64).read_until(b'\n', &mut line).map_err(|e| e.to_string())?;
        head += n;
        if !line.ends_with(b"\n") {
            return Err(if head >= MAX_HEADERS { "the headers are too large".into() } else { "the connection closed".into() });
        }
        let line = std::str::from_utf8(&line).map_err(|_| "a header that is not UTF-8")?;
        let l = line.trim_end();
        if req.method.is_empty() {
            let mut parts = l.split(' ');
            let (m, target) = (parts.next().unwrap_or(""), parts.next().ok_or("not an HTTP request")?);
            req.method = m.to_string();
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            (req.path, req.query) = (path.to_string(), query.to_string());
        } else if l.is_empty() {
            break;
        } else if line.starts_with([' ', '\t']) {
            // A folded header line, which a gateway may read as part of the
            // header above it.
            return Err("a header line may not start with a space".into());
        } else if let Some((k, v)) = l.split_once(':') {
            let k = k.trim().to_ascii_lowercase();
            // A gateway that adds a header rather than replacing it must not let
            // the client's own win.
            if ["suture-caller", "suture-gateway", "content-length", "authorization"].contains(&k.as_str()) && req.header(&k).is_some() {
                return Err(format!("more than one `{k}` header"));
            }
            req.headers.push((k, v.trim().to_string()));
        }
    }
    if !req.method.chars().all(|c| c.is_ascii_uppercase()) || req.method.is_empty() || !req.path.starts_with('/') {
        return Err("not an HTTP request".into());
    }
    if req.header("transfer-encoding").is_some() {
        return Err("a body must be sent with a Content-Length, not a Transfer-Encoding".into());
    }
    let len = req.header("content-length").map_or(Ok(0), |n| n.parse::<usize>().map_err(|_| "a bad Content-Length"))?;
    // Media is sent whole, so its body may be larger (appendix V.2.2).
    let max = if req.path == "/media" { MAX_MEDIA_BYTES } else { MAX_BODY };
    if len > max {
        return Err(format!("the body is too large: at most {max} bytes"));
    }
    req.body = vec![0; len];
    let mut got = 0;
    while got < len {
        left(stream)?;
        match r.read(&mut req.body[got..]).map_err(|e| e.to_string())? {
            0 => return Err("the connection closed".into()),
            n => got += n,
        }
    }
    Ok(req)
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        _ => "Internal Server Error",
    }
}

pub fn write_response(stream: &mut TcpStream, status: u16, body: &Value) -> std::io::Result<()> {
    let body = body.to_string();
    let head = format!("HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reason(status), body.len());
    stream.write_all(head.as_bytes())?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

/// Serves until `stop` is set: one request at a time, and once a second the
/// timers that fell due (appendix L.2.4). Each trace line goes to `out`.
/// However it ends, the pod leaves the pods, so its units move at once.
/// With a `gateway` token, every request but `GET /health` must carry it as
/// `Suture-Gateway`, so only the gateway can name callers (appendix BB.2.1).
pub fn serve(svc: &mut Service, listener: TcpListener, operator: Option<&str>, gateway: Option<&str>, stop: &AtomicBool, clock: impl Fn() -> u64, out: &mut dyn Write) -> std::io::Result<()> {
    let served = serving(svc, listener, operator, gateway, stop, clock, out);
    svc.leave();
    served
}

fn serving(svc: &mut Service, listener: TcpListener, operator: Option<&str>, gateway: Option<&str>, stop: &AtomicBool, clock: impl Fn() -> u64, out: &mut dyn Write) -> std::io::Result<()> {
    listener.set_nonblocking(true)?;
    let mut shown = svc.host.trace.len();
    let mut last = 0;
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let (status, body, unread) = match read_request(&mut stream) {
                    Ok(req) if !from_gateway(&req, gateway) => (401, json!({ "error": "this service is reached only through its gateway" }), false),
                    Ok(req) => {
                        let (status, body) = handle(svc, &req, operator, clock());
                        (status, body, false)
                    }
                    Err(e) => (400, json!({ "error": e }), true),
                };
                // A caller that has gone away does not stop the service.
                let _ = write_response(&mut stream, status, &body);
                if unread {
                    // Closing with unread bytes would reset the connection before
                    // the caller reads why; so read a little of the rest, briefly.
                    let _ = stream.shutdown(std::net::Shutdown::Write);
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
                    let _ = std::io::copy(&mut (&stream).take(MAX_BODY as u64), &mut std::io::sink());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                writeln!(out, "accept: {e}")?;
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        // Once asked to stop, it starts no timer and posts nothing more.
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let now = clock();
        if now != last {
            last = now;
            if let Err(e) = svc.tick(now) {
                writeln!(out, "tick: {e}")?;
            }
        }
        for l in &svc.host.trace[shown..] {
            writeln!(out, "{l}")?;
        }
        svc.host.trim(TRACE_KEEP);
        shown = svc.host.trace.len();
    }
    Ok(())
}
