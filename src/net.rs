//! Nets: the data form of patches and injected structure.
//!
//! A net is a set of agents whose ports are bound to *wire variables*. Every
//! variable must be used exactly twice: two ports, a port and a link end, or
//! — for interface variables — once inside the net plus once implicitly by
//! the context (the replaced pair's auxiliary port, or a host hole).
//! `link(a, b)` connects whatever is at the other ends of `a` and `b`
//! directly, which is how a rule passes a wire straight through.

use serde::{Deserialize, Serialize};

use crate::check::NetError;
use crate::id::{Digest, DigestBuilder, HoleId};
use crate::sig::{KindId, Signature};
use crate::value::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Var(pub u32);

/// What a port of a net agent is bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Slot {
    Var(Var),
    /// The principal port of an effect agent: connected to a fresh outbox hole.
    Emit,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetAgent {
    pub kind: KindId,
    pub payload: Value,
    pub ports: Vec<Slot>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetData {
    pub vars: u32,
    pub agents: Vec<NetAgent>,
    pub links: Vec<(Var, Var)>,
}

impl NetData {
    /// Content digest, used to verify deterministic replay of rule output.
    pub fn digest(&self) -> Digest {
        let mut d = DigestBuilder::new("net");
        d.u32(self.vars);
        d.u64(self.agents.len() as u64);
        let mut buf = Vec::new();
        for a in &self.agents {
            d.u32(a.kind.0 as u32);
            buf.clear();
            a.payload.encode(&mut buf);
            d.bytes(&buf);
            d.u64(a.ports.len() as u64);
            for s in &a.ports {
                match s {
                    Slot::Var(v) => {
                        d.u8(0);
                        d.u32(v.0);
                    }
                    Slot::Emit => d.u8(1),
                }
            }
        }
        d.u64(self.links.len() as u64);
        for (a, b) in &self.links {
            d.u32(a.0);
            d.u32(b.0);
        }
        d.finish()
    }
}

/// Builds a [`NetData`], recording the first misuse instead of panicking.
pub struct NetBuilder<'s> {
    sig: &'s Signature,
    data: NetData,
    err: Option<NetError>,
}

impl<'s> NetBuilder<'s> {
    pub fn new(sig: &'s Signature) -> Self {
        Self::with_vars(sig, 0)
    }

    /// A builder whose first `n` variables are pre-allocated interface vars.
    pub(crate) fn with_vars(sig: &'s Signature, n: u32) -> Self {
        NetBuilder { sig, data: NetData { vars: n, ..NetData::default() }, err: None }
    }

    pub fn sig(&self) -> &'s Signature {
        self.sig
    }

    pub fn var(&mut self) -> Var {
        let v = Var(self.data.vars);
        self.data.vars += 1;
        v
    }

    pub(crate) fn fail(&mut self, e: NetError) {
        self.err.get_or_insert(e);
    }

    fn arity_ok(&mut self, kind: KindId, n: usize) -> bool {
        let expected = self.sig.arity(kind);
        if expected != n {
            self.fail(NetError::Arity { kind: self.sig.kind(kind).name.clone(), expected, got: n });
            return false;
        }
        true
    }

    /// Adds an agent with its principal port bound to `at` and auxiliary
    /// ports bound to `aux`, in declaration order.
    pub fn agent(&mut self, kind: KindId, payload: impl Into<Value>, at: Var, aux: &[Var]) {
        if self.arity_ok(kind, aux.len()) {
            let mut ports = Vec::with_capacity(1 + aux.len());
            ports.push(Slot::Var(at));
            ports.extend(aux.iter().map(|&v| Slot::Var(v)));
            self.data.agents.push(NetAgent { kind, payload: payload.into(), ports });
        }
    }

    /// Adds an effect agent; its principal port will face a fresh outbox hole.
    pub fn effect(&mut self, kind: KindId, payload: impl Into<Value>, aux: &[Var]) {
        if self.arity_ok(kind, aux.len()) {
            let mut ports = Vec::with_capacity(1 + aux.len());
            ports.push(Slot::Emit);
            ports.extend(aux.iter().map(|&v| Slot::Var(v)));
            self.data.agents.push(NetAgent { kind, payload: payload.into(), ports });
        }
    }

    /// Connects whatever is at the other ends of `a` and `b`.
    pub fn link(&mut self, a: Var, b: Var) {
        self.data.links.push((a, b));
    }

    pub fn finish(self) -> Result<NetData, NetError> {
        if let Some(e) = self.err {
            return Err(e);
        }
        let n = self.data.vars;
        let slots = self.data.agents.iter().flat_map(|a| a.ports.iter());
        for s in slots {
            if let Slot::Var(v) = s {
                if v.0 >= n {
                    return Err(NetError::UnknownVar(v.0));
                }
            }
        }
        for (a, b) in &self.data.links {
            for v in [a, b] {
                if v.0 >= n {
                    return Err(NetError::UnknownVar(v.0));
                }
            }
        }
        Ok(self.data)
    }
}

