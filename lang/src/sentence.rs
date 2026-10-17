//! The controlled-English sentence parsers (spec §3.3 and appendix B.2).
//! Each pattern has exactly one meaning; the checker resolves the names.

use crate::ast::*;
use crate::lex::{Cursor, Error, Tok};

/// A literal, if one is next: `"text"`, `3`, `120 EUR`, `12.50 EUR`, `yes`, `no`.
pub fn literal(c: &mut Cursor) -> Result<Option<Literal>, Error> {
    match c.peek() {
        Some(Tok::Str(s)) => {
            let s = s.clone();
            c.next();
            Ok(Some(Literal::Text(s)))
        }
        Some(Tok::Num(n)) => {
            let n = n.clone();
            c.next();
            if let Some(secs) = time_unit(c.peek()) {
                c.next();
                let k: i64 = n.parse().map_err(|_| c.error(format!("`{n}` is not a whole number")))?;
                let d = k.checked_mul(secs).ok_or_else(|| c.error(format!("`{n}` is too long a duration")))?;
                return Ok(Some(Literal::Duration(d)));
            }
            let currency = match c.peek() {
                Some(Tok::Word(w)) if w.len() == 3 && w.chars().all(|ch| ch.is_ascii_uppercase()) => Some(w.clone()),
                _ => None,
            };
            match currency {
                Some(currency) => {
                    c.next();
                    let cents = money(&n).map_err(|m| c.error(m))?;
                    Ok(Some(Literal::Money { cents, currency }))
                }
                None => n
                    .parse::<i64>()
                    .map(|v| Some(Literal::Count(v)))
                    .map_err(|_| c.error(format!("`{n}` is not a whole number"))),
            }
        }
        Some(Tok::Word(w)) if w == "yes" || w == "true" => {
            c.next();
            Ok(Some(Literal::Bool(true)))
        }
        Some(Tok::Word(w)) if w == "no" || w == "false" => {
            c.next();
            Ok(Some(Literal::Bool(false)))
        }
        _ => Ok(None),
    }
}

/// The seconds in a time unit, if the token is one: `second`, `minutes`, `days`…
fn time_unit(t: Option<&Tok>) -> Option<i64> {
    let Some(Tok::Word(w)) = t else { return None };
    Some(match w.as_str() {
        "second" | "seconds" => 1,
        "minute" | "minutes" => 60,
        "hour" | "hours" => 3_600,
        "day" | "days" => 86_400,
        "week" | "weeks" => 604_800,
        _ => return None,
    })
}

/// An amount in cents: at most two decimals.
fn money(n: &str) -> Result<i64, String> {
    let bad = || format!("`{n}` is not an amount with at most two decimals");
    let (whole, frac) = n.split_once('.').unwrap_or((n, ""));
    if whole.is_empty() || frac.len() > 2 || frac.contains('.') {
        return Err(bad());
    }
    let w: i64 = whole.parse().map_err(|_| bad())?;
    let f: i64 = if frac.is_empty() { 0 } else { format!("{frac:0<2}").parse().map_err(|_| bad())? };
    Ok(w * 100 + f)
}

pub fn starts_noun_phrase(c: &Cursor) -> bool {
    ["it", "its", "their", "the", "each"].iter().any(|w| c.is_word(w))
}

/// Whether a calculation starts here: a noun phrase, a number or `(`.
pub fn starts_calc(c: &Cursor) -> bool {
    starts_noun_phrase(c) || matches!(c.peek(), Some(Tok::Num(_) | Tok::Sym('(')))
}

/// `it`, `its x`, `their x`, `the Kind`, `the x`, followed by any number of `'s y`.
pub fn noun_phrase(c: &mut Cursor) -> Result<NounPhrase, Error> {
    let mut np = if c.eat_word("it") {
        NounPhrase { head: Head::It, steps: vec![] }
    } else if c.eat_word("its") || c.eat_word("their") {
        NounPhrase { head: Head::It, steps: vec![c.word()?] }
    } else if c.eat_word("each") {
        NounPhrase { head: Head::Each(c.kind_name()?), steps: vec![] }
    } else if c.eat_word("the") {
        let w = c.word()?;
        if w.starts_with(char::is_uppercase) {
            NounPhrase { head: Head::The(w), steps: vec![] }
        } else {
            NounPhrase { head: Head::It, steps: vec![w] }
        }
    } else {
        return Err(c.error(format!("expected `it`, `its` or `the`, found {}", c.found())));
    };
    while c.eat_poss() {
        np.steps.push(c.word()?);
    }
    Ok(np)
}

