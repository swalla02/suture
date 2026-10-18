//! Read-backs (spec §3.4): a rule's formula and its paraphrase, generated
//! from the compiled rule with fixed templates. No model is involved (D4).

use crate::ir::{Arith, Body, Cond, Lit, Num, Op, Path, Program, Rule, Step};

/// `for each Declaration d: d.value ≤ d.form.order.total`
pub fn formula(p: &Program, r: &Rule) -> String {
    // A global rule (`all Refunds`) has no subject to range over: its body
    // is about the whole system, so no variable is bound.
    let global = r.subject.starts_with("all ");
    let v: String = if global { String::new() } else { r.subject.chars().next().map_or_else(|| "x".to_string(), |c| c.to_lowercase().collect()) };
    let f = |c: &Cond| cond_formula(p, &r.subject, &v, c);
    let filter: Vec<String> = r.filter.iter().map(&f).collect();
    let body = match &r.body {
        Body::Every(c) if filter.is_empty() => f(c),
        Body::Every(c) => format!("{} implies {}", filter.join(" and "), f(c)),
        Body::No { pred, unless } => {
            let mut lhs = filter;
            lhs.push(f(pred));
            match unless {
                Some(u) => format!("{} implies {}", lhs.join(" and "), f(u)),
                None => format!("not ({})", lhs.join(" and ")),
            }
        }
    };
    if global {
        return body;
    }
    format!("for each {} {v}: {body}", r.subject)
}

/// `each declaration on its own: its value is at most its form's order's total`
pub fn paraphrase(p: &Program, r: &Rule) -> String {
    let k = words_of_kind(&r.subject);
    let w = |c: &Cond| cond_words(p, &r.subject, c);
    let filter: Vec<String> = r.filter.iter().map(&w).collect();
    let scope = if filter.is_empty() { String::new() } else { format!(" where {}", filter.join(" and ")) };
    match &r.body {
        Body::Every(c) if r.subject.starts_with("all ") => format!("across the whole system: {}", w(c)),
        Body::Every(c) => format!("each {k}{scope} on its own: {}", w(c)),
        Body::No { pred, unless } => {
            let pred = w(pred);
            let pred = pred.strip_prefix("it ").unwrap_or(&pred);
            let tail = unless.as_ref().map_or_else(String::new, |u| format!(" unless {}", w(u)));
            match pred.strip_prefix("is ") {
                Some(rest) => format!("no {k}{scope} is ever {rest}{tail}"),
                None => format!("no {k}{scope} ever {pred}{tail}"),
            }
        }
    }
}

fn cond_formula(p: &Program, kind: &str, v: &str, c: &Cond) -> String {
    match c {
        Cond::Cmp { left, op, right } => {
            format!("{} {} {}", num_formula(p, kind, v, left), op_symbol(*op), num_formula(p, kind, v, right))
        }
        Cond::State { node, state, negated } => format!("{} is {}{state}", path_formula(p, kind, v, node), not(*negated)),
        Cond::Has { node, part, state, negated } => {
            let x = element_var(part, kind);
            let filter = state.as_ref().map_or_else(String::new, |s| format!(" where {x} is {s}"));
            let inner = format!("exists({x} in {}.{part}{filter})", path_formula(p, kind, v, node));
            if *negated { format!("not {inner}") } else { inner }
        }
        Cond::Contains { set, item, negated } => {
            format!("{} {} {}", num_formula(p, kind, v, item), if *negated { "∉" } else { "∈" }, num_formula(p, kind, v, set))
        }
        Cond::And(cs) => cs.iter().map(|c| cond_formula(p, kind, v, c)).collect::<Vec<_>>().join(" and "),
        Cond::Or(cs) => format!("({})", cs.iter().map(|c| cond_formula(p, kind, v, c)).collect::<Vec<_>>().join(" or ")),
    }
}

