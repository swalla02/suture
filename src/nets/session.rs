//! Experiment B: a session protocol with timeouts.
//!
//! Two logical services, a client and a server, each consume an *inbox*: a
//! stream whose cells are the messages themselves (`Open`, `Accept`,
//! `Data(seq, v)`, `Ack(seq)`, `Close`, `Closed`, `Timeout(tag)`) and whose
//! tail is a labeled hole the host appends to. Protocol states are agents;
//! every (state, message) pair has an explicit rule — legal transitions,
//! tolerated duplicates, and explicit rejections — so coverage analysis can
//! prove no message ever finds a state without a rule.
//!
//! The protocol is stop-and-wait with retransmission: the client sends one
//! data item at a time, arms a timer, and retransmits on timeout up to
//! `max_tries` times per step; the server delivers each sequence number
//! exactly once and re-acknowledges duplicates. Messages and timer firings
//! cross the boundary (`Transmit` and `Timer` effects), because a race
//! between a reply and a timeout is a merge that only the boundary can
//! arbitrate (D2).

use std::collections::BTreeMap;

use crate::machine::{InjectError, Machine, Program, Stats};
use crate::net::{Injection, NetBuilder};
use crate::rng::Rng;
use crate::rule::{RuleCx, RuleError};
use crate::value::{Value, ValueType};
use crate::{tup, KindId, RuleTable, SigBuilder};

#[derive(Clone, Copy, Debug)]
pub struct SessionKinds {
    pub kick: KindId,
    pub open: KindId,
    pub accept: KindId,
    pub data: KindId,
    pub ack: KindId,
    pub close: KindId,
    pub closed: KindId,
    pub timeout: KindId,
    pub c_start: KindId,
    pub c_connecting: KindId,
    pub c_await_ack: KindId,
    pub c_closing: KindId,
    pub c_done: KindId,
    pub s_listen: KindId,
    pub s_established: KindId,
    pub s_closed: KindId,
    pub transmit: KindId,
    pub timer: KindId,
    pub reject: KindId,
    pub tx_done: KindId,
    pub tm_done: KindId,
    pub rj_done: KindId,
    pub session_ok: KindId,
    pub session_aborted: KindId,
    pub delivered: KindId,
}

impl SessionKinds {
    pub fn messages(&self) -> [KindId; 8] {
        [self.kick, self.open, self.accept, self.data, self.ack, self.close, self.closed, self.timeout]
    }
}

#[derive(Clone, Debug)]
pub struct SessionConfig {
    /// Transmissions per protocol step before the client gives up.
    pub max_tries: i64,
}

fn ints(n: usize) -> ValueType {
    ValueType::Tuple(vec![ValueType::Int; n])
}