/// A reference to an existing hole, by identity or by label.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HoleRef {
    Id(HoleId),
    Label(String),
}

/// How an open variable of an injected net attaches to the boundary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Bind {
    /// Consume an existing hole: its peer gets wired into the net.
    Hole(HoleRef),
    /// Create a new hole, optionally labeled.
    New(Option<String>),
}

/// A host request to admit a net into the graph (`INJECT`).
#[derive(Clone, Debug)]
pub struct Injection {
    pub net: NetData,
    pub binds: Vec<(Var, Bind)>,
    /// Idempotency key: a second inject with the same key is a no-op.
    pub key: Option<String>,
    /// Permission to create conserved (capability) agents.
    pub mint: bool,
}

impl Injection {
    pub fn new(net: NetData) -> Self {
        Injection { net, binds: vec![], key: None, mint: false }
    }
    pub fn bind(mut self, v: Var, b: Bind) -> Self {
        self.binds.push((v, b));
        self
    }
    pub fn at_label(self, v: Var, label: &str) -> Self {
        self.bind(v, Bind::Hole(HoleRef::Label(label.to_string())))
    }
    pub fn at_hole(self, v: Var, h: HoleId) -> Self {
        self.bind(v, Bind::Hole(HoleRef::Id(h)))
    }
    pub fn new_hole(self, v: Var, label: Option<&str>) -> Self {
        self.bind(v, Bind::New(label.map(str::to_string)))
    }
    pub fn key(mut self, k: impl Into<String>) -> Self {
        self.key = Some(k.into());
        self
    }
    pub fn mint(mut self) -> Self {
        self.mint = true;
        self
    }
}

/// What an external (implicit) variable use stands for during resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExtKind {
    /// A real endpoint outside the net.
    Terminal,
    /// Wired, outside the net, to external use `j` (e.g. two auxiliary ports
    /// of the reducing pair connected to each other).
    Glue(usize),
}

/// A resolved wire end: a port of a net agent, or external use `k`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    Agent(usize, usize),
    Ext(usize),
}

