//! Static legality of patches and injected nets.
//!
//! These checks depend only on the net and the declared interface, never on
//! the surrounding graph, so they establish rewrite legality independently of
//! context: arity, payload schemas, effect authority, linearity, port-type
//! duality (including preservation of the replaced pair's interface types),
//! conservation of linear capabilities, and size bounds.

use crate::net::{resolve, End, ExtKind, NetData, Resolved, Slot, Var};
use crate::sig::{KindId, Polarity, PortType, Signature};
use crate::value::Value;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    #[error("{kind} takes {expected} auxiliary ports, got {got}")]
    Arity { kind: String, expected: usize, got: usize },
    #[error("unknown wire variable {0}")]
    UnknownVar(u32),
    #[error("kind {0} is not in this program's signature")]
    UnknownKind(u16),
    #[error("wire variable {var} used {uses} times (must be exactly 2)")]
    VarUses { var: u32, uses: usize },
    #[error("closed wire loop through variable {0}")]
    ClosedLoop(u32),
    #[error("ill-typed wire {a} ~ {b}")]
    WireType { a: String, b: String },
    #[error("ill-typed interface: port {port} must be replaced by a {expected} endpoint, got {got}")]
    InterfaceType { port: usize, expected: String, got: String },
    #[error("{0} is not an effect kind but its principal faces the host")]
    EmitOnNonEffect(String),
    #[error("effect kind {0} must be created with its principal facing the host")]
    EffectNeedsEmit(String),
    #[error("rule may not emit effect {0}")]
    EffectNotAuthorized(String),
    #[error("injected nets may not contain effect agents ({0})")]
    EffectInInject(String),
    #[error("payload of {kind} violates its schema: {value}")]
    Payload { kind: String, value: String },
    #[error("conserved capabilities not preserved: consumed {consumed}, created {created}")]
    Conservation { created: String, consumed: String },
    #[error("creating conserved {0} requires mint permission")]
    MintRequired(String),
    #[error("patch has {agents} agents, limit {limit}")]
    TooLarge { agents: usize, limit: usize },
    #[error("{0}")]
    Value(String),
}

