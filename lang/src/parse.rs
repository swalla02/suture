//! Statement parsers (spec §3.2 and appendix B.2). The first word says which
//! statement it is; a capitalized first word starts `Kind needs …`.

use crate::ast::*;
use crate::ir::PERIODS;
use crate::lex::{statements, Cursor, Error, Stmt, Tok};
use crate::sentence::{calc_expr, calc_formula, condition, literal, noun_phrase, predicate, rule_body, starts_noun_phrase, state_word, subject};

/// Parses one source file into `out`.
pub fn parse_file(file: &str, src: &str, out: &mut Source) -> Result<(), Error> {
    for st in statements(file, src)? {
        statement(&st, out)?;
    }
    Ok(())
}

/// Parses every statement of a file into `out`, and returns those that
/// fail with their errors, so one bad sentence hides none after it.
pub fn parse_all(file: &str, src: &str, out: &mut Source) -> Result<Vec<(Stmt, Error)>, Error> {
    let mut failed = vec![];
    for st in statements(file, src)? {
        if let Err(e) = statement(&st, out) {
            failed.push((st, e));
        }
    }
    Ok(failed)
}

pub(crate) fn statement(st: &Stmt, out: &mut Source) -> Result<(), Error> {
    let at = Loc { file: st.file.clone(), line: st.line };
    let first = match st.lines[0].first() {
        Some(Tok::Word(w)) => w.clone(),
        _ => return Err(st.error("a statement starts with a word")),
    };
    match first.as_str() {
        "module" => module(st, at, out),
        "kind" => kind(st, at, out),
        "effect" => effect(st, at, out),
        "rule" => rule(st, at, out),
        "define" => define(st, at, out),
        "desired" => goal(st, at, out),
        "when" => reaction(st, at, out),
        "prefer" => prefer(st, at, out),
        "role" => role(st, at, out),
        "admit" => admit(st, at, out),
        w if w.starts_with(char::is_uppercase) && matches!(st.lines[0].get(1), Some(Tok::Word(x)) if x == "is") => slot(st, at, out),
        w if w.starts_with(char::is_uppercase) && matches!(st.lines[0].get(1), Some(Tok::Word(x)) if x == "uses") => uses(st, at, out),
        w if w.starts_with(char::is_uppercase) => needs(st, at, out),
        _ => Err(st.error(format!(
            "unknown statement `{first}`: expected module, kind, effect, rule, define, desired, when, prefer, role, admit, `Kind needs …` or `Slot is …`"
        ))),
    }
}

/// Tokens back to text, for read-backs: no space before `'s` or `,`.
pub(crate) fn text(toks: &[Tok]) -> String {
    let mut out = String::new();
    for t in toks {
        if !out.is_empty() && !matches!(t, Tok::Poss | Tok::Sym(',')) {
            out.push(' ');
        }
        out.push_str(&t.to_string());
    }
    out
}

/// The tokens after the statement's first `:`.
fn after_colon(toks: &[Tok]) -> &[Tok] {
    toks.iter().position(|t| *t == Tok::Sym(':')).map_or(&[], |i| &toks[i + 1..])
}

fn number<T: std::str::FromStr>(c: &mut Cursor, what: &str) -> Result<T, Error> {
    match c.peek() {
        Some(Tok::Num(n)) => {
            let n = n.clone();
            c.next();
            n.parse().map_err(|_| c.error(format!("{what} must be a whole number, found `{n}`")))
        }
        _ => Err(c.error(format!("expected {what}, found {}", c.found()))),
    }
}

/// `30 seconds`, `1 day`, `3 days` and so on, in seconds.
pub(crate) fn duration(c: &mut Cursor) -> Result<u64, Error> {
    let n: u64 = number(c, "a duration")?;
    let unit = c.word()?;
    let secs = match unit.trim_end_matches('s') {
        "second" => 1,
        "minute" => 60,
        "hour" => 3_600,
        "day" => 86_400,
        "week" => 604_800,
        _ => return Err(c.error(format!("unknown time unit `{unit}`: use seconds, minutes, hours, days or weeks"))),
    };
    Ok(n * secs)
}

