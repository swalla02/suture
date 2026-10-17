//! Read-backs (spec §3.4): a rule's formula and its paraphrase, generated
//! from the compiled rule with fixed templates. No model is involved (D4).

use crate::ir::{Arith, Body, Cond, Lit, Num, Op, Path, Program, Rule, Step};

/// `for each Declaration d: d.value ≤ d.form.order.total`
pub fn formula(p: &Program, r: &Rule) -> String {
    let v: String = r.subject.chars().next().map_or_else(|| "x".to_string(), |c| c.to_lowercase().collect());
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
    format!("for each {} {v}: {body}", r.subject)
}

/// `each declaration on its own: its value is at most its form's order's total`
pub fn paraphrase(p: &Program, r: &Rule) -> String {
    let k = words_of_kind(&r.subject);
    let w = |c: &Cond| cond_words(p, &r.subject, c);
    let filter: Vec<String> = r.filter.iter().map(&w).collect();
    let scope = if filter.is_empty() { String::new() } else { format!(" where {}", filter.join(" and ")) };
    match &r.body {
        Body::Every(c) => format!("each {k}{scope} on its own: {}", w(c)),
        Body::No { pred, unless } => {
            let pred = w(pred);
            let pred = pred.strip_prefix("it ").unwrap_or(&pred);
            let tail = unless.as_ref().map_or_else(String::new, |u| format!(" unless {}", w(u)));
            format!("no {k}{scope} ever {pred}{tail}")
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
            let filter = state.as_ref().map_or_else(String::new, |s| format!(" where s is {s}"));
            let inner = format!("exists(s in {}.{part}{filter})", path_formula(p, kind, v, node));
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
            let a = if *negated { "no" } else { "a" };
            format!("{} has {a} {}{that}", path_words(p, kind, node), words_of_kind(part))
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
    }
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
