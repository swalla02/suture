//! Explanations (spec §5.6): why a unit is where it is, read from its state.

use suture_lang::ir::{Otherwise, Program};

use crate::interp::{duration, in_state, request_key};
use crate::state::{Fetch, Node, Status, Unit};

/// One line for the unit, one per goal, and one per part, indented by depth.
/// `due` gives the seconds left before a pending request's deadline, by key.
pub fn explain(prog: &Program, unit: &Unit, due: impl Fn(&str) -> Option<u64>) -> Vec<String> {
    explain_at(prog, unit, None, due)
}

/// The root kind a kind is a part of, or the kind itself.
fn root_of(prog: &Program, kind: &str) -> String {
    let mut k = kind.to_string();
    while let Some(o) = prog.kind(&k).and_then(|x| x.owner.as_ref()) {
        k = o.kind.clone();
    }
    k
}

/// [`explain`] at the clock `now`, which also gives each sourced value's age.
pub fn explain_at(prog: &Program, unit: &Unit, now: Option<i64>, due: impl Fn(&str) -> Option<u64>) -> Vec<String> {
    let attrs: Vec<String> = unit.root.attrs.iter().map(|(k, v)| format!("{k} {}", crate::interp::shown(prog, k, v))).collect();
    let mut out = vec![format!("{} {} ({})", unit.root.kind, unit.label, attrs.join(", "))];
    for (i, g) in prog.goals.iter().enumerate().filter(|(_, g)| root_of(prog, &g.kind) == unit.root.kind) {
        let nodes = crate::interp::goal_nodes(prog, unit, g);
        if nodes.is_empty() {
            out.push(format!("  goal does not apply: {}", g.source));
            continue;
        }
        let missing: Vec<&str> = g.states.iter().filter(|s| nodes.iter().any(|at| !in_state(prog, unit, at, s))).map(String::as_str).collect();
        out.push(if missing.is_empty() {
            format!("  goal met: {}", g.source)
        } else if unit.alerts.contains(&(i as u32)) {
            format!("  goal late, alert raised: {} (not yet {})", g.source, missing.join(", "))
        } else {
            format!("  goal not met yet: {} (not yet {})", g.source, missing.join(", "))
        });
    }
    totals(prog, unit, &mut out);
    sourced(prog, &unit.root, now, 1, &mut out);
    out.extend(crate::interp::unasked_lines(prog, unit, &[]).into_iter().map(|l| format!("  {} {l}", unit.root.kind)));
    slots(prog, &unit.root, 1, &mut out);
    for (i, _) in unit.root.parts.iter().enumerate() {
        part(prog, unit, &[i], now, &due, &mut out);
    }
    out
}

/// One line per running total other units keep at this unit, outside
/// periods (appendix AI.2.2): `its total days of Applications that are approved: 4`.
fn totals(prog: &Program, unit: &Unit, out: &mut Vec<String>) {
    let mut seen: Vec<&str> = vec![];
    for t in prog.bindings.iter().filter(|b| b.target == unit.root.kind && b.key_attr.is_empty()).filter_map(|b| b.taking.as_ref()) {
        if t.period.is_some() || seen.contains(&t.name.as_str()) {
            continue;
        }
        seen.push(&t.name);
        let v = unit.taken.get(&t.name).cloned().unwrap_or_else(|| crate::interp::zero(t.ty));
        let what = if t.name.starts_with("number of ") { t.name.clone() } else { format!("total {}", t.name) };
        out.push(format!("  its {what}: {}", v.show(&prog.currency)));
    }
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
        (Status::Idle, Some(rule)) => format!("waiting: rule {rule}{}", awaited(prog, unit, now, rule)),
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
    out.extend(crate::interp::unasked_lines(prog, unit, at).into_iter().map(|l| format!("{}{} {l}", "  ".repeat(depth + 1), n.kind)));
    slots(prog, n, depth + 1, out);
    for i in 0..n.parts.len() {
        let mut child = at.to_vec();
        child.push(i);
        part(prog, unit, &child, now, due, out);
    }
}

/// `, on Order's credit (asking bureau, send 1 of 3)`: the sourced facts a
/// waiting rule reads that are not in yet (appendix X.2.1), on the nodes the
/// runtime fetches them for: every live node of the source's kind. A fact
/// that is in and fresh for the rule is not named: then the rule waits on
/// the fact's value.
fn awaited(prog: &Program, unit: &Unit, now: Option<i64>, rule: &str) -> String {
    let Some(r) = prog.rules.iter().find(|r| r.name == rule) else { return String::new() };
    let mut on: Vec<String> = vec![];
    for (kind, attr) in &r.sources {
        let bound = r.fresh.iter().find(|(a, _)| a == attr).map(|(_, secs)| *secs);
        for pos in crate::interp::positions(&unit.root) {
            let n = pos.iter().fold(&unit.root, |n, &i| &n.parts[i]);
            if n.kind != *kind || n.status == Status::Failed {
                continue;
            }
            let f = n.fetched.get(attr).cloned().unwrap_or_default();
            let Some(src) = prog.sources_of(kind, attr).into_iter().find(|s| s.adapter == f.via).or_else(|| prog.sources_of(kind, attr).into_iter().next()) else { continue };
            let picking = n.choosing.keys().any(|k| k.ends_with("?source") && k.contains(&format!("~{attr}#")));
            let state = if picking {
                "a model is picking its source".to_string()
            } else if f.sends > 0 {
                format!("asking {}, send {} of {}", src.adapter, f.sends, prog.attempts)
            } else if f.failed {
                format!("{} failed", src.adapter)
            } else if f.at.is_none() {
                "not fetched yet".to_string()
            } else if let Some(t) = now.filter(|t| !f.fresh(*t, bound)) {
                format!("{} old, too old for the rule", duration((t - f.at.unwrap_or(t)).max(0) as u64))
            } else {
                continue;
            };
            let line = format!("{kind}'s {attr} ({state})");
            if !on.contains(&line) {
                on.push(line);
            }
        }
    }
    if on.is_empty() { String::new() } else { format!(", on {}", on.join(" and ")) }
}