fn type_name(c: &mut Cursor) -> Result<TypeName, Error> {
    let w = c.word()?;
    Ok(match w.as_str() {
        "Text" => TypeName::Text,
        "Bool" => TypeName::Bool,
        "Money" => TypeName::Money,
        "Count" => TypeName::Count,
        "Time" => TypeName::Time,
        "Duration" => TypeName::Duration,
        "set" => {
            c.expect_word("of")?;
            TypeName::Set(c.kind_name()?)
        }
        "list" => {
            c.expect_word("of")?;
            TypeName::List(c.kind_name()?)
        }
        _ if w.starts_with(char::is_uppercase) => TypeName::Kind(w),
        _ => return Err(c.error(format!("unknown type `{w}`: use Text, Bool, Money, Count, Time, Duration, a kind, `list of` a kind or `set of` a kind"))),
    })
}

/// `module shop version 1, currency EUR`
fn module(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("module")?;
    let name = c.word()?;
    c.expect_word("version")?;
    let version = number(&mut c, "a version")?;
    c.expect_sym(',')?;
    c.expect_word("currency")?;
    let currency = c.word()?;
    let (mut attempts, mut candidates) = (None, None);
    while c.eat_sym(',') {
        if c.eat_word("attempts") {
            attempts = Some(number(&mut c, "a number of attempts")?);
        } else if c.eat_word("candidates") {
            let n: u32 = number(&mut c, "a number of candidates")?;
            if n == 0 {
                return Err(c.error("`candidates` must be at least 1"));
            }
            candidates = Some(n);
        } else {
            return Err(c.error(format!("unknown module setting {}: use `attempts` or `candidates`", c.found())));
        }
    }
    c.expect_end()?;
    if out.module.is_some() {
        return Err(st.error("a second `module` statement"));
    }
    out.module = Some(Module { name, version, currency, attempts, candidates, at });
    Ok(())
}

/// `kind Order { country: Text, total: Money, cancelled: Bool = no }`
fn kind(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("kind")?;
    let name = c.kind_name()?;
    let plural = if c.eat_sym('(') {
        c.expect_word("plural")?;
        let p = c.kind_name()?;
        c.expect_sym(')')?;
        Some(p)
    } else {
        None
    };
    c.expect_sym('{')?;
    let mut attrs = vec![];
    if !c.eat_sym('}') {
        loop {
            let attr = c.word()?;
            c.expect_sym(':')?;
            let ty = type_name(&mut c)?;
            let (mut default, mut formula) = (None, None);
            if c.eat_sym('=') {
                // A lone literal is a default; anything else is a formula.
                let saved = c.clone();
                match literal(&mut c)? {
                    Some(l) if matches!(c.peek(), Some(Tok::Sym(',' | '}'))) => default = Some(l),
                    _ => {
                        c = saved;
                        formula = Some(calc_formula(&mut c)?);
                    }
                }
            }
            attrs.push(AttrDecl { name: attr, ty, default, formula });
            if c.eat_sym('}') {
                break;
            }
            c.expect_sym(',')?;
        }
    }
    c.expect_end()?;
    out.kinds.push(KindDecl { name, plural, attrs, at });
    Ok(())
}

/// `Order needs Payment, Shipment [while cond]`
fn needs(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    let owner = c.kind_name()?;
    c.expect_word("needs")?;
    let mut parts = vec![c.kind_name()?];
    while c.eat_sym(',') {
        parts.push(c.kind_name()?);
    }
    let cond = if c.eat_word("while") { Some(condition(&mut c)?) } else { None };
    c.expect_end()?;
    out.needs.push(NeedsDecl { owner, parts, cond, at });
    Ok(())
}

