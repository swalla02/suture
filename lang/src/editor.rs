//! What an editor shows (appendix P): errors and warnings on their lines,
//! the read-back of the statement under the cursor, and the words that can
//! come next. Everything here is fixed code over the parser and the compiled
//! program; nothing guesses (D4).

use crate::ast::Source;
use crate::ir::{Mark, Program, Ty};
use crate::lex::{statements, tokens, Error, Stmt, Tok};
use crate::parse::statement;

/// An error or a warning, on a line of a file (lines count from 1; 0 is
/// the module as a whole).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub file: String,
    pub line: usize,
    pub msg: String,
    pub warning: bool,
}

/// Compiles the files, and returns what to show: the errors, or the
/// warnings with the program. Warnings name no line, so they go on the
/// `module` line.
pub fn diagnostics(files: &[(String, String)]) -> (Vec<Diagnostic>, Option<Program>) {
    match crate::compile(files) {
        Err(es) => (es.into_iter().map(|Error { file, line, msg }| Diagnostic { file, line, msg, warning: false }).collect(), None),
        Ok(p) => {
            let (file, line) = module_line(files).unwrap_or_default();
            let ws = p.warnings.iter().map(|w| Diagnostic { file: file.clone(), line, msg: w.clone(), warning: true }).collect();
            (ws, Some(p))
        }
    }
}

fn module_line(files: &[(String, String)]) -> Option<(String, usize)> {
    files.iter().find_map(|(name, text)| {
        let sts = statements(name, text).ok()?;
        sts.iter().find(|s| matches!(s.lines[0].first(), Some(Tok::Word(w)) if w == "module")).map(|s| (name.clone(), s.line))
    })
}

/// The statement that `line` (from 1) is part of, and its last line.
fn statement_at(text: &str, line: usize) -> Option<(Stmt, usize)> {
    let sts = statements("", text).ok()?;
    let i = sts.iter().rposition(|s| s.line <= line)?;
    let end = sts.get(i + 1).map_or(text.lines().count(), |n| n.line - 1);
    (line <= end).then(|| (sts[i].clone(), end))
}

/// The word at column `col` (in characters) of `line`, if any.
fn word_at(line: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = line.chars().collect();
    let is = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let at = col.min(chars.len());
    let mut start = at;
    while start > 0 && is(chars[start - 1]) {
        start -= 1;
    }
    let mut end = at;
    while end < chars.len() && is(chars[end]) {
        end += 1;
    }
    (start < end).then(|| chars[start..end].iter().collect())
}

fn ty_text(p: &Program, ty: Ty, of: &Option<String>) -> String {
    match (ty, of) {
        (Ty::Set, Some(k)) => format!("set of {}", p.kind(k).map_or(k.clone(), |k| k.plural.clone())),
        (_, Some(k)) => k.clone(),
        (t, None) => format!("{t:?}"),
    }
}

/// What the program says about a kind: its attributes, parts, effect and
/// the rules about it.
pub fn kind_text(p: &Program, name: &str) -> Option<String> {
    let k = p.kind(name)?;
    let mut out = format!("**kind {}** ({})\n", k.name, k.plural);
    if let Some(o) = &k.owner {
        out.push_str(&format!("\npart of a {} (its {})\n", o.kind, o.attr));
    }
    for a in &k.attrs {
        let mut line = format!("- {}: {}", a.name, ty_text(p, a.ty, &a.of));
        match a.mark {
            Some(Mark::Secret) => line.push_str(" secret"),
            Some(Mark::Personal) => line.push_str(" personal"),
            None => {}
        }
        if let Some(d) = &a.default {
            line.push_str(&format!(" = {}", crate::readback::lit_text(p, d)));
        }
        if a.formula.is_some() {
            line.push_str(" (computed)");
        }
        out.push_str(&format!("{line}\n"));
    }
    let parts: Vec<&str> = p.needs.iter().filter(|n| n.owner == k.name).map(|n| n.part.as_str()).collect();
    if !parts.is_empty() {
        out.push_str(&format!("\nneeds {}\n", parts.join(", ")));
    }
    if let Some(e) = p.effect_of(&k.name) {
        out.push_str(&format!("\n{}\n", effect_text(p, e)));
    }
    for r in p.rules.iter().filter(|r| r.subject == k.name) {
        out.push_str(&format!("\nrule {}: {}\n", r.name, r.paraphrase));
    }
    Some(out)
}

