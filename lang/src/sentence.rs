//! The controlled-English sentence parsers (spec §3.3 and appendix B.2).
//! Each pattern has exactly one meaning; the checker resolves the names.

use crate::ast::*;
use crate::lex::{Cursor, Error, Tok};

/// A literal, if one is next: `"text"`, `3`, `120 EUR`, `12.50 EUR`,
/// `-3 EUR`, `2026-03-01`, `2026-03-01 14:30`, `yes`, `no`, and in
/// scenarios `file "dent.png"`.
pub fn literal(c: &mut Cursor) -> Result<Option<Literal>, Error> {
    if matches!(c.peek(), Some(Tok::Sym('-' | '−'))) && matches!(c.peek_at(1), Some(Tok::Num(_))) {
        // Only an amount is negative (appendix AB.2.2).
        c.next();
        return match literal(c)? {
            Some(Literal::Money { cents, currency }) => Ok(Some(Literal::Money { cents: -cents, currency })),
            _ => Err(c.error("only an amount can be negative, as in `-3 EUR`")),
        };
    }
    if let Some(t) = date(c)? {
        return Ok(Some(Literal::Time(t)));
    }
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
                let max = crate::parse::MAX_DURATION_SECS as i64;
                let d = k.checked_mul(secs).filter(|d| d.abs() <= max).ok_or_else(|| c.error(format!("`{n}` is too long: a duration is at most 100 years")))?;
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
        Some(Tok::Word(w)) if w == "file" && matches!(c.peek_at(1), Some(Tok::Str(_))) => {
            c.next();
            let Some(Tok::Str(path)) = c.next() else { unreachable!("peeked") };
            Ok(Some(Literal::File(path.clone())))
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
/// `2026-03-01` or `2026-03-01 14:30`, in UTC, as seconds on the runtime
/// clock, whose 0 is 2026-01-01 00:00 (appendix AB.2.3).
fn date(c: &mut Cursor) -> Result<Option<i64>, Error> {
    let digits = |k: usize, n: usize| matches!(c.peek_at(k), Some(Tok::Num(x)) if x.len() == n && x.bytes().all(|b| b.is_ascii_digit()));
    let dash = |k: usize| matches!(c.peek_at(k), Some(Tok::Sym('-')));
    if !(digits(0, 4) && dash(1) && digits(2, 2) && dash(3) && digits(4, 2)) {
        return Ok(None);
    }
    let num = |k: usize| match c.peek_at(k) {
        Some(Tok::Num(x)) => x.parse::<i64>().unwrap_or(0),
        _ => 0,
    };
    let (y, m, d) = (num(0), num(2), num(4));
    let clock = (digits(5, 1) || digits(5, 2)) && matches!(c.peek_at(6), Some(Tok::Sym(':'))) && digits(7, 2);
    let (h, min) = if clock { (num(5), num(7)) } else { (0, 0) };
    let text = if clock { format!("{y:04}-{m:02}-{d:02} {h}:{min:02}") } else { format!("{y:04}-{m:02}-{d:02}") };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    if !(1..=12).contains(&m) || d < 1 || d > days_in[(m - 1) as usize] || h > 23 || min > 59 {
        return Err(c.error(format!("`{text}` is not a date: write a year, month and day, as in `2026-03-01`, and a time as in `14:30`")));
    }
    for _ in 0..if clock { 8 } else { 5 } {
        c.next();
    }
    Ok(Some((days_from_civil(y, m, d) - DAY_ZERO) * 86_400 + h * 3_600 + min * 60))
}

/// `2026-03-01`, or `2026-03-01 14:30` for a time that is not midnight.
pub fn date_text(t: i64) -> String {
    let (y, m, d, secs) = civil(t);
    let day = format!("{y:04}-{m:02}-{d:02}");
    if secs == 0 { day } else { format!("{day} {}:{:02}", secs / 3_600, secs % 3_600 / 60) }
}

/// A time as RFC 3339 gives it, in UTC: `2026-03-01T14:30:00Z` (appendix AM).
pub fn rfc3339(t: i64) -> String {
    let (y, m, d, secs) = civil(t);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", secs / 3_600, secs % 3_600 / 60, secs % 60)
}

/// The civil date of runtime seconds, and the seconds into that day.
fn civil(t: i64) -> (i64, i64, i64, i64) {
    let z = DAY_ZERO + t.div_euclid(86_400) + 719_468;
    let secs = t.rem_euclid(86_400);
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d, secs)
}

/// An ISO date or date and time, `2026-03-01` or `2026-03-01T14:30[:00[.250]]`,
/// with RFC 3339's `Z` or `+02:00` offset or none (UTC), as runtime seconds;
/// a model and the API give times in this form (appendix AF.2.5, AM.2.1).
/// A fraction of a second is dropped.
pub fn iso_time(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |x: &str| if !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()) { x.parse::<i64>().ok() } else { None };
    let (date, time) = match s.split_once(['T', 't', ' ']) {
        Some((d, t)) => (d, t),
        None => (s.strip_suffix(['Z', 'z']).unwrap_or(s), "00:00Z"),
    };
    // The offset east of UTC, in seconds.
    let (time, offset) = if let Some(t) = time.strip_suffix(['Z', 'z']) {
        (t, 0)
    } else if let Some(i) = time.rfind(['+', '-']) {
        let (h, m) = time[i + 1..].split_once(':').unwrap_or((&time[i + 1..], ""));
        let (h, m) = (num(h).filter(|h| *h <= 23)?, if m.is_empty() { 0 } else { num(m).filter(|m| *m <= 59)? });
        let sign = if time[i..].starts_with('-') { -1 } else { 1 };
        (&time[..i], sign * (h * 3_600 + m * 60))
    } else {
        (time, 0)
    };
    let mut d = date.splitn(3, '-').map(num);
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let mut t = time.splitn(3, ':');
    let (h, min) = (num(t.next()?)?, num(t.next()?)?);
    let sec = match t.next() {
        None => 0,
        Some(x) => match x.split_once('.') {
            Some((s, frac)) if num(frac).is_some() => num(s)?,
            Some(_) => return None,
            None => num(x)?,
        },
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    // RFC 3339 has four-digit years, which also keeps the seconds in range.
    if !(0..=9999).contains(&y) || !(1..=12).contains(&m) || day < 1 || day > days_in[(m - 1) as usize] || h > 23 || min > 59 || sec > 59 {
        return None;
    }
    Some((days_from_civil(y, m, day) - DAY_ZERO) * 86_400 + h * 3_600 + min * 60 + sec - offset)
}

/// An ISO 8601 duration, `P3D`, `PT2H30M`, `P1W` or `-PT45S`, in seconds
/// (appendix AM.2.2). Years and months have no fixed length, so they are
/// not durations here.
pub fn iso_duration(s: &str) -> Option<i64> {
    let s = s.trim();
    let (sign, s) = match s.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, s.strip_prefix('+').unwrap_or(s)),
    };
    let s = s.strip_prefix(['P', 'p'])?;
    let (date, time) = match s.split_once(['T', 't']) {
        Some((d, t)) if !t.is_empty() => (d, Some(t)),
        Some(_) => return None,
        None => (s, None),
    };
    let mut total: i64 = 0;
    let mut any = false;
    let mut add = |part: &str, units: &[(char, i64)]| -> Option<()> {
        let mut rest = part;
        let mut last = 0;
        while !rest.is_empty() {
            let i = rest.find(|c: char| !c.is_ascii_digit())?;
            let n: i64 = rest[..i].parse().ok()?;
            let u = rest[i..].chars().next()?.to_ascii_uppercase();
            // Units come in order, each once.
            let at = units.iter().position(|(c, _)| *c == u).filter(|p| *p + 1 > last)?;
            last = at + 1;
            total = total.checked_add(n.checked_mul(units[at].1)?)?;
            any = true;
            rest = &rest[i + 1..];
        }
        Some(())
    };
    add(date, &[('W', 604_800), ('D', 86_400)])?;
    if let Some(t) = time {
        add(t, &[('H', 3_600), ('M', 60), ('S', 1)])?;
    }
    (any && total <= crate::parse::MAX_DURATION_SECS as i64).then_some(sign * total)
}

