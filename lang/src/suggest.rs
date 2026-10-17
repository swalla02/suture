//! Errors and suggestions (spec §3.5, appendix H): the phrasing table, the
//! closest pattern, and a `did you mean` that is shown only if it compiles.
//! Everything here is fixed code; no model decides anything (D4).

use crate::ast::Source;
use crate::lex::{Error, Stmt, Tok};
use crate::parse::{statement, text};

/// Common alternative phrasings and the canonical phrase for each
/// (appendix H.2.2). Only suggestions use it; programs never do.
const PHRASES: &[(&str, &str)] = &[
    ("can't be more than", "is at most"),
    ("cannot be more than", "is at most"),
    ("can not be more than", "is at most"),
    ("may not be more than", "is at most"),
    ("must not be more than", "is at most"),
    ("may not exceed", "is at most"),
    ("must not exceed", "is at most"),
    ("cannot exceed", "is at most"),
    ("can't exceed", "is at most"),
    ("does not exceed", "is at most"),
    ("doesn't exceed", "is at most"),
    ("must be at most", "is at most"),
    ("is no more than", "is at most"),
    ("is not more than", "is at most"),
    ("is less than or equal to", "is at most"),
    ("can't be less than", "is at least"),
    ("cannot be less than", "is at least"),
    ("must be at least", "is at least"),
    ("is no less than", "is at least"),
    ("is not less than", "is at least"),
    ("is greater than or equal to", "is at least"),
    ("is greater than", "is more than"),
    ("is bigger than", "is more than"),
    ("is larger than", "is more than"),
    ("exceeds", "is more than"),
    ("is smaller than", "is less than"),
    ("is fewer than", "is less than"),
    ("is equal to", "is"),
    ("equals", "is"),
    ("must be", "is"),
    ("should be", "is"),
    ("isn't", "is not"),
    ("must not be", "is not"),
    ("can't be", "is not"),
    ("cannot be", "is not"),
    ("doesn't have", "has no"),
    ("does not have", "has no"),
    ("hasn't got", "has no"),
    ("the sum of", "the total"),
    ("the count of", "the number of"),
    ("how many", "the number of"),
    ("for every", "for each"),
];

fn words(phrase: &str) -> Vec<&str> {
    phrase.split(' ').collect()
}

/// The tokens with every table phrase replaced by its canonical phrase, the
/// longest first at each position, and the phrases found.
pub fn canonical(toks: &[Tok]) -> (Vec<Tok>, Vec<(&'static str, &'static str)>) {
    let mut table: Vec<(Vec<&str>, &str, &str)> = PHRASES.iter().map(|(p, c)| (words(p), *p, *c)).collect();
    table.sort_by_key(|(w, _, _)| std::cmp::Reverse(w.len()));
    let (mut out, mut hits) = (vec![], vec![]);
    let mut i = 0;
    'outer: while i < toks.len() {
        for (w, p, c) in &table {
            let fits = w.iter().enumerate().all(|(k, x)| matches!(toks.get(i + k), Some(Tok::Word(t)) if t.eq_ignore_ascii_case(x)));
            if fits {
                out.extend(words(c).into_iter().map(|x| Tok::Word(x.to_string())));
                if !hits.iter().any(|(q, _)| q == p) {
                    hits.push((*p, *c));
                }
                i += w.len();
                continue 'outer;
            }
        }
        out.push(toks[i].clone());
        i += 1;
    }
    (out, hits)
}

/// Whether the program compiles with `st` in it (appendix H.2.5).
fn compiles(src: &Source, st: &Stmt) -> bool {
    let mut with = src.clone();
    statement(st, &mut with).is_ok() && crate::check::check(&with).is_ok()
}

/// The text after a statement's `:`, as a read-back shows it.
fn sentence(toks: &[Tok]) -> String {
    toks.iter().position(|t| *t == Tok::Sym(':')).map_or_else(|| text(toks), |i| text(&toks[i + 1..]))
}

/// A parse error, improved with the phrasing table and, for a rule, the
/// closest pattern and a suggestion that compiles (spec §3.5).
pub fn improve(src: &Source, st: &Stmt, e: Error) -> Error {
    let toks = st.tokens();
    let first = match toks.first() {
        Some(Tok::Word(w)) => w.as_str(),
        _ => return e,
    };
    if !["rule", "define", "desired", "when"].contains(&first) {
        return e;
    }
    let what = match (first, toks.get(1)) {
        ("rule", Some(Tok::Word(name))) => format!("rule {name}: "),
        _ => String::new(),
    };
    let (fixed, hits) = canonical(&toks);
    let mut msg = match hits.first() {
        Some((p, c)) => format!("{what}\"{p}\" is not a Suture phrase; write \"{c}\""),
        None => format!("{what}{}", e.msg),
    };
    let mut lines = vec![];
    if !hits.is_empty() {
        let again = Stmt { file: st.file.clone(), line: st.line, lines: vec![fixed.clone()] };
        if compiles(src, &again) {
            lines.push(format!("did you mean:     {}", sentence(&fixed)));
        }
    }
    for l in lines {
        msg.push_str("\n  ");
        msg.push_str(&l);
    }
    Error { msg, ..e }
}
