//! Scenarios (spec §7.1, appendix B.2): `when a new …`, `… answers …` and
//! `expect …`, one per indented line under `scenario "title":`.

use crate::ast::Literal;
use crate::lex::{statements, Cursor, Error, Tok};
use crate::parse::duration;
use crate::sentence::{literal, state_word};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scenario {
    /// The file it was read from: `file "…"` is read beside it (appendix V.2.4).
    pub file: String,
    pub title: String,
    pub steps: Vec<ScenarioStep>,
    pub line: usize,
}

/// A scripted model's answer in a scenario.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModelAnswer {
    /// `picks Fast [at 0.9]`, with its confidence in thousandths (appendix AK.2.3).
    Pick(String, Option<u16>),
    Decline,
    Late,
    /// `reads 1200 EUR [at 0.4]`: a value, with its confidence in thousandths (appendix AF).
    Read(Literal, Option<u16>),
    /// `reads nothing`
    Nothing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScenarioStep {
    /// `when a new Order O1 with country France and total 120 EUR`, or
    /// `given a Stock S1 with …` for a shared entity (appendix F.2.1), then
    /// `and a line with …` for each list element, and optionally
    /// `is refused by rule …` or `is refused at the gate` when it must be
    /// refused. `when customer Ada places a new Order …` names its caller
    /// (appendix G.2.1); a `given` (and every `and` after it) is the world,
    /// which never passes the gate.
    Create {
        kind: String,
        name: String,
        attrs: Vec<(String, Literal)>,
        items: Vec<(String, Vec<(String, Literal)>)>,
        by: Option<Caller>,
        given: bool,
        refused: Option<Refused>,
    },
    /// `when support Sam cancels O1 [is refused …]`: an admitted event (appendix G.2.4).
    /// `customer Ada cancels O1 with reason "late"`: data as in an `admit`'s `with`.
    /// `customer Ada cancels O1's Shipment`: an event to the unit's part (appendix R.2.2).
    Event { by: Caller, target: String, part: Option<String>, data: Vec<(String, Literal)>, refused: Option<Refused> },
    /// `when staff Tom changes S1's units to 3 [is refused …]` (appendix G.2.5).
    Change { by: Caller, target: String, attr: String, value: Literal, refused: Option<Refused> },
    /// `and forms answers filled with licence "L-77" [for O1]`
    Answer { adapter: String, outcome: String, data: Vec<(String, Literal)>, target: Option<String> },
    /// `when model "carriers@1" picks Hermes[, then DHL]`, `… declines`,
    /// `… is late`: the stand-in's next answers (appendix J.2.7, K.1).
    Model { model: String, answers: Vec<ModelAnswer> },
    /// `expect O1 is [not] settled`
    Expect { name: String, state: String, negated: bool },
    /// `expect O1's Payment is [not] timed out`: the unit's current part of that kind.
    ExpectPart { name: String, part: String, state: String, negated: bool },
    /// `when 3 days pass`: the clock moves forward (appendix C.1).
    Pass { secs: u64 },
    /// `when it is 2026-03-01`: the clock moves forward to that time
    /// (appendix AB.2.3).
    Until { at: i64 },
    /// `expect [an|no] alert for O1`
    Alert { name: String, negated: bool },
    /// `expect the number of P1's Shipments is 3`: any condition, with the
    /// unit's name standing for the subject (appendix E.2.9).
    ExpectCond { name: String, cond: crate::ast::Cond, text: String },
}

/// `customer Ada places …`: the caller's role and name, and the verb as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Caller {
    pub role: String,
    pub name: String,
    pub verb: String,
}

