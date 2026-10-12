//! Symmetric interaction combinators (Lafont 1997; Mazza's symmetric
//! variant): three agents γ (`Con`), δ (`Dup`), ε (`Era`) over one self-dual
//! port type, and six rules. The system is universal, untyped and strongly
//! confluent — a stress test for the kernel's splicing (pass-through wires,
//! self-wired pairs, vicious circles, unbounded growth).

use crate::machine::{Machine, Program};
use crate::net::{Injection, NetBuilder, Var};
use crate::rng::Rng;
use crate::{Endpoint, HoleId, KindId, RuleTable, SigBuilder, Signature, TypeId};

pub struct CombKinds {
    pub c: TypeId,
    pub con: KindId,
    pub dup: KindId,
    pub era: KindId,
}

pub fn program() -> (Program, CombKinds) {
    let mut b = SigBuilder::new("combinators", 1);
    let c = b.self_dual("C");
    let con = b.kind("Con", c.any()).aux("a", c.any()).aux("b", c.any()).done();
    let dup = b.kind("Dup", c.any()).aux("a", c.any()).aux("b", c.any()).done();
    let era = b.kind("Era", c.any()).done();
    let sig = b.build().expect("combinator signature");

    let mut t = RuleTable::new();
    let annihilate = |cx: &mut crate::RuleCx<'_>| {
        let [a1, a2] = cx.left_aux();
        let [b1, b2] = cx.right_aux();
        cx.link(a1, b1);
        cx.link(a2, b2);
        Ok(())
    };
    t.def("con_con", con, con, annihilate);
    t.def("dup_dup", dup, dup, annihilate);
    t.def("era_era", era, era, |_| Ok(()));
    for (name, k) in [("era_con", con), ("era_dup", dup)] {
        t.def(name, era, k, move |cx| {
            let [a1, a2] = cx.right_aux();
            cx.agent(era, (), a1, &[]);
            cx.agent(era, (), a2, &[]);
            Ok(())
        });
    }
    t.def("con_dup", con, dup, move |cx| {
        let [a1, a2] = cx.left_aux();
        let [b1, b2] = cx.right_aux();
        let (p, q, r, s) = (cx.var(), cx.var(), cx.var(), cx.var());
        cx.agent(dup, (), a1, &[p, q]);
        cx.agent(dup, (), a2, &[r, s]);
        cx.agent(con, (), b1, &[p, r]);
        cx.agent(con, (), b2, &[q, s]);
        Ok(())
    });
    let prog = Program::new(sig, t).expect("combinator rules");
    (prog, CombKinds { c, con, dup, era })
}

/// A random net of `agents` combinators whose ports are wired by a random
/// perfect matching; `free` ports (adjusted for parity) are bound to new
/// holes labeled `f0`, `f1`, ….
pub fn random_net(sig: &Signature, k: &CombKinds, rng: &mut Rng, agents: usize, free: usize) -> Injection {
    let kinds: Vec<KindId> = (0..agents).map(|_| [k.con, k.dup, k.era][rng.index(3)]).collect();
    let mut ports: Vec<(usize, usize)> =
        kinds.iter().enumerate().flat_map(|(i, &kind)| (0..=sig.arity(kind)).map(move |p| (i, p))).collect();
    rng.shuffle(&mut ports);
    let mut free = free.min(ports.len());
    if (ports.len() - free) % 2 == 1 {
        free = if free < ports.len() { free + 1 } else { free - 1 };
    }
    let mut b = NetBuilder::new(sig);
    let mut slot: Vec<Vec<Option<Var>>> = kinds.iter().map(|&kd| vec![None; 1 + sig.arity(kd)]).collect();
    let mut open = vec![];
    for &(i, p) in &ports[..free] {
        let v = b.var();
        slot[i][p] = Some(v);
        open.push(v);
    }
    for pair in ports[free..].chunks(2) {
        let v = b.var();
        slot[pair[0].0][pair[0].1] = Some(v);
        slot[pair[1].0][pair[1].1] = Some(v);
    }
    for (i, &kind) in kinds.iter().enumerate() {
        let vars: Vec<Var> = slot[i].iter().map(|v| v.expect("every port wired")).collect();
        b.agent(kind, (), vars[0], &vars[1..]);
    }
    let mut inj = Injection::new(b.finish().expect("random net is well formed"));
    for (j, v) in open.into_iter().enumerate() {
        inj = inj.new_hole(v, Some(&format!("f{j}")));
    }
    inj
}

/// A random tree of `Con` nodes with `Era` leaves; returns its root variable.
pub fn tree(b: &mut NetBuilder, k: &CombKinds, rng: &mut Rng, depth: u32) -> Var {
    let root = b.var();
    if depth == 0 || rng.chance(0.3) {
        b.agent(k.era, (), root, &[]);
    } else {
        let l = tree(b, k, rng, depth - 1);
        let r = tree(b, k, rng, depth - 1);
        b.agent(k.con, (), root, &[l, r]);
    }
    root
}

/// The shape of the `Con`/`Era` tree attached to hole `h`: `e` for a leaf,
/// `(left right)` for a node. `None` if the structure is not such a tree.
pub fn readback_tree(m: &Machine, k: &CombKinds, h: HoleId) -> Option<String> {
    fn go(m: &Machine, k: &CombKinds, e: Endpoint, depth: usize) -> Option<String> {
        let Endpoint::Port(a, 0) = e else { return None };
        let ag = m.graph().agent(a)?;
        if depth > 64 {
            return None;
        }
        if ag.kind == k.era {
            Some("e".into())
        } else if ag.kind == k.con {
            Some(format!("({} {})", go(m, k, ag.ports[1], depth + 1)?, go(m, k, ag.ports[2], depth + 1)?))
        } else {
            None
        }
    }
    go(m, k, m.graph().hole(h)?.peer, 0)
}
