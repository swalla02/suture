//! The service's HTTP API (appendix L.2.5–L.2.6): a small HTTP/1.1 server
//! on the standard library, one request at a time, so inputs are applied in
//! the order they arrive.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use serde_json::{json, Value};

use crate::explain::explain_at;
use crate::service::{self, Record, Service, Who};

/// The most a request's headers and body may hold.
const MAX_HEADERS: usize = 16 * 1024;
const MAX_BODY: usize = 1024 * 1024;

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

/// Answers one request at Unix time `now`. `operator` is the token operator
/// requests must carry; without one, they are refused.
pub fn handle(svc: &mut Service, req: &Request, operator: Option<&str>, now: u64) -> (u16, Value) {
    let error = |status: u16, msg: String| (status, json!({ "error": msg }));
    match (req.method.as_str(), req.path.as_str()) {
        ("GET", "/health") => (200, json!({ "ok": true, "module": svc.host.prog.module, "version": svc.host.prog.version })),
        ("GET", "/pending") => {
            let prog = svc.host.prog.clone();
            let list: Vec<Value> = svc.host.pending().into_iter().map(|a| service::request_json(&prog, a)).collect();
            (200, json!({ "pending": list }))
        }
        ("GET", "/trace") => {
            let from = req.query.split('&').find_map(|kv| kv.strip_prefix("from=")).and_then(|n| n.parse::<usize>().ok()).unwrap_or(0);
            let lines = svc.host.trace.get(from..).unwrap_or_default();
            (200, json!({ "from": from, "next": svc.host.trace.len().max(from), "lines": lines }))
        }
        ("GET", p) if p.starts_with("/units/") => {
            let label = &p["/units/".len()..];
            let Some(unit) = svc.host.unit(label) else { return error(404, format!("there is no unit {label}")) };
            let host = &svc.host;
            let explain = explain_at(&host.prog, &unit, Some(host.now as i64), |k| host.deadline(k).map(|d| d - host.now));
            let gate: Vec<&String> = host.gate_for(label);
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
        ("POST", "/world" | "/answers") => {
            match (operator, req.header("authorization").and_then(|a| a.strip_prefix("Bearer "))) {
                (None, _) => return error(403, "operator requests are refused: SUTURE_OPERATOR_TOKEN is not set".into()),
                (Some(t), Some(given)) if t == given => {}
                _ => return error(401, "an operator request needs `Authorization: Bearer <SUTURE_OPERATOR_TOKEN>`".into()),
            }
            let r = body(req).and_then(|b| if req.path == "/world" { world(svc, &b) } else { answer(svc, &b) });
            match r {
                Ok(r) => apply(svc, r, now),
                Err(e) => error(400, e),
            }
        }
        (_, "/health" | "/pending" | "/trace" | "/inputs" | "/world" | "/answers") => error(405, format!("{} is not allowed on {}", req.method, req.path)),
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
        return Ok(Record::Event { by, verb: text(e, "verb")?, target: text(e, "target")? });
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
    let mut r = BufReader::new(stream);
    let mut head = String::new();
    let mut line = String::new();
    let mut req = Request::default();
    loop {
        line.clear();
        if r.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("the connection closed".into());
        }
        head.push_str(&line);
        if head.len() > MAX_HEADERS {
            return Err("the headers are too large".into());
        }
        let l = line.trim_end();
        if req.method.is_empty() {
            let mut parts = l.split(' ');
            let (m, target) = (parts.next().unwrap_or(""), parts.next().ok_or("not an HTTP request")?);
            req.method = m.to_string();
            let (path, query) = target.split_once('?').unwrap_or((target, ""));
            (req.path, req.query) = (path.to_string(), query.to_string());
        } else if l.is_empty() {
            break;
        } else if let Some((k, v)) = l.split_once(':') {
            req.headers.push((k.trim().to_ascii_lowercase(), v.trim().to_string()));
        }
    }
    let len = req.header("content-length").map_or(Ok(0), |n| n.parse::<usize>().map_err(|_| "a bad Content-Length"))?;
    if len > MAX_BODY {
        return Err("the body is too large".into());
    }
    req.body = vec![0; len];
    r.read_exact(&mut req.body).map_err(|e| e.to_string())?;
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
pub fn serve(svc: &mut Service, listener: TcpListener, operator: Option<&str>, stop: &AtomicBool, clock: impl Fn() -> u64, out: &mut dyn Write) -> std::io::Result<()> {
    listener.set_nonblocking(true)?;
    let mut shown = svc.host.trace.len();
    let mut last = 0;
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                stream.set_nonblocking(false)?;
                stream.set_read_timeout(Some(Duration::from_secs(5)))?;
                let (status, body) = match read_request(&mut stream) {
                    Ok(req) => handle(svc, &req, operator, clock()),
                    Err(e) => (400, json!({ "error": e })),
                };
                // A caller that has gone away does not stop the service.
                let _ = write_response(&mut stream, status, &body);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => writeln!(out, "accept: {e}")?,
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
        shown = svc.host.trace.len();
    }
    Ok(())
}
