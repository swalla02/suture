mod common;

use common::*;
use suture::*;

fn agents_of(m: &Machine, kind: KindId) -> Vec<AgentId> {
    m.graph().agents_sorted().into_iter().filter(|(_, a)| a.kind == kind).map(|(id, _)| id).collect()
}

#[test]
fn external_wait() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    let mut b = NetBuilder::new(&m0.prog.sig);
    let (x, y, r) = (b.var(), b.var(), b.var());
    b.agent(m0.add, (), x, &[y, r]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(x, Some("x")).new_hole(y, None).new_hole(r, None)).unwrap();
    let e = m.explain();
    assert_eq!(e.groups.len(), 1);
    assert!(matches!(&e.groups[0].terminal, Terminal::Hole { label: Some(l), .. } if l == "x"));
    assert_eq!(e.groups[0].agents, agents_of(&m, m0.add));
    assert_eq!(e.external().len(), 1);
}

#[test]
fn pending_effect() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(start_go(&m0, "out")).unwrap();
    m.run(None);
    let ask = agents_of(&m, m0.ask);
    let e = m.explain();
    assert_eq!(e.groups.len(), 1);
    assert!(matches!(&e.groups[0].terminal, Terminal::Effect { agent, .. } if *agent == ask[0]));
    assert_eq!(e.groups[0].agents, ask);
}

#[test]
fn stuck_pair() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    let mut b = NetBuilder::new(&m0.prog.sig);
    let (p, y, r) = (b.var(), b.var(), b.var());
    b.agent(m0.bogus, (), p, &[]);
    b.agent(m0.add, (), p, &[y, r]);
    m.inject(Injection::new(b.finish().unwrap()).new_hole(y, None).new_hole(r, None)).unwrap();
    assert!(matches!(m.explain().groups[0].terminal, Terminal::Ready(..)));
    m.run(None);
    let e = m.explain();
    assert_eq!(e.stuck().len(), 1);
    assert_eq!(e.stuck()[0].agents.len(), 2);
}

#[test]
fn deadlock_cycle() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    let mut b = NetBuilder::new(&m0.prog.sig);
    let (p, q) = (b.var(), b.var());
    b.agent(m0.s, (), p, &[q]); // S1.0 ~ S2.1
    b.agent(m0.s, (), q, &[p]); // S2.0 ~ S1.1
    m.inject(Injection::new(b.finish().unwrap())).unwrap();
    assert!(m.is_quiescent());
    let e = m.explain();
    let d = e.deadlocks();
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].agents.len(), 2);
    assert!(matches!(&d[0].terminal, Terminal::Deadlock(c) if c.len() == 2));
}

#[test]
fn chains_group_by_terminal() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 3, 0, "out")).unwrap();
    m.run(None);
    let e = m.explain();
    assert_eq!(e.groups.len(), 1);
    assert_eq!(e.groups[0].agents.len(), 4, "S S S Z all wait on the result hole");
}

#[test]
fn render_and_dot_mention_kinds() {
    let m0 = mini();
    let mut m = Machine::new(m0.prog.clone());
    m.inject(sum(&m0, 1, 1, "out")).unwrap();
    m.inject(start_go(&m0, "eff")).unwrap();
    let before = m.render_explanation();
    assert!(before.contains("ready"), "{before}");
    m.run(None);
    let text = m.render_explanation();
    assert!(text.contains("waiting on hole 'out'"), "{text}");
    assert!(text.contains("pending effect Ask"), "{text}");
    assert!(text.contains("S×2"), "{text}");
    let dot = m.to_dot();
    assert!(dot.starts_with("digraph"));
    assert!(dot.contains("\"out\"") && dot.contains("S#"), "{dot}");
}
