//! Explanations (spec §5.6): why a unit is where it is, read from its state.

use suture_lang::ir::{Otherwise, Program};

use crate::interp::{duration, holds, in_state, request_key};
use crate::state::{Fetch, Node, Status, Unit};

/// One line for the unit, one per goal, and one per part, indented by depth.
/// `due` gives the seconds left before a pending request's deadline, by key.
pub fn explain(prog: &Program, unit: &Unit, due: impl Fn(&str) -> Option<u64>) -> Vec<String> {
    explain_at(prog, unit, None, due)
}

/// [`explain`] at the clock `now`, which also gives each sourced value's age.
pub fn explain_at(prog: &Program, unit: &Unit, now: Option<i64>, due: impl Fn(&str) -> Option<u64>) -> Vec<String> {
    let attrs: Vec<String> = unit.root.attrs.iter().map(|(k, v)| format!("{k} {}", crate::interp::shown(prog, k, v))).collect();
    let mut out = vec![format!("{} {} ({})", unit.root.kind, unit.label, attrs.join(", "))];
    for (i, g) in prog.goals.iter().enumerate().filter(|(_, g)| g.kind == unit.root.kind) {
        if !g.filter.iter().all(|f| holds(prog, unit, &[], f)) {
            out.push(format!("  goal does not apply: {}", g.source));
            continue;
        }
        let missing: Vec<&str> = g.states.iter().filter(|s| !in_state(prog, unit, &[], s)).map(String::as_str).collect();
        out.push(if missing.is_empty() {
            format!("  goal met: {}", g.source)
        } else if unit.alerts.contains(&(i as u32)) {
            format!("  goal late, alert raised: {} (not yet {})", g.source, missing.join(", "))
        } else {
            format!("  goal not met yet: {} (not yet {})", g.source, missing.join(", "))
        });
    }
    sourced(prog, &unit.root, now, 1, &mut out);
    slots(prog, &unit.root, 1, &mut out);
    for (i, _) in unit.root.parts.iter().enumerate() {
        part(prog, unit, &[i], now, &due, &mut out);
    }
    out
}

/// One line per sourced attribute of the node (appendix I.2): where its
/// value came from, how old it is, or where its fetch stands.
fn sourced(prog: &Program, n: &Node, now: Option<i64>, depth: usize, out: &mut Vec<String>) {
    let mut seen: Vec<&str> = vec![];
    for first in prog.sources.iter().filter(|s| s.kind == n.kind) {
        if seen.contains(&first.attr.as_str()) {
            continue;
        }
        seen.push(&first.attr);
        let f = n.fetched.get(&first.attr).cloned().unwrap_or_default();
        // The source the open or last fetch asked (appendix K.2.4).
        let src = prog.sources_of(&n.kind, &first.attr).into_iter().find(|s| s.adapter == f.via).unwrap_or(first);
        let value = n.attrs.get(&src.attr).map(|v| crate::interp::shown(prog, &src.attr, v));
        let picking = n.choosing.iter().find(|(k, _)| k.ends_with("?source") && k.contains(&format!("~{}#", src.attr)));
        let what = match (&f, value) {
            _ if picking.is_some() => format!("a model is picking its source among {}", picking.expect("checked").1.join(", ")),
            (f, _) if f.sends > 0 => format!("asking {}, send {} of {}", src.adapter, f.sends, prog.attempts),
            (f, _) if f.failed => match prog.otherwise_of(&n.kind, &src.attr) {
                Otherwise::Again(secs) => format!("{} failed; asking again {} after the failure", src.adapter, duration(*secs)),
                Otherwise::Act(_) => format!("{} failed, and its `otherwise` was applied", src.adapter),
                _ => format!("{} failed; the steps its rules guard wait", src.adapter),
            },
            (Fetch { at: Some(at), .. }, Some(v)) => {
                let from = if f.fallback { format!("{v}, the fallback after {} failed", src.adapter) } else { format!("{v} from {}", src.adapter) };
                let age = now.map(|t| format!(", {} old", duration((t - at).max(0) as u64))).unwrap_or_default();
                let left = match (now, f.until) {
                    (Some(t), Some(u)) if t < u => format!(", trusted {} more", duration((u - t) as u64)),
                    (Some(_), Some(_)) => ", no longer trusted".to_string(),
                    _ => String::new(),
                };
                format!("{from}{age}{left}")
            }
            _ => "not fetched yet".to_string(),
        };
        out.push(format!("{}{}: {what}", "  ".repeat(depth), src.attr));
    }
}

/// One line per slot the node fills: its current alternative, and which of
/// its alternatives that is.
fn slots(prog: &Program, n: &Node, depth: usize, out: &mut Vec<String>) {
    for (key, options) in n.choosing.iter().filter(|(k, _)| !k.ends_with("?source")) {
        let slot = key.rsplit_once('?').and_then(|(_, r)| r.rsplit_once('#')).map_or("", |(s, _)| s);
        out.push(format!("{}{slot}: a model is choosing among {}", "  ".repeat(depth), options.join(", ")));
    }
    for (slot, alt) in &n.slots {
        let (i, of) = prog.slot(slot).map_or((0, 0), |s| (s.alternatives.iter().position(|a| a == alt).map_or(0, |i| i + 1), s.alternatives.len()));
        out.push(format!("{}{slot}: {alt} ({i} of {of})", "  ".repeat(depth)));
    }
}

fn part(prog: &Program, unit: &Unit, at: &[usize], now: Option<i64>, due: &impl Fn(&str) -> Option<u64>, out: &mut Vec<String>) {
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
            format!("requested from {adapter}, send {} of {}{}", n.sends, prog.attempts, left.unwrap_or_default())
        }
        (Status::Done, _) => n.outcome.clone().unwrap_or_else(|| "done".to_string()),
        (Status::Failed, _) => format!("failed ({})", n.outcome.as_deref().unwrap_or("?")),
    };
    let attempt = if n.attempt > 1 { format!(" (attempt {})", n.attempt) } else { String::new() };
    let attrs: Vec<String> = n.attrs.iter().map(|(k, v)| format!("{k} {}", crate::interp::shown(prog, k, v))).collect();
    let attrs = if attrs.is_empty() { String::new() } else { format!(" — {}", attrs.join(", ")) };
    out.push(format!("{}{}{attempt}: {what}{attrs}", "  ".repeat(depth), n.kind));
    sourced(prog, n, now, depth + 1, out);
    slots(prog, n, depth + 1, out);
    for i in 0..n.parts.len() {
        let mut child = at.to_vec();
        child.push(i);
        part(prog, unit, &child, now, due, out);
    }
}