/// How a step must be refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    Rule(String),
    Gate,
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
        // A line that adds a list element (`and a member with …`) or says the
        // creation is refused continues the step above it.
        let mut lines: Vec<(Vec<Tok>, usize)> = vec![];
        for (line, &at) in st.lines[1..].iter().zip(&st.at[1..]) {
            let word = |i: usize| match line.get(i) {
                Some(Tok::Word(w)) => w.as_str(),
                _ => "",
            };
            let element = word(2).starts_with(char::is_lowercase) && word(2) != "new";
            let continues = (word(0) == "and" && (word(1) == "a" || word(1) == "an") && element) || (word(0) == "is" && word(1) == "refused");
            match lines.last_mut() {
                Some((prev, _)) if continues => prev.extend(line.iter().cloned()),
                _ => lines.push((line.clone(), at)),
            }
        }
        // Errors name the line they are on; an `and` after an `expect` is
        // another expect.
        let (mut steps, mut given) = (vec![], false);
        for (line, at) in &lines {
            let mut c = Cursor::new(line, &st.file, *at);
            let expecting = matches!(steps.last(), Some(ScenarioStep::Expect { .. } | ScenarioStep::ExpectPart { .. } | ScenarioStep::Alert { .. } | ScenarioStep::ExpectCond { .. }));
            if expecting && c.is_word("and") {
                let mut e = c.clone();
                e.next();
                if let Ok(s) = expect_rest(&mut e)
                    && e.at_end() {
                    steps.push(s);
                    continue;
                }
            }
            if let Some(many) = many(&c, given)? {
                given = matches!(many.first(), Some(ScenarioStep::Create { given: true, .. }));
                steps.extend(many);
                continue;
            }
            steps.push(if c.eat_word("expect") { expect_rest(&mut c)? } else { step_input(&mut c, &mut given)? });
            c.expect_end()?;
        }
        out.push(Scenario { file: file.to_string(), title, steps, line: st.line });
    }
    Ok(out)
}

/// What follows `expect`.
fn expect_rest(c: &mut Cursor) -> Result<ScenarioStep, Error> {
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
    if let Ok(short) = short_expect(c)
        && c.at_end() {
        return Ok(short);
    }
    *c = saved;
    expect_cond(c)
}

