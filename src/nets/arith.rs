//! Peano arithmetic as a typed interaction system (Lafont 1990, §2): numbers
//! are chains of `S` ending in `Z`; `Add` and `Mul` consume their first
//! argument through their principal port. Multiplication shares its second
//! argument through an explicit duplicator `Dup` and discards it through an
//! explicit eraser `Era` — sharing and erasure are never implicit.

use crate::machine::{InjectError, Machine, Program};
use crate::net::{Injection, NetBuilder, Var};
use crate::rng::Rng;
use crate::{Endpoint, HoleId, KindId, RuleTable, SigBuilder, TypeId};

pub struct ArithKinds {
    pub nat: TypeId,
    pub z: KindId,
    pub s: KindId,
    pub add: KindId,
    pub mul: KindId,
    pub dup: KindId,
    pub era: KindId,
}

pub fn program() -> (Program, ArithKinds) {
    let mut b = SigBuilder::new("arith", 1);
    let nat = b.polarized("Nat");
    let z = b.kind("Z", nat.pos()).done();
    let s = b.kind("S", nat.pos()).aux("pred", nat.neg()).done();
    let add = b.kind("Add", nat.neg()).aux("y", nat.neg()).aux("r", nat.pos()).done();
    let mul = b.kind("Mul", nat.neg()).aux("y", nat.neg()).aux("r", nat.pos()).done();
    let dup = b.kind("Dup", nat.neg()).aux("a", nat.pos()).aux("b", nat.pos()).done();
    let era = b.kind("Era", nat.neg()).done();
    let sig = b.build().expect("arith signature");

    let mut t = RuleTable::new();
    // 0 + y = y
    t.def("add_z", z, add, |cx| {
        let [y, r] = cx.right_aux();
        cx.link(y, r);
        Ok(())
    });
    // S(x) + y = S(x + y)
    t.def("add_s", s, add, move |cx| {
        let [x] = cx.left_aux();
        let [y, r] = cx.right_aux();
        let r1 = cx.var();
        cx.agent(add, (), x, &[y, r1]);
        cx.agent(s, (), r, &[r1]);
        Ok(())
    });
    // 0 * y = 0, discarding y
    t.def("mul_z", z, mul, move |cx| {
        let [y, r] = cx.right_aux();
        cx.agent(era, (), y, &[]);
        cx.agent(z, (), r, &[]);
        Ok(())
    });
    // S(x) * y = x * y + y, sharing y
    t.def("mul_s", s, mul, move |cx| {
        let [x] = cx.left_aux();
        let [y, r] = cx.right_aux();
        let (y1, y2, r1) = (cx.var(), cx.var(), cx.var());
        cx.agent(dup, (), y, &[y1, y2]);
        cx.agent(mul, (), x, &[y1, r1]);
        cx.agent(add, (), r1, &[y2, r]);
        Ok(())
    });
    t.def("dup_z", z, dup, move |cx| {
        let [a, b] = cx.right_aux();
        cx.agent(z, (), a, &[]);
        cx.agent(z, (), b, &[]);
        Ok(())
    });
    t.def("dup_s", s, dup, move |cx| {
        let [x] = cx.left_aux();
        let [a, b] = cx.right_aux();
        let (a1, b1) = (cx.var(), cx.var());
        cx.agent(s, (), a, &[a1]);
        cx.agent(s, (), b, &[b1]);
        cx.agent(dup, (), x, &[a1, b1]);
        Ok(())
    });
    t.def("era_z", z, era, |_| Ok(()));
    t.def("era_s", s, era, move |cx| {
        let [x] = cx.left_aux();
        cx.agent(era, (), x, &[]);
        Ok(())
    });
    let prog = Program::new(sig, t).expect("arith rules");
    (prog, ArithKinds { nat, z, s, add, mul, dup, era })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Expr {
    Num(u64),
    Add(Box<Expr>, Box<Expr>),
    Mul(Box<Expr>, Box<Expr>),
}

#[allow(clippy::should_implement_trait)] // `Expr::add(a, b)` builds a term; it is not `a + b`
impl Expr {
    pub fn num(n: u64) -> Expr {
        Expr::Num(n)
    }
    pub fn add(a: Expr, b: Expr) -> Expr {
        Expr::Add(Box::new(a), Box::new(b))
    }
    pub fn mul(a: Expr, b: Expr) -> Expr {
        Expr::Mul(Box::new(a), Box::new(b))
    }

    pub fn eval(&self) -> u64 {
        match self {
            Expr::Num(n) => *n,
            Expr::Add(a, b) => a.eval() + b.eval(),
            Expr::Mul(a, b) => a.eval() * b.eval(),
        }
    }

    pub fn random(rng: &mut Rng, depth: u32, max_leaf: u64) -> Expr {
        if depth == 0 || rng.chance(0.3) {
            return Expr::Num(rng.below(max_leaf + 1));
        }
        let (a, b) = (Expr::random(rng, depth - 1, max_leaf), Expr::random(rng, depth - 1, max_leaf));
        if rng.chance(0.5) { Expr::add(a, b) } else { Expr::mul(a, b) }
    }
}

/// Builds `e` into `b`; returns the variable carrying its value.
pub fn build(b: &mut NetBuilder, k: &ArithKinds, e: &Expr) -> Var {
    match e {
        Expr::Num(n) => {
            let top = b.var();
            let mut cur = top;
            for _ in 0..*n {
                let next = b.var();
                b.agent(k.s, (), cur, &[next]);
                cur = next;
            }
            b.agent(k.z, (), cur, &[]);
            top
        }
        Expr::Add(x, y) | Expr::Mul(x, y) => {
            let (vx, vy, r) = (build(b, k, x), build(b, k, y), b.var());
            let op = if matches!(e, Expr::Add(..)) { k.add } else { k.mul };
            b.agent(op, (), vx, &[vy, r]);
            r
        }
    }
}

/// Injects `e` with its result attached to a new hole labeled `label`.
pub fn inject_expr(m: &mut Machine, k: &ArithKinds, e: &Expr, label: &str) -> Result<HoleId, InjectError> {
    let mut b = NetBuilder::new(&m.program().sig);
    let r = build(&mut b, k, e);
    let net = b.finish()?;
    let rc = m.inject(Injection::new(net).new_hole(r, Some(label)))?;
    Ok(rc.new_holes[0].1)
}

/// Reads back the numeral attached to hole `h`, if it is in normal form.
pub fn readback(m: &Machine, k: &ArithKinds, h: HoleId) -> Option<u64> {
    let mut e = m.graph().hole(h)?.peer;
    let mut n = 0;
    loop {
        let Endpoint::Port(a, 0) = e else { return None };
        let ag = m.graph().agent(a)?;
        if ag.kind == k.z {
            return Some(n);
        }
        if ag.kind != k.s {
            return None;
        }
        n += 1;
        e = ag.ports[1];
    }
}
