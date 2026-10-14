//! Running a scenario against simulated adapters (spec §7.1, appendix B.1).

use suture_lang::ast::Literal;
use suture_lang::ir::Program;
use suture_lang::scenario::{Scenario, ScenarioStep};

use crate::host::Host;
use crate::interp::in_state;
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
            ScenarioStep::Create { kind, name, attrs } => values(prog, attrs).and_then(|v| host.create(kind, name, &v)),
            ScenarioStep::Answer { adapter, outcome, data, target } => {
                values(prog, data).and_then(|v| host.answer(adapter, outcome, &v, target.as_deref()))
            }
            ScenarioStep::Expect { name, state, negated } => {
                let r = expect(&host, name, state, *negated);
                let mark = if r.is_ok() { "✓" } else { "✗" };
                host.trace.push(format!("{mark} expect {name} is {}{state}", if *negated { "not " } else { "" }));
                r
            }
        };
        if let Err(e) = result {
            failures.push(format!("step {}: {e}", i + 1));
        }
    }
    Report { title: sc.title.clone(), host, failures }
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

fn expect(host: &Host, name: &str, state: &str, negated: bool) -> Result<(), String> {
    let unit = host.unit(name).ok_or_else(|| format!("there is no unit {name}"))?;
    if in_state(&host.prog, &unit.root, &[], state) == negated {
        return Err(format!("expected {name} is {}{state}", if negated { "not " } else { "" }));
    }
    Ok(())
}