fn it() -> NounPhrase {
    NounPhrase { head: Head::It, steps: vec![] }
}

/// What follows `is` or `is not`: a calculation, or a bare word.
fn term(c: &mut Cursor) -> Result<Term, Error> {
    let calc = starts_calc(c) || matches!(c.peek(), Some(Tok::Str(_))) || ["yes", "no", "true", "false"].iter().any(|w| c.is_word(w));
    if !calc {
        return Ok(Term::Word(state_word(c)?));
    }
    Ok(match calc_expr(c)? {
        Num::Lit(l) => Term::Lit(l),
        Num::Np(np) => Term::Np(np),
        n => Term::Calc(n),
    })
}

/// A calculation: sums of products, `×` and `/` binding tighter (appendix E.2.3).
pub fn calc_expr(c: &mut Cursor) -> Result<Num, Error> {
    sum_of(c, false)
}

/// A formula after `=` in a kind (appendix E.2.5): a calculation in which a
/// bare attribute stands for the entity's own (`seats * 12 EUR`, `order's total`).
pub fn calc_formula(c: &mut Cursor) -> Result<Num, Error> {
    sum_of(c, true)
}

fn sum_of(c: &mut Cursor, bare: bool) -> Result<Num, Error> {
    let mut left = product(c, bare)?;
    loop {
        let op = if c.eat_sym('+') {
            '+'
        } else if c.eat_sym('-') || c.eat_sym('−') {
            '-'
        } else {
            return Ok(left);
        };
        left = Num::Bin(Box::new(left), op, Box::new(product(c, bare)?));
    }
}

fn product(c: &mut Cursor, bare: bool) -> Result<Num, Error> {
    let mut left = atom(c, bare)?;
    loop {
        let op = if c.eat_sym('*') || c.eat_sym('×') {
            '*'
        } else if c.eat_sym('/') {
            '/'
        } else {
            return Ok(left);
        };
        left = Num::Bin(Box::new(left), op, Box::new(atom(c, bare)?));
    }
}

fn word_at(c: &Cursor, k: usize, w: &str) -> bool {
    matches!(c.peek_at(k), Some(Tok::Word(x)) if x == w)
}

/// `( … )`, a literal, `the number of …`, `the total ⟨attr⟩ of …`, or a noun phrase.
fn atom(c: &mut Cursor, bare: bool) -> Result<Num, Error> {
    if c.eat_sym('(') {
        let n = sum_of(c, bare)?;
        c.expect_sym(')')?;
        return Ok(n);
    }
    if word_at(c, 0, "the") && word_at(c, 1, "total") && word_at(c, 2, "of") {
        c.expect_word("the")?;
        c.expect_word("total")?;
        c.expect_word("of")?;
        let calc = Box::new(sum_of(c, bare)?);
        c.expect_word("over")?;
        return Ok(Num::Over { calc, list: noun_phrase(c)? });
    }
    if word_at(c, 0, "the") && word_at(c, 1, "number") && word_at(c, 2, "of") {
        c.expect_word("the")?;
        c.expect_word("number")?;
        c.expect_word("of")?;
        let of = noun_phrase(c)?;
        return Ok(Num::Number { of, filter: that(c)? });
    }
    if word_at(c, 0, "the") && word_at(c, 1, "total") && word_at(c, 3, "taken") && word_at(c, 4, "from") {
        c.expect_word("the")?;
        c.expect_word("total")?;
        let attr = c.word()?;
        c.expect_word("taken")?;
        c.expect_word("from")?;
        return Ok(Num::Taken { attr, from: noun_phrase(c)? });
    }
    if word_at(c, 0, "the") && word_at(c, 1, "total") && word_at(c, 3, "of") {
        c.expect_word("the")?;
        c.expect_word("total")?;
        let attr = c.word()?;
        c.expect_word("of")?;
        let of = noun_phrase(c)?;
        return Ok(Num::Total { attr, of, filter: that(c)? });
    }
    if let Some(l) = literal(c)? {
        return Ok(Num::Lit(l));
    }
    let bare_word = matches!(c.peek(), Some(Tok::Word(w)) if w.starts_with(char::is_lowercase) && !starts_noun_phrase(c));
    if bare && bare_word {
        let mut np = NounPhrase { head: Head::It, steps: vec![c.word()?] };
        while c.eat_poss() {
            np.steps.push(c.word()?);
        }
        return Ok(Num::Np(np));
    }
    Ok(Num::Np(noun_phrase(c)?))
}