fn step_input(c: &mut Cursor, given: &mut bool) -> Result<ScenarioStep, Error> {
    if c.eat_word("given") {
        *given = true;
    } else if c.eat_word("when") {
        *given = false;
    } else if !c.eat_word("and") {
        return Err(c.error(format!("expected `given`, `when`, `and` or `expect`, found {}", c.found())));
    }
    if c.is_word("it") && word_at(c, 1, "is") {
        c.next();
        c.next();
        return match literal(c)? {
            Some(Literal::Time(at)) if c.at_end() => Ok(ScenarioStep::Until { at }),
            _ => Err(c.error("`it is` takes a date, as in `when it is 2026-03-01` or `when it is 2026-03-01 14:30`")),
        };
    }
    if matches!(c.peek(), Some(Tok::Num(_))) {
        let secs = duration(c)?;
        if !(c.eat_word("pass") || c.eat_word("passes")) {
            return Err(c.error(format!("expected `pass` after a duration, found {}", c.found())));
        }
        return Ok(ScenarioStep::Pass { secs });
    }
    if c.is_word("model")
        && let Some(Tok::Str(model)) = c.peek_at(1) {
        let model = model.clone();
        c.next();
        c.next();
        let answers = if c.eat_word("reads") {
            let mut reads = vec![read(c)?];
            while c.eat_sym(',') {
                c.expect_word("then")?;
                reads.push(read(c)?);
            }
            reads
        } else if c.eat_word("declines") {
            vec![ModelAnswer::Decline]
        } else if c.eat_word("is") {
            c.expect_word("late")?;
            vec![ModelAnswer::Late]
        } else {
            c.expect_word("picks")?;
            // A router picks a model by its quoted name (appendix AJ.2.4).
            let pick = |c: &mut Cursor| -> Result<ModelAnswer, Error> {
                let name = match c.peek() {
                    Some(Tok::Str(m)) => {
                        let m = m.clone();
                        c.next();
                        m
                    }
                    _ => c.word()?,
                };
                Ok(ModelAnswer::Pick(name, confidence(c)?))
            };
            let mut picks = vec![pick(c)?];
            while c.eat_sym(',') {
                c.expect_word("then")?;
                picks.push(pick(c)?);
            }
            picks
        };
        c.expect_end()?;
        return Ok(ScenarioStep::Model { model, answers });
    }
    let by = if c.is_word("a") || c.is_word("an") || matches!(c.peek_at(1), Some(Tok::Word(w)) if w == "answers") {
        None
    } else {
        // `customer Ada places a new Order O1 …`
        let role = c.word()?;
        let name = c.word()?;
        let verb = c.word()?;
        if *given {
            return Err(c.error("a `given` is the world, with no caller; write `when` for a caller's input"));
        }
        Some(Caller { role, name, verb })
    };
    if let Some(by) = by.clone().filter(|b| b.verb == "changes") {
        let target = c.word()?;
        if !c.eat_poss() {
            return Err(c.error(format!("`changes {target}`: name the attribute, as in `changes {target}'s units to 3`")));
        }
        let attr = c.word()?;
        c.expect_word("to")?;
        let value = match literal(c)? {
            Some(l) => l,
            None => Literal::Text(c.word()?),
        };
        let refused = refusal(c)?;
        return Ok(ScenarioStep::Change { by, target, attr, value, refused });
    }
    if let Some(by) = by.clone().filter(|_| !(c.is_word("a") || c.is_word("an"))) {
        let target = c.word()?;
        let part = if c.eat_poss() { Some(c.kind_name()?) } else { None };
        let data = if c.eat_word("with") { pairs(c)? } else { vec![] };
        let refused = refusal(c)?;
        return Ok(ScenarioStep::Event { by, target, part, data, refused });
    }
    if by.is_some() || c.is_word("a") || c.is_word("an") {
        if !(c.eat_word("a") || c.eat_word("an")) {
            return Err(c.error(format!("expected `a new ⟨Kind⟩`, found {}", c.found())));
        }
        c.eat_word("new");
        let kind = c.kind_name()?;
        let name = c.word()?;
        let (attrs, items) = values_and_items(c)?;
        let refused = refusal(c)?;
        return Ok(ScenarioStep::Create { kind, name, attrs, items, by, given: *given, refused });
    }
    let adapter = c.word()?;
    c.expect_word("answers")?;
    let outcome = c.word()?;
    let data = if c.eat_word("with") { pairs(c)? } else { vec![] };
    let target = if c.eat_word("for") { Some(c.word()?) } else { None };
    Ok(ScenarioStep::Answer { adapter, outcome, data, target })
}

/// `with ⟨attr⟩ ⟨value⟩, …` and each `and a ⟨element⟩ with …` of a creation.
type Pairs = Vec<(String, Literal)>;
fn values_and_items(c: &mut Cursor) -> Result<(Pairs, Vec<(String, Pairs)>), Error> {
    let attrs = if c.eat_word("with") { pairs(c)? } else { vec![] };
    let mut items = vec![];
    while c.is_word("and") {
        c.expect_word("and")?;
        if !c.eat_word("a") {
            c.expect_word("an")?;
        }
        let element = c.word()?;
        c.expect_word("with")?;
        items.push((element, pairs(c)?));
    }
    Ok((attrs, items))
}

