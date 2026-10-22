//! Errors and suggestions (spec §3.5, appendix H): the phrasing table, the
//! closest pattern, and a `did you mean` that is shown only if it compiles.
//! Everything here is fixed code; no model decides anything (D4).

use crate::ast::{Num, Source, TypeName};
use crate::ir::PERIODS;
use crate::lex::{tokens, Error, Stmt, Tok};
use crate::parse::{statement, text};

/// Common alternative phrasings and the canonical phrase for each
/// (appendix H.2.2). Only suggestions use it; programs never do.
pub(crate) const PHRASES: &[(&str, &str)] = &[
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
    ("if it isn't", "unless it is"),
    ("if it is not", "unless it is"),
    ("may not have", "has no"),
    ("must not have", "has no"),
    ("cannot have", "has no"),
    ("can't have", "has no"),
    ("may have", "has"),
    ("can have", "has"),
    ("must have", "has"),
    ("should have", "has"),
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

/// Whether `st` adds no error to the program (appendix H.2.5): other
/// statements may still be wrong, and that is not the suggestion's fault.
fn compiles(src: &Source, st: &Stmt) -> bool {
    let mut with = src.clone();
    if statement(st, &mut with).is_err() {
        return false;
    }
    let before = crate::check::check(src).err().map_or(0, |e| e.len());
    match crate::check::check(&with) {
        Ok(_) => true,
        Err(e) => e.len() <= before && !e.iter().any(|e| e.file == st.file && e.line == st.line),
    }
}

/// The text after a statement's `:`, as a read-back shows it.
fn sentence(toks: &[Tok]) -> String {
    toks.iter().position(|t| *t == Tok::Sym(':')).map_or_else(|| text(toks), |i| text(&toks[i + 1..]))
}

/// The rule patterns of §3.3, in the order ties are broken (appendix H.2.3).
/// `<cmp>` is whichever comparison the user wrote.
const PATTERNS: &[&str] = &[
    "every <Kind> is <state>",
    "every <Kind> 's <attr> <cmp> <num>",
    "no <Kind> has a <Kind> unless <cond>",
    "no <Kind> has a <Kind> that is <state>",
    "no <Kind> has a <Kind> that is <state> unless <cond>",
    "no <Kind> whose <attr> is <value> has a <Kind>",
    "the total <attr> of each <Kind> 's <Kinds> <cmp> <num>",
    "the number of each <Kind> 's <Kinds> <cmp> <num>",
    "the total <attr> taken from each <Kind> <cmp> <num>",
    "for each <Kind> and <period> , the total <attr> of their <Kinds> <cmp> <num>",
];

const COMPARISONS: [&str; 4] = ["is at most", "is at least", "is more than", "is less than"];

fn ignored(w: &str) -> bool {
    ["a", "an", "the", "each", "every", "'s", ","].iter().any(|x| x.eq_ignore_ascii_case(w))
}

/// What a module declares, as the slots of a pattern see it.
struct Decls {
    /// (name, plural)
    kinds: Vec<(String, String)>,
    /// (kind, attribute, type)
    attrs: Vec<(String, String, TypeName)>,
    states: Vec<String>,
}

impl Decls {
    fn of(src: &Source) -> Self {
        let kinds = src.kinds.iter().map(|k| (k.name.clone(), k.plural.clone().unwrap_or_else(|| crate::check::plural_of(&k.name)))).collect();
        let attrs = src.kinds.iter().flat_map(|k| k.attrs.iter().map(|a| (k.name.clone(), a.name.clone(), a.ty.clone()))).collect();
        let mut states: Vec<String> = src.defines.iter().map(|d| d.state.clone()).collect();
        for e in &src.effects {
            states.extend(e.marks.iter().cloned());
            states.push(e.success.clone());
            states.extend(e.failures.iter().cloned());
        }
        for k in &src.kinds {
            states.extend(k.attrs.iter().filter(|a| a.ty == TypeName::Bool).map(|a| a.name.clone()));
            states.extend(k.attrs.iter().flat_map(|a| a.states.iter().cloned()));
        }
        Decls { kinds, attrs, states }
    }

    /// The word as the slot spells it, if it fits the slot.
    fn fits(&self, slot: &str, w: &str) -> Option<String> {
        match slot {
            "<Kind>" => self.kinds.iter().find(|(k, _)| k.eq_ignore_ascii_case(w)).map(|(k, _)| k.clone()),
            "<Kinds>" => self.kinds.iter().find(|(_, p)| p.eq_ignore_ascii_case(w)).map(|(_, p)| p.clone()),
            "<attr>" => self.attrs.iter().any(|(_, a, _)| a == w).then(|| w.to_string()),
            "<state>" => self.states.iter().any(|s| s == w).then(|| w.to_string()),
            "<value>" => (self.states.iter().any(|s| s == w)
                || self.attrs.iter().any(|(_, a, _)| a == w)
                || ["yes", "no"].contains(&w)
                || w.starts_with(|c: char| c == '"' || c.is_ascii_digit()))
            .then(|| w.to_string()),
            "<period>" => PERIODS.contains(&w).then(|| w.to_string()),
            _ => None,
        }
    }

    /// The kind whose plural is `p`.
    fn kind_of_plural(&self, p: &str) -> Option<&str> {
        self.kinds.iter().find(|(_, x)| x == p).map(|(k, _)| k.as_str())
    }
}

/// A rule sentence split at its comparison or its `unless` (appendix H.2.4).
struct Said {
    head: Vec<String>,
    /// `is at most`, …, or `unless`, and the text after it.
    split: Option<(String, String)>,
}

impl Said {
    fn of(toks: &[Tok]) -> Self {
        let word = |i: usize| match toks.get(i) {
            Some(Tok::Word(w)) => w.to_lowercase(),
            _ => String::new(),
        };
        for i in 0..toks.len() {
            let three = format!("{} {} {}", word(i), word(i + 1), word(i + 2));
            let split = if COMPARISONS.contains(&three.as_str()) {
                Some((three, i + 3))
            } else if word(i) == "unless" {
                Some(("unless".to_string(), i + 1))
            } else {
                None
            };
            if let Some((what, from)) = split {
                return Said { head: Self::words(&toks[..i]), split: Some((what, text(&toks[from..]))) };
            }
        }
        Said { head: Self::words(toks), split: None }
    }

    fn words(toks: &[Tok]) -> Vec<String> {
        toks.iter().filter(|t| !matches!(t, Tok::Sym(_))).map(|t| t.to_string()).collect()
    }
}

/// How many words differ between a pattern and what was said, and what
/// fills each of the pattern's words (appendix H.2.4).
fn score(d: &Decls, said: &Said, pattern: &str) -> (usize, Vec<Option<String>>) {
    let mut used = vec![false; said.head.len()];
    let (mut differ, mut fills) = (0, vec![]);
    let split = said.split.as_ref();
    let mut split_used = false;
    for el in pattern.split(' ') {
        let fill = match el {
            "<cmp>" | "unless" | "<num>" | "<cond>" => {
                let want_unless = matches!(el, "unless" | "<cond>");
                match split {
                    Some((w, tail)) if (w == "unless") == want_unless => {
                        split_used = true;
                        Some(if el.starts_with('<') && el != "<cmp>" { tail.clone() } else { w.clone() })
                    }
                    _ => {
                        if el == "<cmp>" || el == "unless" {
                            differ += 1;
                        }
                        None
                    }
                }
            }
            _ if el.starts_with('<') => {
                let hit = (0..used.len()).find_map(|i| (!used[i]).then(|| d.fits(el, &said.head[i]).map(|f| (i, f))).flatten());
                match hit {
                    Some((i, f)) => {
                        used[i] = true;
                        Some(f)
                    }
                    None => {
                        // An attribute the declarations can choose is not a difference.
                        if el != "<attr>" {
                            differ += 1;
                        }
                        None
                    }
                }
            }
            _ if ignored(el) => None,
            _ => {
                match (0..used.len()).find(|&i| !used[i] && said.head[i].eq_ignore_ascii_case(el)) {
                    Some(i) => used[i] = true,
                    None => differ += 1,
                }
                None
            }
        };
        fills.push(fill);
    }
    differ += (0..used.len()).filter(|&i| !used[i] && !ignored(&said.head[i])).count();
    if split.is_some() && !split_used {
        differ += 1;
    }
    (differ, fills)
}

/// The pattern as the user reads it: its comparison is theirs.
fn show_pattern(pattern: &str, said: &Said) -> String {
    let cmp = said.split.as_ref().filter(|(w, _)| w != "unless").map_or("is at most", |(w, _)| w.as_str());
    let out: Vec<String> = pattern
        .split(' ')
        .map(|el| match el {
            "<cmp>" => cmp.to_string(),
            _ if el.starts_with('<') => format!("⟨{}⟩", &el[1..el.len() - 1]),
            _ => el.to_string(),
        })
        .collect();
    out.join(" ").replace(" 's", "'s").replace(" ,", ",")
}

/// The pattern filled with the user's words, and where a slot is empty with
/// the one clear choice the declarations give (appendix H.2.5).
fn fill(d: &Decls, src: &Source, pattern: &str, fills: &[Option<String>]) -> Option<String> {
    let els: Vec<&str> = pattern.split(' ').collect();
    let slot = |name: &str| els.iter().position(|e| *e == name).and_then(|i| fills[i].clone());
    let mut out = vec![];
    for (el, f) in els.iter().zip(fills) {
        let w = match (*el, f) {
            (_, Some(f)) => f.clone(),
            ("<attr>", None) => {
                if pattern.contains("taken from") {
                    let target = slot("<Kind>")?;
                    let u = src.uses.iter().find(|u| u.target == target)?;
                    match &u.taking {
                        Some(Num::Np(np)) => np.steps.last()?.clone(),
                        _ => return None,
                    }
                } else if pattern.contains("<Kinds>") {
                    let kind = d.kind_of_plural(&slot("<Kinds>")?)?.to_string();
                    let of = |ty: TypeName| d.attrs.iter().find(|(k, _, t)| *k == kind && *t == ty).map(|(_, a, _)| a.clone());
                    of(TypeName::Money).or_else(|| of(TypeName::Count))?
                } else {
                    return None;
                }
            }
            (e, None) if e.starts_with('<') => return None,
            (e, None) => e.to_string(),
        };
        out.push(w);
    }
    Some(out.join(" ").replace(" 's", "'s").replace(" ,", ","))
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
    let rule = match (first, toks.get(1)) {
        ("rule", Some(Tok::Word(name))) => Some(name.clone()),
        _ => None,
    };
    let what = rule.as_ref().map_or_else(String::new, |n| format!("rule {n}: "));
    let (fixed, hits) = canonical(&toks);
    let restate = |sentence: &str| -> Option<Stmt> {
        let head = text(&toks[..toks.iter().position(|t| *t == Tok::Sym(':'))? + 1]);
        let line = tokens(&format!("{head} {sentence}")).ok()?;
        Some(Stmt { file: st.file.clone(), line: st.line, lines: vec![line], at: vec![st.line] })
    };
    let mut mean = None;
    if !hits.is_empty() && compiles(src, &Stmt { file: st.file.clone(), line: st.line, lines: vec![fixed.clone()], at: vec![st.line] }) {
        mean = Some(sentence(&fixed));
    }
    let mut closest = None;
    if rule.is_some() {
        let body = &fixed[fixed.iter().position(|t| *t == Tok::Sym(':')).map_or(0, |i| i + 1)..];
        let mut body = body.to_vec();
        if body.ends_with(&[Tok::Sym(','), Tok::Word("checked".into()), Tok::Word("globally".into())]) {
            body.truncate(body.len() - 3);
        }
        let said = Said::of(&body);
        let d = Decls::of(src);
        let best = PATTERNS.iter().map(|p| (p, score(&d, &said, p))).min_by_key(|(_, (n, _))| *n);
        if let Some((p, (_, fills))) = best {
            closest = Some(show_pattern(p, &said));
            if mean.is_none() {
                let globally = if body.len() < fixed.len() - fixed.iter().position(|t| *t == Tok::Sym(':')).map_or(0, |i| i + 1) { ", checked globally" } else { "" };
                // A suggestion keeps every negation, or it would say the opposite.
                let negations = |w: &[String]| w.iter().filter(|w| ["no", "not"].contains(&w.to_lowercase().as_str())).count();
                let said_neg = negations(&Said::words(&body));
                // Nor does it drop what follows the user's `unless` or comparison.
                let tail = said.split.as_ref().map(|(_, t)| t.clone());
                mean = fill(&d, src, p, &fills)
                    .filter(|s| tail.as_ref().is_none_or(|t| s.ends_with(t.as_str())))
                    .map(|s| format!("{s}{globally}"))
                    .filter(|s| tokens(s).is_ok_and(|t| negations(&Said::words(&t)) == said_neg))
                    .filter(|s| restate(s).is_some_and(|st| compiles(src, &st)));
            }
        }
    }
    let quoted = |f: fn(&(&'static str, &'static str)) -> &'static str| hits.iter().map(|h| format!("\"{}\"", f(h))).collect::<Vec<_>>().join(" and ");
    let mut msg = match (hits.len(), &mean) {
        (0, _) => format!("{what}{}", e.msg),
        (n, m) => {
            let are = if n == 1 { "is not a Suture phrase" } else { "are not Suture phrases" };
            let write = if m.is_none() { format!("; write {}", quoted(|h| h.1)) } else { String::new() };
            format!("{what}{} {are}{write}", quoted(|h| h.0))
        }
    };
    if let Some(c) = closest {
        msg.push_str(&format!("\n  closest pattern:  {c}"));
    }
    if let Some(m) = mean {
        msg.push_str(&format!("\n  did you mean:     {m}"));
    }
    Error { msg, ..e }
}

/// The edit distance between two words, counting a swap of two letters as one.
fn distance(a: &str, b: &str) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    let mut d = vec![vec![0; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    d[0] = (0..=b.len()).collect();
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

/// The one name at most two letters from `w`, if there is exactly one.
fn near<'a>(w: &str, names: impl IntoIterator<Item = &'a String>) -> Option<&'a String> {
    let mut close = names.into_iter().filter(|n| *n != w && distance(&n.to_lowercase(), &w.to_lowercase()) <= 2);
    let one = close.next()?;
    close.all(|n| n == one).then_some(one)
}

/// The word that follows `marker` in `msg`, without its backticks.
fn after<'m>(msg: &'m str, marker: &str) -> Option<&'m str> {
    let rest = &msg[msg.find(marker)? + marker.len()..];
    let rest = rest.trim_start_matches('`');
    let end = rest.find(|c: char| !(c.is_alphanumeric() || c == '_')).unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// A checker error about an unknown kind, attribute or state, with `did you