/// Seconds as an ISO 8601 duration: `P3D`, `PT2H30M`, `PT0S`, `-P1DT12H`.
pub fn iso_duration_text(secs: i64) -> String {
    let sign = if secs < 0 { "-" } else { "" };
    let s = secs.unsigned_abs();
    let (d, h, m, x) = (s / 86_400, s % 86_400 / 3_600, s % 3_600 / 60, s % 60);
    let mut out = format!("{sign}P");
    if d > 0 {
        out += &format!("{d}D");
    }
    if h + m + x > 0 || d == 0 {
        out.push('T');
        for (n, u) in [(h, 'H'), (m, 'M'), (x, 'S')] {
            if n > 0 {
                out += &format!("{n}{u}");
            }
        }
        if h + m + x == 0 {
            out += "0S";
        }
    }
    out
}

/// 2026-01-01, the runtime clock's 0, as days since 1970-01-01.
const DAY_ZERO: i64 = 20_454;

/// Days since 1970-01-01 of a civil date (Howard Hinnant's algorithm).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

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
    starts_noun_phrase(c)
        || matches!(c.peek(), Some(Tok::Num(_) | Tok::Sym('(')))
        || matches!((c.peek(), c.peek_at(1)), (Some(Tok::Sym('-' | '−')), Some(Tok::Num(_))))
}

