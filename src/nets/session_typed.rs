//! Experiment B0: a session-typed channel *inside* the graph.
//!
//! The channel between client and server is a wire whose port type is the
//! protocol state. A message is an agent placed on the wire, carrying the
//! continuation channel (typed with the next state) on its auxiliary port;
//! the receiver's rule consumes it and places its reply on the continuation.
//!
//! ```text
//! S0: Open    client → server   (cont: S1)
//! S1: Accept  server → client   (cont: S2)
//! S2: Data    client → server   (cont: S3)   |  Close (cont: S4)
//! S3: Ack     server → client   (cont: S2)
//! S4: Closed  server → client
//! ```
//!
//! Every message has principal type `Sk+`, every receiver `Sk-`, so a rule
//! that sends a message the protocol does not allow in the current state is
//! rejected by the patch type check — sequencing is enforced statically. What
//! this encoding cannot express is a timeout: a receiver's single principal
//! port faces the channel, so nothing else (such as a timer) can reach it.

use crate::machine::{InjectError, Machine, Program};
use crate::net::{Injection, NetBuilder};
use crate::value::{Value, ValueType};
use crate::{tup, KindId, RuleTable, SigBuilder, TypeId};

#[derive(Clone, Copy, Debug)]
pub struct TypedKinds {
    pub s: [TypeId; 5],
    pub open: KindId,
    pub accept: KindId,
    pub data: KindId,
    pub close: KindId,
    pub ack: KindId,
    pub closed: KindId,
    pub s_await_open: KindId,
    pub s_await_choice: KindId,
    pub c_await_accept: KindId,
    pub c_await_ack: KindId,
    pub c_await_closed: KindId,
    pub session_ok: KindId,
    pub delivered: KindId,
}

/// Deliberately broken rule variants, to show the type check stopping them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bug {
    None,
    /// After `Accept`, the client sends `Ack` (an S3 message) on S2.
    ClientSendsAckAfterAccept,
    /// The server answers `Data` with `Closed` (an S4 message) on S3.
    ServerRepliesClosedToData,
}