pub(crate) struct PatchSpec<'a> {
    /// Types of the replaced pair's auxiliary ports, left agent first.
    pub iface: &'a [PortType],
    pub consumed: [(KindId, &'a Value); 2],
    pub may_emit: &'a [KindId],
    pub max_agents: usize,
}

/// The type a connecting endpoint must have; for self-dual types the
/// polarity is irrelevant to `compatible`, so flipping is harmless.
pub(crate) fn dual(p: PortType) -> PortType {
    PortType { ty: p.ty, pol: if p.pol == Polarity::Pos { Polarity::Neg } else { Polarity::Pos } }
}

fn check_agents(sig: &Signature, net: &NetData, max_agents: usize, may_emit: Option<&[KindId]>) -> Result<(), NetError> {
    if net.agents.len() > max_agents {
        return Err(NetError::TooLarge { agents: net.agents.len(), limit: max_agents });
    }
    if let Some(a) = net.agents.iter().find(|a| !sig.has_kind(a.kind)) {
        return Err(NetError::UnknownKind(a.kind.0));
    }
    for a in &net.agents {
        let d = sig.kind(a.kind);
        if a.ports.len() != 1 + d.aux.len() {
            return Err(NetError::Arity { kind: d.name.clone(), expected: d.aux.len(), got: a.ports.len().saturating_sub(1) });
        }
        if !d.payload.admits(&a.payload) {
            return Err(NetError::Payload { kind: d.name.clone(), value: a.payload.to_string() });
        }
        if a.ports[1..].contains(&Slot::Emit) {
            return Err(NetError::EmitOnNonEffect(d.name.clone()));
        }
        match (d.effect, a.ports[0], may_emit) {
            (true, _, None) => return Err(NetError::EffectInInject(d.name.clone())),
            (true, Slot::Var(_), Some(_)) => return Err(NetError::EffectNeedsEmit(d.name.clone())),
            (true, Slot::Emit, Some(allowed)) if !allowed.contains(&a.kind) => {
                return Err(NetError::EffectNotAuthorized(d.name.clone()));
            }
            (false, Slot::Emit, _) => return Err(NetError::EmitOnNonEffect(d.name.clone())),
            _ => {}
        }
    }
    Ok(())
}

/// Sorted multiset of `(class, key)` for the conserved agents among `items`.
pub(crate) fn conserved_multiset<'a>(
    sig: &Signature,
    items: impl Iterator<Item = (KindId, &'a Value)>,
) -> Vec<(String, Value)> {
    let mut v: Vec<_> = items.filter_map(|(k, p)| sig.conserved_key(k, p)).collect();
    v.sort();
    v
}

fn fmt_multiset(m: &[(String, Value)]) -> String {
    let items: Vec<String> = m.iter().map(|(c, k)| format!("{c}:{k}")).collect();
    format!("{{{}}}", items.join(", "))
}

/// Full static legality of a rule's patch. Returns the context-free wiring.
pub(crate) fn check_patch(sig: &Signature, net: &NetData, spec: &PatchSpec) -> Result<Resolved, NetError> {
    check_agents(sig, net, spec.max_agents, Some(spec.may_emit))?;
    let k = spec.iface.len();
    if (net.vars as usize) < k {
        return Err(NetError::UnknownVar(net.vars));
    }
    let ext_vars: Vec<Var> = (0..k as u32).map(Var).collect();
    let resolved = resolve(net, &ext_vars, &vec![ExtKind::Terminal; k])?;
    let port = |e: End| match e {
        End::Agent(i, p) => {
            let kind = net.agents[i].kind;
            let t = sig.port_type(kind, p);
            (t, format!("{}.{}:{}", sig.kind(kind).name, p, sig.fmt_port_type(t)))
        }
        End::Ext(j) => {
            let t = dual(spec.iface[j]);
            (t, format!("outside of interface port {j}:{}", sig.fmt_port_type(t)))
        }
    };
    for &(a, b) in &resolved.conns {
        let ((ta, na), (tb, nb)) = (port(a), port(b));
        if sig.compatible(ta, tb) {
            continue;
        }
        return Err(match (a, b) {
            (End::Ext(j), End::Agent(..)) | (End::Agent(..), End::Ext(j)) => {
                let got = if let End::Agent(..) = a { ta } else { tb };
                NetError::InterfaceType {
                    port: j,
                    expected: sig.fmt_port_type(spec.iface[j]),
                    got: sig.fmt_port_type(got),
                }
            }
            _ => NetError::WireType { a: na, b: nb },
        });
    }
    let consumed = conserved_multiset(sig, spec.consumed.iter().map(|(k, v)| (*k, *v)));
    let created = conserved_multiset(sig, net.agents.iter().map(|a| (a.kind, &a.payload)));
    if consumed != created {
        return Err(NetError::Conservation { created: fmt_multiset(&created), consumed: fmt_multiset(&consumed) });
    }
    Ok(resolved)
}

/// Context-free legality of an injected net (wiring is checked at inject time).
pub(crate) fn check_inject_static(sig: &Signature, net: &NetData, mint: bool, max_agents: usize) -> Result<(), NetError> {
    check_agents(sig, net, max_agents, None)?;
    if !mint {
        if let Some(a) = net.agents.iter().find(|a| sig.kind(a.kind).conserved.is_some()) {
            return Err(NetError::MintRequired(sig.kind(a.kind).name.clone()));
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod fixtures {
    use crate::{KindId, SigBuilder, Signature, TypeId, ValueType};

    pub(crate) struct Fx {
        pub sig: Signature,
        pub nat: TypeId,
        pub z: KindId,
        pub s: KindId,
        pub add: KindId,
        pub era: KindId,
        pub lease: KindId,
        pub ask: KindId,
    }

    /// Peano kinds plus a conserved `Lease(Int)` and an effect `Ask(out)`.
    pub(crate) fn fx() -> Fx {
        let mut b = SigBuilder::new("fx", 1);
        let nat = b.polarized("Nat");
        let z = b.kind("Z", nat.pos()).done();
        let s = b.kind("S", nat.pos()).aux("pred", nat.neg()).done();
        let add = b.kind("Add", nat.neg()).aux("y", nat.neg()).aux("r", nat.pos()).done();
        let era = b.kind("Era", nat.neg()).done();
        let lease = b
            .kind("Lease", nat.pos())
            .payload(ValueType::Tuple(vec![ValueType::Int]))
            .conserved("lease", 0)
            .done();
        let ask = b.kind("Ask", nat.neg()).aux("out", nat.pos()).effect().done();
        Fx { sig: b.build().unwrap(), nat, z, s, add, era, lease, ask }
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::fx;
    use super::*;
    use crate::net::{End, NetBuilder, Var};
    use crate::{tup, Value};

    #[test]
    fn linearity() {
        let f = fx();
        let iface = [f.nat.neg(), f.nat.neg(), f.nat.pos()]; // S(x) >< Add(y, r)
        let spec = PatchSpec {
            iface: &iface,
            consumed: [(f.s, &Value::Unit), (f.add, &Value::Unit)],
            may_emit: &[],
            max_agents: 16,
        };
        let mut b = NetBuilder::with_vars(&f.sig, 3);
        b.link(Var(1), Var(2)); // x never used by the RHS
        assert_eq!(check_patch(&f.sig, &b.finish().unwrap(), &spec).unwrap_err(), NetError::VarUses { var: 0, uses: 1 });
        let mut b = NetBuilder::with_vars(&f.sig, 3);
        b.link(Var(1), Var(2));
        b.agent(f.era, (), Var(0), &[]);
        b.agent(f.z, (), Var(1), &[]);
        assert_eq!(check_patch(&f.sig, &b.finish().unwrap(), &spec).unwrap_err(), NetError::VarUses { var: 1, uses: 3 });
    }

    #[test]
    fn static_wire_types() {
        let f = fx();
        let spec = PatchSpec { iface: &[], consumed: [(f.z, &Value::Unit), (f.era, &Value::Unit)], may_emit: &[], max_agents: 16 };
        let mut b = NetBuilder::new(&f.sig);
        let v = b.var();
        b.agent(f.z, (), v, &[]);
        b.agent(f.z, (), v, &[]);
        assert!(matches!(check_patch(&f.sig, &b.finish().unwrap(), &spec), Err(NetError::WireType { .. })));
    }

    #[test]
    fn interface_types_preserved() {
        let f = fx();
        let iface = [f.nat.neg(), f.nat.neg(), f.nat.pos()];
        let spec = PatchSpec { iface: &iface, consumed: [(f.s, &Value::Unit), (f.add, &Value::Unit)], may_emit: &[], max_agents: 16 };
        let mut b = NetBuilder::with_vars(&f.sig, 3);
        b.agent(f.z, (), Var(1), &[]); // a producer where Add.y (a consumer) was
        b.link(Var(0), Var(2));
        let err = check_patch(&f.sig, &b.finish().unwrap(), &spec).unwrap_err();
        assert_eq!(err, NetError::InterfaceType { port: 1, expected: "Nat-".into(), got: "Nat+".into() });
    }

    #[test]
    fn add_s_patch_is_valid() {
        let f = fx();
        let iface = [f.nat.neg(), f.nat.neg(), f.nat.pos()];
        let spec = PatchSpec { iface: &iface, consumed: [(f.s, &Value::Unit), (f.add, &Value::Unit)], may_emit: &[], max_agents: 16 };
        let mut b = NetBuilder::with_vars(&f.sig, 3);
        let r1 = b.var();
        b.agent(f.add, (), Var(0), &[Var(1), r1]);
        b.agent(f.s, (), Var(2), &[r1]);
        let r = check_patch(&f.sig, &b.finish().unwrap(), &spec).unwrap();
        assert_eq!(r.conns.len(), 4);
        assert_eq!(r.vicious, 0);
    }

    #[test]
    fn pass_through_link() {
        let f = fx();
        let iface = [f.nat.neg(), f.nat.pos()]; // Z >< Add(y, r)
        let spec = PatchSpec { iface: &iface, consumed: [(f.z, &Value::Unit), (f.add, &Value::Unit)], may_emit: &[], max_agents: 16 };
        let mut b = NetBuilder::with_vars(&f.sig, 2);
        b.link(Var(0), Var(1));
        let r = check_patch(&f.sig, &b.finish().unwrap(), &spec).unwrap();
        assert_eq!(r.conns, vec![(End::Ext(0), End::Ext(1))]);
    }

    #[test]
    fn effects_need_emit_and_authority() {
        let f = fx();
        let spec = |may: &'static [crate::KindId]| PatchSpec {
            iface: &[],
            consumed: [(f.z, &Value::Unit), (f.era, &Value::Unit)],
            may_emit: may,
            max_agents: 16,
        };
        let mut b = NetBuilder::new(&f.sig);
        let (p, o) = (b.var(), b.var());
        b.agent(f.ask, (), p, &[o]);
        b.agent(f.z, (), p, &[]);
        b.agent(f.era, (), o, &[]);
        assert_eq!(check_patch(&f.sig, &b.finish().unwrap(), &spec(&[])).unwrap_err(), NetError::EffectNeedsEmit("Ask".into()));

        let mut b = NetBuilder::new(&f.sig);
        let o = b.var();
        b.effect(f.s, (), &[o]);
        b.agent(f.z, (), o, &[]);
        assert_eq!(check_patch(&f.sig, &b.finish().unwrap(), &spec(&[])).unwrap_err(), NetError::EmitOnNonEffect("S".into()));

        let mut b = NetBuilder::new(&f.sig);
        let o = b.var();
        b.effect(f.ask, (), &[o]);
        b.agent(f.era, (), o, &[]);
        let net = b.finish().unwrap();
        assert_eq!(check_patch(&f.sig, &net, &spec(&[])).unwrap_err(), NetError::EffectNotAuthorized("Ask".into()));
        let ask = [f.ask];
        let ok = PatchSpec { iface: &[], consumed: [(f.z, &Value::Unit), (f.era, &Value::Unit)], may_emit: &ask, max_agents: 16 };
        assert_eq!(check_patch(&f.sig, &net, &ok).unwrap().conns.len(), 1);
    }

    #[test]
    fn conservation() {
        let f = fx();
        let one = tup![1];
        let spec = PatchSpec { iface: &[], consumed: [(f.lease, &one), (f.era, &Value::Unit)], may_emit: &[], max_agents: 16 };
        let net = |keys: &[i64]| {
            let mut b = NetBuilder::new(&f.sig);
            for &k in keys {
                let v = b.var();
                b.agent(f.lease, tup![k], v, &[]);
                b.agent(f.era, (), v, &[]);
            }
            b.finish().unwrap()
        };
        assert!(matches!(check_patch(&f.sig, &net(&[1, 1]), &spec), Err(NetError::Conservation { .. })));
        assert!(matches!(check_patch(&f.sig, &net(&[2]), &spec), Err(NetError::Conservation { .. })));
        assert!(matches!(check_patch(&f.sig, &net(&[]), &spec), Err(NetError::Conservation { .. })));
        assert!(check_patch(&f.sig, &net(&[1]), &spec).is_ok());
    }

    #[test]
    fn payload_schema_and_size() {
        let f = fx();
        let spec = PatchSpec { iface: &[], consumed: [(f.z, &Value::Unit), (f.era, &Value::Unit)], may_emit: &[], max_agents: 1 };
        let mut b = NetBuilder::new(&f.sig);
        let v = b.var();
        b.agent(f.z, (), v, &[]);
        b.agent(f.era, (), v, &[]);
        assert_eq!(check_patch(&f.sig, &b.finish().unwrap(), &spec).unwrap_err(), NetError::TooLarge { agents: 2, limit: 1 });
        let spec = PatchSpec { max_agents: 8, ..spec };
        let mut b = NetBuilder::new(&f.sig);
        let v = b.var();
        b.agent(f.z, 5, v, &[]);
        b.agent(f.era, (), v, &[]);
        assert!(matches!(check_patch(&f.sig, &b.finish().unwrap(), &spec), Err(NetError::Payload { .. })));
    }

    #[test]
    fn inject_static_rules() {
        let f = fx();
        let mut b = NetBuilder::new(&f.sig);
        let v = b.var();
        b.agent(f.lease, tup![1], v, &[]);
        let net = b.finish().unwrap();
        assert_eq!(check_inject_static(&f.sig, &net, false, 16).unwrap_err(), NetError::MintRequired("Lease".into()));
        assert!(check_inject_static(&f.sig, &net, true, 16).is_ok());
        let mut b = NetBuilder::new(&f.sig);
        let (p, o) = (b.var(), b.var());
        b.agent(f.ask, (), p, &[o]);
        let net = b.finish().unwrap();
        assert_eq!(check_inject_static(&f.sig, &net, true, 16).unwrap_err(), NetError::EffectInInject("Ask".into()));
    }
}
