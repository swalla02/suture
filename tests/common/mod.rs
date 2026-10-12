#![allow(dead_code)]

use suture::*;

/// A small program: Peano addition, an effect round trip (`Start >< Go`
/// emits `Ask(7)`; the host answers `Reply(v)`; the result is `Result(v + 7)`),
/// and kinds that fault in each possible way.
pub struct Mini {
    pub prog: Program,
    pub nat: TypeId,
    pub z: KindId,
    pub s: KindId,
    pub add: KindId,
    pub bogus: KindId,
    pub lease: KindId,
    pub start: KindId,
    pub boom: KindId,
    pub nope: KindId,
    pub go: KindId,
    pub ask: KindId,
    pub reply: KindId,
    pub result: KindId,
}

pub fn mini() -> Mini {
    mini_with(0, 1)
}

/// `add_s_variant` 1 builds the result with an extra (wrong) successor, to
/// simulate rule code that changed; `add_s_version` sets the rule version.
pub fn mini_with(add_s_variant: u8, add_s_version: u32) -> Mini {
    let mut b = SigBuilder::new("mini", 1);
    let nat = b.polarized("Nat");
    let ping = b.polarized("Ping");
    let ans = b.polarized("Ans");
    let out = b.polarized("Out");
    let z = b.kind("Z", nat.pos()).done();
    let s = b.kind("S", nat.pos()).aux("pred", nat.neg()).done();
    let add = b.kind("Add", nat.neg()).aux("y", nat.neg()).aux("r", nat.pos()).done();
    let bogus = b.kind("Bogus", nat.pos()).done();
    let lease = b
        .kind("Lease", nat.pos())
        .payload(ValueType::Tuple(vec![ValueType::Int]))
        .conserved("lease", 0)
        .done();
    let start = b.kind("Start", ping.pos()).done();
    let boom = b.kind("Boom", ping.pos()).done();
    let nope = b.kind("Nope", ping.pos()).done();
    let go = b.kind("Go", ping.neg()).aux("out", out.pos()).done();
    let ask = b.kind("Ask", ans.neg()).aux("out", out.pos()).payload(ValueType::Int).effect().done();
    let reply = b.kind("Reply", ans.pos()).payload(ValueType::Int).done();
    let result = b.kind("Result", out.pos()).payload(ValueType::Int).done();
    let sig = b.build().unwrap();

    let mut t = RuleTable::new();
    t.def("add_z", z, add, |cx| {
        let [y, r] = cx.right_aux();
        cx.link(y, r);
        Ok(())
    });
    t.def("add_s", s, add, move |cx| {
        let [x] = cx.left_aux();
        let [y, r] = cx.right_aux();
        let r1 = cx.var();
        cx.agent(add, (), x, &[y, r1]);
        if add_s_variant == 1 {
            let r2 = cx.var();
            cx.agent(s, (), r, &[r2]);
            cx.agent(s, (), r2, &[r1]);
        } else {
            cx.agent(s, (), r, &[r1]);
        }
        Ok(())
    })
    .version(add_s_version);
    t.def("start_go", start, go, move |cx| {
        let [o] = cx.right_aux();
        cx.effect(ask, 7, &[o]);
        Ok(())
    })
    .emits(&[ask]);
    t.def("reply_ask", reply, ask, move |cx| {
        let v = cx.left().as_int().ok_or_else(|| cx.reject("reply must be an int"))?;
        let n = cx.right().as_int().ok_or_else(|| cx.reject("ask must be an int"))?;
        let [o] = cx.right_aux();
        cx.agent(result, v + n, o, &[]);
        Ok(())
    });
    t.def("boom_go", boom, go, |_cx| -> Result<(), RuleError> { panic!("boom") });
    t.def("nope_go", nope, go, |cx| Err(cx.reject("no")));

    Mini {
        prog: Program::new(sig, t).unwrap(),
        nat,
        z,
        s,
        add,
        bogus,
        lease,
        start,
        boom,
        nope,
        go,
        ask,
        reply,
        result,
    }
}

/// Builds `S^n(Z)`; returns the variable at the top principal port.
pub fn num(b: &mut NetBuilder, m: &Mini, n: u64) -> Var {
    let top = b.var();
    let mut cur = top;
    for _ in 0..n {
        let next = b.var();
        b.agent(m.s, (), cur, &[next]);
        cur = next;
    }
    b.agent(m.z, (), cur, &[]);
    top
}

/// An injection computing `a + b` into a new hole labeled `label`.
pub fn sum(m: &Mini, a: u64, b: u64, label: &str) -> Injection {
    let mut nb = NetBuilder::new(&m.prog.sig);
    let x = num(&mut nb, m, a);
    let y = num(&mut nb, m, b);
    let r = nb.var();
    nb.agent(m.add, (), x, &[y, r]);
    Injection::new(nb.finish().unwrap()).new_hole(r, Some(label))
}

/// Reads a Peano numeral attached to hole `h`.
pub fn read_nat(m: &Machine, mini: &Mini, h: HoleId) -> Option<u64> {
    let mut e = m.graph().hole(h)?.peer;
    let mut n = 0;
    loop {
        let Endpoint::Port(a, 0) = e else { return None };
        let ag = m.graph().agent(a)?;
        if ag.kind == mini.z {
            return Some(n);
        }
        if ag.kind != mini.s {
            return None;
        }
        n += 1;
        e = ag.ports[1];
    }
}

pub struct NoResultAbove(pub i64, pub KindId);

impl Policy for NoResultAbove {
    fn name(&self) -> &str {
        "no-result-above"
    }
    fn check(&self, v: &PatchView<'_>) -> Result<(), String> {
        for p in v.created_of(self.1) {
            if p.as_int().unwrap_or(0) > self.0 {
                return Err(format!("result {p} exceeds {}", self.0));
            }
        }
        Ok(())
    }
}
