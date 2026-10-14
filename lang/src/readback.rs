//! Read-backs (spec §3.4): a rule's formula and its paraphrase, generated
//! from the compiled rule with fixed templates. No model is involved (D4).

use crate::ir::{Body, Cond, Lit, Op, Operand, Path, Program, Rule, Step};

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
        Cond::Cmp { left, op, right } => format!(
            "{} {} {}",
            path_formula(p, kind, v, left),
            op_symbol(*op),
            operand(p, right, |x| path_formula(p, kind, v, x))
        ),
        Cond::State { node, state, negated } => format!("{} is {}{state}", path_formula(p, kind, v, node), not(*negated)),
        Cond::Has { node, part, state, negated } => {
            let filter = state.as_ref().map_or_else(String::new, |s| format!(" where s is {s}"));
            let inner = format!("exists(s in {}.{part}{filter})", path_formula(p, kind, v, node));
            if *negated { format!("not {inner}") } else { inner }
        }
        Cond::And(cs) => cs.iter().map(|c| cond_formula(p, kind, v, c)).collect::<Vec<_>>().join(" and "),
        Cond::Or(cs) => format!("({})", cs.iter().map(|c| cond_formula(p, kind, v, c)).collect::<Vec<_>>().join(" or ")),
    }
}

fn cond_words(p: &Program, kind: &str, c: &Cond) -> String {
    match c {
        Cond::Cmp { left, op, right } => format!(
            "{} {} {}",
            path_words(p, kind, left),
            op_words(*op),
            operand(p, right, |x| path_words(p, kind, x))
        ),
        Cond::State { node, state, negated } => format!("{} is {}{state}", path_words(p, kind, node), not(*negated)),
        Cond::Has { node, part, state, negated } => {
            let that = state.as_ref().map_or_else(String::new, |s| format!(" that is {s}"));
            let a = if *negated { "no" } else { "a" };
            format!("{} has {a} {}{that}", path_words(p, kind, node), words_of_kind(part))
        }
        Cond::And(cs) => cs.iter().map(|c| cond_words(p, kind, c)).collect::<Vec<_>>().join(" and "),
        Cond::Or(cs) => cs.iter().map(|c| cond_words(p, kind, c)).collect::<Vec<_>>().join(" or "),
    }
}

fn not(negated: bool) -> &'static str {
    if negated { "not " } else { "" }
}

fn operand(p: &Program, o: &Operand, path: impl Fn(&Path) -> String) -> String {
    match o {
        Operand::Lit(l) => lit_text(p, l),
        Operand::Path(x) => path(x),
    }
}

/// A literal as it is written: `"France"`, `120 EUR`, `12.50 EUR`, `3`, `yes`.
pub fn lit_text(p: &Program, l: &Lit) -> String {
    match l {
        Lit::Text(s) => format!("\"{s}\""),
        Lit::Bool(true) => "yes".into(),
        Lit::Bool(false) => "no".into(),
        Lit::Count(n) => n.to_string(),
        Lit::Money(c) if c % 100 == 0 => format!("{} {}", c / 100, p.currency),
        Lit::Money(c) => format!("{}.{:02} {}", c / 100, (c % 100).abs(), p.currency),
    }
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
            Step::Part(k) => {
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
