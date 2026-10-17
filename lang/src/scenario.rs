//! Scenarios (spec §7.1, appendix B.2): `when a new …`, `… answers …` and
//! `expect …`, one per indented line under `scenario "title":`.

use crate::ast::Literal;
use crate::lex::{statements, Cursor, Error, Tok};
use crate::parse::duration;
use crate::sentence::{literal, state_word};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scenario {
    pub title: String,
    pub steps: Vec<ScenarioStep>,
    pub line: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScenarioStep {
    /// `when a new Order O1 with country France and total 120 EUR`
    Create { kind: String, name: String, attrs: Vec<(String, Literal)> },
    /// `and forms answers filled with licence "L-77" [for O1]`
    Answer { adapter: String, outcome: String, data: Vec<(String, Literal)>, target: Option<String> },
    /// `expect O1 is [not] settled`
    Expect { name: String, state: String, negated: bool },
    /// `expect O1's Payment is [not] timed out`: the unit's current part of that kind.
    ExpectPart { name: String, part: String, state: String, negated: bool },
    /// `when 3 days pass`: the clock moves forward (appendix C.1).
    Pass { secs: u64 },
    /// `expect [an|no] alert for O1`
    Alert { name: String, negated: bool },
    /// `expect the number of P1's Shipments is 3`: any condition, with the
    /// unit's name standing for the subject (appendix E.2.9).
    ExpectCond { name: String, cond: crate::ast::Cond, text: String },
}

pub fn parse_scenarios(file: &str, src: &str) -> Result<Vec<Scenario>, Error> {
    let mut out = vec![];
    for st in statements(file, src)? {
        let mut c = Cursor::new(&st.lines[0], &st.file, st.line);
        c.expect_word("scenario")?;
        let title = match c.peek() {
            Some(Tok::Str(s)) => s.clone(),
            _ => return Err(c.error(format!("expected the scenario's title in quotes, found {}", c.found()))),
        };
        c.next();
        c.expect_sym(':')?;
        c.expect_end()?;
        let mut steps = vec![];
        for line in &st.lines[1..] {
            let mut c = Cursor::new(line, &st.file, st.line);
            steps.push(step(&mut c)?);
            c.expect_end()?;
        }
        out.push(Scenario { title, steps, line: st.line });
    }
    Ok(out)
}

fn step(c: &mut Cursor) -> Result<ScenarioStep, Error> {
    if c.eat_word("expect") {
        let alert = if c.eat_word("no") {
            Some(true)
        } else if c.eat_word("an") || c.eat_word("a") {
            Some(false)
        } else {
            None
        };
        if let Some(negated) = alert {
            c.expect_word("alert")?;
            c.expect_word("for")?;
            return Ok(ScenarioStep::Alert { name: c.word()?, negated });
        }
        let saved = c.clone();
        if let Ok(short) = short_expect(c) {
            if c.at_end() {
                return Ok(short);
            }
        }
        *c = saved;
        return expect_cond(c);
    }
    if !(c.eat_word("when") || c.eat_word("and")) {
        return Err(c.error(format!("expected `when`, `and` or `expect`, found {}", c.found())));
    }
    if matches!(c.peek(), Some(Tok::Num(_))) {
        let secs = duration(c)?;
        if !(c.eat_word("pass") || c.eat_word("passes")) {
            return Err(c.error(format!("expected `pass` after a duration, found {}", c.found())));
        }
        return Ok(ScenarioStep::Pass { secs });
    }
    if c.eat_word("a") || c.eat_word("an") {
        c.expect_word("new")?;
        let kind = c.kind_name()?;
        let name = c.word()?;
        let attrs = if c.eat_word("with") { pairs(c)? } else { vec![] };
        return Ok(ScenarioStep::Create { kind, name, attrs });
    }
    let adapter = c.word()?;
    c.expect_word("answers")?;
    let outcome = c.word()?;
    let data = if c.eat_word("with") { pairs(c)? } else { vec![] };
    let target = if c.eat_word("for") { Some(c.word()?) } else { None };
    Ok(ScenarioStep::Answer { adapter, outcome, data, target })
}

/// `expect O1 is [not] paid`, `expect O1's Payment is [not] timed out`.
fn short_expect(c: &mut Cursor) -> Result<ScenarioStep, Error> {
    let name = c.word()?;
    let part = if c.eat_poss() { Some(c.kind_name()?) } else { None };
    c.expect_word("is")?;
    let negated = c.eat_word("not");
    let state = state_word(c)?;
    Ok(match part {
        Some(part) => ScenarioStep::ExpectPart { name, part, state, negated },
        None => ScenarioStep::Expect { name, state, negated },
    })
}

/// Any condition about one unit: its name, followed by `'s` or by `is` or
/// `has`, stands where a subject would, so `P1's` reads as `its`.
fn expect_cond(c: &mut Cursor) -> Result<ScenarioStep, Error> {
    let toks: Vec<Tok> = c.by_ref().cloned().collect();
    let text = crate::parse::text(&toks);
    let is_name = |i: usize| {
        matches!(&toks[i], Tok::Word(w) if w.starts_with(char::is_uppercase))
            && (i == 0 || toks[i - 1] != Tok::Poss)
            && matches!(toks.get(i + 1), Some(Tok::Poss) | Some(Tok::Word(_)))
            && match toks.get(i + 1) {
                Some(Tok::Word(w)) => w == "is" || w == "has",
                _ => true,
            }
    };
    let Some(at) = (0..toks.len()).find(|&i| is_name(i)) else {
        return Err(c.error(format!("`expect {text}`: name the unit it is about, as in `expect P1's total is 90 EUR`")));
    };
    let Tok::Word(name) = toks[at].clone() else { unreachable!("is_name checked it is a word") };
    let mut rewritten = toks.clone();
    if rewritten.get(at + 1) == Some(&Tok::Poss) {
        rewritten.splice(at..at + 2, [Tok::Word("its".into())]);
    } else {
        rewritten[at] = Tok::Word("it".into());
    }
    if (0..rewritten.len()).any(|i| matches!(&rewritten[i], Tok::Word(w) if *w == name)) {
        return Err(c.error(format!("`expect {text}`: an expectation is about one unit, {name}")));
    }
    let mut inner = Cursor::new(&rewritten, "", 0);
    let cond = crate::sentence::condition(&mut inner).map_err(|e| c.error(format!("`expect {text}`: {}", e.msg)))?;
    inner.expect_end().map_err(|e| c.error(format!("`expect {text}`: {}", e.msg)))?;
    Ok(ScenarioStep::ExpectCond { name, cond, text })
}

/// `a 1, b "x" and c 3 EUR`. A bare word is text.
fn pairs(c: &mut Cursor) -> Result<Vec<(String, Literal)>, Error> {
    let mut out = vec![];
    loop {
        let name = c.word()?;
        let value = match literal(c)? {
            Some(l) => l,
            None => Literal::Text(c.word()?),
        };
        out.push((name, value));
        if !(c.eat_sym(',') || c.eat_word("and")) {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eur(cents: i64) -> Literal {
        Literal::Money { cents, currency: "EUR".into() }
    }

    #[test]
    fn scenarios_parse() {
        let src = "scenario \"a French order\":\n  when   a new Order O1 with country France and total 120 EUR\n  and    forms answers filled with value 100 EUR and contents alcohol for O1\n  and    carriers answers delivered\n  expect O1 is not cancelled\n";
        let s = parse_scenarios("o.scenario", src).unwrap();
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].title, "a French order");
        let country = ("country".to_string(), Literal::Text("France".into()));
        assert_eq!(
            s[0].steps[0],
            ScenarioStep::Create { kind: "Order".into(), name: "O1".into(), attrs: vec![country, ("total".into(), eur(12000))] }
        );
        assert_eq!(
            s[0].steps[1],
            ScenarioStep::Answer {
                adapter: "forms".into(),
                outcome: "filled".into(),
                data: vec![("value".into(), eur(10000)), ("contents".into(), Literal::Text("alcohol".into()))],
                target: Some("O1".into()),
            }
        );
        assert_eq!(s[0].steps[3], ScenarioStep::Expect { name: "O1".into(), state: "cancelled".into(), negated: true });
    }

    #[test]
    fn time_and_alert_steps_parse() {
        let src = "scenario \"late\":\n  when   3 days pass\n  and    1 second passes\n  expect an alert for O1\n  expect no alert for O2\n  expect O1's Payment is timed out\n";
        let s = &parse_scenarios("o.scenario", src).unwrap()[0];
        assert_eq!(s.steps[0], ScenarioStep::Pass { secs: 259_200 });
        assert_eq!(s.steps[1], ScenarioStep::Pass { secs: 1 });
        assert_eq!(s.steps[2], ScenarioStep::Alert { name: "O1".into(), negated: false });
        assert_eq!(s.steps[3], ScenarioStep::Alert { name: "O2".into(), negated: true });
        assert_eq!(s.steps[4], ScenarioStep::ExpectPart { name: "O1".into(), part: "Payment".into(), state: "timed out".into(), negated: false });
    }

    #[test]
    fn scenario_errors_say_what_was_expected() {
        let e = parse_scenarios("o.scenario", "scenario \"x\":\n  then O1 is paid\n").unwrap_err();
        assert!(e.msg.contains("expected `when`, `and` or `expect`"), "{e}");
    }
}