fn cond_words(p: &Program, kind: &str, c: &Cond) -> String {
    match c {
        Cond::Cmp { left, op, right } => format!("{} {} {}", num_words(p, kind, left), op_words(*op), num_words(p, kind, right)),
        Cond::State { node, state, negated } => format!("{} is {}{state}", path_words(p, kind, node), not(*negated)),
        Cond::Has { node, part, state, negated } => {
            let that = state.as_ref().map_or_else(String::new, |s| format!(" that is {s}"));
            let part = words_of_kind(part);
            let a = match (*negated, part.starts_with(|c: char| "aeiou".contains(c))) {
                (true, _) => "no",
                (false, true) => "an",
                (false, false) => "a",
            };
            format!("{} has {a} {part}{that}", path_words(p, kind, node))
        }
        Cond::Contains { set, item, negated } => {
            format!("{} {} {}", num_words(p, kind, set), if *negated { "does not contain" } else { "contains" }, num_words(p, kind, item))
        }
        Cond::And(cs) => cs.iter().map(|c| cond_words(p, kind, c)).collect::<Vec<_>>().join(" and "),
        Cond::Or(cs) => cs.iter().map(|c| cond_words(p, kind, c)).collect::<Vec<_>>().join(" or "),
    }
}

fn not(negated: bool) -> &'static str {
    if negated { "not " } else { "" }
}

/// `sum(r in o.refunds where r is refunded: r.amount)`, `o.total / 10 + 1 EUR`
fn num_formula(p: &Program, kind: &str, v: &str, n: &Num) -> String {
    match n {
        Num::Path(x) => path_formula(p, kind, v, x),
        Num::Lit(l) => lit_text(p, l),
        Num::Bin(x, op, y) => bin(*op, x, y, |m| num_formula(p, kind, v, m)),
        Num::Sum { of, kind: k, calc, filter, .. } => {
            let e = element_var(k, v);
            let filter = filter.as_ref().map_or_else(String::new, |f| format!(" where {}", cond_formula(p, k, &e, f)));
            format!("sum({e} in {}{filter}: {})", relation_formula(p, kind, v, of, k), num_formula(p, k, &e, calc))
        }
        Num::Count { of, kind: k, filter } => {
            let e = element_var(k, v);
            let filter = filter.as_ref().map_or_else(String::new, |f| format!(" where {}", cond_formula(p, k, &e, f)));
            format!("count({e} in {}{filter})", relation_formula(p, kind, v, of, k))
        }
        Num::Taken { of, name, .. } => match contribution(p, kind, name) {
            Some(b) => {
                let e = element_var(&b.subject, v);
                let rel = match &b.key {
                    Num::Path(Path { steps }) => match steps.as_slice() {
                        [Step::Attr(a)] => format!("{} where {e}.{a} = {v}", b.subject),
                        _ => b.subject.clone(),
                    },
                    _ => format!("all {}", b.subject),
                };
                let t = b.taking.as_ref().expect("a contribution takes an amount");
                let per = t.period.as_ref().map_or_else(String::new, |per| format!(", per {per} of {e}.created"));
                match &t.amount {
                    Num::Lit(_) => format!("count({e} in {rel}{per})"),
                    a => format!("sum({e} in {rel}{per}: {})", num_formula(p, &b.subject, &e, a)),
                }
            }
            None => format!("taken_{name}({})", path_formula(p, kind, v, of)),
        },
    }
}

/// The implicit binding behind a total across units (appendix F.2.11).
fn contribution<'p>(p: &'p Program, kind: &str, name: &str) -> Option<&'p crate::ir::Binding> {
    p.bindings.iter().find(|b| b.target == kind && b.key_attr.is_empty() && b.taking.as_ref().is_some_and(|t| t.name == name))
}