/// `that ⟨cond⟩` after a relation: one condition about each of its
/// entities. It never takes an `and` or `or`, which belong to the sentence
/// around it; name a combined condition with `define`.
fn that(c: &mut Cursor) -> Result<Option<Box<Cond>>, Error> {
    Ok(if c.eat_word("that") { Some(Box::new(simple(c)?)) } else { None })
}

/// A state's name: one word, or `timed out` (appendix C.2).
pub fn state_word(c: &mut Cursor) -> Result<String, Error> {
    let w = c.word()?;
    Ok(if w == "timed" && c.eat_word("out") { "timed out".into() } else { w })
}

/// What follows a noun phrase: `is [not] t`, `is at most t` and the other
/// comparisons, `has a K [that is s]`, or `has no K [that is s]`.
pub fn predicate(c: &mut Cursor, left: Num) -> Result<Cond, Error> {
    let contains = if c.eat_word("contains") || c.eat_word("contain") {
        Some(false)
    } else if (c.is_word("does") || c.is_word("do")) && word_at(c, 1, "not") {
        c.next();
        c.next();
        c.expect_word("contain")?;
        Some(true)
    } else {
        None
    };
    if let Some(negated) = contains {
        let Num::Np(set) = left else {
            return Err(c.error("only a set contains something; a calculation does not"));
        };
        return Ok(Cond::Contains { set, item: noun_phrase(c)?, negated });
    }
    if c.eat_word("has") || c.eat_word("have") {
        let Num::Np(left) = left else {
            return Err(c.error("only a noun phrase has parts; a calculation has none"));
        };
        let negated = if c.eat_word("no") {
            true
        } else if c.eat_word("a") || c.eat_word("an") {
            false
        } else {
            return Err(c.error(format!("expected `a`, `an` or `no` after `has`, found {}", c.found())));
        };
        let kind = c.kind_name()?;
        let state = if c.eat_word("that") {
            c.expect_word("is")?;
            Some(state_word(c)?)
        } else {
            None
        };
        let first = Cond::Has { owner: left.clone(), negated, kind, state };
        if !negated && c.is_word("or") && (word_at(c, 1, "a") || word_at(c, 1, "an")) {
            c.expect_word("or")?;
            if !c.eat_word("a") {
                c.expect_word("an")?;
            }
            let kind = c.kind_name()?;
            let state = if c.eat_word("that") {
                c.expect_word("is")?;
                Some(state_word(c)?)
            } else {
                None
            };
            return Ok(Cond::Or(vec![first, Cond::Has { owner: left, negated: false, kind, state }]));
        }
        return Ok(first);
    }
    if !(c.eat_word("is") || c.eat_word("are")) {
        return Err(c.error(format!("expected `is`, found {}", c.found())));
    }
    let op = if c.eat_word("at") {
        if c.eat_word("most") {
            Some(CmpOp::AtMost)
        } else {
            c.expect_word("least")?;
            Some(CmpOp::AtLeast)
        }
    } else if c.eat_word("more") {
        c.expect_word("than")?;
        Some(CmpOp::MoreThan)
    } else if c.eat_word("less") {
        c.expect_word("than")?;
        Some(CmpOp::LessThan)
    } else {
        None
    };
    if let Some(op) = op {
        return Ok(Cond::Cmp { left, op, right: term(c)? });
    }
    let negated = c.eat_word("not");
    Ok(Cond::Is { left, negated, right: term(c)? })
}

/// One condition. One that starts with `is` or `has` is about the subject.
fn simple(c: &mut Cursor) -> Result<Cond, Error> {
    let about_it = ["is", "are", "has", "have"].iter().any(|w| c.is_word(w));
    let left = if about_it { Num::Np(it()) } else { calc_expr(c)? };
    predicate(c, left)
}