#[derive(Debug)]
pub(crate) struct Resolved {
    pub conns: Vec<(End, End)>,
    /// Closed wire loops that passed through the context (Lafont's vicious
    /// circles); they carry no agents and are dropped.
    pub vicious: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Use {
    Port(usize, usize),
    Link(usize),
    Ext(usize),
}

/// Computes the final wires a net induces: every chain of variables, links
/// and glued external uses is followed from terminal to terminal.
pub(crate) fn resolve(net: &NetData, ext_vars: &[Var], ext: &[ExtKind]) -> Result<Resolved, NetError> {
    let n = net.vars as usize;
    let mut uses: Vec<Vec<Use>> = vec![Vec::with_capacity(2); n];
    let mut put = |v: Var, u: Use| -> Result<(), NetError> {
        uses.get_mut(v.0 as usize).ok_or(NetError::UnknownVar(v.0))?.push(u);
        Ok(())
    };
    for (i, a) in net.agents.iter().enumerate() {
        for (p, s) in a.ports.iter().enumerate() {
            if let Slot::Var(v) = s {
                put(*v, Use::Port(i, p))?;
            }
        }
    }
    for (l, &(a, b)) in net.links.iter().enumerate() {
        if a == b {
            return Err(NetError::ClosedLoop(a.0));
        }
        put(a, Use::Link(l))?;
        put(b, Use::Link(l))?;
    }
    for (k, &v) in ext_vars.iter().enumerate() {
        put(v, Use::Ext(k))?;
    }
    for (v, u) in uses.iter().enumerate() {
        if u.len() != 2 {
            return Err(NetError::VarUses { var: v as u32, uses: u.len() });
        }
    }

    let other = |v: usize, came: Use| if uses[v][0] == came { uses[v][1] } else { uses[v][0] };
    let terminal = |u: Use| match u {
        Use::Port(..) => true,
        Use::Ext(k) => ext[k] == ExtKind::Terminal,
        Use::Link(_) => false,
    };
    let as_end = |u: Use| match u {
        Use::Port(i, p) => End::Agent(i, p),
        Use::Ext(k) => End::Ext(k),
        Use::Link(_) => unreachable!("links are never terminals"),
    };
    // Steps across a non-terminal use to the variable on its far side.
    let cross = |v: usize, u: Use| -> (usize, Use) {
        match u {
            Use::Link(l) => {
                let (a, b) = net.links[l];
                let w = if a.0 as usize == v { b } else { a };
                (w.0 as usize, Use::Link(l))
            }
            Use::Ext(k) => match ext[k] {
                ExtKind::Glue(j) => (ext_vars[j].0 as usize, Use::Ext(j)),
                ExtKind::Terminal => unreachable!(),
            },
            Use::Port(..) => unreachable!(),
        }
    };

    let mut visited = vec![false; n];
    let mut conns = Vec::new();
    for v in 0..n {
        for start in [uses[v][0], uses[v][1]] {
            if visited[v] || !terminal(start) {
                continue;
            }
            let (mut cur, mut came) = (v, start);
            loop {
                visited[cur] = true;
                let next = other(cur, came);
                if terminal(next) {
                    conns.push((as_end(start), as_end(next)));
                    break;
                }
                (cur, came) = cross(cur, next);
            }
        }
    }

    let mut vicious = 0;
    for v in 0..n {
        if visited[v] {
            continue;
        }
        let mut through_context = false;
        let (mut cur, mut came) = (v, uses[v][1]);
        loop {
            visited[cur] = true;
            let next = other(cur, came);
            if matches!(next, Use::Ext(_)) {
                through_context = true;
            }
            (cur, came) = cross(cur, next);
            if cur == v {
                break;
            }
        }
        if !through_context {
            return Err(NetError::ClosedLoop(v as u32));
        }
        vicious += 1;
    }
    Ok(Resolved { conns, vicious })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::fixtures::fx;

    #[test]
    fn builder_records_first_error() {
        let f = fx();
        let mut b = NetBuilder::new(&f.sig);
        let v = b.var();
        b.agent(f.add, (), v, &[]); // Add has two aux ports
        b.agent(f.z, (), v, &[v]); // also wrong, but only the first error is kept
        assert!(matches!(b.finish(), Err(NetError::Arity { expected: 2, got: 0, .. })));
        let mut b = NetBuilder::new(&f.sig);
        b.agent(f.z, (), Var(99), &[]);
        assert_eq!(b.finish().unwrap_err(), NetError::UnknownVar(99));
    }

    #[test]
    fn glue_joins_new_agents_across_the_pair() {
        let f = fx();
        let mut b = NetBuilder::with_vars(&f.sig, 2);
        b.agent(f.era, (), Var(0), &[]);
        b.agent(f.z, (), Var(1), &[]);
        let net = b.finish().unwrap();
        let r = resolve(&net, &[Var(0), Var(1)], &[ExtKind::Glue(1), ExtKind::Glue(0)]).unwrap();
        assert_eq!(r.conns, vec![(End::Agent(0, 0), End::Agent(1, 0))]);
        assert_eq!(r.vicious, 0);
    }

    #[test]
    fn glue_vicious_circle() {
        let f = fx();
        let mut b = NetBuilder::with_vars(&f.sig, 2);
        b.link(Var(0), Var(1));
        let net = b.finish().unwrap();
        let r = resolve(&net, &[Var(0), Var(1)], &[ExtKind::Glue(1), ExtKind::Glue(0)]).unwrap();
        assert!(r.conns.is_empty());
        assert_eq!(r.vicious, 1);
    }

    #[test]
    fn closed_link_loops_are_errors() {
        let f = fx();
        let mut b = NetBuilder::new(&f.sig);
        let (x, y) = (b.var(), b.var());
        b.link(x, y);
        b.link(y, x);
        let net = b.finish().unwrap();
        assert!(matches!(resolve(&net, &[], &[]), Err(NetError::ClosedLoop(_))));
        let mut b = NetBuilder::new(&f.sig);
        let x = b.var();
        b.link(x, x);
        let net = b.finish().unwrap();
        assert!(matches!(resolve(&net, &[], &[]), Err(NetError::ClosedLoop(_))));
    }

    #[test]
    fn link_chains_resolve_end_to_end() {
        let f = fx();
        let mut b = NetBuilder::new(&f.sig);
        let (a, m, c) = (b.var(), b.var(), b.var());
        b.agent(f.z, (), a, &[]);
        b.link(a, m);
        b.link(m, c);
        b.agent(f.era, (), c, &[]);
        let net = b.finish().unwrap();
        assert_eq!(resolve(&net, &[], &[]).unwrap().conns, vec![(End::Agent(0, 0), End::Agent(1, 0))]);
    }

    #[test]
    fn net_digest_tracks_content() {
        let f = fx();
        let mk = |n: i64| {
            let mut b = NetBuilder::new(&f.sig);
            let v = b.var();
            b.agent(f.lease, crate::tup![n], v, &[]);
            b.agent(f.era, (), v, &[]);
            b.finish().unwrap()
        };
        assert_eq!(mk(1).digest(), mk(1).digest());
        assert_ne!(mk(1).digest(), mk(2).digest());
    }
}