/// `Delivery is Shipment or Refund [or …]`
fn slot(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    let name = c.kind_name()?;
    c.expect_word("is")?;
    let mut alternatives = vec![c.kind_name()?];
    c.expect_word("or")?;
    alternatives.push(c.kind_name()?);
    while c.eat_word("or") {
        alternatives.push(c.kind_name()?);
    }
    c.expect_end()?;
    out.slots.push(SlotDecl { name, alternatives, at });
    Ok(())
}

/// `S uses a C [per element] where cond[, taking num]`
fn uses(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    let subject = c.kind_name()?;
    c.expect_word("uses")?;
    if !c.eat_word("a") {
        c.expect_word("an")?;
    }
    let target = c.kind_name()?;
    let per = if c.eat_word("per") { Some(c.word()?) } else { None };
    c.expect_word("where")?;
    let cond = condition(&mut c)?;
    let taking = if c.eat_sym(',') {
        c.expect_word("taking")?;
        Some(calc_expr(&mut c)?)
    } else {
        None
    };
    c.expect_end()?;
    out.uses.push(UsesDecl { subject, target, per, cond, taking, source: text(&toks), at });
    Ok(())
}

/// `prefer Delivery: Shipment, then Refund after 2 days`, or for a `uses`,
/// `prefer Stock for a line: lowest price, then those whose …`
fn prefer(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("prefer")?;
    let choice = c.kind_name()?;
    let per = if c.eat_word("for") {
        if !c.eat_word("a") {
            c.expect_word("an")?;
        }
        Some(c.word()?)
    } else {
        None
    };
    c.expect_sym(':')?;
    if c.is_word("lowest") || c.is_word("highest") || c.is_word("those") {
        let mut criteria = vec![criterion(&mut c)?];
        while c.eat_sym(',') {
            c.expect_word("then")?;
            criteria.push(criterion(&mut c)?);
        }
        c.expect_end()?;
        out.prefers.push(PreferDecl { choice, per, order: vec![], criteria, at });
        return Ok(());
    }
    let mut order = vec![(c.kind_name()?, None)];
    while c.eat_sym(',') {
        c.expect_word("then")?;
        let k = c.kind_name()?;
        let after = if c.eat_word("after") { Some(duration(&mut c)?) } else { None };
        order.push((k, after));
    }
    c.expect_end()?;
    out.prefers.push(PreferDecl { choice, per, order, criteria: vec![], at });
    Ok(())
}

/// `lowest fee`, `highest the Order's total`, `those whose warehouse's country is …`, `those that are express`.
fn criterion(c: &mut Cursor) -> Result<Criterion, Error> {
    if c.eat_word("those") {
        if c.eat_word("whose") {
            let mut steps = vec![c.word()?];
            while c.eat_poss() {
                steps.push(c.word()?);
            }
            return Ok(Criterion::Those(predicate(c, Num::Np(NounPhrase { head: Head::It, steps }))?));
        }
        c.expect_word("that")?;
        return Ok(Criterion::Those(condition(c)?));
    }
    let lowest = c.eat_word("lowest");
    if !lowest {
        c.expect_word("highest")?;
    }
    let n = calc_formula(c)?;
    Ok(if lowest { Criterion::Lowest(n) } else { Criterion::Highest(n) })
}

/// A field that an effect sends: a noun phrase, or a bare attribute such as `amount`.
fn field(c: &mut Cursor) -> Result<NounPhrase, Error> {
    if starts_noun_phrase(c) {
        return noun_phrase(c);
    }
    let mut np = NounPhrase { head: Head::It, steps: vec![c.word()?] };
    while c.eat_poss() {
        np.steps.push(c.word()?);
    }
    Ok(np)
}

/// An effect's success outcome, the data it carries, and its failure outcomes.
type Outcomes = (String, Vec<(String, TypeName)>, Vec<String>);