fn effect_text(p: &Program, e: &crate::ir::Effect) -> String {
    let manifest = crate::manifest::adapters(p);
    let receives = manifest.lines().find(|l| l.trim_start().starts_with(&format!("{} ", e.adapter))).map(|l| l.trim().to_string());
    let mut outcomes = vec![e.success.clone()];
    outcomes.extend(e.failures.iter().cloned());
    let within = crate::readback::duration_text(e.within_secs as i64);
    let mut out = format!("{} {} via {} within {within}; it answers {}", e.verb, e.kind, e.adapter, outcomes.join(" | "));
    if let Some(r) = receives {
        out.push_str(&format!("\n\n{r}"));
    }
    out
}

/// The read-back of what is under the cursor, as Markdown: the statement's
/// meaning as the compiler understood it, or what a kind name refers to.
/// `line` counts from 1 and `col` from 0, in characters.
pub fn hover(p: &Program, text: &str, line: usize, col: usize) -> Option<String> {
    let here = text.lines().nth(line.checked_sub(1)?)?;
    // A blank or comment line belongs to no statement.
    if tokens(strip(here)).map_or(true, |t| t.is_empty()) {
        return None;
    }
    let word = word_at(here, col);
    let (st, _) = statement_at(text, line)?;
    let toks = st.tokens();
    let w = |i: usize| match toks.get(i) {
        Some(Tok::Word(w)) => Some(w.as_str()),
        _ => None,
    };
    // A kind's name is read back as the kind, wherever it is.
    if let Some(k) = word.as_deref().filter(|w| p.kind(w).is_some()) {
        return kind_text(p, k);
    }
    match w(0)? {
        "rule" => {
            let r = p.rules.iter().find(|r| Some(r.name.as_str()) == w(1))?;
            let mut out = format!("**rule {}**\n\nunderstood as: {}\n\nin other words: {}\n\ncost: {}", r.name, r.formula, r.paraphrase, r.tier);
            if let Some(src) = crate::readback::rule_sources(p, r) {
                out.push_str(&format!("\n\nsources: {src}"));
            }
            Some(out)
        }
        "kind" => kind_text(p, w(1)?),
        "effect" => p.effects.iter().find(|e| Some(e.verb.as_str()) == w(1) && Some(e.kind.as_str()) == w(2)).map(|e| effect_text(p, e)),
        "source" => {
            let (kind, attr) = (w(1)?, w(3)?);
            let lines: Vec<String> = p.sources_of(kind, attr).into_iter().map(|s| crate::readback::source_line(p, s)).collect();
            (!lines.is_empty()).then(|| lines.join("\n\n"))
        }
        "allow" | "role" | "admit" => {
            let mut out = crate::manifest::adapters(p);
            if !p.roles.is_empty() {
                out.push_str(&format!("\nroles: {}", crate::readback::roles(p)));
            }
            Some(format!("```text\n{}\n```", out.trim_end()))
        }
        k if w(1) == Some("uses") && p.bindings.iter().any(|b| b.subject == k) => {
            let lines: Vec<String> = p.bindings.iter().filter(|b| b.subject == k).map(|b| crate::readback::choice(p, b)).collect();
            Some(lines.join("\n\n"))
        }
        // Otherwise, the first kind the statement names.
        _ => toks.iter().find_map(|t| match t {
            Tok::Word(w) => kind_text(p, w),
            _ => None,
        }),
    }
}

/// One thing the editor may insert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    /// What it is: `keyword`, `kind`, `attribute`, `adapter`, `phrase` or
    /// `statement`.
    pub detail: String,
    /// The text inserted, with `${n:…}` placeholders for a statement.
    pub insert: String,
}

/// The grammar's words: every word the parsers look for, read from their
/// own source, so the list cannot fall behind them.
fn grammar_words() -> &'static [String] {
    static WORDS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    WORDS.get_or_init(scan_grammar)
}

fn scan_grammar() -> Vec<String> {
    let mut out = vec![];
    for src in [include_str!("parse.rs"), include_str!("sentence.rs")] {
        let marker = "_word(\"";
        let mut rest = src;
        while let Some(i) = rest.find(marker) {
            rest = &rest[i + marker.len()..];
            let w: String = rest.chars().take_while(|c| c.is_ascii_alphabetic() || *c == '-').collect();
            if !w.is_empty() && rest[w.len()..].starts_with('"') {
                out.push(w);
            }
        }
        // `match` arms on words: `"seconds" | "minutes" =>`.
        for line in src.lines().map(str::trim).filter(|l| l.starts_with('"') && l.contains("=>")) {
            for part in line.split("=>").next().unwrap_or_default().split('|') {
                let w = part.trim().trim_matches('"');
                if !w.is_empty() && w.chars().all(|c| c.is_ascii_alphabetic()) {
                    out.push(w.to_string());
                }
            }
        }
    }
    for t in ["Text", "Bool", "Count", "Money", "Duration", "Time", "Media", "secret", "personal", "yes", "no", "true", "false"] {
        out.push(t.to_string());
    }
    out.sort();
    out.dedup();
    out
}