/// `the total amount of its refunds that are refunded`, `its total / 10 + 1 EUR`
fn num_words(p: &Program, kind: &str, n: &Num) -> String {
    match n {
        Num::Path(x) => path_words(p, kind, x),
        Num::Lit(l) => lit_text(p, l),
        Num::Bin(x, op, y) => bin(*op, x, y, |m| num_words(p, kind, m)),
        Num::Sum { of, kind: k, calc, filter, .. } => match calc.as_ref() {
            Num::Path(Path { steps }) if steps.len() == 1 && matches!(steps[0], Step::Attr(_)) => {
                let Step::Attr(a) = &steps[0] else { unreachable!("matched above") };
                format!("the total {a} of {}{}", relation_words(p, kind, of, k), that_words(p, k, filter.as_deref()))
            }
            other => {
                let one = p.lists.iter().find(|l| l.element == *k).map_or_else(|| words_of_kind(k), |l| l.singular());
                format!("the total of {} over {}", element_words(p, kind, k, &one, other), relation_words(p, kind, of, k))
            }
        },
        Num::Count { of, kind: k, filter } => {
            format!("the number of {}{}", relation_words(p, kind, of, k), that_words(p, k, filter.as_deref()))
        }
        Num::Taken { of, name, .. } => match contribution(p, kind, name) {
            Some(b) => {
                let t = b.taking.as_ref().expect("a contribution takes an amount");
                let base = t.period.as_ref().map_or(name.as_str(), |per| name.strip_suffix(&format!(" per {per}")).unwrap_or(name));
                let (what, plural) = base.split_once(" of ").unwrap_or(("", base));
                let whose = if matches!(b.key, Num::Lit(_)) { "all" } else { "its" };
                let per = t.period.as_ref().map_or_else(String::new, |per| format!(" in a {per}"));
                if what == "number" {
                    format!("the number of {whose} {plural}{per}")
                } else {
                    format!("the total {what} of {whose} {plural}{per}")
                }
            }
            None => format!("the total {name} taken from {}", path_words(p, kind, of)),
        },
    }
}

/// A calculation evaluated at each element, in words: `the line's price`
/// for the element, `its discount` for the owner it reads through its owner step.
fn element_words(p: &Program, owner: &str, element: &str, one: &str, n: &Num) -> String {
    match n {
        Num::Path(Path { steps }) if steps.first() == Some(&Step::Owner) => path_words(p, owner, &Path { steps: steps[1..].to_vec() }),
        Num::Path(path) => {
            let names: Vec<String> = step_names(p, element, path).into_iter().map(|(n, part)| if part { words_of_kind(&n) } else { n }).collect();
            if names.is_empty() { format!("the {one}") } else { format!("the {one}'s {}", names.join("'s ")) }
        }
        Num::Bin(x, op, y) => bin(*op, x, y, |m| element_words(p, owner, element, one, m)),
        other => num_words(p, element, other),
    }
}

/// `x op y`, with parentheses where the reading needs them.
fn bin(op: Arith, x: &Num, y: &Num, show: impl Fn(&Num) -> String) -> String {
    let prec = |a: Arith| if matches!(a, Arith::Add | Arith::Sub) { 1 } else { 2 };
    let side = |m: &Num, right: bool| match m {
        // A right operand of the same precedence is bracketed too: with whole
        // numbers, `10 × (n / k)` is not `10 × n / k`.
        Num::Bin(_, inner, _) if prec(*inner) < prec(op) || (right && prec(*inner) == prec(op)) => {
            format!("({})", show(m))
        }
        _ => show(m),
    };
    let sym = match op {
        Arith::Add => "+",
        Arith::Sub => "−",
        Arith::Mul => "×",
        Arith::Div => "/",
    };
    format!("{} {sym} {}", side(x, false), side(y, true))
}

/// The variable for each entity of a relation: its kind's initial, unless
/// the subject already has that one.
fn element_var(kind: &str, subject: &str) -> String {
    let e: String = kind.chars().next().map_or_else(|| "e".to_string(), |c| c.to_lowercase().collect());
    if e == subject { format!("{e}2") } else { e }
}

/// A relation's name: its list's, when the kind is a list element, else the kind's plural.
fn plural_words(p: &Program, k: &str) -> String {
    if let Some(l) = p.lists.iter().find(|l| l.element == k) {
        return l.name.clone();
    }
    words_of_kind(&p.kind(k).map_or_else(|| format!("{k}s"), |x| x.plural.clone()))
}

/// `o.refunds`
fn relation_formula(p: &Program, kind: &str, v: &str, of: &Path, k: &str) -> String {
    format!("{}.{}", path_formula(p, kind, v, of), plural_words(p, k).replace(' ', "_"))
}

/// `its refunds`, `its form's declarations`
fn relation_words(p: &Program, kind: &str, of: &Path, k: &str) -> String {
    let owner = path_words(p, kind, of);
    if owner == "it" { format!("its {}", plural_words(p, k)) } else { format!("{owner}'s {}", plural_words(p, k)) }
}

