//! Running a scenario against simulated adapters (spec §7.1, appendix B.1).

use suture_lang::ast::Literal;
use suture_lang::ir::Program;
use suture_lang::scenario::{Caller, ModelAnswer, Refused, Scenario, ScenarioStep};

use crate::choice::Scripted;
use std::path::Path;

use crate::host::Host;
use crate::media::{mime_of, Store};
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
    run_with(prog, sc, |_| {})
}

/// Like [`run`], with the host first prepared by `setup`: to register
/// model backends, for instance.
pub fn run_with(prog: &Program, sc: &Scenario, setup: impl FnOnce(&mut Host)) -> Report {
    let mut host = Host::new(prog.clone());
    setup(&mut host);
    let mut failures = vec![];
    // `file "…"` is read beside the scenario's file (appendix V.2.4).
    let base = Path::new(&sc.file).parent().map(Path::to_path_buf).unwrap_or_default();
    for (i, step) in sc.steps.iter().enumerate() {
        let result = match step {
            ScenarioStep::Create { kind, name, attrs, items, by, given, refused } => {
                let created = create(prog, &mut host, &base, kind, name, attrs, items, (!*given).then_some(by.as_ref()));
                refusal(&mut host, name, refused.as_ref(), created, "it was created")
            }
            ScenarioStep::Event { by, target, part, data, refused } => {
                let r = values(prog, &mut host.media, &base, data).and_then(|d| host.input_event(Some(by), target, part.as_deref(), &d));
                refusal(&mut host, target, refused.as_ref(), r, "it was admitted")
            }
            ScenarioStep::Change { by, target, attr, value, refused } => {
                let r = values(prog, &mut host.media, &base, &[(attr.clone(), value.clone())]).and_then(|v| host.input_change(Some(by), target, attr, v[0].1.clone()));
                refusal(&mut host, target, refused.as_ref(), r, "it was changed")
            }
            ScenarioStep::Model { model, .. }
                if !prog.decisions.iter().any(|d| d.model.as_ref() == Some(model) || d.route.as_ref().is_some_and(|r| r.router == *model || r.among.contains(&Some(model.clone()))))
                    && !prog.sources.iter().any(|s| s.model && s.adapter == *model) =>
            {
                Err(format!("no `decide` or source names model \"{model}\""))
            }
            ScenarioStep::Model { model, answers } => {
                let scripted: Result<Vec<Scripted>, String> = answers
                    .iter()
                    .map(|a| {
                        Ok(match a {
                            ModelAnswer::Pick(p) => Scripted::Pick(p.clone()),
                            ModelAnswer::Decline | ModelAnswer::Nothing => Scripted::Decline,
                            ModelAnswer::Late => Scripted::Late,
                            ModelAnswer::Read(l, sure) => Scripted::Read(values(prog, &mut host.media, &base, &[("value".into(), l.clone())])?.remove(0).1, *sure),
                        })
                    })
                    .collect();
                scripted.map(|scripted| {
                    let shown: Vec<String> = scripted
                        .iter()
                        .zip(answers)
                        .map(|(s, a)| match (s, a) {
                            (_, ModelAnswer::Nothing) => "read nothing".into(),
                            (Scripted::Pick(p), _) => format!("pick {p}"),
                            (Scripted::Decline, _) => "decline".into(),
                            (Scripted::Late, _) => "be late".into(),
                            (Scripted::Read(v, sure), _) => {
                                let at = sure.map(|t| format!(" at {}", f64::from(t) / 1000.0)).unwrap_or_default();
                                format!("read {}{at}", v.show(&prog.currency))
                            }
                        })
                        .collect();
                    // `will pick Fast, then Small`
                    let shown = shown.join(", then ").replace(", then pick ", ", then ").replace(", then read ", ", then ");
                    host.trace.push(format!("      model \"{model}\" will {shown}"));
                    host.models.answers(model, scripted);
                })
            }
            ScenarioStep::Answer { adapter, outcome, data, target } => {
                values(prog, &mut host.media, &base, data).and_then(|v| host.answer(adapter, outcome, &v, target.as_deref()))
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
            ScenarioStep::Until { at } => {
                let now = i64::try_from(host.now).unwrap_or(i64::MAX);
                let date = suture_lang::sentence::date_text(*at);
                if *at < now {
                    Err(format!("it is already past {date}: the clock only moves forward"))
                } else {
                    host.trace.push(format!("  … it is {date}"));
                    host.pass(u64::try_from(*at - now).unwrap_or(0))
                }
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
#[allow(clippy::too_many_arguments)] // a scenario's creation, with where its files are
fn create(
    prog: &Program,
    host: &mut Host,
    base: &Path,
    kind: &str,
    name: &str,
    attrs: &[(String, Literal)],
    items: &[(String, Vec<(String, Literal)>)],
    gated: Option<Option<&Caller>>,
) -> Result<(), String> {
    let given = values(prog, &mut host.media, base, attrs)?;
    let mut elements = vec![];
    for (one, pairs) in items {
        let list = prog
            .lists
            .iter()
            .find(|l| l.owner == kind && l.singulars().contains(one))
            .ok_or_else(|| format!("{kind} has no list whose element is called {one}"))?;
        elements.push((list.name.clone(), values(prog, &mut host.media, base, pairs)?));
    }
    match gated {
        Some(by) => host.input_new(by, kind, name, &given, &elements),
        None => host.create_with(kind, name, &given, &elements),
    }
}

fn values(prog: &Program, media: &mut Store, base: &Path, pairs: &[(String, Literal)]) -> Result<Vec<(String, Val)>, String> {
    pairs
        .iter()
        .map(|(k, l)| {
            let v = match l {
                Literal::Text(s) => Val::Text(s.clone()),
                Literal::Bool(b) => Val::Bool(*b),
                Literal::Count(n) => Val::Count(*n),
                Literal::Duration(s) => Val::Duration(*s),
                Literal::Time(t) => Val::Time(*t),
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
                Literal::File(name) => {
                    let mime = mime_of(name).ok_or_else(|| format!("{k}: file \"{name}\": name a .png, .jpg, .gif, .webp, .mp3, .wav, .ogg, .mp4, .webm, .pdf or .txt file"))?;
                    let path = base.join(name);
                    let data = std::fs::read(&path).map_err(|e| format!("{k}: file \"{name}\": {}: {e}", path.display()))?;
                    media.put(mime, data).map_err(|e| format!("{k}: file \"{name}\": {e}"))?
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
