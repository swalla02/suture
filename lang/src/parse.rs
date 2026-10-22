//! Statement parsers (spec §3.2 and appendix B.2). The first word says which
//! statement it is; a capitalized first word starts `Kind needs …`.

use crate::ast::*;
use crate::ir::{PERIODS, READ, UNREAD};
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
        "source" => source(st, at, out),
        "decide" => decide(st, at, out),
        "rule" => rule(st, at, out),
        "define" => define(st, at, out),
        "desired" => goal(st, at, out),
        "when" => reaction(st, at, out),
        "prefer" => prefer(st, at, out),
        "role" => role(st, at, out),
        "admit" => admit(st, at, out),
        "apply" => apply(st, at, out),
        "allow" => allow(st, at, out),
        "keep" => keep(st, at, out),
        w if w.starts_with(char::is_uppercase) && matches!(st.lines[0].get(1), Some(Tok::Word(x)) if x == "is") => slot(st, at, out),
        w if w.starts_with(char::is_uppercase) && matches!(st.lines[0].get(1), Some(Tok::Word(x)) if x == "uses") => uses(st, at, out),
        w if w.starts_with(char::is_uppercase) => needs(st, at, out),
        _ => Err(st.error(format!(
            "unknown statement `{first}`: expected module, kind, effect, source, decide, rule, define, desired, when, prefer, role, admit, apply, allow, keep, `Kind needs …` or `Slot is …`"
        ))),
    }
}