/// ` that are lost`, from the condition about each entity.
fn that_words(p: &Program, k: &str, f: Option<&Cond>) -> String {
    let Some(f) = f else { return String::new() };
    let one = |c: &Cond| {
        let w = cond_words(p, k, c);
        let w = w.strip_prefix("it ").unwrap_or(&w).to_string();
        if let Some(rest) = w.strip_prefix("is ") {
            format!("are {rest}")
        } else if let Some(rest) = w.strip_prefix("has ") {
            format!("have {rest}")
        } else {
            w
        }
    };
    let words = match f {
        Cond::And(cs) => cs.iter().map(one).collect::<Vec<_>>().join(" and "),
        Cond::Or(cs) => cs.iter().map(one).collect::<Vec<_>>().join(" or "),
        c => one(c),
    };
    format!(" that {words}")
}

/// A literal as it is written: `"France"`, `120 EUR`, `12.50 EUR`, `3`, `yes`.
/// An open choice for `suture build` (appendix F.3): who chooses what, by
/// which pool key, and how candidates are ranked.
pub fn choice(p: &Program, b: &crate::ir::Binding) -> String {
    let key = num_words(p, &b.subject, &b.key);
    if b.key_attr.is_empty() {
        let to = if b.target.starts_with("all ") { "the".to_string() } else { format!("its {}'s", b.key_words()) };
        let t = b.taking.as_ref().map_or_else(String::new, |t| t.name.clone());
        if b.target.starts_with("all ") {
            return format!("each {} adds to {to} total {t} (one total for the whole system: a bottleneck)", b.subject);
        }
        return format!("each {} adds to {to} total {t} (no choice: the key names the {})", b.subject, b.target);
    }
    let key = match b.per.as_ref().and_then(|l| p.lists.iter().find(|x| x.name == *l)) {
        Some(l) => key.replace("its element", &format!("the {}", l.singular())),
        None => key,
    };
    let per = b.per.as_ref().map_or_else(String::new, |l| format!(" for each of its {l}"));
    let how = if b.key_member { "contain" } else { "is" };
    let rank = match b.prefer.len() {
        0 => "in the order they were created".to_string(),
        n => format!("by `prefer {}` ({n} criteri{})", b.target, if n == 1 { "on" } else { "a" }),
    };
    format!("{} uses a {}{per}: pool key {}'s {} {how} {key}, up to {} candidates, ranked {rank}", b.subject, b.target, b.target, b.key_attr, p.candidates)
}

fn a_kind(k: &str) -> String {
    if k.starts_with(['A', 'E', 'I', 'O', 'U']) { format!("an {k}") } else { format!("a {k}") }
}

/// `customer (a Customer), support, staff` (appendix G.2.1).
/// Who handles a choice point (appendix J.2.8): the `decide`s that can,
/// most specific first, down to the first that always applies, then the
/// `prefer` ranking every model falls back to.
pub fn handlers(p: &Program, b: &crate::ir::Binding) -> Vec<String> {
    let mut chain = vec![b.subject.clone()];
    while let Some(o) = p.kind(chain.last().expect("not empty")).and_then(|x| x.owner.as_ref()) {
        if chain.contains(&o.kind) {
            break;
        }
        chain.push(o.kind.clone());
    }
    let mut ranked: Vec<((bool, usize, bool), &crate::ir::Decision)> = p
        .decisions
        .iter()
        .filter(|d| d.point.as_ref().is_none_or(|x| *x == b.target))
        .filter_map(|d| {
            let near = match &d.scope {
                None => 0,
                Some(k) => chain.len() - chain.iter().position(|c| c == k)?,
            };
            Some(((d.point.is_some(), near, d.cond.is_some()), d))
        })
        .collect();
    ranked.sort_by(|x, y| y.0.cmp(&x.0));
    let mut out = vec![];
    for (_, d) in ranked {
        let scope = d.source.split_once(" for ").and_then(|(_, rest)| rest.rsplit_once(" with ")).map(|(s, _)| s.to_string());
        out.push(match scope {
            Some(s) => format!("for {s}: {}", d.handler()),
            None => d.handler(),
        });
        if d.cond.is_none() {
            break;
        }
    }
    out.push(if b.prefer.is_empty() { "otherwise the order they were created".to_string() } else { format!("otherwise `prefer {}`", b.target) });
    out
}

pub fn roles(p: &Program) -> String {
    let shown: Vec<String> = p.roles.iter().map(|r| match &r.kind {
        Some(k) => format!("{} (a {k})", r.name),
        None => r.name.clone(),
    }).collect();
    shown.join(", ")
}

