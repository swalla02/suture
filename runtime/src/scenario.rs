//! Running a scenario against simulated adapters (spec §7.1, appendix B.1).

use suture_lang::ast::Literal;
use suture_lang::ir::Program;
use suture_lang::scenario::{Caller, Refused, Scenario, ScenarioStep};

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
            ScenarioStep::Create { kind, name, attrs, items, by, given, refused } => {
                let created = create(prog, &mut host, kind, name, attrs, items, (!*given).then_some(by.as_ref()));
                refusal(&mut host, name, refused.as_ref(), created, "it was created")
            }
            ScenarioStep::Event { by, target, refused } => {
                let r = host.input_event(Some(by), target);
                refusal(&mut host, target, refused.as_ref(), r, "it was admitted")
            }
            ScenarioStep::Change { by, target, attr, value, refused } => {
                let r = values(prog, &[(attr.clone(), value.clone())]).and_then(|v| host.input_change(Some(by), target, attr, v[0].1.clone()));
                refusal(&mut host, target, refused.as_ref(), r, "it was changed")
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

/// Whether a step was refused as the scenario says it must be.
fn refusal(host: &mut Host, name: &str, refused: Option<&Refused>, r: Result<(), String>, done: &str) -> Result<(), String> {
    let (want, why) = match refused {
        None => return r,
        Some(Refused::Rule(rule)) => (format!("refused by rule {rule}"), format!("is refused by rule {rule}")),
        Some(Refused::Gate) => ("refused at the gate".to_string(), "is refused at the gate".to_string()),
    };
    match r {
        Err(e) if e.match_indices(&want).any(|(i, _)| !e[i + want.len()..].starts_with(|c: char| c.is_alphanumeric() || c == '_')) => {
            host.trace.push(format!("✓ {name} {why}"));
            Ok(())
        }
        Err(e) => Err(format!("expected {name} to be {want}, but: {e}")),
        Ok(()) => Err(format!("expected {name} to be {want}, but {done}")),
    }
}

/// A creation with its list elements, named in the singular (`a line`).
/// `gated` holds the caller of an input; a `given` passes no gate.
fn create(
    prog: &Program,
    host: &mut Host,
    kind: &str,
    name: &str,
    attrs: &[(String, Literal)],
    items: &[(String, Vec<(String, Literal)>)],
    gated: Option<Option<&Caller>>,
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
    match gated {
        Some(by) => host.input_new(by, kind, name, &given, &elements),
        None => host.create_with(kind, name, &given, &elements),
    }
}

fn values(prog: &Program, pairs: &[(String, Literal)]) -> Result<Vec<(String, Val)>, String> {
    pairs
        .iter()
        .map(|(k, l)| {
            let v = match l {
                Literal::Text(s) => Val::Text(s.clone()),
                Literal::Bool(b) => Val::Bool(*b),
                Literal::Count(n) => Val::Count(*n),
                Literal::Duration(s) => Val::Duration(*s),
                // Names of shared entities: the host checks and resolves them.
                Literal::Set(names) => {
                    let mut names = names.clone();
                    names.sort();
                    names.dedup();
                    Val::Set(names)
                }
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
    if in_state(&host.prog, &unit, &at, state) == negated {
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
    if holds(&host.prog, &unit, &[], &c) { Ok(()) } else { Err(format!("expected {text}")) }
}

fn alert(host: &Host, name: &str, negated: bool) -> Result<(), String> {
    let unit = host.unit(name).ok_or_else(|| format!("there is no unit {name}"))?;
    match (unit.alerts.is_empty(), negated) {
        (true, false) => Err(format!("expected an alert for {name}")),
        (false, true) => Err(format!("expected no alert for {name}")),
        _ => Ok(()),
    }
}