/// Tokens back to text, for read-backs: no space before `'s`, `,` or `:`.
pub(crate) fn text(toks: &[Tok]) -> String {
    let mut out = String::new();
    for t in toks {
        if !out.is_empty() && !matches!(t, Tok::Poss | Tok::Sym(',' | ':')) {
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

/// The longest duration a program or scenario may name: 100 years, so that
/// no deadline or clock can overflow (appendix T.2.1).
pub const MAX_DURATION_SECS: u64 = 100 * 365 * 86_400;

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
    n.checked_mul(secs).filter(|d| *d <= MAX_DURATION_SECS).ok_or_else(|| c.error(format!("`{n} {unit}` is too long: a duration is at most 100 years")))
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
        "Media" => TypeName::Media,
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
            let n: u32 = number(&mut c, "a number of attempts")?;
            if n == 0 {
                return Err(c.error("`attempts` must be at least 1"));
            }
            attempts = Some(n);
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
            // `reception: awaiting | received | lost`: a state attribute (appendix Z.2.1).
            let mut states = vec![];
            if matches!(c.peek(), Some(Tok::Word(w)) if w.starts_with(char::is_lowercase)) && matches!(c.peek_at(1), Some(Tok::Sym('|'))) {
                states.push(c.word()?);
                while c.eat_sym('|') {
                    states.push(c.word()?);
                }
            }
            let ty = if states.is_empty() { type_name(&mut c)? } else { TypeName::Text };
            let mark = if c.eat_word("secret") {
                Some(crate::ir::Mark::Secret)
            } else if c.eat_word("personal") {
                Some(crate::ir::Mark::Personal)
            } else {
                None
            };
            let (mut default, mut formula) = (None, None);
            if !states.is_empty() {
                // A state attribute starts in its first state, or the one after `=`.
                let first = if c.eat_sym('=') { c.word()? } else { states[0].clone() };
                default = Some(Literal::Text(first));
            } else if c.eat_sym('=') {
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
            attrs.push(AttrDecl { name: attr, ty, mark, default, formula, states });
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

/// `source <Kind>'s <attr> via <adapter> within <duration>`, then indented
/// `sends …` and `reports …`, and optionally `good for …` and `otherwise …`
/// lines (appendix I).
fn source(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let mut c = Cursor::new(&st.lines[0], &st.file, st.line);
    c.expect_word("source")?;
    let kind = c.kind_name()?;
    if !c.eat_poss() {
        return Err(c.error(format!("expected `{kind}'s` and the attribute it sources, found {}", c.found())));
    }
    let attr = c.word()?;
    c.expect_word("via")?;
    // `via model "reader@1"`: a model reads the value (appendix AF).
    let model = c.is_word("model") && matches!(c.peek_at(1), Some(Tok::Str(_)));
    if model {
        c.next();
    }
    let adapter = match c.peek() {
        Some(Tok::Str(m)) if model => {
            let m = m.clone();
            c.next();
            m
        }
        _ => c.word()?,
    };
    c.expect_word("within")?;
    let within_secs = duration(&mut c)?;
    c.expect_end()?;
    let (mut sends, mut reports, mut good_for, mut otherwise) = (None, None, None, None);
    let (mut asking, mut sure) = (None, None);
    for line in &st.lines[1..] {
        let mut c = Cursor::new(line, &st.file, st.line);
        if c.eat_word("sends") {
            let mut list = vec![field(&mut c)?];
            while c.eat_sym(',') {
                list.push(field(&mut c)?);
            }
            c.expect_end()?;
            sends = Some(list);
        } else if c.eat_word("reports") {
            if model {
                return Err(c.error("a model reads the value or it does not; a source via a model has no `reports` line"));
            }
            reports = Some(outcomes(&mut c)?);
        } else if model && c.eat_word("asking") {
            let Some(Tok::Str(text)) = c.peek() else {
                return Err(c.error(format!("expected what the model is asked, in quotes, found {}", c.found())));
            };
            asking = Some(text.clone());
            c.next();
            c.expect_end()?;
        } else if model && c.is_word("at") {
            sure = at_least_sure(&mut c, true)?;
            c.expect_end()?;
        } else if c.eat_word("good") {
            c.expect_word("for")?;
            good_for = Some(duration(&mut c)?);
            c.expect_end()?;
        } else if c.eat_word("otherwise") {
            otherwise = Some(if c.eat_word("use") {
                let l = crate::sentence::literal(&mut c)?.ok_or_else(|| c.error(format!("expected a value after `use`, found {}", c.found())))?;
                OtherwiseDecl::Use(l)
            } else if c.eat_word("ask") {
                c.expect_word("again")?;
                c.expect_word("after")?;
                OtherwiseDecl::Again(duration(&mut c)?)
            } else {
                let mut actions = vec![action(&mut c)?];
                while c.eat_word("and") {
                    actions.push(action(&mut c)?);
                }
                OtherwiseDecl::Act(actions)
            });
            c.expect_end()?;
        } else {
            let more = if model { ", `asking`, `at least … sure`" } else { "" };
            return Err(c.error(format!("expected `sends`, `reports`, `good for`{more} or `otherwise`, found {}", c.found())));
        }
    }
    let sends = sends.ok_or_else(|| st.error(if model { "a source needs a `sends` line saying what the model reads" } else { "a source needs a `sends` line saying what the adapter receives" }))?;
    let (success, data, failures) = match reports {
        Some(r) => r,
        None if model => (READ.to_string(), vec![], vec![UNREAD.to_string()]),
        None => return Err(st.error("a source needs a `reports` line")),
    };
    out.sources.push(SourceDecl { kind, attr, adapter, model, asking, sure, within_secs, sends, success, data, failures, good_for, otherwise, at });
    Ok(())
}

/// `rule <name>: <every … | no …>`
fn rule(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("rule")?;
    let name = c.word()?;
    c.expect_sym(':')?;
    let (mut period, mut span) = (None, None);
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
        // `… and day from their from to their to, …` (appendix AD.2.1).
        if c.eat_word("from") {
            if !c.eat_word("their") {
                c.expect_word("its")?;
            }
            let start = c.word()?;
            c.expect_word("to")?;
            if !c.eat_word("their") {
                c.expect_word("its")?;
            }
            span = Some((start, c.word()?));
        }
        c.expect_sym(',')?;
        RuleBody::Every { subject: Subject { kind, filter: vec![] }, cond: condition(&mut c)? }
    } else {
        rule_body(&mut c)?
    };
    let (mut globally, mut fresh) = (false, vec![]);
    while c.eat_sym(',') {
        if c.eat_word("with") {
            // `with credit at most 1 hour old[ and score at most …]` (appendix I.2.3).
            loop {
                let attr = c.word()?;
                c.expect_word("at")?;
                c.expect_word("most")?;
                let secs = duration(&mut c)?;
                c.expect_word("old")?;
                fresh.push((attr, secs));
                if !c.eat_word("and") {
                    break;
                }
            }
        } else {
            c.expect_word("checked")?;
            c.expect_word("globally")?;
            globally = true;
        }
    }
    c.expect_end()?;
    out.rules.push(RuleDecl { name, body, period, span, globally, fresh, source: text(after_colon(&toks)), at });
    Ok(())
}

/// `keep the total days of each Allocation's Applications that are approved` (appendix AI)
fn keep(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("keep")?;
    let (kind, total) = crate::sentence::kept_total(&mut c)?;
    c.expect_end()?;
    out.keeps.push(KeepDecl { kind, total, at });
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

/// `decide [Carrier] [for Orders [whose …]] with model "carriers@1" | with prefer` (appendix J).
fn decide(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("decide")?;
    let point = if c.is_word("for") || c.is_word("with") {
        None
    } else {
        let k = c.kind_name()?;
        // `decide Order's credit`: the source of a sourced attribute.
        if c.eat_poss() { Some(format!("{k}'s {}", c.word()?)) } else { Some(k) }
    };
    let (scope, cond) = if c.eat_word("for") {
        let scope = c.kind_name()?;
        let cond = if c.eat_word("whose") {
            let mut steps = vec![c.word()?];
            while c.eat_poss() {
                steps.push(c.word()?);
            }
            Some(predicate(&mut c, Num::Np(NounPhrase { head: Head::It, steps }))?)
        } else {
            None
        };
        (Some(scope), cond)
    } else {
        (None, None)
    };
    c.expect_word("with")?;
    let model = handler_model(&mut c)?;
    let mut or = vec![];
    while c.eat_word("or") {
        or.push(handler_model(&mut c)?);
    }
    let asks = model.is_some() || or.iter().any(Option::is_some);
    let within_ms = within(&mut c, asks)?;
    let sure = at_least_sure(&mut c, asks)?;
    // `otherwise Writeoff`: what is taken when the model is no help (appendix AL).
    let otherwise = if c.eat_word("otherwise") {
        if !asks {
            return Err(c.error("`with prefer` answers at once; `otherwise` is for when a model is no help"));
        }
        Some(c.word()?)
    } else {
        None
    };
    // `, picked by model "router@1"`: a model picks the model (appendix AJ).
    let router = if c.eat_sym(',') {
        c.expect_word("picked")?;
        c.expect_word("by")?;
        c.expect_word("model")?;
        let r = quoted_model(&mut c)?;
        let within = within(&mut c, true)?;
        let sure = at_least_sure(&mut c, true)?;
        let otherwise = if c.eat_word("otherwise") { Some(crate::ir::candidate(&handler_model(&mut c)?)) } else { None };
        Some((r, within, sure, otherwise))
    } else {
        None
    };
    c.expect_end()?;
    if !or.is_empty() && router.is_none() {
        return Err(c.error("a choice among models ends with the model that picks one: `, picked by model \"router@1\"`"));
    }
    if or.is_empty() && router.is_some() {
        return Err(c.error("`picked by` picks among models: name two or more, as in `with model \"fast@1\" or model \"careful@1\", picked by model \"router@1\"`"));
    }
    out.decides.push(DecideDecl { point, scope, cond, model, within_ms, sure, otherwise, or, router, source: text(&toks), at });
    Ok(())
}

/// `at least 0.8 sure`, in thousandths: the confidence below which a model's
/// answer is not used (appendix AF.2.1, AK.2.1). Only after a model (`asks`).
fn at_least_sure(c: &mut Cursor, asks: bool) -> Result<Option<u16>, Error> {
    if !c.is_word("at") {
        return Ok(None);
    }
    if !asks {
        return Err(c.error("`with prefer` answers at once; `at least … sure` bounds a model's answer"));
    }
    c.next();
    c.expect_word("least")?;
    let bound = match c.peek() {
        Some(Tok::Num(n)) => n.parse::<f64>().ok().filter(|x| (0.0..=1.0).contains(x)).map(|x| (x * 1000.0).round() as u16),
        _ => None,
    };
    let Some(bound) = bound else {
        return Err(c.error(format!("expected a confidence from 0 to 1, as in `at least 0.8 sure`, found {}", c.found())));
    };
    c.next();
    c.expect_word("sure")?;
    Ok(Some(bound))
}

/// `model "carriers@1"`, or `prefer`: `None`.
fn handler_model(c: &mut Cursor) -> Result<Option<String>, Error> {
    if c.eat_word("prefer") {
        return Ok(None);
    }
    c.expect_word("model")?;
    quoted_model(c).map(Some)
}

/// `"carriers@1"`
fn quoted_model(c: &mut Cursor) -> Result<String, Error> {
    let Some(Tok::Str(m)) = c.peek() else {
        return Err(c.error(format!("expected the model's name in quotes, as in \"carriers@1\", found {}", c.found())));
    };
    let m = m.clone();
    c.next();
    Ok(m)
}

/// `within 300 milliseconds`, `within 2 seconds`, in milliseconds; a model's
/// deadline, so only after one (`asks`).
fn within(c: &mut Cursor, asks: bool) -> Result<Option<u64>, Error> {
    if !c.eat_word("within") {
        return Ok(None);
    }
    if !asks {
        return Err(c.error("`with prefer` answers at once; `within` bounds a model's answer"));
    }
    let n = match c.peek() {
        Some(Tok::Num(n)) => n.clone(),
        _ => return Err(c.error(format!("expected a duration, as in `within 300 milliseconds`, found {}", c.found()))),
    };
    if c.peek_at(1).is_some_and(|t| matches!(t, Tok::Word(w) if w == "millisecond" || w == "milliseconds")) {
        c.next();
        c.next();
        Ok(Some(n.parse::<u64>().map_err(|_| c.error(format!("`{n}` is not a whole number")))?))
    } else {
        Ok(Some(duration(c)? * 1000))
    }
}

/// `allow payments to receive Order's card, Order's address` (appendix N.2.3).
fn allow(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("allow")?;
    let adapter = c.word()?;
    c.expect_word("to")?;
    c.expect_word("receive")?;
    let mut fields = vec![];
    loop {
        let kind = c.kind_name()?;
        if !c.eat_poss() {
            return Err(c.error(format!("expected `{kind}'s` and the attribute, as in `Order's card`, found {}", c.found())));
        }
        fields.push((kind, c.word()?));
        if !(c.eat_sym(',') || c.eat_word("and")) {
            break;
        }
    }
    c.expect_end()?;
    out.allows.push(AllowDecl { adapter, fields, at });
    Ok(())
}

/// `apply rule early to Bookings created from version 2` (appendix M.2.3).
fn apply(st: &Stmt, at: Loc, out: &mut Source) -> Result<(), Error> {
    let toks = st.tokens();
    let mut c = Cursor::new(&toks, &st.file, st.line);
    c.expect_word("apply")?;
    c.expect_word("rule")?;
    let rule = c.word()?;
    c.expect_word("to")?;
    let plural = c.word()?;
    c.expect_word("created")?;
    c.expect_word("from")?;
    c.expect_word("version")?;
    let from = match c.peek() {
        Some(Tok::Num(n)) => n.parse::<u32>().map_err(|_| c.error(format!("`{n}` is not a version number")))?,
        _ => return Err(c.error(format!("expected a version number, as in `created from version 2`, found {}", c.found()))),
    };
    c.next();
    c.expect_end()?;
    out.applies.push(ApplyDecl { rule, plural, from, source: text(&toks), at });
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
    let mut data = vec![];
    if c.eat_word("with") {
        if !matches!(what, Admitted::Event { .. }) {
            return Err(c.error("only an event carries data; a new entity or a change is given its attributes"));
        }
        loop {
            let name = c.word()?;
            c.expect_sym(':')?;
            data.push((name, type_name(&mut c)?));
            if !(c.eat_sym(',') || c.eat_word("and")) {
                break;
            }
        }
    }
    let cond = if c.eat_word("where") { Some(condition(&mut c)?) } else { None };
    c.expect_end()?;
    out.admits.push(AdmitDecl { what, roles, data, cond, source: text(&toks[1..]), at });
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