/// `given 14 Talks T1 to T14 with length 45`, or `when member Ada books
/// 3 new Bookings B1 to B3 with …`: one creation per name, in order, each
/// with the same values (appendix AS). None when the line is not a range.
fn many(line: &Cursor, given: bool) -> Result<Option<Vec<ScenarioStep>>, Error> {
    let mut c = line.clone();
    let given = if c.eat_word("given") {
        true
    } else if c.eat_word("when") {
        false
    } else if c.eat_word("and") {
        given
    } else {
        return Ok(None);
    };
    let by = if matches!(c.peek(), Some(Tok::Num(_))) {
        None
    } else if matches!(c.peek_at(3), Some(Tok::Num(_))) && (0..3).all(|i| matches!(c.peek_at(i), Some(Tok::Word(_)))) && !c.is_word("model") {
        let (role, name, verb) = (c.word()?, c.word()?, c.word()?);
        Some(Caller { role, name, verb })
    } else {
        return Ok(None);
    };
    let n = match (c.peek(), c.peek_at(1)) {
        (Some(Tok::Num(n)), Some(Tok::Word(w))) if w.starts_with(char::is_uppercase) || (w == "new" && by.is_some()) => n.clone(),
        _ => return Ok(None),
    };
    c.next();
    if given && by.is_some() {
        return Err(c.error("a `given` is the world, with no caller; write `when` for a caller's input"));
    }
    if by.is_some() {
        c.expect_word("new")?;
    }
    let kinds = c.kind_name()?;
    let first = c.word()?;
    c.expect_word("to")?;
    let last = c.word()?;
    let split = |w: &str| {
        let digits = w.len() - w.trim_end_matches(|ch: char| ch.is_ascii_digit()).len();
        let (pre, num) = w.split_at(w.len() - digits);
        num.parse::<u64>().ok().map(|n| (pre.to_string(), n, digits))
    };
    let count: u64 = n.parse().map_err(|_| c.error(format!("`{n}` is not a whole number of entities")))?;
    let names: Vec<String> = match (split(&first), split(&last)) {
        // `T09 to T11` keeps the width of a first name with a leading zero.
        (Some((p, a, w)), Some((q, b, _))) if p == q && b >= a && b - a + 1 == count && count <= 1000 => {
            let width = if first[p.len()..].starts_with('0') { w } else { 0 };
            (a..=b).map(|i| format!("{p}{i:0width$}")).collect()
        }
        _ => return Err(c.error(format!("`{n} {kinds} {first} to {last}`: name the first and the last with the same prefix and numbers {n} apart, as in `{n} {kinds} T1 to T{n}` (at most 1000)"))),
    };
    let (attrs, items) = values_and_items(&mut c)?;
    if c.is_word("is") {
        return Err(c.error("a range cannot be refused; refuse its next entity on a line of its own"));
    }
    c.expect_end()?;
    Ok(Some(names.into_iter().map(|name| ScenarioStep::Create { kind: kinds.clone(), name, attrs: attrs.clone(), items: items.clone(), by: by.clone(), given, refused: None }).collect()))
}

/// `1200 EUR [at 0.4]` or `nothing`: what a model reads (appendix AF).
fn read(c: &mut Cursor) -> Result<ModelAnswer, Error> {
    if c.eat_word("nothing") {
        return Ok(ModelAnswer::Nothing);
    }
    let Some(value) = literal(c)? else {
        return Err(c.error(format!("expected the value the model reads, or `nothing`, found {}", c.found())));
    };
    Ok(ModelAnswer::Read(value, confidence(c)?))
}

/// `at 0.4` after a scripted answer, in thousandths.
fn confidence(c: &mut Cursor) -> Result<Option<u16>, Error> {
    if !c.eat_word("at") {
        return Ok(None);
    }
    let sure = match c.peek() {
        Some(Tok::Num(n)) => n.parse::<f64>().ok().filter(|x| (0.0..=1.0).contains(x)).map(|x| (x * 1000.0).round() as u16),
        _ => None,
    };
    let Some(sure) = sure else { return Err(c.error(format!("expected a confidence from 0 to 1, as in `at 0.4`, found {}", c.found()))) };
    c.next();
    Ok(Some(sure))
}