pub fn program(cfg: &SessionConfig) -> (Program, SessionKinds) {
    let mut b = SigBuilder::new("session", 1);
    let inbox = b.polarized("Inbox");
    let status = b.polarized("Status");
    let app = b.polarized("App");
    let tx = b.polarized("Tx");
    let tm = b.polarized("Tm");
    let rj = b.polarized("Rj");
    let list = || ValueType::List(Box::new(ValueType::Int));
    let mut cell = |name: &str, fields: usize| b.kind(name, inbox.pos()).aux("rest", inbox.neg()).payload(ints(fields)).done();
    let (kick, open, accept) = (cell("Kick", 1), cell("Open", 1), cell("Accept", 1));
    let (data, ack, close, closed, timeout) =
        (cell("Data", 3), cell("Ack", 2), cell("Close", 1), cell("Closed", 1), cell("Timeout", 2));
    let tuple = |fields: Vec<ValueType>| ValueType::Tuple(fields);
    let k = SessionKinds {
        kick,
        open,
        accept,
        data,
        ack,
        close,
        closed,
        timeout,
        c_start: b.kind("CStart", inbox.neg()).aux("status", status.pos()).payload(tuple(vec![ValueType::Int, list()])).done(),
        c_connecting: b
            .kind("CConnecting", inbox.neg())
            .aux("status", status.pos())
            .payload(tuple(vec![ValueType::Int, list(), ValueType::Int, ValueType::Int]))
            .done(),
        c_await_ack: b
            .kind("CAwaitAck", inbox.neg())
            .aux("status", status.pos())
            .payload(tuple(vec![ValueType::Int, list(), ValueType::Int, ValueType::Int, ValueType::Int]))
            .done(),
        c_closing: b.kind("CClosing", inbox.neg()).aux("status", status.pos()).payload(ints(3)).done(),
        c_done: b.kind("CDone", inbox.neg()).payload(ints(1)).done(),
        s_listen: b.kind("SListen", inbox.neg()).aux("app", app.pos()).payload(ints(1)).done(),
        s_established: b
            .kind("SEstablished", inbox.neg())
            .aux("app", app.pos())
            .payload(tuple(vec![ValueType::Int, ValueType::Int, list()]))
            .done(),
        s_closed: b.kind("SClosed", inbox.neg()).payload(ints(1)).done(),
        transmit: b.kind("Transmit", tx.neg()).payload(tuple(vec![ValueType::Str, ValueType::Str, list()])).effect().done(),
        timer: b.kind("Timer", tm.neg()).payload(ints(2)).effect().done(),
        reject: b
            .kind("Reject", rj.neg())
            .payload(tuple(vec![ValueType::Int, ValueType::Str, ValueType::Str]))
            .effect()
            .done(),
        tx_done: b.kind("TxDone", tx.pos()).done(),
        tm_done: b.kind("TmDone", tm.pos()).done(),
        rj_done: b.kind("RjDone", rj.pos()).done(),
        session_ok: b.kind("SessionOk", status.pos()).payload(ints(1)).done(),
        session_aborted: b.kind("SessionAborted", status.pos()).payload(tuple(vec![ValueType::Int, ValueType::Str])).done(),
        delivered: b.kind("Delivered", app.pos()).payload(tuple(vec![ValueType::Int, list()])).done(),
    };
    let sig = b.build().expect("session signature");
    let mut t = RuleTable::new();
    let effects = [k.transmit, k.timer, k.reject];
    let max = cfg.max_tries;
    for state in [k.c_start, k.c_connecting, k.c_await_ack, k.c_closing, k.c_done] {
        for msg in k.messages() {
            let name = format!("{}><{}", sig.kind(state).name, sig.kind(msg).name);
            t.def(&name, state, msg, move |cx| client(cx, &k, max)).emits(&effects);
        }
    }
    for state in [k.s_listen, k.s_established, k.s_closed] {
        for msg in k.messages() {
            let name = format!("{}><{}", sig.kind(state).name, sig.kind(msg).name);
            t.def(&name, state, msg, move |cx| server(cx, &k)).emits(&effects);
        }
    }
    t.def("tx_done", k.tx_done, k.transmit, |_| Ok(()));
    t.def("tm_done", k.tm_done, k.timer, |_| Ok(()));
    t.def("rj_done", k.rj_done, k.reject, |_| Ok(()));
    (Program::new(sig, t).expect("session rules"), k)
}

fn transmit(cx: &mut RuleCx<'_>, k: &SessionKinds, to: &str, kind: &str, fields: &[i64]) {
    cx.effect(k.transmit, tup![to, kind, Value::list(fields.iter().map(|&f| Value::Int(f)))], &[]);
}

fn reject(cx: &mut RuleCx<'_>, k: &SessionKinds, sid: i64) {
    let (state, msg) = (cx.sig().kind(cx.left_kind()).name.clone(), cx.sig().kind(cx.right_kind()).name.clone());
    cx.effect(k.reject, tup![sid, state, msg], &[]);
}

