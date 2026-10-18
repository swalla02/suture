//! The port graph: agents, endpoints, holes and labels.
//!
//! Every port of every agent is connected to exactly one *endpoint*: another
//! agent's port or a *hole*. Holes are the graph's boundary — the only places
//! where the host can attach new structure (`INJECT`) or where effect agents
//! wait for the outside world. Holes may carry a unique *label*, a stable
//! address for a stream tail such as a mailbox.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::id::{AgentId, Digest, DigestBuilder, HoleId, IdBuildHasher};
use crate::sig::{KindId, Signature};
use crate::value::Value;

/// One end of a wire: port `n` of an agent (0 is the principal port) or a hole.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Endpoint {
    Port(AgentId, u8),
    Hole(HoleId),
}

impl Endpoint {
    fn encode(&self, d: &mut DigestBuilder) {
        match self {
            Endpoint::Port(a, p) => {
                d.u8(0);
                d.u128(a.0);
                d.u8(*p);
            }
            Endpoint::Hole(h) => {
                d.u8(1);
                d.u128(h.0);
            }
        }
    }
}

/// How an agent came to exist: injected by the host, or created by the
/// reduction of a pair under a rule (index into the program's rule table).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cause {
    Inject { seq: u64 },
    Reduce { left: AgentId, right: AgentId, rule: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
    pub kind: KindId,
    pub payload: Value,
    /// `ports[0]` is the peer of the principal port, `ports[i]` of aux port `i`.
    pub ports: Vec<Endpoint>,
    pub cause: Cause,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoleOrigin {
    Inject { seq: u64 },
    Effect { agent: AgentId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hole {
    pub peer: Endpoint,
    pub label: Option<String>,
    pub origin: HoleOrigin,
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("dangling endpoint: {0}")]
    Dangling(String),
    #[error("asymmetric wire: {0}")]
    Asymmetric(String),
    #[error("port wired to itself: {0}")]
    SelfWire(String),
    #[error("arity mismatch: {0}")]
    Arity(String),
    #[error("payload violates schema: {0}")]
    Payload(String),
    #[error("ill-typed wire: {0}")]
    WireType(String),
    #[error("label table inconsistent: {0}")]
    Label(String),
    #[error("ready queue inconsistent: {0}")]
    Ready(String),
    #[error("effect index inconsistent: {0}")]
    Outbox(String),
}

#[derive(Clone, Debug, Default)]
pub struct Graph {
    agents: HashMap<AgentId, Agent, IdBuildHasher>,
    holes: HashMap<HoleId, Hole, IdBuildHasher>,
    labels: HashMap<String, HoleId>,
}

impl Graph {
    pub fn agent(&self, id: AgentId) -> Option<&Agent> {
        self.agents.get(&id)
    }

    pub fn hole(&self, id: HoleId) -> Option<&Hole> {
        self.holes.get(&id)
    }

    pub fn hole_by_label(&self, label: &str) -> Option<HoleId> {
        self.labels.get(label).copied()
    }

    /// Every label, in the order its hole was created.
    pub fn labels(&self) -> Vec<(HoleId, &str)> {
        let mut v: Vec<_> = self.labels.iter().map(|(l, h)| (*h, l.as_str())).collect();
        v.sort();
        v
    }

    /// The endpoint `e` is wired to, if `e` exists.
    pub fn peer(&self, e: Endpoint) -> Option<Endpoint> {
        match e {
            Endpoint::Port(a, p) => self.agents.get(&a)?.ports.get(p as usize).copied(),
            Endpoint::Hole(h) => Some(self.holes.get(&h)?.peer),
        }
    }

    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    pub fn hole_count(&self) -> usize {
        self.holes.len()
    }

    pub fn agents_sorted(&self) -> Vec<(AgentId, &Agent)> {
        let mut v: Vec<_> = self.agents.iter().map(|(k, a)| (*k, a)).collect();
        v.sort_by_key(|(k, _)| *k);
        v
    }

    pub fn holes_sorted(&self) -> Vec<(HoleId, &Hole)> {
        let mut v: Vec<_> = self.holes.iter().map(|(k, h)| (*k, h)).collect();
        v.sort_by_key(|(k, _)| *k);
        v
    }

    /// The agent whose principal port faces `id`'s principal port, if any.
    pub fn active_partner(&self, id: AgentId) -> Option<AgentId> {
        match self.agents.get(&id)?.ports[0] {
            Endpoint::Port(b, 0) if b != id => Some(b),
            _ => None,
        }
    }

    /// All active pairs, each once as `(min, max)`, sorted.
    pub fn active_pairs(&self) -> Vec<(AgentId, AgentId)> {
        let mut v: Vec<_> = self
            .agents
            .keys()
            .filter_map(|&a| self.active_partner(a).filter(|&b| a < b).map(|b| (a, b)))
            .collect();
        v.sort();
        v
    }

    /// Verifies the well-formedness conditions of the IR (see `docs/ir-spec.md`).
    pub fn check(&self, sig: &Signature) -> Result<(), GraphError> {
        for (&id, a) in &self.agents {
            let decl = sig.kind(a.kind);
            if a.ports.len() != 1 + decl.aux.len() {
                return Err(GraphError::Arity(format!("{} {id}", decl.name)));
            }
            if !decl.payload.admits(&a.payload) {
                return Err(GraphError::Payload(format!("{} {id} = {}", decl.name, a.payload)));
            }
            for (p, &peer) in a.ports.iter().enumerate() {
                let here = Endpoint::Port(id, p as u8);
                if peer == here {
                    return Err(GraphError::SelfWire(format!("{here:?}")));
                }
                match self.peer(peer) {
                    None => return Err(GraphError::Dangling(format!("{here:?} -> {peer:?}"))),
                    Some(back) if back != here => {
                        return Err(GraphError::Asymmetric(format!("{here:?} -> {peer:?} -> {back:?}")));
                    }
                    _ => {}
                }
                if let Endpoint::Port(b, q) = peer {
                    let (ta, tb) = (sig.port_type(a.kind, p), sig.port_type(self.agents[&b].kind, q as usize));
                    if !sig.compatible(ta, tb) {
                        return Err(GraphError::WireType(format!(
                            "{here:?}:{} ~ {peer:?}:{}",
                            sig.fmt_port_type(ta),
                            sig.fmt_port_type(tb)
                        )));
                    }
                }
            }
        }
        for (&h, hole) in &self.holes {
            let here = Endpoint::Hole(h);
            if hole.peer == here {
                return Err(GraphError::SelfWire(format!("{here:?}")));
            }
            match self.peer(hole.peer) {
                None => return Err(GraphError::Dangling(format!("{here:?} -> {:?}", hole.peer))),
                Some(back) if back != here => {
                    return Err(GraphError::Asymmetric(format!("{here:?} -> {:?} -> {back:?}", hole.peer)));
                }
                _ => {}
            }
            if let Some(l) = &hole.label
                && self.labels.get(l) != Some(&h) {
                return Err(GraphError::Label(format!("hole {h} label {l:?} not indexed")));
            }
        }
        for (l, h) in &self.labels {
            if self.holes.get(h).and_then(|x| x.label.as_ref()) != Some(l) {
                return Err(GraphError::Label(format!("label {l:?} points at {h}")));
            }
        }
        Ok(())
    }

    /// A digest of the complete graph state; equal graphs have equal digests
    /// regardless of insertion history.
    pub fn digest(&self) -> Digest {
        let mut d = DigestBuilder::new("graph");
        let agents = self.agents_sorted();
        d.u64(agents.len() as u64);
        let mut buf = Vec::new();
        for (id, a) in agents {
            d.u128(id.0);
            d.u32(a.kind.0 as u32);
            buf.clear();
            a.payload.encode(&mut buf);
            d.bytes(&buf);
            d.u64(a.ports.len() as u64);
            for p in &a.ports {
                p.encode(&mut d);
            }
            match a.cause {
                Cause::Inject { seq } => {
                    d.u8(0);
                    d.u64(seq);
                }
                Cause::Reduce { left, right, rule } => {
                    d.u8(1);
                    d.u128(left.0);
                    d.u128(right.0);
                    d.u32(rule);
                }
            }
        }
        let holes = self.holes_sorted();
        d.u64(holes.len() as u64);
        for (id, h) in holes {
            d.u128(id.0);
            h.peer.encode(&mut d);
            match &h.label {
                None => d.u8(0),
                Some(l) => {
                    d.u8(1);
                    d.str(l);
                }
            }
            match h.origin {
                HoleOrigin::Inject { seq } => {
                    d.u8(0);
                    d.u64(seq);
                }
                HoleOrigin::Effect { agent } => {
                    d.u8(1);
                    d.u128(agent.0);
                }
            }
        }
        d.finish()
    }

    pub(crate) fn insert_agent(&mut self, id: AgentId, a: Agent) {
        let old = self.agents.insert(id, a);
        debug_assert!(old.is_none(), "agent id collision {id}");
    }

    pub(crate) fn remove_agent(&mut self, id: AgentId) -> Option<Agent> {
        self.agents.remove(&id)
    }

    pub(crate) fn insert_hole(&mut self, id: HoleId, h: Hole) {
        if let Some(l) = &h.label {
            let old = self.labels.insert(l.clone(), id);
            debug_assert!(old.is_none(), "label collision {l}");
        }
        let old = self.holes.insert(id, h);
        debug_assert!(old.is_none(), "hole id collision {id}");
    }

    pub(crate) fn remove_hole(&mut self, id: HoleId) -> Option<Hole> {
        let h = self.holes.remove(&id)?;
        if let Some(l) = &h.label {
            self.labels.remove(l);
        }
        Some(h)
    }

    /// Points `at` to `peer` (one direction only).
    pub(crate) fn set_end(&mut self, at: Endpoint, peer: Endpoint) {
        match at {
            Endpoint::Port(a, p) => {
                self.agents.get_mut(&a).expect("set_end on missing agent").ports[p as usize] = peer;
            }
            Endpoint::Hole(h) => {
                self.holes.get_mut(&h).expect("set_end on missing hole").peer = peer;
            }
        }
    }

    /// Wires `a` and `b` to each other.
    pub(crate) fn link(&mut self, a: Endpoint, b: Endpoint) {
        self.set_end(a, b);
        self.set_end(b, a);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentId, HoleId, KindId, SigBuilder, Signature, Value};

    fn peano() -> (Signature, KindId, KindId, KindId) {
        let mut b = SigBuilder::new("t", 1);
        let nat = b.polarized("Nat");
        let z = b.kind("Z", nat.pos()).done();
        let s = b.kind("S", nat.pos()).aux("pred", nat.neg()).done();
        let add = b.kind("Add", nat.neg()).aux("y", nat.neg()).aux("r", nat.pos()).done();
        (b.build().unwrap(), z, s, add)
    }

    fn agent(kind: KindId, ports: usize) -> Agent {
        Agent {
            kind,
            payload: Value::Unit,
            ports: vec![Endpoint::Hole(HoleId(0)); ports],
            cause: Cause::Inject { seq: 0 },
        }
    }

    fn hole(peer: Endpoint, label: Option<&str>) -> Hole {
        Hole { peer, label: label.map(str::to_string), origin: HoleOrigin::Inject { seq: 0 } }
    }

    #[test]
    fn wiring_and_active_pairs() {
        let (sig, z, s, add) = peano();
        let mut g = Graph::default();
        let (zi, si, ai) = (AgentId::from_inject(0, 0), AgentId::from_inject(0, 1), AgentId::from_inject(0, 2));
        let h = HoleId::from_inject(0, 0);
        g.insert_agent(zi, agent(z, 1));
        g.insert_agent(si, agent(s, 2));
        g.insert_hole(h, hole(Endpoint::Port(si, 0), Some("out")));
        g.link(Endpoint::Port(zi, 0), Endpoint::Port(si, 1));
        g.link(Endpoint::Port(si, 0), Endpoint::Hole(h));
        g.check(&sig).unwrap();
        assert!(g.active_pairs().is_empty());
        assert_eq!(g.peer(Endpoint::Port(zi, 0)), Some(Endpoint::Port(si, 1)));
        assert_eq!(g.hole_by_label("out"), Some(h));

        // Replace the hole by an Add agent: S >< Add becomes an active pair.
        g.remove_hole(h);
        let (h1, h2) = (HoleId::from_inject(1, 0), HoleId::from_inject(1, 1));
        g.insert_agent(ai, agent(add, 3));
        g.insert_hole(h1, hole(Endpoint::Port(ai, 1), None));
        g.insert_hole(h2, hole(Endpoint::Port(ai, 2), None));
        g.link(Endpoint::Port(si, 0), Endpoint::Port(ai, 0));
        g.link(Endpoint::Port(ai, 1), Endpoint::Hole(h1));
        g.link(Endpoint::Port(ai, 2), Endpoint::Hole(h2));
        g.check(&sig).unwrap();
        let pair = if si < ai { (si, ai) } else { (ai, si) };
        assert_eq!(g.active_pairs(), vec![pair]);
        assert_eq!(g.active_partner(si), Some(ai));
        assert_eq!(g.active_partner(zi), None);
        assert_eq!(g.hole_by_label("out"), None);
        assert_eq!((g.agent_count(), g.hole_count()), (3, 2));
    }

    #[test]
    fn check_detects_malformed_graphs() {
        let (sig, z, s, _) = peano();
        let (a, b) = (AgentId::from_inject(0, 0), AgentId::from_inject(0, 1));

        let mut g = Graph::default();
        g.insert_agent(a, agent(z, 1));
        g.insert_agent(b, agent(z, 1));
        g.set_end(Endpoint::Port(a, 0), Endpoint::Port(b, 0));
        assert!(g.check(&sig).is_err(), "one-sided wire");

        let mut g = Graph::default();
        g.insert_agent(a, agent(z, 1));
        g.insert_agent(b, agent(z, 1));
        g.link(Endpoint::Port(a, 0), Endpoint::Port(b, 0));
        assert!(matches!(g.check(&sig), Err(GraphError::WireType(_))), "Nat+ ~ Nat+");

        let mut g = Graph::default();
        g.insert_agent(a, agent(s, 2));
        g.link(Endpoint::Port(a, 0), Endpoint::Port(a, 1));
        g.check(&sig).unwrap(); // principal wired to own aux is legal (a deadlock, not malformed)
        g.set_end(Endpoint::Port(a, 0), Endpoint::Port(a, 0));
        assert!(matches!(g.check(&sig), Err(GraphError::SelfWire(_))));

        let mut g = Graph::default();
        g.insert_agent(a, agent(s, 1));
        assert!(matches!(g.check(&sig), Err(GraphError::Arity(_))));

        let mut g = Graph::default();
        let mut bad = agent(z, 1);
        bad.payload = Value::Int(3);
        g.insert_agent(a, bad);
        assert!(matches!(g.check(&sig), Err(GraphError::Payload(_))));
    }

    #[test]
    fn digest_independent_of_insertion_order() {
        let (_, z, s, _) = peano();
        let (zi, si) = (AgentId::from_inject(0, 0), AgentId::from_inject(0, 1));
        let h = HoleId::from_inject(0, 0);
        let build = |zfirst: bool, payload: Value| {
            let mut g = Graph::default();
            let mut sa = agent(s, 2);
            sa.payload = payload;
            if zfirst {
                g.insert_agent(zi, agent(z, 1));
                g.insert_agent(si, sa);
            } else {
                g.insert_agent(si, sa);
                g.insert_agent(zi, agent(z, 1));
            }
            g.insert_hole(h, hole(Endpoint::Port(si, 0), Some("out")));
            g.link(Endpoint::Port(zi, 0), Endpoint::Port(si, 1));
            g.link(Endpoint::Port(si, 0), Endpoint::Hole(h));
            g
        };
        assert_eq!(build(true, Value::Unit).digest(), build(false, Value::Unit).digest());
        assert_ne!(build(true, Value::Unit).digest(), build(true, Value::Int(1)).digest());
    }

    #[test]
    fn labels_track_holes() {
        let (_, z, _, _) = peano();
        let mut g = Graph::default();
        let a = AgentId::from_inject(0, 0);
        let h = HoleId::from_inject(0, 0);
        g.insert_agent(a, agent(z, 1));
        g.insert_hole(h, hole(Endpoint::Port(a, 0), Some("x")));
        assert_eq!(g.hole_by_label("x"), Some(h));
        assert_eq!(g.hole(h).unwrap().label.as_deref(), Some("x"));
        g.remove_hole(h);
        assert_eq!(g.hole_by_label("x"), None);
        assert_eq!(g.holes_sorted().len(), 0);
    }
}
