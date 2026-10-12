//! Structural explanation of a machine's state.
//!
//! Every agent's principal port points at exactly one thing, so "what is
//! this agent waiting for?" defines a functional graph. Following it from any
//! agent ends at a hole (external input or a pending effect), at an active
//! pair (ready to reduce, or stuck), or in a cycle of agents each waiting on
//! the next — a deadlock that no rewrite can ever break.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::graph::{Endpoint, HoleOrigin};
use crate::id::{AgentId, HoleId};
use crate::machine::Machine;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Terminal {
    /// Waiting for the host to attach something at this hole.
    Hole { hole: HoleId, label: Option<String> },
    /// An effect agent waiting for its reply at its outbox hole.
    Effect { agent: AgentId, hole: HoleId },
    /// An active pair that will reduce.
    Ready(AgentId, AgentId),
    /// An active pair that faulted and will never reduce.
    Stuck(AgentId, AgentId),
    /// A cycle of agents each waiting on the next (sorted members).
    Deadlock(Vec<AgentId>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitGroup {
    pub terminal: Terminal,
    /// Every agent whose wait chain ends at `terminal`, sorted.
    pub agents: Vec<AgentId>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Explanation {
    pub groups: Vec<WaitGroup>,
}

impl Explanation {
    pub fn deadlocks(&self) -> Vec<&WaitGroup> {
        self.groups.iter().filter(|g| matches!(g.terminal, Terminal::Deadlock(_))).collect()
    }
    pub fn stuck(&self) -> Vec<&WaitGroup> {
        self.groups.iter().filter(|g| matches!(g.terminal, Terminal::Stuck(..))).collect()
    }
    /// Groups waiting on the host: holes and pending effects.
    pub fn external(&self) -> Vec<&WaitGroup> {
        self.groups.iter().filter(|g| matches!(g.terminal, Terminal::Hole { .. } | Terminal::Effect { .. })).collect()
    }
}

enum Step {
    Done(Terminal),
    Follow(AgentId),
}

impl Machine {
    fn wait_step(&self, a: AgentId) -> Step {
        let g = self.graph();
        let ag = g.agent(a).expect("agent exists");
        match ag.ports[0] {
            Endpoint::Hole(h) => {
                let hole = g.hole(h).expect("hole exists");
                if hole.origin == (HoleOrigin::Effect { agent: a }) {
                    Step::Done(Terminal::Effect { agent: a, hole: h })
                } else {
                    Step::Done(Terminal::Hole { hole: h, label: hole.label.clone() })
                }
            }
            Endpoint::Port(b, 0) => {
                let (x, y) = if a <= b { (a, b) } else { (b, a) };
                if self.is_faulted(x, y) {
                    Step::Done(Terminal::Stuck(x, y))
                } else {
                    Step::Done(Terminal::Ready(x, y))
                }
            }
            Endpoint::Port(b, _) => Step::Follow(b),
        }
    }

    /// Groups every agent by the terminal its wait chain reaches.
    pub fn explain(&self) -> Explanation {
        let agents: Vec<AgentId> = self.graph().agents_sorted().into_iter().map(|(id, _)| id).collect();
        let index: BTreeMap<AgentId, usize> = agents.iter().enumerate().map(|(i, a)| (*a, i)).collect();
        // 0 = unvisited, 1 = on the current path, 2 = resolved
        let mut state = vec![0u8; agents.len()];
        let mut result: Vec<Option<Terminal>> = vec![None; agents.len()];
        for start in 0..agents.len() {
            if state[start] != 0 {
                continue;
            }
            let mut path = vec![];
            let mut cur = start;
            let terminal = loop {
                if state[cur] == 2 {
                    break result[cur].clone().unwrap();
                }
                if state[cur] == 1 {
                    let pos = path.iter().position(|&p| p == cur).unwrap();
                    let mut cycle: Vec<AgentId> = path[pos..].iter().map(|&i| agents[i]).collect();
                    cycle.sort();
                    break Terminal::Deadlock(cycle);
                }
                state[cur] = 1;
                path.push(cur);
                match self.wait_step(agents[cur]) {
                    Step::Done(t) => break t,
                    Step::Follow(b) => cur = index[&b],
                }
            };
            for i in path {
                state[i] = 2;
                result[i] = Some(terminal.clone());
            }
        }
        let mut groups: BTreeMap<Terminal, Vec<AgentId>> = BTreeMap::new();
        for (i, t) in result.into_iter().enumerate() {
            groups.entry(t.unwrap()).or_default().push(agents[i]);
        }
        Explanation { groups: groups.into_iter().map(|(terminal, agents)| WaitGroup { terminal, agents }).collect() }
    }

    /// A human-readable account of what the machine is waiting for.
    pub fn render_explanation(&self) -> String {
        let e = self.explain();
        let mut out = String::new();
        let ready = self.ready_count();
        let _ = writeln!(
            out,
            "{} agents, {} holes, {} ready, {} faults",
            self.graph().agent_count(),
            self.graph().hole_count(),
            ready,
            self.faults().len()
        );
        for g in &e.groups {
            let mut kinds: BTreeMap<&str, usize> = BTreeMap::new();
            for a in &g.agents {
                *kinds.entry(&self.sig().kind(self.graph().agent(*a).unwrap().kind).name).or_default() += 1;
            }
            let summary: Vec<String> = kinds.iter().map(|(k, n)| format!("{k}×{n}")).collect();
            let head = match &g.terminal {
                Terminal::Hole { hole, label: Some(l) } => format!("waiting on hole '{l}' ({hole})"),
                Terminal::Hole { hole, label: None } => format!("waiting on hole {hole}"),
                Terminal::Effect { agent, hole } => format!("pending effect {} at {hole}", self.describe(*agent)),
                Terminal::Ready(a, b) => format!("ready pair {} >< {}", self.describe(*a), self.describe(*b)),
                Terminal::Stuck(a, b) => {
                    let why = self
                        .faults()
                        .into_iter()
                        .find(|f| (f.left == *a && f.right == *b) || (f.left == *b && f.right == *a))
                        .map(|f| f.error)
                        .unwrap_or_default();
                    format!("STUCK {} >< {}: {why}", self.describe(*a), self.describe(*b))
                }
                Terminal::Deadlock(c) => {
                    let names: Vec<String> = c.iter().map(|a| self.describe(*a)).collect();
                    format!("DEADLOCK cycle [{}]", names.join(", "))
                }
            };
            let _ = writeln!(out, "- {head}: {} agent(s) [{}]", g.agents.len(), summary.join(", "));
        }
        out
    }
}