/// Sends item `seq` (or `Close` when all items are acknowledged), arms timer
/// `tag`, and returns the next client state.
fn send_step(cx: &mut RuleCx<'_>, k: &SessionKinds, sid: i64, items: &Value, seq: i64, tag: i64) -> (KindId, Value) {
    let xs = items.items().unwrap_or(&[]);
    cx.effect(k.timer, tup![sid, tag], &[]);
    if (seq as usize) < xs.len() {
        transmit(cx, k, "server", "Data", &[sid, seq, xs[seq as usize].as_int().unwrap_or(0)]);
        (k.c_await_ack, tup![sid, items.clone(), seq, 1, tag])
    } else {
        transmit(cx, k, "server", "Close", &[sid]);
        (k.c_closing, tup![sid, 1, tag])
    }
}

fn client(cx: &mut RuleCx<'_>, k: &SessionKinds, max_tries: i64) -> Result<(), RuleError> {
    let (state, msg) = (cx.left_kind(), cx.right_kind());
    let (s, m) = (cx.left().clone(), cx.right().clone());
    let [rest] = cx.right_aux();
    let sid = s.int(0)?;
    if state == k.c_done {
        let [] = cx.left_aux();
        cx.agent(k.c_done, s, rest, &[]); // session over: drain late traffic
        return Ok(());
    }
    let [st] = cx.left_aux();
    // `None`: stay in the current state; `Some`: move to a new one.
    let next: Option<(KindId, Value)> = if m.int(0)? != sid {
        reject(cx, k, sid);
        None
    } else if state == k.c_start {
        if msg == k.kick {
            transmit(cx, k, "server", "Open", &[sid]);
            cx.effect(k.timer, tup![sid, 1], &[]);
            Some((k.c_connecting, tup![sid, s.get(1)?.clone(), 1, 1]))
        } else {
            reject(cx, k, sid);
            None
        }
    } else if state == k.c_connecting {
        let (items, tries, tag) = (s.get(1)?.clone(), s.int(2)?, s.int(3)?);
        if msg == k.accept {
            Some(send_step(cx, k, sid, &items, 0, tag + 1))
        } else if msg == k.timeout {
            if m.int(1)? != tag {
                None // a stale timer
            } else if tries < max_tries {
                transmit(cx, k, "server", "Open", &[sid]);
                cx.effect(k.timer, tup![sid, tag + 1], &[]);
                Some((k.c_connecting, tup![sid, items, tries + 1, tag + 1]))
            } else {
                cx.agent(k.session_aborted, tup![sid, "connect"], st, &[]);
                cx.agent(k.c_done, tup![sid], rest, &[]);
                return Ok(());
            }
        } else {
            reject(cx, k, sid);
            None
        }
    } else if state == k.c_await_ack {
        let (items, seq, tries, tag) = (s.get(1)?.clone(), s.int(2)?, s.int(3)?, s.int(4)?);
        if msg == k.ack {
            let acked = m.int(1)?;
            if acked == seq {
                Some(send_step(cx, k, sid, &items, seq + 1, tag + 1))
            } else if acked < seq {
                None // duplicate acknowledgment
            } else {
                reject(cx, k, sid);
                None
            }
        } else if msg == k.timeout {
            if m.int(1)? != tag {
                None
            } else if tries < max_tries {
                let v = items.items().and_then(|xs| xs.get(seq as usize)).and_then(Value::as_int).unwrap_or(0);
                transmit(cx, k, "server", "Data", &[sid, seq, v]);
                cx.effect(k.timer, tup![sid, tag + 1], &[]);
                Some((k.c_await_ack, tup![sid, items, seq, tries + 1, tag + 1]))
            } else {
                cx.agent(k.session_aborted, tup![sid, "data"], st, &[]);
                cx.agent(k.c_done, tup![sid], rest, &[]);
                return Ok(());
            }
        } else if msg == k.accept {
            None // duplicate Accept from a retransmitted Open
        } else {
            reject(cx, k, sid);
            None
        }
    } else {
        // CClosing
        let (tries, tag) = (s.int(1)?, s.int(2)?);
        if msg == k.closed {
            cx.agent(k.session_ok, tup![sid], st, &[]);
            cx.agent(k.c_done, tup![sid], rest, &[]);
            return Ok(());
        } else if msg == k.timeout {
            if m.int(1)? != tag {
                None
            } else if tries < max_tries {
                transmit(cx, k, "server", "Close", &[sid]);
                cx.effect(k.timer, tup![sid, tag + 1], &[]);
                Some((k.c_closing, tup![sid, tries + 1, tag + 1]))
            } else {
                cx.agent(k.session_aborted, tup![sid, "close"], st, &[]);
                cx.agent(k.c_done, tup![sid], rest, &[]);
                return Ok(());
            }
        } else if msg == k.ack || msg == k.accept {
            None // late duplicates
        } else {
            reject(cx, k, sid);
            None
        }
    };
    let (kind, payload) = next.unwrap_or((state, s));
    cx.agent(kind, payload, rest, &[st]);
    Ok(())
}

