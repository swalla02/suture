//! Graphviz export: a minimal graph inspector.

use std::fmt::Write as _;

use crate::graph::Endpoint;
use crate::machine::Machine;

fn node(e: Endpoint) -> String {
    match e {
        Endpoint::Port(a, _) => format!("a{:032x}", a.0),
        Endpoint::Hole(h) => format!("h{:032x}", h.0),
    }
}

fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

impl Machine {
    /// The graph in DOT format. Active pairs are drawn bold red; holes are
    /// diamonds labeled with their label (or id).
    pub fn to_dot(&self) -> String {
        let (g, sig) = (self.graph(), self.sig());
        let mut out = String::from("digraph suture {\n  rankdir=LR;\n  node [shape=box, fontname=\"monospace\"];\n");
        for (id, _) in g.agents_sorted() {
            let _ = writeln!(out, "  a{:032x} [label=\"{}\"];", id.0, esc(&self.describe(id)));
        }
        for (id, h) in g.holes_sorted() {
            let label = h.label.clone().unwrap_or_else(|| id.to_string());
            let _ = writeln!(out, "  h{:032x} [shape=diamond, label=\"{}\"];", id.0, esc(&label));
        }
        let port_name = |e: Endpoint| match e {
            Endpoint::Port(_, 0) => "*".to_string(),
            Endpoint::Port(a, p) => sig.kind(g.agent(a).unwrap().kind).aux[p as usize - 1].name.clone(),
            Endpoint::Hole(_) => String::new(),
        };
        let mut wires = vec![];
        for (id, a) in g.agents_sorted() {
            for (p, &peer) in a.ports.iter().enumerate() {
                let here = Endpoint::Port(id, p as u8);
                if here < peer {
                    wires.push((here, peer));
                }
            }
        }
        for (id, h) in g.holes_sorted() {
            let here = Endpoint::Hole(id);
            if matches!(h.peer, Endpoint::Hole(_)) && here < h.peer {
                wires.push((here, h.peer));
            }
        }
        for (a, b) in wires {
            let active = matches!((a, b), (Endpoint::Port(_, 0), Endpoint::Port(_, 0)));
            let style = if active { ", color=red, penwidth=2" } else { "" };
            let _ = writeln!(
                out,
                "  {} -> {} [arrowhead=none, taillabel=\"{}\", headlabel=\"{}\"{style}];",
                node(a),
                node(b),
                esc(&port_name(a)),
                esc(&port_name(b))
            );
        }
        out.push_str("}\n");
        out
    }
}