/// `ok [with a: T, b: T] [| fails with x, y]`
fn outcomes(c: &mut Cursor) -> Result<Outcomes, Error> {
    let success = c.word()?;
    let mut data = vec![];
    if c.eat_word("with") {
        loop {
            let name = c.word()?;
            c.expect_sym(':')?;
            data.push((name, type_name(c)?));
            if !c.eat_sym(',') {
                break;
            }
        }
    }
    let mut failures = vec![];
    if c.eat_sym('|') {
        c.expect_word("fails")?;
        c.expect_word("with")?;
        failures.push(c.word()?);
        while c.eat_sym(',') {
            failures.push(c.word()?);
        }
    }
    c.expect_end()?;
    Ok((success, data, failures))
}

/// `effect <verb> <Kind> via <adapter> within <duration>`, then indented
/// `sends …`, optionally `marks …`, and `reports …` lines.
fn effect(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let mut c = Cursor::new(&st.lines[0], &st.file, st.line);
    c.expect_word("effect")?;
    let verb = c.word()?;
    let kind = c.kind_name()?;
    c.expect_word("via")?;
    let adapter = c.word()?;
    c.expect_word("within")?;
    let within_secs = duration(&mut c)?;
    c.expect_end()?;
    let (mut sends, mut marks, mut reports) = (None, None, None);
    for line in &st.lines[1..] {
        let mut c = Cursor::new(line, &st.file, st.line);
        if c.eat_word("sends") {
            let mut list = vec![field(&mut c)?];
            while c.eat_sym(',') {
                list.push(field(&mut c)?);
            }
            c.expect_end()?;
            sends = Some(list);
        } else if c.eat_word("marks") {
            marks = Some(c.word()?);
            c.expect_end()?;
        } else if c.eat_word("reports") {
            reports = Some(outcomes(&mut c)?);
        } else {
            return Err(c.error(format!("expected `sends`, `marks` or `reports`, found {}", c.found())));
        }
    }
    let sends = sends.ok_or_else(|| st.error("an effect needs a `sends` line saying what the adapter receives"))?;
    let (success, data, failures) = reports.ok_or_else(|| st.error("an effect needs a `reports` line"))?;
    out.effects.push(EffectDecl { verb, kind, adapter, within_secs, sends, marks, success, data, failures, at });
    Ok(())
}

/// `rule <name>: <every … | no …>`
fn rule(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("rule")?;
    let name = c.word()?;
    c.expect_sym(':')?;
    let mut period = None;
    let body = if c.eat_word("for") {
        // `for each Customer and month, ⟨bound⟩` (appendix F.2.12).
        c.expect_word("each")?;
        let kind = c.kind_name()?;
        c.expect_word("and")?;
        let p = c.word()?;
        if !PERIODS.contains(&p.as_str()) {
            return Err(c.error(format!("a period is a day, week, month or year, not `{p}`")));
        }
        period = Some(p);
        c.expect_sym(',')?;
        RuleBody::Every { subject: Subject { kind, filter: vec![] }, cond: condition(&mut c)? }
    } else {
        rule_body(&mut c)?
    };
    let globally = c.eat_sym(',');
    if globally {
        c.expect_word("checked")?;
        c.expect_word("globally")?;
    }
    c.expect_end()?;
    out.rules.push(RuleDecl { name, body, period, globally, source: text(after_colon(&toks)), at });
    Ok(())
}

/// `define: an Order is paid when <cond>`
fn define(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("define")?;
    c.expect_sym(':')?;
    if !(c.eat_word("a") || c.eat_word("an")) {
        return Err(c.error(format!("expected `a` or `an`, found {}", c.found())));
    }
    let kind = c.kind_name()?;
    c.expect_word("is")?;
    let state = c.word()?;
    c.expect_word("when")?;
    let cond = condition(&mut c)?;
    c.expect_end()?;
    out.defines.push(DefineDecl { kind, state, cond, at });
    Ok(())
}