fn server(cx: &mut RuleCx<'_>, k: &SessionKinds) -> Result<(), RuleError> {
    let (state, msg) = (cx.left_kind(), cx.right_kind());
    let (s, m) = (cx.left().clone(), cx.right().clone());
    let [rest] = cx.right_aux();
    let sid = s.int(0)?;
    if state == k.s_closed {
        let [] = cx.left_aux();
        if msg == k.close && m.int(0)? == sid {
            transmit(cx, k, "client", "Closed", &[sid]); // our Closed was lost
        }
        cx.agent(k.s_closed, s, rest, &[]);
        return Ok(());
    }
    let [app] = cx.left_aux();
    let next: Option<(KindId, Value)> = if m.int(0)? != sid {
        reject(cx, k, sid);
        None
    } else if state == k.s_listen {
        if msg == k.open {
            transmit(cx, k, "client", "Accept", &[sid]);
            Some((k.s_established, tup![sid, 0, Value::list([])]))
        } else {
            reject(cx, k, sid);
            None
        }
    } else {
        // SEstablished
        let (expected, delivered) = (s.int(1)?, s.list_at(2)?.to_vec());
        if msg == k.open {
            transmit(cx, k, "client", "Accept", &[sid]);
            None
        } else if msg == k.data {
            let seq = m.int(1)?;
            if seq == expected {
                transmit(cx, k, "client", "Ack", &[sid, seq]);
                let mut d = delivered;
                d.push(Value::Int(m.int(2)?));
                Some((k.s_established, tup![sid, expected + 1, Value::list(d)]))
            } else if seq < expected {
                transmit(cx, k, "client", "Ack", &[sid, seq]); // duplicate: re-acknowledge, deliver nothing
                None
            } else {
                reject(cx, k, sid);
                None
            }
        } else if msg == k.close {
            transmit(cx, k, "client", "Closed", &[sid]);
            cx.agent(k.delivered, tup![sid, Value::list(delivered)], app, &[]);
            cx.agent(k.s_closed, tup![sid], rest, &[]);
            return Ok(());
        } else {
            reject(cx, k, sid);
            None
        }
    };
    let (kind, payload) = next.unwrap_or((state, s));
    cx.agent(kind, payload, rest, &[app]);
    Ok(())
}

// ------------------------------------------------------------- host side

pub fn client_inbox(sid: i64) -> String {
    format!("c:{sid}")
}
pub fn server_inbox(sid: i64) -> String {
    format!("s:{sid}")
}

/// Creates both endpoints: the client inbox pre-loaded with `Kick`, the
/// server listening, and status/app result holes.
pub fn open_session(m: &mut Machine, k: &SessionKinds, sid: i64, items: &[i64]) -> Result<(), InjectError> {
    let mut b = NetBuilder::new(&m.program().sig);
    let (c0, c_tail, st, s0, app) = (b.var(), b.var(), b.var(), b.var(), b.var());
    b.agent(k.c_start, tup![sid, Value::list(items.iter().map(|&i| Value::Int(i)))], c0, &[st]);
    b.agent(k.kick, tup![sid], c0, &[c_tail]);
    b.agent(k.s_listen, tup![sid], s0, &[app]);
    let inj = Injection::new(b.finish()?)
        .new_hole(c_tail, Some(&client_inbox(sid)))
        .new_hole(st, Some(&format!("cstatus:{sid}")))
        .new_hole(s0, Some(&server_inbox(sid)))
        .new_hole(app, Some(&format!("app:{sid}")))
        .key(format!("open:{sid}"));
    m.inject(inj).map(|_| ())
}