/// mean` when one declared name is close (appendix H.2.6).
pub fn names(src: &Source, e: Error) -> Error {
    let kinds: Vec<String> = src.kinds.iter().map(|k| k.name.clone()).chain(src.slots.iter().map(|s| s.name.clone())).collect();
    let attrs_of = |k: &str| -> Vec<String> { src.kinds.iter().filter(|x| x.name == k).flat_map(|x| x.attrs.iter().map(|a| a.name.clone())).collect() };
    let kind_before = |m: &str, marker: &str| m.find(marker).and_then(|i| m[..i].split(' ').next_back()).map(str::to_string);
    let m = &e.msg;
    let hint = if let Some(w) = after(m, "unknown kind ").or_else(|| after(m, "there is no kind ")).or_else(|| after(m, " does not need a ")).or_else(|| after(m, " does not need an ")) {
        near(w, &kinds).cloned()
    } else if let Some(w) = after(m, "has no state ") {
        let listed: Vec<String> = m.split("its states are ").nth(1).map_or(vec![], |s| s.split(", ").map(str::to_string).collect());
        near(w, &listed).cloned()
    } else if let Some(w) = after(m, "has no attribute, part or owner called ") {
        let k = kind_before(m, " has no attribute").unwrap_or_default();
        let mut names = attrs_of(&k);
        names.extend(kinds.iter().map(|k| k.to_lowercase()));
        near(w, &names).cloned()
    } else if let Some(w) = after(m, "has no attribute ") {
        let k = kind_before(m, " has no attribute").filter(|k| k != "it").or_else(|| kind_before(m, " is shared")).unwrap_or_default();
        near(w, &attrs_of(&k)).cloned()
    } else {
        None
    };
    match hint {
        Some(h) => Error { msg: format!("{m}; did you mean {h}?"), ..e },
        None => e,
    }
}