/// Conditions joined by `and` or by `or`; one sentence cannot mix the two.
/// After a joiner, a bare state shares the previous condition's left side:
/// `it is cancelled and paid and not refunded`.
pub fn condition(c: &mut Cursor) -> Result<Cond, Error> {
    let first = simple(c)?;
    let joiner = match c.peek() {
        Some(Tok::Word(w)) if w == "and" || w == "or" => w.clone(),
        _ => return Ok(first),
    };
    let mut parts = vec![first];
    while c.eat_word(&joiner) {
        let next = if ["is", "are", "has", "have"].iter().any(|w| c.is_word(w)) || starts_calc(c) {
            simple(c)?
        } else {
            let Some(Cond::Is { left, .. }) = parts.last() else {
                return Err(c.error(format!("expected a condition after `{joiner}`, found {}", c.found())));
            };
            let left = left.clone();
            let negated = c.eat_word("not");
            Cond::Is { left, negated, right: Term::Word(state_word(c)?) }
        };
        parts.push(next);
    }
    if c.is_word("and") || c.is_word("or") {
        return Err(c.error("one sentence cannot mix `and` with `or`; name a part with `define`"));
    }
    Ok(if joiner == "and" { Cond::And(parts) } else { Cond::Or(parts) })
}

/// `[state] Kind [whose attr is [not] t] [that cond]`.
pub fn subject(c: &mut Cursor) -> Result<Subject, Error> {
    let adjective = match c.peek() {
        Some(Tok::Word(w)) if !w.starts_with(char::is_uppercase) => {
            let w = w.clone();
            c.next();
            Some(w)
        }
        _ => None,
    };
    let kind = c.kind_name()?;
    let mut filter = vec![];
    if let Some(state) = adjective {
        filter.push(Cond::Is { left: Num::Np(it()), negated: false, right: Term::Word(state) });
    }
    if c.eat_word("whose") {
        let attr = c.word()?;
        filter.push(predicate(c, Num::Np(NounPhrase { head: Head::It, steps: vec![attr] }))?);
    }
    if c.eat_word("that") {
        filter.push(condition(c)?);
    }
    Ok(Subject { kind, filter })
}

/// `every S's attr …`, `every S is …`, `no S has … [unless cond]`, `no S is … [unless cond]`.
pub fn rule_body(c: &mut Cursor) -> Result<RuleBody, Error> {
    if c.is_word("the") {
        // A bound for each entity: `the total amount of each Order's Refunds is at most its total`.
        let cond = condition(c)?;
        let Some(kind) = each_in_cond(&cond) else {
            return Err(c.error("a bound names what it is about with `each`, as in `the total amount of each Order's Refunds`"));
        };
        return Ok(RuleBody::Every { subject: Subject { kind, filter: vec![] }, cond });
    }
    if c.eat_word("every") {
        let subject = subject(c)?;
        let mut left = it();
        while c.eat_poss() {
            left.steps.push(c.word()?);
        }
        let cond = predicate(c, Num::Np(left))?;
        return Ok(RuleBody::Every { subject, cond });
    }
    if !c.eat_word("no") {
        return Err(c.error(format!("a rule starts with `every` or `no`, found {}", c.found())));
    }
    let subject = subject(c)?;
    let pred = predicate(c, Num::Np(it()))?;
    let unless = if c.eat_word("unless") { Some(condition(c)?) } else { None };
    Ok(RuleBody::No { subject, pred, unless })
}

