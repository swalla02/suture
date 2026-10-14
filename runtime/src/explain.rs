//! Explanations (spec §5.6): why a unit is where it is, read from its state.

use suture_lang::ir::Program;

use crate::interp::{duration, holds, in_state, request_key, ATTEMPTS};
use crate::state::{Node, Status, Unit};

/// One line for the unit, one per goal, and one per part, indented by depth.
/// `due` gives the seconds left before a pending request's deadline, by key.
pub fn explain(prog: &Program, unit: &Unit, due: impl Fn(&str) -> Option<u64>) -> Vec<String> {
    let attrs: Vec<String> = unit.root.attrs.iter().map(|(k, v)| format!("{k} {}", v.show(&prog.currency))).collect();
    let mut out = vec![format!("{} {} ({})", unit.root.kind, unit.label, attrs.join(", "))];
    for (i, g) in prog.goals.iter().enumerate().filter(|(_, g)| g.kind == unit.root.kind) {
        if !g.filter.iter().all(|f| holds(prog, &unit.root, &[], f)) {
            out.push(format!("  goal does not apply: {}", g.source));
            continue;
        }
        let missing: Vec<&str> = g.states.iter().filter(|s| !in_state(prog, &unit.root, &[], s)).map(String::as_str).collect();
        out.push(if missing.is_empty() {
            format!("  goal met: {}", g.source)
        } else if unit.alerts.contains(&(i as u32)) {
            format!("  goal late, alert raised: {} (not yet {})", g.source, missing.join(", "))
        } else {
            format!("  goal not met yet: {} (not yet {})", g.source, missing.join(", "))
        });
    }
    slots(prog, &unit.root, 1, &mut out);
    for (i, _) in unit.root.parts.iter().enumerate() {
        part(prog, unit, &[i], &due, &mut out);
    }
    out
}

/// One line per slot the node fills: its current alternative, and which of
/// its alternatives that is.
fn slots(prog: &Program, n: &Node, depth: usize, out: &mut Vec<String>) {
    for (slot, alt) in &n.slots {
        let (i, of) = prog.slot(slot).map_or((0, 0), |s| (s.alternatives.iter().position(|a| a == alt).map_or(0, |i| i + 1), s.alternatives.len()));
        out.push(format!("{}{slot}: {alt} ({i} of {of})", "  ".repeat(depth)));
    }
}

fn part(prog: &Program, unit: &Unit, at: &[usize], due: &impl Fn(&str) -> Option<u64>, out: &mut Vec<String>) {
    let n = at.iter().fold(&unit.root, |n, &i| &n.parts[i]);
    let depth = at.len();
    let effect = prog.effect_of(&n.kind);
    let what = match (n.status, &n.waiting) {
        (Status::Idle, Some(rule)) => format!("waiting: rule {rule}"),
        (Status::Idle, None) if effect.is_some() => "not requested yet".to_string(),
        (Status::Idle, None) => "waiting for its parts".to_string(),
        (Status::Requested, _) => {
            let left = due(&request_key(&unit.label, &unit.root, at)).map(|s| format!(", answer due in {}", duration(s)));
            let adapter = effect.map_or("?", |e| e.adapter.as_str());
            format!("requested from {adapter}, send {} of {ATTEMPTS}{}", n.sends, left.unwrap_or_default())
        }
        (Status::Done, _) => n.outcome.clone().unwrap_or_else(|| "done".to_string()),
        (Status::Failed, _) => format!("failed ({})", n.outcome.as_deref().unwrap_or("?")),
    };
    let attempt = if n.attempt > 1 { format!(" (attempt {})", n.attempt) } else { String::new() };
    let attrs: Vec<String> = n.attrs.iter().map(|(k, v)| format!("{k} {}", v.show(&prog.currency))).collect();
    let attrs = if attrs.is_empty() { String::new() } else { format!(" — {}", attrs.join(", ")) };
    out.push(format!("{}{}{attempt}: {what}{attrs}", "  ".repeat(depth), n.kind));
    slots(prog, n, depth + 1, out);
    for i in 0..n.parts.len() {
        let mut child = at.to_vec();
        child.push(i);
        part(prog, unit, &child, due, out);
    }
}