/// Statement templates, for an empty line.
const STATEMENTS: &[(&str, &str)] = &[
    ("kind", "kind ${1:Name} { ${2:attr}: ${3:Text} }"),
    ("needs", "${1:Kind} needs ${2:Part}"),
    ("effect", "effect ${1:verb} ${2:Kind} via ${3:adapter} within ${4:30 seconds}\n  sends    ${5:its attr}\n  reports  ${6:done} | fails with ${7:failed}"),
    ("source", "source ${1:Kind}'s ${2:attr} via ${3:adapter} within ${4:2 seconds}\n  sends    ${5:its attr}\n  reports  ${6:found}\n  good for ${7:1 day}"),
    ("rule", "rule ${1:name}: every ${2:Kind}'s ${3:attr} is at most ${4:1}"),
    ("define", "define: a ${1:Kind} is ${2:state} when ${3:its attr is yes}"),
    ("desired", "desired within ${1:1 day}: every ${2:Kind} is ${3:state}"),
    ("when", "when a ${1:Kind} is ${2:outcome}: ${3:action}"),
    ("role", "role ${1:name} is a ${2:Kind}"),
    ("admit", "admit ${1:verb} a new ${2:Kind} by ${3:role}"),
    ("allow", "allow ${1:adapter} to receive ${2:Kind}'s ${3:attr}"),
    ("prefer", "prefer ${1:Kind}: lowest ${2:attr}"),
    ("decide", "decide ${1:Kind} with model \"${2:name}@${3:1}\""),
    ("apply", "apply rule ${1:name} to ${2:Kinds} created from version ${3:2}"),
];

/// Whether the tokens can begin a statement: they parse, or the parser
/// stops only at their end, wanting more.
fn viable(lines: &[Vec<Tok>]) -> bool {
    let st = Stmt { file: String::new(), line: 1, lines: lines.to_vec() };
    match statement(&st, &mut Source::default()) {
        Ok(()) => true,
        Err(e) => e.msg.contains("the end of the statement"),
    }
}

