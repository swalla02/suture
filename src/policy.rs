//! Hard policies: independent checks applied to every patch before it is
//! installed.
//!
//! A policy sees the consumed pair and the agents the patch would create. It
//! can only accept or reject; a rejection faults the pair. Policies are how
//! residency, tenancy and similar invariants are *enforced* rather than
//! scored, independently of whether the rules already respect them.

use crate::net::NetAgent;
use crate::sig::{KindId, Signature};
use crate::value::Value;

pub trait Policy: Send + Sync {
    fn name(&self) -> &str;
    fn check(&self, view: &PatchView<'_>) -> Result<(), String>;
}

pub struct PatchView<'a> {
    pub sig: &'a Signature,
    pub rule: &'a str,
    pub left: (KindId, &'a Value),
    pub right: (KindId, &'a Value),
    pub created: &'a [NetAgent],
}

impl PatchView<'_> {
    /// Payloads of created agents of kind `k`.
    pub fn created_of(&self, k: KindId) -> impl Iterator<Item = &Value> {
        self.created.iter().filter(move |a| a.kind == k).map(|a| &a.payload)
    }

    /// Payloads of consumed agents of kind `k` (zero, one or two).
    pub fn consumed_of(&self, k: KindId) -> impl Iterator<Item = &Value> {
        [self.left, self.right].into_iter().filter(move |(kind, _)| *kind == k).map(|(_, v)| v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::fixtures::fx;
    use crate::net::{NetAgent, Slot, Var};
    use crate::{tup, Value};

    #[test]
    fn view_helpers_select_by_kind() {
        let f = fx();
        let created = vec![
            NetAgent { kind: f.lease, payload: tup![7], ports: vec![Slot::Var(Var(0))] },
            NetAgent { kind: f.era, payload: Value::Unit, ports: vec![Slot::Var(Var(0))] },
        ];
        let one = tup![1];
        let v = PatchView { sig: &f.sig, rule: "r", left: (f.lease, &one), right: (f.era, &Value::Unit), created: &created };
        assert_eq!(v.created_of(f.lease).cloned().collect::<Vec<_>>(), vec![tup![7]]);
        assert_eq!(v.consumed_of(f.lease).cloned().collect::<Vec<_>>(), vec![tup![1]]);
        assert_eq!(v.consumed_of(f.z).count(), 0);
    }
}