/// For each attribute callers may change, the kinds whose units carry it
/// and are refreshed and checked when it changes (appendix G.2.7): those
/// that refer to its kind, directly or through other references, and
/// those bound to it.
pub fn watches(p: &Program) -> Vec<String> {
    let mut out = vec![];
    for a in &p.admits {
        let crate::ir::Admitted::Change { kind, attr } = &a.what else { continue };
        let mut carry = vec![kind.clone()];
        let mut i = 0;
        while i < carry.len() {
            let k = carry[i].clone();
            for x in &p.kinds {
                let refers = x.attrs.iter().any(|y| y.of.as_deref() == Some(k.as_str()) && x.owner.as_ref().is_none_or(|o| o.attr != y.name));
                if refers && !carry.contains(&x.name) {
                    carry.push(x.name.clone());
                }
            }
            i += 1;
        }
        let plural = |k: &str| p.kind(k).map_or(k.to_string(), |x| x.plural.clone());
        let mut readers: Vec<String> = carry[1..].iter().map(|k| format!("{} that refer to {}", plural(k), a_kind(kind))).collect();
        for b in p.bindings.iter().filter(|b| b.target == *kind) {
            readers.push(format!("{} bound to {}", plural(&b.subject), a_kind(kind)));
        }
        readers.dedup();
        let who = a.roles.join(", ");
        let line = if readers.is_empty() {
            format!("{kind}'s {attr}, changed by {who}: no other unit carries it")
        } else {
            format!("{kind}'s {attr}, changed by {who}: carried by {}; each change is checked there first, then refreshed", readers.join(" and "))
        };
        if !out.contains(&line) {
            out.push(line);
        }
    }
    out
}

/// A strict rule's sourced facts (appendix I.2.8): `credit from bureau, at
/// most 1 hour old`, or `None` for a rule that reads none.
pub fn rule_sources(p: &Program, r: &Rule) -> Option<String> {
    let parts: Vec<String> = r
        .sources
        .iter()
        .filter_map(|(k, a)| p.source_of(k, a))
        .map(|s| {
            let bound = r.fresh.iter().find(|(a, _)| *a == s.attr).map(|(_, secs)| *secs).into_iter().chain(s.good_for).min();
            match bound {
                Some(secs) => format!("{} from {}, at most {} old", s.attr, s.adapter, duration_text(secs as i64)),
                None => format!("{} from {}, fetched once", s.attr, s.adapter),
            }
        })
        .collect();
    (!parts.is_empty()).then(|| parts.join("; "))
}

/// One line for `suture build`'s `sourcing:` section.
pub fn source_line(p: &Program, s: &crate::ir::Source) -> String {
    let sends: Vec<&str> = s.sends.iter().map(|(n, _)| n.as_str()).collect();
    let trusted = s.good_for.map_or_else(|| "trusted until the unit ends".to_string(), |secs| format!("trusted for {}", duration_text(secs as i64)));
    let otherwise = match &s.otherwise {
        crate::ir::Otherwise::Hold => "if it fails, the steps its rules guard wait".to_string(),
        crate::ir::Otherwise::Use(l) => format!("if it fails, use {}", lit_text(p, l)),
        crate::ir::Otherwise::Again(secs) => format!("if it fails, ask again after {}", duration_text(*secs as i64)),
        crate::ir::Otherwise::Act(actions) => {
            let acts: Vec<String> = actions.iter().map(|a| format!("{} is {}{}", target_text(&a.target), if a.value { "" } else { "not " }, a.attr)).collect();
            format!("if it fails, {}", acts.join(" and "))
        }
    };
    let readers: Vec<&str> = p.rules.iter().filter(|r| r.sources.iter().any(|(k, a)| *k == s.kind && *a == s.attr)).map(|r| r.name.as_str()).collect();
    let read = if readers.is_empty() { "no rule reads it, so it is never fetched".to_string() } else { format!("read by rule{} {}", if readers.len() == 1 { "" } else { "s" }, readers.join(", ")) };
    format!(
        "{}'s {} from {} within {} (sends {}); {trusted}; {otherwise}; {read}",
        s.kind,
        s.attr,
        s.adapter,
        duration_text(s.within_secs as i64),
        sends.join(", ")
    )
}