/// The words that can come next at column `col` (in characters) of
/// `line` (from 1): each one the parser accepts there, kinds and
/// attributes from the program, and the canonical phrases (appendix P.2.3).
pub fn complete(p: Option<&Program>, text: &str, line: usize, col: usize) -> Vec<Completion> {
    let Some(here) = text.lines().nth(line.saturating_sub(1)).or(Some("")) else { return vec![] };
    let mut before: String = here.chars().take(col).collect();
    // Nothing is offered inside a comment.
    if strip(&before).len() < before.len() {
        return vec![];
    }
    // Right after `Kind'`, the possessive is being typed: complete `'s` too.
    let poss = before.ends_with('\'');
    if poss {
        before.pop();
    }
    let typed: String = if poss { String::new() } else { before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-').collect::<Vec<_>>().into_iter().rev().collect() };
    let head = &before[..before.len() - typed.len()];
    let indented = here.starts_with(' ') || here.starts_with('\t');
    let item = |label: &str, detail: &str, insert: &str| Completion { label: label.into(), detail: detail.into(), insert: insert.into() };
    let starts = |w: &str| w.starts_with(&typed);
    if !indented && head.trim().is_empty() {
        let mut out: Vec<Completion> = STATEMENTS.iter().filter(|(l, _)| starts(l)).map(|(l, t)| item(l, "statement", t)).collect();
        out.extend(p.into_iter().flat_map(|p| p.kinds.iter()).filter(|k| starts(&k.name)).map(|k| item(&k.name, "kind", &k.name)));
        return out;
    }
    // The statement's lines up to the cursor.
    let mut lines: Vec<Vec<Tok>> = vec![];
    if indented {
        let Some((st, _)) = statement_at(text, line) else { return vec![] };
        let all: Vec<&str> = text.lines().collect();
        for l in st.line..line {
            if let Ok(t) = tokens(strip(all[l - 1]))
                && !t.is_empty()
            {
                lines.push(t);
            }
        }
    }
    let Ok(mut last) = tokens(strip(head)) else { return vec![] };
    if poss {
        last.push(Tok::Poss);
    }
    lines.push(last);
    let prev = lines.last().and_then(|l| l.last()).cloned();
    let before_prev = lines.last().and_then(|l| l.len().checked_sub(2).and_then(|i| l.get(i))).cloned();
    let with = |extra: &[&str]| {
        let mut ls = lines.clone();
        let last = ls.last_mut().expect("a line");
        for w in extra {
            last.push(Tok::Word((*w).to_string()));
        }
        viable(&ls)
    };
    let mut out = vec![];
    // After `Kind's`, that kind's attributes; after `its`, any kind's.
    let attrs: Vec<(String, String)> = match (&prev, &before_prev, p) {
        (Some(Tok::Poss), Some(Tok::Word(k)), Some(p)) if p.kind(k).is_some() => p.kind(k).map(|k| k.attrs.iter().map(|a| (a.name.clone(), ty_text(p, a.ty, &a.of))).collect()).unwrap_or_default(),
        (Some(Tok::Word(w)), _, Some(p)) if w == "its" || w == "their" => {
            let mut v: Vec<(String, String)> = p.kinds.iter().flat_map(|k| k.attrs.iter().map(|a| (a.name.clone(), ty_text(p, a.ty, &a.of)))).collect();
            v.sort();
            v.dedup_by(|a, b| a.0 == b.0);
            v
        }
        _ => vec![],
    };
    for (a, ty) in &attrs {
        if starts(a) && with(&[a]) {
            let insert = if poss { format!("s {a}") } else { a.clone() };
            out.push(item(a, &format!("attribute: {ty}"), &insert));
        }
    }
    if !attrs.is_empty() || poss {
        return out;
    }
    // Where the grammar takes any word, a made-up one fits too: there the
    // program's names are offered, not keywords.
    let (open_lower, open_upper) = (with(&["zqxv"]), with(&["Zqxv"]));
    let after = |ws: &[&str]| matches!(&prev, Some(Tok::Word(w)) if ws.contains(&w.as_str()));
    if let Some(p) = p {
        if open_lower && after(&["via", "allow"]) {
            let mut adapters: Vec<&str> = p.effects.iter().map(|e| e.adapter.as_str()).chain(p.sources.iter().map(|s| s.adapter.as_str())).collect();
            adapters.sort();
            adapters.dedup();
            return adapters.into_iter().filter(|a| starts(a)).map(|a| item(a, "adapter", a)).collect();
        }
        for k in &p.kinds {
            for name in [&k.name, &k.plural] {
                if starts(name) && with(&[name]) && !out.iter().any(|c: &Completion| c.label == *name) {
                    out.push(item(name, "kind", name));
                }
            }
        }
        if open_lower {
            let mut states: Vec<&str> = p.defines.iter().map(|d| d.state.as_str()).collect();
            states.sort();
            states.dedup();
            out.extend(states.into_iter().filter(|s| starts(s)).map(|s| item(s, "state", s)));
        }
    }
    // The canonical phrases (appendix H.2.2); after their first word, the rest.
    let mut phrases: Vec<&str> = crate::suggest::PHRASES.iter().map(|(_, c)| *c).filter(|c| c.contains(' ')).collect();
    phrases.sort();
    phrases.dedup();
    for ph in phrases {
        let ws: Vec<&str> = ph.split(' ').collect();
        let rest = after(&ws[..1]);
        let ws = if rest { &ws[1..] } else { &ws[..] };
        let label = ws.join(" ");
        if (ws.len() > 1 || rest) && starts(&label) && with(ws) && !out.iter().any(|c| c.label == label) {
            out.push(item(&label, "phrase", &label));
        }
    }
    for w in grammar_words().iter().cloned() {
        let open = if w.starts_with(char::is_uppercase) { open_upper } else { open_lower };
        if !open && starts(&w) && with(&[&w]) && !out.iter().any(|c| c.label == w) {
            out.push(item(&w, "keyword", &w));
        }
    }
    out
}

/// The line without its `--` comment.
fn strip(line: &str) -> &str {
    let mut in_str = false;
    let b = line.as_bytes();
    for i in 0..b.len() {
        match b[i] {
            b'"' => in_str = !in_str,
            b'-' if !in_str && b.get(i + 1) == Some(&b'-') => return &line[..i],
            _ => {}
        }
    }
    line
}