/// Appends a message cell to an inbox.
pub fn deliver(m: &mut Machine, inbox: &str, kind: KindId, payload: Value) -> Result<(), InjectError> {
    let mut b = NetBuilder::new(&m.program().sig);
    let (p, rest) = (b.var(), b.var());
    b.agent(kind, payload, p, &[rest]);
    m.inject(Injection::new(b.finish()?).at_label(p, inbox).new_hole(rest, Some(inbox))).map(|_| ())
}

fn ack_effect(m: &mut Machine, kind: KindId, hole: crate::HoleId) -> Result<(), InjectError> {
    let mut b = NetBuilder::new(&m.program().sig);
    let p = b.var();
    b.agent(kind, (), p, &[]);
    m.inject(Injection::new(b.finish()?).at_hole(p, hole).key(format!("ack:{hole}"))).map(|_| ())
}

#[derive(Clone, Debug)]
pub struct NetConfig {
    pub seed: u64,
    /// Probability a transmitted message is lost.
    pub drop: f64,
    /// Probability a transmitted message is duplicated.
    pub dup: f64,
    /// Probability, per step, of injecting an invalid message.
    pub adversarial: f64,
    pub items: Vec<i64>,
    pub max_tries: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientOutcome {
    Ok,
    Aborted(String),
}

#[derive(Clone, Debug)]
pub struct SessionReport {
    pub client: ClientOutcome,
    /// What the server delivered to its application, in order.
    pub delivered: Vec<i64>,
    pub transmitted: BTreeMap<String, u64>,
    pub rejects: u64,
    pub timeouts_fired: u64,
    pub stats: Stats,
    pub explanation: String,
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Runs one session over a simulated network that drops, duplicates and
/// reorders messages, fires timers in random order, and optionally injects
/// invalid messages.
pub fn simulate(cfg: &NetConfig) -> Result<SessionReport, String> {
    let (prog, k) = program(&SessionConfig { max_tries: cfg.max_tries });
    let mut m = Machine::new(prog.clone());
    let sid = 1;
    open_session(&mut m, &k, sid, &cfg.items).map_err(err)?;
    let mut rng = Rng::new(cfg.seed);
    let mut in_flight: Vec<(String, KindId, Value)> = vec![];
    let mut timers: Vec<i64> = vec![];
    let mut transmitted: BTreeMap<String, u64> = BTreeMap::new();
    let (mut rejects, mut fired) = (0, 0);
    for _ in 0..100_000 {
        m.run(None);
        m.commit().map_err(err)?;
        if let Some(f) = m.faults().first() {
            return Err(format!("fault: {f:?}"));
        }
        // The transport takes custody of every committed effect.
        for e in m.outbox() {
            if e.kind == k.transmit {
                let (to, kind) = (e.payload.str(0).map_err(err)?, e.payload.str(1).map_err(err)?);
                *transmitted.entry(kind.to_string()).or_default() += 1;
                let inbox = if to == "server" { server_inbox(sid) } else { client_inbox(sid) };
                let mk = prog.sig.kind_id(kind).ok_or_else(|| format!("unknown message {kind}"))?;
                let fields = Value::tuple(e.payload.list_at(2).map_err(err)?.iter().cloned());
                if !rng.chance(cfg.drop) {
                    in_flight.push((inbox.clone(), mk, fields.clone()));
                    if rng.chance(cfg.dup) {
                        in_flight.push((inbox, mk, fields));
                    }
                }
                ack_effect(&mut m, k.tx_done, e.hole).map_err(err)?;
            } else if e.kind == k.timer {
                timers.push(e.payload.int(1).map_err(err)?);
                ack_effect(&mut m, k.tm_done, e.hole).map_err(err)?;
            } else if e.kind == k.reject {
                rejects += 1;
                ack_effect(&mut m, k.rj_done, e.hole).map_err(err)?;
            }
        }
        m.run(None);
        if rng.chance(cfg.adversarial) {
            let (inbox, kind, payload) = invalid_message(&k, sid, &mut rng);
            deliver(&mut m, &inbox, kind, payload).map_err(err)?;
            continue;
        }
        if in_flight.is_empty() && timers.is_empty() {
            break;
        }
        // Deliveries usually beat timers, but not always: timeouts race replies.
        if !timers.is_empty() && (in_flight.is_empty() || rng.chance(0.15)) {
            let tag = timers.swap_remove(rng.index(timers.len()));
            deliver(&mut m, &client_inbox(sid), k.timeout, tup![sid, tag]).map_err(err)?;
            fired += 1;
        } else {
            let (inbox, kind, payload) = in_flight.swap_remove(rng.index(in_flight.len()));
            deliver(&mut m, &inbox, kind, payload).map_err(err)?;
        }
    }
    m.run(None);
    if !m.is_quiescent() || !m.outbox().is_empty() {
        return Err(format!("did not settle:\n{}", m.render_explanation()));
    }
    let client = match m.hole(&format!("cstatus:{sid}")).and_then(|h| m.hole_peer(h)) {
        Some((_, 0, a)) if a.kind == k.session_ok => ClientOutcome::Ok,
        Some((_, 0, a)) if a.kind == k.session_aborted => ClientOutcome::Aborted(a.payload.str(1).map_err(err)?.to_string()),
        _ => return Err(format!("client never finished:\n{}", m.render_explanation())),
    };
    let delivered = match m.hole(&format!("app:{sid}")).and_then(|h| m.hole_peer(h)) {
        Some((_, 0, a)) if a.kind == k.delivered => a.payload.list_at(1).map_err(err)?.to_vec(),
        Some((_, 1, a)) if a.kind == k.s_established => a.payload.list_at(2).map_err(err)?.to_vec(),
        _ => vec![],
    };
    let delivered = delivered.iter().map(|v| v.as_int().unwrap_or(i64::MIN)).collect();
    Ok(SessionReport {
        client,
        delivered,
        transmitted,
        rejects,
        timeouts_fired: fired,
        stats: m.stats().clone(),
        explanation: m.render_explanation(),
    })
}

/// A message no correct peer would send in any state where it could be
/// accepted: wrong session, impossible sequence numbers, or wrong direction.
/// (Forged *valid* messages — e.g. a correct `Ack` — are indistinguishable
/// from real ones without authentication, which is out of scope here.)
fn invalid_message(k: &SessionKinds, sid: i64, rng: &mut Rng) -> (String, KindId, Value) {
    let far = 1000 + rng.below(100) as i64;
    let to_server = [
        (k.kick, tup![sid]),
        (k.accept, tup![sid]),
        (k.ack, tup![sid, 0]),
        (k.closed, tup![sid]),
        (k.timeout, tup![sid, 0]),
        (k.data, tup![sid, far, 99]),
        (k.open, tup![sid + 1]),
    ];
    let to_client = [
        (k.kick, tup![sid]),
        (k.open, tup![sid]),
        (k.data, tup![sid, 0, 0]),
        (k.close, tup![sid]),
        (k.ack, tup![sid, far]),
        (k.accept, tup![sid + 1]),
    ];
    if rng.chance(0.5) {
        let (kd, p) = to_server[rng.index(to_server.len())].clone();
        (server_inbox(sid), kd, p)
    } else {
        let (kd, p) = to_client[rng.index(to_client.len())].clone();
        (client_inbox(sid), kd, p)
    }
}
