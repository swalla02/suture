//! The kernel program that every compiled Suture program runs on (spec
//! appendix B.3.1): one unit agent per root entity, messages queued at its
//! mailbox, outside requests as effects, and three generic rules.

use std::sync::Arc;

use suture::{DigestBuilder, KindId, PatchView, Policy, Program as KernelProgram, RuleTable, SigBuilder, ValueType};
use suture_lang::ir::Program;

use crate::interp;
use crate::state::{answer_from_value, send_from_value, send_value, stamped, unstamp, Ask, Decide, Msg, Unit};

/// The runtime's kernel kinds.
#[derive(Clone, Copy, Debug)]
pub struct Kinds {
    /// A unit, waiting at its mailbox.
    pub unit: KindId,
    /// A message, queued at a mailbox.
    pub msg: KindId,
    /// An outside request: an effect whose hole the adapter's answer fills.
    pub ask: KindId,
    /// An adapter's answer, injected at an `Ask`'s hole.
    pub answer: KindId,
    /// An internal message for a mailbox: an effect that the host routes.
    pub send: KindId,
    /// The host's routing agent, which joins a `Send` to a mailbox.
    pub deliver: KindId,
    /// A binding asking for a choice: an effect whose hole the host fills
    /// with the pick (appendix F.2.7).
    pub decide: KindId,
    /// The host's pick, injected at a `Decide`'s hole.
    pub pick: KindId,
}

/// The kernel program for a compiled program. Its fingerprint pins the
/// compiled program's hash (plan 1), so a journal replays only under the
/// same program.
/// The most a unit, or a message, holds: its encoded bytes (appendix BO.2.5).
pub const UNIT_BYTES: usize = 64 << 10;

pub fn program(prog: Arc<Program>) -> (KernelProgram, Kinds) {
    let mut b = SigBuilder::new(&format!("suture:{}", prog.module), prog.version);
    let (mailbox, reply, route) = (b.polarized("Mailbox"), b.polarized("Reply"), b.polarized("Route"));
    let choice = b.polarized("Choice");
    let k = Kinds {
        unit: b.kind("Unit", mailbox.neg()).payload(ValueType::Any).done(),
        msg: b.kind("Msg", mailbox.pos()).aux("rest", mailbox.neg()).payload(ValueType::Any).done(),
        ask: b.kind("Ask", reply.neg()).payload(ValueType::Any).effect().done(),
        answer: b.kind("Answer", reply.pos()).payload(ValueType::Any).done(),
        send: b.kind("Send", route.neg()).payload(ValueType::Any).effect().done(),
        deliver: b.kind("Deliver", route.pos()).aux("to", mailbox.pos()).aux("next", mailbox.neg()).payload(ValueType::Any).done(),
        decide: b.kind("Decide", choice.neg()).payload(ValueType::Any).effect().done(),
        pick: b.kind("Pick", choice.pos()).payload(ValueType::Any).done(),
    };
    let sig = b.build().expect("the runtime signature is valid");
    let mut t = RuleTable::new();
    let p = prog.clone();
    t.def("step", k.msg, k.unit, move |cx| {
        let [rest] = cx.left_aux();
        let (now, msg) = unstamp(cx.left()).map_err(|e| cx.reject(e))?;
        let unit = Unit::from_value(cx.right()).map_err(|e| cx.reject(e))?;
        let out = interp::step_at(&p, &unit, &msg, now);
        cx.agent(k.unit, out.unit.to_value(), rest, &[]);
        for a in &out.asks {
            cx.effect(k.ask, a.to_value(), &[]);
        }
        for (to, m) in &out.sends {
            cx.effect(k.send, send_value(to, m), &[]);
        }
        for d in &out.decides {
            cx.effect(k.decide, d.to_value(), &[]);
        }
        Ok(())
    })
    .emits(&[k.ask, k.send, k.decide]);
    t.def("pick", k.pick, k.decide, move |cx| {
        let d = Decide::from_value(cx.right()).map_err(|e| cx.reject(e))?;
        let to = cx.left().as_str().ok_or_else(|| cx.reject("a pick names a unit"))?.to_string();
        let take = Msg::Take { from: d.label.clone(), key: d.key(), taken: d.taken.clone() };
        cx.effect(k.send, send_value(&to, &take), &[]);
        Ok(())
    })
    .emits(&[k.send]);
    t.def("answer", k.answer, k.ask, move |cx| {
        let ask = Ask::from_value(cx.right()).map_err(|e| cx.reject(e))?;
        let (outcome, data) = answer_from_value(cx.left()).map_err(|e| cx.reject(e))?;
        let msg = Msg::Answer { key: ask.key.clone(), outcome, data };
        cx.effect(k.send, send_value(&ask.label, &msg), &[]);
        Ok(())
    })
    .emits(&[k.send]);
    t.def("deliver", k.deliver, k.send, move |cx| {
        let [to, next] = cx.left_aux();
        let now = cx.left().as_int().ok_or_else(|| cx.reject("a delivery carries the clock"))?;
        let (_, msg) = send_from_value(cx.right()).map_err(|e| cx.reject(e))?;
        cx.agent(k.msg, stamped(now, &msg), to, &[next]);
        Ok(())
    });
    let hash = {
        let mut d = DigestBuilder::new("suture-program");
        d.str(&serde_json::to_string(&*prog).expect("compiled programs serialize"));
        d.finish()
    };
    let mut kernel = KernelProgram::new(sig, t)
        .expect("the runtime rules are valid")
        .with_policy(RulesHold { prog, unit: k.unit })
        .with_fingerprint(hash);
    kernel.limits.max_payload_bytes = UNIT_BYTES;
    (kernel, k)
}

/// The safety net of spec D8: whatever the interpreter computes, a unit
/// state that breaks a rule is never installed.
struct RulesHold {
    prog: Arc<Program>,
    unit: KindId,
}

impl Policy for RulesHold {
    fn name(&self) -> &str {
        "rules-hold"
    }

    fn check(&self, view: &PatchView<'_>) -> Result<(), String> {
        for payload in view.created_of(self.unit) {
            let unit = Unit::from_value(payload)?;
            if let Some(r) = interp::violated(&self.prog, &unit) {
                return Err(format!("rule {} would break", r.name));
            }
        }
        Ok(())
    }
}