/// `it`, `its x`, `their x`, `the Kind`, `the x`, followed by any number of `'s y`.
pub fn noun_phrase(c: &mut Cursor) -> Result<NounPhrase, Error> {
    let mut np = if c.eat_word("it") {
        NounPhrase { head: Head::It, steps: vec![] }
    } else if c.eat_word("its") || c.eat_word("their") {
        NounPhrase { head: Head::It, steps: vec![c.word()?] }
    } else if c.eat_word("each") {
        NounPhrase { head: Head::Each(c.kind_name()?), steps: vec![] }
    } else if c.eat_word("all") {
        NounPhrase { head: Head::All(c.kind_name()?), steps: vec![] }
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
    if word_at(c, 0, "the") && word_at(c, 1, "days") && word_at(c, 2, "in") && word_at(c, 3, "a") && word_at(c, 4, "row") && word_at(c, 5, "of") {
        for w in ["the", "days", "in", "a", "row", "of"] {
            c.expect_word(w)?;
        }
        let of = noun_phrase(c)?;
        return Ok(Num::Run { of, filter: that(c)? });
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
        let that = |c: &mut Cursor| -> Result<Option<String>, Error> {
            if c.eat_word("that") {
                c.expect_word("is")?;
                return Ok(Some(state_word(c)?));
            }
            Ok(None)
        };
        let kind = c.kind_name()?;
        let mut each = vec![(kind, that(c)?)];
        while !negated && c.is_word("or") && (word_at(c, 1, "a") || word_at(c, 1, "an")) {
            c.expect_word("or")?;
            if !c.eat_word("a") {
                c.expect_word("an")?;
            }
            let kind = c.kind_name()?;
            each.push((kind, that(c)?));
        }
        // `has a Writeoff or a Cash that is sent`: the last `that` is about each
        // of them, unless another has one of its own (appendix W.2.2).
        let last = each.last().and_then(|(_, s)| s.clone());
        if each.len() > 1 && each[..each.len() - 1].iter().all(|(_, s)| s.is_none()) {
            for e in &mut each {
                e.1 = last.clone();
            }
        }
        let mut has: Vec<Cond> = each.into_iter().map(|(kind, state)| Cond::Has { owner: left.clone(), negated, kind, state }).collect();
        return Ok(if has.len() == 1 { has.remove(0) } else { Cond::Or(has) });
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
            return Err(c.error("a bound names what it is about with `each`, as in `the total amount of each Order's Refunds`, or `all`, as in `the total amount of all Refunds`"));
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

/// `the total days of each Allocation's Applications [that …]`, `the number
/// of each …`: a total and the kind named by its `each` (appendix AI).
pub fn kept_total(c: &mut Cursor) -> Result<(String, Num), Error> {
    let total = atom(c, false)?;
    match &total {
        Num::Total { of, .. } | Num::Number { of, .. } if matches!(&of.head, Head::Each(_)) && !of.steps.is_empty() => {
            let Head::Each(k) = &of.head else { unreachable!("matched above") };
            Ok((k.clone(), total.clone()))
        }
        _ => Err(c.error("`keep` names a total of each entity, as in `keep the total days of each Allocation's Applications` or `keep the number of each Allocation's Applications that are open`")),
    }
}

/// The kind named by `each` in a bound, if any.
fn each_in_cond(c: &Cond) -> Option<String> {
    fn num(n: &Num) -> Option<String> {
        let np = |np: &NounPhrase| match &np.head {
            Head::Each(k) => Some(k.clone()),
            Head::All(k) => Some(format!("all {k}")),
            _ => None,
        };
        match n {
            Num::Np(x) | Num::Total { of: x, .. } | Num::Number { of: x, .. } | Num::Taken { from: x, .. } | Num::Run { of: x, .. } => np(x),
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