pub fn program(bug: Bug) -> (Program, TypedKinds) {
    let mut b = SigBuilder::new("session-typed", 1);
    let s = [b.polarized("S0"), b.polarized("S1"), b.polarized("S2"), b.polarized("S3"), b.polarized("S4")];
    let (app, status) = (b.polarized("App"), b.polarized("Status"));
    let ints = ValueType::List(Box::new(ValueType::Int));
    let k = TypedKinds {
        s,
        open: b.kind("Open", s[0].pos()).aux("cont", s[1].pos()).done(),
        accept: b.kind("Accept", s[1].pos()).aux("cont", s[2].pos()).done(),
        data: b
            .kind("Data", s[2].pos())
            .aux("cont", s[3].pos())
            .payload(ValueType::Tuple(vec![ValueType::Int, ValueType::Int]))
            .done(),
        close: b.kind("Close", s[2].pos()).aux("cont", s[4].pos()).done(),
        ack: b.kind("Ack", s[3].pos()).aux("cont", s[2].pos()).payload(ValueType::Int).done(),
        closed: b.kind("Closed", s[4].pos()).done(),
        s_await_open: b.kind("SAwaitOpen", s[0].neg()).aux("app", app.pos()).payload(ints.clone()).done(),
        s_await_choice: b.kind("SAwaitChoice", s[2].neg()).aux("app", app.pos()).payload(ints.clone()).done(),
        c_await_accept: b.kind("CAwaitAccept", s[1].neg()).aux("status", status.pos()).payload(ints.clone()).done(),
        c_await_ack: b
            .kind("CAwaitAck", s[3].neg())
            .aux("status", status.pos())
            .payload(ValueType::Tuple(vec![ints.clone(), ValueType::Int]))
            .done(),
        c_await_closed: b.kind("CAwaitClosed", s[4].neg()).aux("status", status.pos()).done(),
        session_ok: b.kind("SessionOk", status.pos()).done(),
        delivered: b.kind("Delivered", app.pos()).payload(ints).done(),
    };
    let sig = b.build().expect("typed session signature");
    let mut t = RuleTable::new();

    /// The client's next move after the server acknowledged `seq - 1`:
    /// send item `seq`, or close when all items are through.
    fn client_send(cx: &mut crate::RuleCx<'_>, k: &TypedKinds, items: Value, seq: i64, cont: crate::Var, st: crate::Var) {
        let xs = items.items().unwrap_or(&[]).to_vec();
        let c2 = cx.var();
        if (seq as usize) < xs.len() {
            cx.agent(k.data, tup![seq, xs[seq as usize].clone()], cont, &[c2]);
            cx.agent(k.c_await_ack, tup![items, seq], c2, &[st]);
        } else {
            cx.agent(k.close, (), cont, &[c2]);
            cx.agent(k.c_await_closed, (), c2, &[st]);
        }
    }

    t.def("s_open", k.s_await_open, k.open, move |cx| {
        let [app] = cx.left_aux();
        let [cont] = cx.right_aux();
        let acc = cx.left().clone();
        let c2 = cx.var();
        cx.agent(k.accept, (), cont, &[c2]);
        cx.agent(k.s_await_choice, acc, c2, &[app]);
        Ok(())
    });
    t.def("c_accept", k.c_await_accept, k.accept, move |cx| {
        let [st] = cx.left_aux();
        let [cont] = cx.right_aux();
        let items = cx.left().clone();
        if bug == Bug::ClientSendsAckAfterAccept {
            let c2 = cx.var();
            cx.agent(k.ack, 0, cont, &[c2]);
            cx.agent(k.c_await_ack, tup![items, 0], c2, &[st]);
        } else {
            client_send(cx, &k, items, 0, cont, st);
        }
        Ok(())
    });
    t.def("s_data", k.s_await_choice, k.data, move |cx| {
        let [app] = cx.left_aux();
        let [cont] = cx.right_aux();
        let (acc, msg) = (cx.left().clone(), cx.right().clone());
        let (seq, v) = (msg.int(0)?, msg.get(1)?.clone());
        if bug == Bug::ServerRepliesClosedToData {
            cx.agent(k.closed, (), cont, &[]);
            cx.agent(k.delivered, acc, app, &[]);
            return Ok(());
        }
        let c2 = cx.var();
        let mut d = acc.items().unwrap_or(&[]).to_vec();
        d.push(v);
        cx.agent(k.ack, seq, cont, &[c2]);
        cx.agent(k.s_await_choice, Value::list(d), c2, &[app]);
        Ok(())
    });
    t.def("s_close", k.s_await_choice, k.close, move |cx| {
        let [app] = cx.left_aux();
        let [cont] = cx.right_aux();
        let acc = cx.left().clone();
        cx.agent(k.closed, (), cont, &[]);
        cx.agent(k.delivered, acc, app, &[]);
        Ok(())
    });
    t.def("c_ack", k.c_await_ack, k.ack, move |cx| {
        let [st] = cx.left_aux();
        let [cont] = cx.right_aux();
        let state = cx.left().clone();
        let (items, seq) = (state.get(0)?.clone(), state.int(1)?);
        client_send(cx, &k, items, seq + 1, cont, st);
        Ok(())
    });
    t.def("c_closed", k.c_await_closed, k.closed, move |cx| {
        let [st] = cx.left_aux();
        cx.agent(k.session_ok, (), st, &[]);
        Ok(())
    });
    (Program::new(sig, t).expect("typed session rules"), k)
}

/// Starts a session: the server waits on S0, the client's `Open` is already
/// on the channel, and the client waits for `Accept` behind it. Results
/// appear at holes `app` and `status`.
pub fn start(m: &mut Machine, k: &TypedKinds, items: &[i64]) -> Result<(), InjectError> {
    let mut b = NetBuilder::new(&m.program().sig);
    let (ch0, ch1, app, st) = (b.var(), b.var(), b.var(), b.var());
    b.agent(k.s_await_open, Value::list([]), ch0, &[app]);
    b.agent(k.open, (), ch0, &[ch1]);
    b.agent(k.c_await_accept, Value::list(items.iter().map(|&i| Value::Int(i))), ch1, &[st]);
    let inj = Injection::new(b.finish()?).new_hole(app, Some("app")).new_hole(st, Some("status"));
    m.inject(inj).map(|_| ())
}