/// The kind named by `each` in a bound, if any.
fn each_in_cond(c: &Cond) -> Option<String> {
    fn num(n: &Num) -> Option<String> {
        let np = |np: &NounPhrase| if let Head::Each(k) = &np.head { Some(k.clone()) } else { None };
        match n {
            Num::Np(x) | Num::Total { of: x, .. } | Num::Number { of: x, .. } | Num::Taken { from: x, .. } => np(x),
            Num::Bin(a, _, b) => num(a).or_else(|| num(b)),
            Num::Over { list, .. } => np(list),
            Num::Lit(_) => None,
        }
    }
    match c {
        Cond::Is { left, .. } | Cond::Cmp { left, .. } => num(left),
        Cond::And(cs) | Cond::Or(cs) => cs.iter().find_map(each_in_cond),
        Cond::Has { .. } | Cond::Contains { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lex::tokens;

    fn with<T>(src: &str, f: impl FnOnce(&mut Cursor) -> Result<T, Error>) -> Result<T, Error> {
        let toks = tokens(src).unwrap();
        let mut c = Cursor::new(&toks, "t.st", 1);
        let out = f(&mut c)?;
        c.expect_end()?;
        Ok(out)
    }

    fn np(steps: &[&str]) -> NounPhrase {
        NounPhrase { head: Head::It, steps: steps.iter().map(|s| s.to_string()).collect() }
    }

    fn word(w: &str) -> Term {
        Term::Word(w.into())
    }

    fn is(left: NounPhrase, negated: bool, right: &str) -> Cond {
        Cond::Is { left: Num::Np(left), negated, right: word(right) }
    }

    #[test]
    fn timed_out_is_one_state() {
        assert_eq!(with("its Payment is timed out", condition).unwrap(), is(np(&["Payment"]), false, "timed out"));
        assert_eq!(with("it is not timed out", condition).unwrap(), is(np(&[]), true, "timed out"));
        let has = with("it has a Payment that is timed out", condition).unwrap();
        assert_eq!(has, Cond::Has { owner: np(&[]), negated: false, kind: "Payment".into(), state: Some("timed out".into()) });
        let both = with("it is cancelled and timed out", condition).unwrap();
        assert_eq!(both, Cond::And(vec![is(np(&[]), false, "cancelled"), is(np(&[]), false, "timed out")]));
    }

    #[test]
    fn literals() {
        let eur = |cents| Some(Literal::Money { cents, currency: "EUR".into() });
        assert_eq!(with("120 EUR", literal).unwrap(), eur(12000));
        assert_eq!(with("12.5 EUR", literal).unwrap(), eur(1250));
        assert_eq!(with("3", literal).unwrap(), Some(Literal::Count(3)));
        assert_eq!(with("\"L-77\"", literal).unwrap(), Some(Literal::Text("L-77".into())));
        assert_eq!(with("no", literal).unwrap(), Some(Literal::Bool(false)));
        assert!(with("1.234 EUR", literal).is_err());
    }

    #[test]
    fn noun_phrases() {
        assert_eq!(with("its form's order's total", noun_phrase).unwrap(), np(&["form", "order", "total"]));
        assert_eq!(with("the order's total", noun_phrase).unwrap(), np(&["order", "total"]));
        assert_eq!(
            with("the Order's country", noun_phrase).unwrap(),
            NounPhrase { head: Head::The("Order".into()), steps: vec!["country".into()] }
        );
        assert_eq!(with("it", noun_phrase).unwrap(), np(&[]));
        assert!(with("Order", noun_phrase).is_err());
    }

    #[test]
    fn conditions() {
        assert_eq!(with("its country is France", condition).unwrap(), is(np(&["country"]), false, "France"));
        assert_eq!(with("it is not cancelled", condition).unwrap(), is(np(&[]), true, "cancelled"));
        assert_eq!(
            with("its value is at most its form's order's total", condition).unwrap(),
            Cond::Cmp { left: Num::Np(np(&["value"])), op: CmpOp::AtMost, right: Term::Np(np(&["form", "order", "total"])) }
        );
        assert_eq!(
            with("has a Shipment that is sent", condition).unwrap(),
            Cond::Has { owner: np(&[]), negated: false, kind: "Shipment".into(), state: Some("sent".into()) }
        );
        assert_eq!(
            with("it is cancelled and paid and not refunded", condition).unwrap(),
            Cond::And(vec![is(np(&[]), false, "cancelled"), is(np(&[]), false, "paid"), is(np(&[]), true, "refunded")])
        );
        let e = with("it is paid and settled or cancelled", condition).unwrap_err();
        assert!(e.msg.contains("cannot mix"), "{e}");
    }

    #[test]
    fn subjects_and_rule_bodies() {
        let r = with(
            "no Order whose country is France has a Shipment that is sent unless its CustomsForm is done",
            rule_body,
        )
        .unwrap();
        let RuleBody::No { subject, pred, unless } = r else { panic!("not a `no` rule") };
        assert_eq!(subject.kind, "Order");
        assert_eq!(subject.filter, vec![is(np(&["country"]), false, "France")]);
        assert!(matches!(pred, Cond::Has { ref kind, .. } if kind == "Shipment"));
        assert_eq!(unless, Some(is(np(&["CustomsForm"]), false, "done")));

        let r = with("every Declaration's value is at most its form's order's total", rule_body).unwrap();
        let RuleBody::Every { subject, cond } = r else { panic!("not an `every` rule") };
        assert_eq!((subject.kind.as_str(), subject.filter.len()), ("Declaration", 0));
        assert!(matches!(cond, Cond::Cmp { op: CmpOp::AtMost, .. }));

        let r = with("no cancelled Order has a Shipment that is sent", rule_body).unwrap();
        let RuleBody::No { subject, unless, .. } = r else { panic!("not a `no` rule") };
        assert_eq!(subject.filter, vec![is(np(&[]), false, "cancelled")]);
        assert_eq!(unless, None);

        let e = with("some Order is paid", rule_body).unwrap_err();
        assert!(e.msg.contains("starts with `every` or `no`"), "{e}");
    }
}
