//! Running a scenario against simulated adapters (spec §7.1, appendix B.1).

use suture_lang::ast::Literal;
use suture_lang::ir::Program;
use suture_lang::scenario::{Scenario, ScenarioStep};

use crate::host::Host;
use crate::interp::{duration, holds, in_state};
use crate::state::Val;

/// What a scenario did: its host, with the trace, and the steps that failed.
pub struct Report {
    pub title: String,
    pub host: Host,
    pub failures: Vec<String>,
}

impl Report {
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }
}

/// Runs every step in order. A step that cannot apply fails the report, and
/// the run continues with the next step.
pub fn run(prog: &Program, sc: &Scenario) -> Report {
    let mut host = Host::new(prog.clone());
    let mut failures = vec![];
    for (i, step) in sc.steps.iter().enumerate() {
        let result = match step {
            ScenarioStep::Create { kind, name, attrs, items, refused } => {
                let created = create(prog, &mut host, kind, name, attrs, items);
                match (refused, created) {
                    (None, r) => r,
                    (Some(rule), Err(e)) if e.contains(&format!("refused by rule {rule}")) => {
                        host.trace.push(format!("✓ {name} is refused by rule {rule}"));
                        Ok(())
                    }
                    (Some(rule), Err(e)) => Err(format!("expected {name} to be refused by rule {rule}, but: {e}")),
                    (Some(rule), Ok(())) => Err(format!("expected {name} to be refused by rule {rule}, but it was created")),
                }
            }
            ScenarioStep::Answer { adapter, outcome, data, target } => {
                values(prog, data).and_then(|v| host.answer(adapter, outcome, &v, target.as_deref()))
            }
            ScenarioStep::Expect { name, state, negated } => {
                let r = expect(&host, name, None, state, *negated);
                checked(&mut host, r, format!("expect {name} is {}{state}", not(*negated)))
            }
            ScenarioStep::ExpectPart { name, part, state, negated } => {
                let r = expect(&host, name, Some(part), state, *negated);
                checked(&mut host, r, format!("expect {name}'s {part} is {}{state}", not(*negated)))
            }
            ScenarioStep::Alert { name, negated } => {
                let r = alert(&host, name, *negated);
                checked(&mut host, r, format!("expect {} alert for {name}", if *negated { "no" } else { "an" }))
            }
            ScenarioStep::ExpectCond { name, cond, text } => {
                let r = expect_cond(&host, name, cond, text);
                checked(&mut host, r, format!("expect {text}"))
            }
            ScenarioStep::Pass { secs } => {
                host.trace.push(format!("  … {} pass", duration(*secs)));
                host.pass(*secs)
            }
        };
        if let Err(e) = result {
            failures.push(format!("step {}: {e}", i + 1));
        }
    }
    Report { title: sc.title.clone(), host, failures }
}

/// A creation with its list elements, named in the singular (`a line`).
fn create(
    prog: &Program,
    host: &mut Host,
    kind: &str,
    name: &str,
    attrs: &[(String, Literal)],
    items: &[(String, Vec<(String, Literal)>)],
) -> Result<(), String> {
    let given = values(prog, attrs)?;
    let mut elements = vec![];
    for (one, pairs) in items {
        let list = prog
            .lists
            .iter()
            .find(|l| l.owner == kind && l.singulars().contains(one))
            .ok_or_else(|| format!("{kind} has no list whose element is called {one}"))?;
        elements.push((list.name.clone(), values(prog, pairs)?));
    }
    host.create_with(kind, name, &given, &elements)
}

fn values(prog: &Program, pairs: &[(String, Literal)]) -> Result<Vec<(String, Val)>, String> {
    pairs
        .iter()
        .map(|(k, l)| {
            let v = match l {
                Literal::Text(s) => Val::Text(s.clone()),
                Literal::Bool(b) => Val::Bool(*b),
                Literal::Count(n) => Val::Count(*n),
                Literal::Money { cents, currency } if *currency == prog.currency => Val::Money(*cents),
                Literal::Money { currency, .. } => {
                    return Err(format!("{k}: an amount in {currency} is not in this module's currency, {}", prog.currency));
                }
            };
            Ok((k.clone(), v))
        })
        .collect()
}

fn not(negated: bool) -> &'static str {
    if negated { "not " } else { "" }
}

/// Traces an expectation with ✓ or ✗.
fn checked(host: &mut Host, r: Result<(), String>, what: String) -> Result<(), String> {
    host.trace.push(format!("{} {what}", if r.is_ok() { "✓" } else { "✗" }));
    r
}

/// The unit, or its current part of kind `part`, is (not) in `state`.
fn expect(host: &Host, name: &str, part: Option<&str>, state: &str, negated: bool) -> Result<(), String> {
    let unit = host.unit(name).ok_or_else(|| format!("there is no unit {name}"))?;
    let (kind, at) = match part {
        None => (unit.root.kind.clone(), vec![]),
        Some(k) => match unit.root.parts.iter().rposition(|n| n.kind == k) {
            Some(i) => (k.to_string(), vec![i]),
            None => return Err(format!("{name} has no {k}")),
        },
    };
    let states = host.prog.states(&kind);
    if !states.iter().any(|s| s == state) {
        return Err(format!("{kind} has no state {state}; its states are {}", states.join(", ")));
    }
    if in_state(&host.prog, &unit.root, &at, state) == negated {
        let whose = part.map(|k| format!("{name}'s {k}")).unwrap_or_else(|| name.to_string());
        return Err(format!("expected {whose} is {}{state}", not(negated)));
    }
    Ok(())
}

/// Any condition about the unit, checked against the program like a sentence.
fn expect_cond(host: &Host, name: &str, cond: &suture_lang::ast::Cond, text: &str) -> Result<(), String> {
    let unit = host.unit(name).ok_or_else(|| format!("there is no unit {name}"))?;
    let c = suture_lang::check::condition(&host.prog, &unit.root.kind, cond)
        .map_err(|es| es.iter().map(|e| e.msg.clone()).collect::<Vec<_>>().join("; "))?;
    if holds(&host.prog, &unit.root, &[], &c) { Ok(()) } else { Err(format!("expected {text}")) }
}

fn alert(host: &Host, name: &str, negated: bool) -> Result<(), String> {
    let unit = host.unit(name).ok_or_else(|| format!("there is no unit {name}"))?;
    match (unit.alerts.is_empty(), negated) {
        (true, false) => Err(format!("expected an alert for {name}")),
        (false, true) => Err(format!("expected no alert for {name}")),
        _ => Ok(()),
    }
}