/// `desired [within 3 days]: every <subject> is <state> [and <state> …]`
fn goal(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("desired")?;
    let within_secs = if c.eat_word("within") { Some(duration(&mut c)?) } else { None };
    c.expect_sym(':')?;
    c.expect_word("every")?;
    let subject = subject(&mut c)?;
    c.expect_word("is")?;
    let mut states = vec![state_word(&mut c)?];
    while c.eat_word("and") {
        states.push(state_word(&mut c)?);
    }
    c.expect_end()?;
    out.goals.push(GoalDecl { subject, states, within_secs, source: text(after_colon(&toks)), at });
    Ok(())
}

/// `when a Payment is declined: its Order is cancelled [and …]`, or for an
/// admitted event `when a customer or support cancels an Order: …`.
fn reaction(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("when")?;
    if !(c.eat_word("a") || c.eat_word("an")) {
        return Err(c.error(format!("expected `a` or `an`, found {}", c.found())));
    }
    let mut by = vec![];
    let (kind, outcome) = if matches!(c.peek(), Some(Tok::Word(w)) if w.starts_with(char::is_lowercase)) {
        by.push(c.word()?);
        while c.eat_sym(',') || c.eat_word("or") {
            by.push(c.word()?);
        }
        let verb = c.word()?;
        if !(c.eat_word("a") || c.eat_word("an")) {
            return Err(c.error(format!("expected `a` or `an` after `{verb}`, found {}", c.found())));
        }
        (c.kind_name()?, verb)
    } else {
        let kind = c.kind_name()?;
        c.expect_word("is")?;
        (kind, state_word(&mut c)?)
    };
    c.expect_sym(':')?;
    let mut actions = vec![action(&mut c)?];
    while c.eat_word("and") {
        actions.push(action(&mut c)?);
    }
    c.expect_end()?;
    out.reactions.push(ReactionDecl { kind, outcome, by, actions, at });
    Ok(())
}

/// `role customer is a Customer`, `role support`
fn role(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("role")?;
    let name = c.word()?;
    if name.starts_with(char::is_uppercase) {
        return Err(c.error(format!("a role is named in lower case: `role {}`", name.to_lowercase())));
    }
    let kind = if c.eat_word("is") {
        if !(c.eat_word("a") || c.eat_word("an")) {
            return Err(c.error(format!("expected `a` or `an`, found {}", c.found())));
        }
        Some(c.kind_name()?)
    } else {
        None
    };
    c.expect_end()?;
    out.roles.push(RoleDecl { name, kind, at });
    Ok(())
}

/// `admit place a new Order by customer where …`, `admit cancel Order by
/// customer, support`, `admit change Stock's units by staff` (appendix G.2).
fn admit(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("admit")?;
    let verb = c.word()?;
    let what = if c.eat_word("a") || c.eat_word("an") {
        c.expect_word("new")?;
        let mut kinds = vec![c.kind_name()?];
        while c.eat_sym(',') || c.eat_word("or") {
            kinds.push(c.kind_name()?);
        }
        Admitted::New { verb, kinds }
    } else if verb == "change" {
        let kind = c.kind_name()?;
        if !c.eat_poss() {
            return Err(c.error(format!("`admit change {kind}`: name the attribute, as in `admit change {kind}'s units`")));
        }
        Admitted::Change { kind, attr: c.word()? }
    } else {
        Admitted::Event { verb, kind: c.kind_name()? }
    };
    c.expect_word("by")?;
    let mut roles = vec![c.word()?];
    while c.eat_sym(',') || c.eat_word("or") {
        roles.push(c.word()?);
    }
    let cond = if c.eat_word("where") { Some(condition(&mut c)?) } else { None };
    c.expect_end()?;
    out.admits.push(AdmitDecl { what, roles, cond, source: text(&toks[1..]), at });
    Ok(())
}

