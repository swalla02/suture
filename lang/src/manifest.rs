//! The manifest (appendix N.2.7): what each adapter receives, with its
//! marks, and may answer; the roles; and what is admitted. It is derived
//! from the program, for the security review of a deploy.

use crate::ir::{Mark, Path, Program, Step};

/// The kind and attribute a send reads, walking from `kind`.
fn field(p: &Program, kind: &str, path: &Path) -> Option<(String, String)> {
    let mut cur = kind.to_string();
    let (last, steps) = path.steps.split_last()?;
    for s in steps {
        cur = match s {
            Step::Part(k) | Step::Bound(k) => k.clone(),
            Step::Owner => p.kind(&cur)?.owner.as_ref()?.kind.clone(),
            Step::Ref(a) => p.kind(&cur)?.attr(a)?.of.clone()?,
            _ => return None,
        };
    }
    match last {
        Step::Attr(a) => Some((cur, a.clone())),
        _ => None,
    }
}

fn sends(p: &Program, kind: &str, sends: &[(String, Path)]) -> Vec<String> {
    sends
        .iter()
        .map(|(name, path)| match field(p, kind, path) {
            Some((k, a)) => {
                let mark = match p.kind(&k).and_then(|x| x.attr(&a)).and_then(|x| x.mark) {
                    Some(Mark::Secret) => " (secret)",
                    Some(Mark::Personal) => " (personal)",
                    None => "",
                };
                format!("{k}.{a}{mark}")
            }
            None => name.clone(),
        })
        .collect()
}

/// The manifest as text.
pub fn manifest(p: &Program) -> String {
    let mut out = format!("manifest of {} v{}\n\n{}", p.module, p.version, adapters(p));
    if !p.roles.is_empty() {
        let roles: Vec<String> = p.roles.iter().map(|r| r.kind.as_ref().map_or_else(|| r.name.clone(), |k| format!("{} (a {k})", r.name))).collect();
        out.push_str(&format!("\nroles     {}\n", roles.join(", ")));
    }
    let mut seen = vec![];
    for a in &p.admits {
        if !seen.contains(&&a.source) {
            out.push_str(&format!("{}{}\n", if seen.is_empty() { "admitted  " } else { "          " }, a.source));
            seen.push(&a.source);
        }
    }
    out
}

/// What each adapter receives and may answer, and the marked attributes.
pub fn adapters(p: &Program) -> String {
    // A source a model reads names a model, not an adapter (appendix AF).
    let mut adapters: Vec<&str> = p.effects.iter().map(|e| e.adapter.as_str()).chain(p.sources.iter().filter(|s| !s.model).map(|s| s.adapter.as_str())).collect();
    adapters.sort();
    adapters.dedup();
    let mut out = "adapters:\n".to_string();
    for a in adapters {
        let (mut receives, mut answers) = (vec![], vec![]);
        for e in p.effects.iter().filter(|e| e.adapter == a) {
            receives.extend(sends(p, &e.kind, &e.sends));
            let data: Vec<String> = e.data.iter().map(|(n, _)| n.clone()).collect();
            let success = if data.is_empty() { e.success.clone() } else { format!("{} ({})", e.success, data.join(", ")) };
            let mut outcomes = vec![success];
            outcomes.extend(e.failures.iter().cloned());
            answers.push(format!("{} {}: {}", e.verb, e.kind, outcomes.join(" | ")));
        }
        for s in p.sources.iter().filter(|s| s.adapter == a && !s.model) {
            receives.extend(sends(p, &s.kind, &s.sends));
            let mut outcomes = vec![format!("{} ({})", s.success, s.attr)];
            outcomes.extend(s.failures.iter().cloned());
            answers.push(format!("{}'s {}: {}", s.kind, s.attr, outcomes.join(" | ")));
        }
        receives.dedup();
        out.push_str(&format!("  {a:<10} receives  {}\n", if receives.is_empty() { "nothing".into() } else { receives.join(", ") }));
        for (i, x) in answers.iter().enumerate() {
            out.push_str(&format!("  {:<10} {}  {x}\n", "", if i == 0 { "answers " } else { "        " }));
        }
    }
    let marked: Vec<String> = p
        .kinds
        .iter()
        .flat_map(|k| k.attrs.iter().filter_map(move |a| a.mark.map(|m| format!("{}.{} ({})", k.name, a.name, if m == Mark::Secret { "secret" } else { "personal" }))))
        .collect();
    if !marked.is_empty() {
        out.push_str(&format!("marked:   {}\n", marked.join(", ")));
    }
    out
}
