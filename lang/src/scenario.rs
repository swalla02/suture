//! Scenarios (spec §7.1, appendix B.2): `when a new …`, `… answers …` and
//! `expect …`, one per indented line under `scenario "title":`.

use crate::ast::Literal;
use crate::lex::{statements, Cursor, Error, Tok};
use crate::sentence::literal;

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
        let name = c.word()?;
        c.expect_word("is")?;
        let negated = c.eat_word("not");
        return Ok(ScenarioStep::Expect { name, state: c.word()?, negated });
    }
    if !(c.eat_word("when") || c.eat_word("and")) {
        return Err(c.error(format!("expected `when`, `and` or `expect`, found {}", c.found())));
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
    fn scenario_errors_say_what_was_expected() {
        let e = parse_scenarios("o.scenario", "scenario \"x\":\n  then O1 is paid\n").unwrap_err();
        assert!(e.msg.contains("expected `when`, `and` or `expect`"), "{e}");
    }
}