/// `is refused by rule late`, `is refused at the gate`, or nothing.
fn refusal(c: &mut Cursor) -> Result<Option<Refused>, Error> {
    if !c.eat_word("is") {
        return Ok(None);
    }
    c.expect_word("refused")?;
    if c.eat_word("at") {
        c.expect_word("the")?;
        c.expect_word("gate")?;
        return Ok(Some(Refused::Gate));
    }
    c.expect_word("by")?;
    c.expect_word("rule")?;
    Ok(Some(Refused::Rule(c.word()?)))
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

/// `a 1, b "x" and c 3 EUR`. A bare word is text, or the name of a shared
/// entity; `{DE, FR}` names several.
fn pairs(c: &mut Cursor) -> Result<Vec<(String, Literal)>, Error> {
    let mut out = vec![];
    loop {
        let name = c.word()?;
        let value = if c.eat_sym('{') {
            let mut names = vec![];
            if !c.eat_sym('}') {
                names.push(c.word()?);
                while c.eat_sym(',') {
                    names.push(c.word()?);
                }
                c.expect_sym('}')?;
            }
            Literal::Set(names)
        } else {
            match literal(c)? {
                Some(l) => l,
                None => Literal::Text(c.word()?),
            }
        };
        out.push((name, value));
        let next_element = c.is_word("and") && matches!(c.peek_at(1), Some(Tok::Word(w)) if w == "a" || w == "an");
        if next_element || !(c.eat_sym(',') || c.eat_word("and")) {
            return Ok(out);
        }
    }
}

fn word_at(c: &Cursor, k: usize, w: &str) -> bool {
    matches!(c.peek_at(k), Some(Tok::Word(x)) if x == w)
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
            ScenarioStep::Create {
                kind: "Order".into(),
                name: "O1".into(),
                attrs: vec![country, ("total".into(), eur(12000))],
                items: vec![],
                by: None,
                given: false,
                refused: None
            }
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
    fn callers_and_refusals_parse() {
        let src = "scenario \"x\":\n  given  a Customer Ada with name Ada\n  and    a Customer Bob with name Bob\n  when   customer Ada places a new Order O1 with customer Ada\n  and    customer Bob places a new Order O2 with customer Ada\n         is refused at the gate\n";
        let s = &parse_scenarios("o.scenario", src).unwrap()[0];
        let ScenarioStep::Create { given, by, .. } = &s.steps[1] else { panic!("{:?}", s.steps[1]) };
        assert!(*given && by.is_none());
        let ScenarioStep::Create { given, by, refused, .. } = &s.steps[2] else { panic!() };
        assert!(!*given && refused.is_none());
        assert_eq!(by, &Some(Caller { role: "customer".into(), name: "Ada".into(), verb: "places".into() }));
        let ScenarioStep::Create { refused, .. } = &s.steps[3] else { panic!() };
        assert_eq!(refused, &Some(Refused::Gate));
        let src = "scenario \"x\":\n  when   support Sam cancels O1\n         is refused by rule late\n";
        let s = &parse_scenarios("o.scenario", src).unwrap()[0];
        let by = Caller { role: "support".into(), name: "Sam".into(), verb: "cancels".into() };
        assert_eq!(s.steps[0], ScenarioStep::Event { by, target: "O1".into(), part: None, data: vec![], refused: Some(Refused::Rule("late".into())) });
        let src = "scenario \"x\":\n  when   staff Tom changes S1's units to 3\n";
        let s = &parse_scenarios("o.scenario", src).unwrap()[0];
        let by = Caller { role: "staff".into(), name: "Tom".into(), verb: "changes".into() };
        assert_eq!(s.steps[0], ScenarioStep::Change { by, target: "S1".into(), attr: "units".into(), value: Literal::Count(3), refused: None });
        let e = parse_scenarios("o.scenario", "scenario \"x\":\n  given  customer Ada places a new Order O1\n").unwrap_err();
        assert!(e.msg.contains("a `given` is the world"), "{e}");
    }

    #[test]
    fn scenario_errors_say_what_was_expected() {
        let e = parse_scenarios("o.scenario", "scenario \"x\":\n  then O1 is paid\n").unwrap_err();
        assert!(e.msg.contains("expected `given`, `when`, `and` or `expect`"), "{e}");
    }
}