/// `its Order is cancelled`, `the Order is not cancelled`
fn action(c: &mut Cursor) -> Result<Action, Error> {
    let target = noun_phrase(c)?;
    if c.eat_word("becomes") {
        let Some((attr, rest)) = target.steps.split_last() else {
            return Err(c.error("`… becomes …` changes an attribute: name it, as in `its order's credit becomes …`"));
        };
        let target = NounPhrase { head: target.head.clone(), steps: rest.to_vec() };
        return Ok(Action { target, attr: attr.clone(), value: true, becomes: Some(calc_expr(c)?) });
    }
    c.expect_word("is")?;
    let value = !c.eat_word("not");
    let attr = c.word()?;
    Ok(Action { target, attr, value, becomes: None })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"module shop version 1, currency EUR
kind Order { country: Text, total: Money, cancelled: Bool = no }
kind Payment { order: Order }
Order needs Payment while it is not cancelled
effect capture Payment via payments within 30 seconds
  sends    the order's total, amount
  reports  captured with reference: Text | fails with declined
rule paid: no Order has a Shipment that is sent unless its Payment is captured
define: an Order is paid when its Payment is captured
desired within 3 days: every Order that is not cancelled is paid and settled
when a Payment is declined: its Order is cancelled
"#;

    #[test]
    fn every_statement_kind_parses() {
        let mut s = Source::default();
        parse_file("shop.st", SRC, &mut s).unwrap();
        let m = s.module.unwrap();
        assert_eq!((m.name.as_str(), m.version, m.currency.as_str()), ("shop", 1, "EUR"));
        assert_eq!(s.kinds[0].attrs[2].default, Some(Literal::Bool(false)));
        assert_eq!(s.kinds[1].attrs[0].ty, TypeName::Kind("Order".into()));
        assert_eq!(s.needs[0].parts, vec!["Payment".to_string()]);
        assert!(s.needs[0].cond.is_some());
        let e = &s.effects[0];
        assert_eq!(
            (e.verb.as_str(), e.kind.as_str(), e.adapter.as_str(), e.within_secs),
            ("capture", "Payment", "payments", 30)
        );
        assert_eq!(e.sends.len(), 2);
        assert_eq!((e.success.as_str(), e.failures.clone()), ("captured", vec!["declined".to_string()]));
        assert_eq!(e.data, vec![("reference".to_string(), TypeName::Text)]);
        assert_eq!(s.rules[0].source, "no Order has a Shipment that is sent unless its Payment is captured");
        assert_eq!((s.defines[0].kind.as_str(), s.defines[0].state.as_str()), ("Order", "paid"));
        assert_eq!(s.goals[0].states, vec!["paid".to_string(), "settled".to_string()]);
        assert_eq!(s.goals[0].within_secs, Some(3 * 86_400));
        assert_eq!(s.goals[0].source, "every Order that is not cancelled is paid and settled");
        assert_eq!(s.reactions[0].actions[0].attr, "cancelled");
        assert!(s.reactions[0].actions[0].value);
    }

    #[test]
    fn statement_errors_say_what_was_expected() {
        let mut s = Source::default();
        let e = parse_file("x.st", "frobnicate the thing", &mut s).unwrap_err();
        assert!(e.msg.starts_with("unknown statement `frobnicate`"), "{e}");
        let e = parse_file("x.st", "effect ask Form via forms within 3 days\n  reports filled\n", &mut s).unwrap_err();
        assert!(e.msg.contains("`sends` line"), "{e}");
        let e = parse_file("x.st", "effect ask Form via forms within 3 fortnights\n", &mut s).unwrap_err();
        assert!(e.msg.contains("unknown time unit"), "{e}");
        let e = parse_file("x.st", "kind A { x: colour }", &mut s).unwrap_err();
        assert!(e.msg.contains("unknown type `colour`"), "{e}");
    }
}
