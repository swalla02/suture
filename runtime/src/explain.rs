//! Explanations (spec §5.6): why a unit is where it is, read from its state.

use suture_lang::ir::Program;

use crate::interp::{holds, in_state};
use crate::state::{Node, Status, Unit};

/// One line for the unit, one per goal, and one per part, indented by depth.
pub fn explain(prog: &Program, unit: &Unit) -> Vec<String> {
    let attrs: Vec<String> = unit.root.attrs.iter().map(|(k, v)| format!("{k} {}", v.show(&prog.currency))).collect();
    let mut out = vec![format!("{} {} ({})", unit.root.kind, unit.label, attrs.join(", "))];
    for g in prog.goals.iter().filter(|g| g.kind == unit.root.kind) {
        if !g.filter.iter().all(|f| holds(prog, &unit.root, &[], f)) {
            out.push(format!("  goal does not apply: {}", g.source));
            continue;
        }
        let missing: Vec<&str> = g.states.iter().filter(|s| !in_state(prog, &unit.root, &[], s)).map(String::as_str).collect();
        out.push(if missing.is_empty() {
            format!("  goal met: {}", g.source)
        } else {
            format!("  goal not met yet: {} (not yet {})", g.source, missing.join(", "))
        });
    }
    for p in &unit.root.parts {
        part(prog, p, 1, &mut out);
    }
    out
}

fn part(prog: &Program, n: &Node, depth: usize, out: &mut Vec<String>) {
    let effect = prog.effect_of(&n.kind);
    let what = match (n.status, &n.waiting) {
        (Status::Idle, Some(rule)) => format!("waiting: rule {rule}"),
        (Status::Idle, None) if effect.is_some() => "not requested yet".to_string(),
        (Status::Idle, None) => "waiting for its parts".to_string(),
        (Status::Requested, _) => format!("requested from {}", effect.map_or("?", |e| e.adapter.as_str())),
        (Status::Done, _) => n.outcome.clone().unwrap_or_else(|| "done".to_string()),
        (Status::Failed, _) => format!("failed ({})", n.outcome.as_deref().unwrap_or("?")),
    };
    let attempt = if n.attempt > 1 { format!(" (attempt {})", n.attempt) } else { String::new() };
    let attrs: Vec<String> = n.attrs.iter().map(|(k, v)| format!("{k} {}", v.show(&prog.currency))).collect();
    let attrs = if attrs.is_empty() { String::new() } else { format!(" — {}", attrs.join(", ")) };
    out.push(format!("{}{}{attempt}: {what}{attrs}", "  ".repeat(depth), n.kind));
    for p in &n.parts {
        part(prog, p, depth + 1, out);
    }
}