/// `it`, `its Order`: where an action applies.
fn target_text(path: &crate::ir::Path) -> String {
    let names: Vec<String> = path
        .steps
        .iter()
        .map(|s| match s {
            crate::ir::Step::Part(k) | crate::ir::Step::Slot(k) => k.clone(),
            crate::ir::Step::Owner => "owner".into(),
            crate::ir::Step::Attr(a) => a.clone(),
            _ => "…".into(),
        })
        .collect();
    if names.is_empty() { "it".into() } else { format!("its {}", names.join("'s ")) }
}

pub fn lit_text(p: &Program, l: &Lit) -> String {
    match l {
        Lit::Text(s) => format!("\"{s}\""),
        Lit::Bool(true) => "yes".into(),
        Lit::Bool(false) => "no".into(),
        Lit::Count(n) => n.to_string(),
        Lit::Duration(secs) => duration_text(*secs),
        Lit::Money(c) if c % 100 == 0 => format!("{} {}", c / 100, p.currency),
        Lit::Money(c) => format!("{}.{:02} {}", c / 100, (c % 100).abs(), p.currency),
    }
}

/// `30 seconds`, `2 days`: the largest unit up to weeks that divides evenly.
pub fn duration_text(secs: i64) -> String {
    let (n, unit) = [(604_800, "week"), (86_400, "day"), (3_600, "hour"), (60, "minute")]
        .into_iter()
        .find(|(n, _)| secs != 0 && secs % n == 0)
        .map_or((secs, "second"), |(n, unit)| (secs / n, unit));
    format!("{n} {unit}{}", if n == 1 { "" } else { "s" })
}

fn op_symbol(op: Op) -> &'static str {
    match op {
        Op::Eq => "=",
        Op::Ne => "≠",
        Op::Lt => "<",
        Op::Le => "≤",
        Op::Gt => ">",
        Op::Ge => "≥",
    }
}

fn op_words(op: Op) -> &'static str {
    match op {
        Op::Eq => "is",
        Op::Ne => "is not",
        Op::Lt => "is less than",
        Op::Le => "is at most",
        Op::Gt => "is more than",
        Op::Ge => "is at least",
    }
}

/// Each step's name, and whether it is a part: an attribute, a part's kind,
/// or, for an owner step, the name of the attribute that holds the owner.
fn step_names(p: &Program, kind: &str, path: &Path) -> Vec<(String, bool)> {
    let mut out = vec![];
    let mut cur = kind.to_string();
    for s in &path.steps {
        match s {
            Step::Attr(a) => out.push((a.clone(), false)),
            Step::Ref(a) => {
                out.push((a.clone(), false));
                cur = p.kind(&cur).and_then(|k| k.attr(a)).and_then(|x| x.of.clone()).unwrap_or_default();
            }
            Step::Part(k) | Step::Slot(k) => {
                out.push((k.clone(), true));
                cur = k.clone();
            }
            Step::Bound(k) => {
                out.push((k.clone(), true));
                cur = k.clone();
            }
            Step::Cand => out.push(("candidate".into(), false)),
            Step::Elem => out.push(("element".into(), false)),
            Step::Caller => out.push(("caller".into(), false)),
            Step::Owner => match p.kind(&cur).and_then(|k| k.owner.clone()) {
                Some(o) => {
                    out.push((o.attr, false));
                    cur = o.kind;
                }
                None => out.push(("owner".into(), false)),
            },
        }
    }
    out
}

/// `d.form.order.total`
fn path_formula(p: &Program, kind: &str, v: &str, path: &Path) -> String {
    step_names(p, kind, path).iter().fold(v.to_string(), |acc, (n, _)| format!("{acc}.{n}"))
}

/// `its form's order's total`, or `it` for the subject itself.
fn path_words(p: &Program, kind: &str, path: &Path) -> String {
    let names: Vec<String> =
        step_names(p, kind, path).into_iter().map(|(n, part)| if part { words_of_kind(&n) } else { n }).collect();
    if names.is_empty() { "it".into() } else { format!("its {}", names.join("'s ")) }
}

/// `CustomsForm` becomes `customs form`.
pub fn words_of_kind(k: &str) -> String {
    let mut out = String::new();
    for (i, ch) in k.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            out.push(' ');
        }
        out.extend(ch.to_lowercase());
    }
    out
}
